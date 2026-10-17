//! Runtime Security synchronous scan (C03).
//!
//! A [`ScanDraft`] is the operator's explicit input, matching v1: an AI
//! security profile (by name or ID), a prompt and/or response, and an
//! optional application name. It is validated at admission (2 MiB per text,
//! the escaped request inside the 8 MiB SDK ceiling) and runs as one bounded
//! operation: the scan key is read only at dispatch, the SDK client is bound
//! to the credential generation's session, retries are disabled and
//! cancellation reaches the SDK request. The result keeps the service's
//! category, action and detector names verbatim; the verdict follows v1
//! (block → Block, allow+benign → Allow, allow+malicious → Alert) and every
//! other combination, or an incomplete scan, is Unknown — never Allow.
//! Failures carry the SDK's remote outcome; nothing is retried or replayed.

use std::sync::{Arc, Mutex};

use scm_sdk::airs::scan::models::{AiProfile, Metadata, Optional, ScanResponse};
use scm_sdk::airs::scan::{Content, Scanner, SyncScanOptions};
use scm_sdk::validation::Mode;
use scm_sdk::{ErrorKind, RemoteOutcome, Session};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::authority::CredentialStore;
use crate::runtime::{Claim, Outcome, Work};
use crate::scan::{MAX_REQUEST_BYTES, ScanBinding};
use crate::transport::NativeTransport;
use crate::vault::{RecordId, SessionVault, StorageMode, Vault, VaultError};

/// Largest prompt or response text (UTF-8 bytes).
pub const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
/// Longest profile name (UTF-16 code units, the SDK's schema limit).
pub const MAX_PROFILE_NAME_UNITS: usize = 100;
/// Longest application name (characters; v1's limit).
pub const MAX_APP_NAME_CHARS: usize = 256;
/// Settled scans kept per tenant for this session (v1 keeps five).
pub const HISTORY_PER_TENANT: usize = 5;
/// Room left in the request ceiling for the profile, metadata and keys.
const ENVELOPE_RESERVE: usize = 16 * 1024;
/// Longest service-provided string kept for display.
const MAX_SHOWN_CHARS: usize = 512;

/// The AI security profile a scan uses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProfileSelector {
    Name { name: String },
    Id { id: String },
}

/// The operator's scan input, captured when Scan is pressed.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanDraft {
    pub profile: ProfileSelector,
    pub prompt: String,
    pub response: String,
    pub app_name: Option<String>,
}

impl std::fmt::Debug for ScanDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScanDraft")
            .field("prompt_bytes", &self.prompt.len())
            .field("response_bytes", &self.response.len())
            .finish_non_exhaustive()
    }
}

fn uuid_shaped(text: &str) -> bool {
    text.len() == 36
        && text.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

impl ScanDraft {
    /// Normalizes and validates the draft; `None` when it cannot be sent.
    pub fn validate(self) -> Option<Self> {
        let profile = match self.profile {
            ProfileSelector::Name { name } => {
                let name = name.trim().to_owned();
                let ok = !name.is_empty()
                    && name.encode_utf16().count() <= MAX_PROFILE_NAME_UNITS
                    && !name.chars().any(char::is_control);
                ok.then_some(ProfileSelector::Name { name })?
            }
            ProfileSelector::Id { id } => {
                let id = id.trim().to_ascii_lowercase();
                uuid_shaped(&id).then_some(ProfileSelector::Id { id })?
            }
        };
        let app_name = match self.app_name.map(|a| a.trim().to_owned()) {
            Some(a) if a.is_empty() => None,
            Some(a)
                if a.chars().count() <= MAX_APP_NAME_CHARS && !a.chars().any(char::is_control) =>
            {
                Some(a)
            }
            Some(_) => return None,
            None => None,
        };
        let has_content = !self.prompt.trim().is_empty() || !self.response.trim().is_empty();
        if !has_content
            || self.prompt.len() > MAX_TEXT_BYTES
            || self.response.len() > MAX_TEXT_BYTES
        {
            return None;
        }
        let draft = Self {
            profile,
            prompt: self.prompt,
            response: self.response,
            app_name,
        };
        // JSON escaping can grow text up to six-fold; refuse here rather than
        // let the SDK fail an admitted operation on its request ceiling.
        (draft.escaped_bytes() + ENVELOPE_RESERVE <= MAX_REQUEST_BYTES).then_some(draft)
    }

    fn escaped_bytes(&self) -> usize {
        serde_json::to_vec(&(&self.prompt, &self.response, &self.app_name))
            .map(|v| v.len())
            .unwrap_or(usize::MAX)
    }

    /// Bytes this scan holds while queued.
    pub fn queued_bytes(&self) -> usize {
        self.prompt.len() + self.response.len() + self.app_name.as_ref().map_or(0, String::len)
    }
}

/// The desktop's reading of a completed scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Allow,
    Block,
    /// Allowed by policy but categorized malicious.
    Alert,
    /// Anything else, including incomplete scans: never treated as Allow.
    Unknown,
}

pub fn verdict(action: &str, category: &str, incomplete: bool) -> Verdict {
    match (action, category) {
        ("block", _) => Verdict::Block,
        _ if incomplete => Verdict::Unknown,
        ("allow", "benign") => Verdict::Allow,
        ("allow", "malicious") => Verdict::Alert,
        _ => Verdict::Unknown,
    }
}

/// One detector flag as the service reported it (unknown names included).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub detector: String,
    pub detected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ContentErrorView {
    pub content_type: Option<String>,
    pub feature: Option<String>,
    pub status: Option<String>,
}

/// A completed scan, redacted to identifiers, verdict fields and flags.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanResultView {
    pub scan_id: String,
    pub report_id: String,
    pub tr_id: Option<String>,
    pub profile_name: Option<String>,
    pub profile_id: Option<String>,
    /// Verbatim service category (unknown values preserved).
    pub category: String,
    /// Verbatim service action (unknown values preserved).
    pub action: String,
    pub verdict: Verdict,
    /// The service reported the scan timed out (incomplete).
    pub timeout: bool,
    /// The service reported a scan error (incomplete).
    pub service_error: bool,
    pub content_errors: Vec<ContentErrorView>,
    pub prompt_detected: Vec<Detection>,
    pub response_detected: Vec<Detection>,
}

fn shown(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_SHOWN_CHARS)
        .collect()
}

fn shown_optional(value: &Optional<String>) -> Option<String> {
    match value {
        Optional::Value(text) => Some(shown(text)),
        _ => None,
    }
}

fn detections<T: Serialize>(value: &Optional<T>) -> Vec<Detection> {
    let Optional::Value(flags) = value else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::to_value(flags) else {
        return Vec::new();
    };
    let mut found: Vec<Detection> = map
        .into_iter()
        .filter_map(|(name, value)| {
            value.as_bool().map(|detected| Detection {
                detector: shown(&name),
                detected,
            })
        })
        .collect();
    found.sort_by(|a, b| a.detector.cmp(&b.detector));
    found
}

impl ScanResultView {
    pub fn from_response(response: &ScanResponse) -> Self {
        // Any reported content-level error makes the scan incomplete, even
        // when the global flags disagree (the schema does not tie them).
        let incomplete = response.timeout || response.error || !response.errors.is_empty();
        Self {
            scan_id: shown(&response.scan_id),
            report_id: shown(&response.report_id),
            tr_id: shown_optional(&response.tr_id),
            profile_name: shown_optional(&response.profile_name),
            profile_id: shown_optional(&response.profile_id),
            category: shown(&response.category),
            action: shown(&response.action),
            verdict: verdict(&response.action, &response.category, incomplete),
            timeout: response.timeout,
            service_error: response.error,
            content_errors: response
                .errors
                .iter()
                .take(32)
                .map(|e| ContentErrorView {
                    content_type: shown_optional(&e.content_type),
                    feature: shown_optional(&e.feature),
                    status: shown_optional(&e.status),
                })
                .collect(),
            prompt_detected: detections(&response.prompt_detected),
            response_detected: detections(&response.response_detected),
        }
    }
}

/// Why a scan did not produce a result (sanitized; no server text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScanFailureKind {
    Unauthorized,
    Forbidden,
    Conflict,
    RateLimited,
    BadRequest,
    Unprocessable,
    ServerError,
    HttpOther,
    Timeout,
    Cancelled,
    Network,
    Redirect,
    MalformedResponse,
    TooLarge,
    InvalidRequest,
    CredentialLocked,
    CredentialDenied,
    CredentialMissing,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanFailure {
    pub kind: ScanFailureKind,
    pub status: Option<u16>,
}

impl ScanFailure {
    fn of(kind: ScanFailureKind) -> Self {
        Self { kind, status: None }
    }

    pub fn from_sdk(error: &scm_sdk::Error) -> Self {
        let status = error.status();
        let kind = match (error.kind(), status) {
            (ErrorKind::Http, Some(401)) => ScanFailureKind::Unauthorized,
            (ErrorKind::Http, Some(403)) => ScanFailureKind::Forbidden,
            (ErrorKind::Http, Some(409)) => ScanFailureKind::Conflict,
            (ErrorKind::Http, Some(429)) => ScanFailureKind::RateLimited,
            (ErrorKind::Http, Some(400)) => ScanFailureKind::BadRequest,
            (ErrorKind::Http, Some(422)) => ScanFailureKind::Unprocessable,
            (ErrorKind::Http, Some(s)) if s >= 500 => ScanFailureKind::ServerError,
            (ErrorKind::Http, _) => ScanFailureKind::HttpOther,
            (ErrorKind::Deadline, _) => ScanFailureKind::Timeout,
            (ErrorKind::Cancelled, _) => ScanFailureKind::Cancelled,
            (ErrorKind::Transport | ErrorKind::Io, _) => ScanFailureKind::Network,
            (ErrorKind::Destination, _) => ScanFailureKind::Redirect,
            (ErrorKind::Decode | ErrorKind::Protocol, _) => ScanFailureKind::MalformedResponse,
            (ErrorKind::Limit, _) => ScanFailureKind::TooLarge,
            (ErrorKind::Validation, _) => ScanFailureKind::InvalidRequest,
            _ => ScanFailureKind::Other,
        };
        Self { kind, status }
    }
}

pub fn outcome_of(error: &scm_sdk::Error) -> Outcome {
    match error.remote_outcome() {
        RemoteOutcome::NotSent => Outcome::NotSent,
        RemoteOutcome::Rejected => Outcome::Rejected,
        _ => Outcome::Unknown,
    }
}

/// What a finished scan job leaves for its record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScanPayload {
    Result(Box<ScanResultView>),
    Failure(ScanFailure),
}

pub type PayloadSlot = Arc<Mutex<Option<ScanPayload>>>;

/// Everything one admitted scan needs at dispatch.
pub(crate) struct ScanJob {
    pub keychain: Arc<dyn CredentialStore>,
    pub session_vault: Arc<SessionVault>,
    pub record: RecordId,
    pub mode: StorageMode,
    pub origin: String,
    pub transport: Arc<NativeTransport>,
    pub session: Session,
    pub draft: ScanDraft,
    pub slot: PayloadSlot,
}

impl ScanJob {
    pub fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    fn leave(&self, payload: ScanPayload) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(payload);
        }
    }

    async fn run(self, claim: Claim) -> Outcome {
        let keychain = Arc::clone(&self.keychain);
        let session_vault = Arc::clone(&self.session_vault);
        let record = self.record.clone();
        let mode = self.mode;
        let read = tokio::task::spawn_blocking(move || match mode {
            StorageMode::Keychain => keychain.get(&record),
            StorageMode::Session => session_vault.get(&record),
        })
        .await
        .unwrap_or(Err(VaultError::Unavailable));
        let secret = match read {
            Ok(secret) => secret,
            Err(error) => {
                self.leave(ScanPayload::Failure(ScanFailure::of(match error {
                    VaultError::Locked => ScanFailureKind::CredentialLocked,
                    VaultError::Denied => ScanFailureKind::CredentialDenied,
                    _ => ScanFailureKind::CredentialMissing,
                })));
                return Outcome::NotSent;
            }
        };
        let key = match std::str::from_utf8(&secret) {
            Ok(text) => Zeroizing::new(text.to_owned()),
            Err(_) => {
                self.leave(ScanPayload::Failure(ScanFailure::of(
                    ScanFailureKind::CredentialMissing,
                )));
                return Outcome::NotSent;
            }
        };
        drop(secret);
        if claim.cancel.is_cancelled() {
            self.leave(ScanPayload::Failure(ScanFailure::of(
                ScanFailureKind::Cancelled,
            )));
            return Outcome::NotSent;
        }
        let transport: Arc<dyn scm_sdk::transport::Transport> = self.transport.clone();
        let prepared = (|| {
            let binding =
                ScanBinding::with_session(&self.origin, key, transport, self.session.clone())
                    .ok()?;
            let scanner = Scanner::new(binding.client().clone()).ok()?;
            let profile = match &self.draft.profile {
                ProfileSelector::Name { name } => {
                    AiProfile::builder().profile_name(Optional::Value(name.clone()))
                }
                ProfileSelector::Id { id } => {
                    AiProfile::builder().profile_id(Optional::Value(id.clone()))
                }
            }
            .build_validated(Mode::Request)
            .ok()?;
            let mut content = Content::builder();
            if !self.draft.prompt.is_empty() {
                content = content.prompt(self.draft.prompt.clone());
            }
            if !self.draft.response.is_empty() {
                content = content.response(self.draft.response.clone());
            }
            let content = content.build().ok()?;
            let mut options = SyncScanOptions::default();
            if let Some(app) = &self.draft.app_name {
                options.metadata = Some(
                    Metadata::builder()
                        .app_name(Optional::Value(app.clone()))
                        .build()
                        .ok()?,
                );
            }
            let remaining = claim.deadline.saturating_duration_since(Instant::now());
            options.request = Some(ScanBinding::request_options(remaining).ok()?);
            Some((scanner, profile, content, options))
        })();
        let Some((scanner, profile, content, options)) = prepared else {
            self.leave(ScanPayload::Failure(ScanFailure::of(
                ScanFailureKind::InvalidRequest,
            )));
            return Outcome::NotSent;
        };
        let token = options.request.as_ref().map(|r| r.cancellation_token());
        let scan = scanner.sync_scan(&profile, &content, Some(&options));
        tokio::pin!(scan);
        let result = tokio::select! {
            result = &mut scan => result,
            () = claim.cancel.cancelled() => {
                if let Some(token) = &token {
                    token.cancel();
                }
                scan.await
            }
        };
        match result {
            Ok(response) => {
                self.leave(ScanPayload::Result(Box::new(
                    ScanResultView::from_response(&response),
                )));
                Outcome::Completed
            }
            Err(error) => {
                self.leave(ScanPayload::Failure(ScanFailure::from_sdk(&error)));
                outcome_of(&error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(prompt: &str, response: &str) -> ScanDraft {
        ScanDraft {
            profile: ProfileSelector::Name {
                name: "Production".into(),
            },
            prompt: prompt.into(),
            response: response.into(),
            app_name: Some(" Desktop ".into()),
        }
    }

    #[test]
    fn verdicts_follow_v1_and_never_default_to_allow() {
        assert_eq!(verdict("block", "malicious", false), Verdict::Block);
        assert_eq!(verdict("block", "benign", true), Verdict::Block);
        assert_eq!(verdict("allow", "benign", false), Verdict::Allow);
        assert_eq!(verdict("allow", "malicious", false), Verdict::Alert);
        assert_eq!(
            verdict("allow", "benign", true),
            Verdict::Unknown,
            "incomplete"
        );
        assert_eq!(verdict("future-action", "benign", false), Verdict::Unknown);
        assert_eq!(verdict("allow", "future-category", false), Verdict::Unknown);
        assert_eq!(verdict("ALLOW", "benign", false), Verdict::Unknown);
    }

    #[test]
    fn drafts_need_content_a_valid_profile_and_bounded_text() {
        let ok = draft("hello", "").validate().expect("valid");
        assert_eq!(ok.app_name.as_deref(), Some("Desktop"));
        assert!(draft("  ", " ").validate().is_none(), "content required");
        assert!(
            draft(&"x".repeat(MAX_TEXT_BYTES + 1), "")
                .validate()
                .is_none()
        );
        assert!(
            draft(&"x".repeat(MAX_TEXT_BYTES), &"y".repeat(MAX_TEXT_BYTES))
                .validate()
                .is_some()
        );
        // Control characters escape to six bytes each, past the ceiling.
        assert!(
            draft(&"\u{1}".repeat(MAX_TEXT_BYTES), "")
                .validate()
                .is_none()
        );
        let mut named = draft("x", "");
        named.profile = ProfileSelector::Name {
            name: "n".repeat(101),
        };
        assert!(named.validate().is_none());
        let mut by_id = draft("x", "");
        by_id.profile = ProfileSelector::Id {
            id: "0123ABCD-0123-4567-89ab-0123456789AB".into(),
        };
        assert_eq!(
            by_id.validate().map(|d| d.profile),
            Some(ProfileSelector::Id {
                id: "0123abcd-0123-4567-89ab-0123456789ab".into()
            })
        );
        let mut bad_id = draft("x", "");
        bad_id.profile = ProfileSelector::Id {
            id: "not-a-uuid".into(),
        };
        assert!(bad_id.validate().is_none());
        let mut long_app = draft("x", "");
        long_app.app_name = Some("a".repeat(257));
        assert!(long_app.validate().is_none());
    }

    #[test]
    fn drafts_never_format_their_text() {
        assert!(!format!("{:?}", draft("secret prompt", "")).contains("secret"));
    }
}
