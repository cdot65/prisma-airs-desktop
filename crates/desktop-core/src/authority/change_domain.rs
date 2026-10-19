//! Reviewed Runtime changes inside the authority (C09). A captured draft is
//! normalized in Rust and its baseline read (v1's bounded walk) into an
//! immutable review bound to the context, tenant, management client, draft
//! and baseline digest. RequestApply compares freshly captured input with
//! the stored draft and opens the native operator confirmation; only its
//! private completion admits the write, which re-reads the baseline first
//! and refuses on any change (v1 conflict). Results keep their remote
//! outcome and an operation-specific readback; a one-time API key stays in
//! a zeroizing buffer here and is shown only by the native secret sheet.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::time::Instant;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::{ConfirmationRequest, Refusal, State};
use crate::changes::{
    ChangeAction, ChangeDraft, ChangeKind, ChangeProblem, Mutation, confirmation, digest, impact,
    item_name, normalize, preview_after, take_secret, with_auth_code,
};
use crate::context::{ContextRef, ReviewId};
use crate::management::{
    BindingSource, ManagementBinding, ManagementFailure, ManagementResource, PageError, Walk,
    outcome_of,
};
use crate::runtime::{Claim, OperationId, OperationState, Outcome, Work};
use crate::tenants::TenantId;
use crate::vault::SessionVault;

/// How long a prepared review may be applied (the default of
/// `CoreConfig::review_lifetime`).
pub const REVIEW_LIFETIME: Duration = Duration::from_secs(15 * 60);
/// Settled change results kept for the active context.
const RETAINED_RESULTS: usize = 16;

/// Why a change did not apply (sanitized; no server text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChangeFailure {
    /// The draft or its checks (v1 validation and key creation rules).
    Problem { problem: ChangeProblem },
    /// The item changed since review, or the service reported a conflict.
    Conflict,
    /// The service or the management credential.
    Service { failure: ManagementFailure },
    /// The review expired before it was confirmed; review it again.
    Expired,
    /// The desktop could not start the change (its work queue was full);
    /// nothing was sent. Review it again.
    Capacity,
}

impl From<ChangeProblem> for ChangeFailure {
    fn from(problem: ChangeProblem) -> Self {
        Self::Problem { problem }
    }
}

impl From<PageError> for ChangeFailure {
    fn from(error: PageError) -> Self {
        match error {
            PageError::Sdk(error) => Self::Service {
                failure: ManagementFailure::from_sdk(&error),
            },
            PageError::Invalid => Self::Service {
                failure: ManagementFailure::InvalidResponse,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeReviewState {
    /// The baseline is being read.
    Preparing,
    Ready,
    /// The native confirmation is open; the editor is held.
    Confirming,
    /// Older than [`REVIEW_LIFETIME`]: it can no longer be applied.
    Expired,
    Failed,
}

/// What the readback after a write found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Readback {
    Found,
    /// Not in the complete inventory (expected after a delete).
    Absent,
    /// The bounded walk ended before the inventory did.
    Incomplete,
    /// The readback itself failed; the write's outcome stands.
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SecretState {
    /// The change returned no one-time secret.
    None,
    /// A one-time API key is held in memory for an explicit reveal.
    Held,
    /// The operator cleared it (or it was never kept).
    Forgotten,
    /// It was handed to the native sheet once and is no longer held here.
    Shown,
}

/// The review the presentation may show (no secrets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangeReviewView {
    pub review: ReviewId,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub target: String,
    pub state: ChangeReviewState,
    pub failure: Option<ChangeFailure>,
    /// The item as read for review (JSON, secret fields removed).
    pub before: Option<String>,
    /// The expected result (JSON, server-determined fields labelled).
    pub after: Option<String>,
    /// For a key version: the existing key it is added beside.
    pub source_key: Option<String>,
    pub impact: String,
    /// What the operator types to confirm ("CREATE" or the item's name).
    pub confirmation: Option<String>,
}

/// One applied change as the presentation may show it (never the secret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangeResultView {
    pub operation: OperationId,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub target: String,
    pub name: Option<String>,
    pub settled: bool,
    pub outcome: Option<Outcome>,
    pub failure: Option<ChangeFailure>,
    /// The service's reply (JSON, secret fields removed).
    pub response: Option<String>,
    pub readback: Option<Readback>,
    /// The item as read back (JSON, secret fields removed).
    pub current: Option<String>,
    pub secret: SecretState,
}

/// Why a chosen dictionary file was not staged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StagedFileFailure {
    /// Over v1's 64 Ki UTF-16 units (or the read bound).
    TooLarge,
    /// Not UTF-8 text.
    NotText,
    Empty,
    /// The file could not be read (not a regular file, permissions).
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StagedFileState {
    /// The native file sheet is open.
    Choosing,
    Ready,
    Failed,
}

/// A keyword file the operator chose in the native sheet, read once by the
/// desktop and referenced from a dictionary draft by its token (the source
/// file is never written; the renderer never sees a path).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StagedFileView {
    pub state: StagedFileState,
    /// Use as `fileRef` in a dictionary draft.
    pub token: Option<String>,
    /// Display name only (no directory).
    pub name: Option<String>,
    pub bytes: Option<u32>,
    pub lines: Option<u32>,
    pub failure: Option<StagedFileFailure>,
}

pub(super) struct StagedFile {
    token: String,
    context: ContextRef,
    name: String,
    text: String,
}

pub(super) enum Staging {
    Choosing { context: ContextRef, lease: u64 },
    Ready(StagedFile),
    Failed(StagedFileFailure),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangesView {
    pub review: Option<ChangeReviewView>,
    /// The dictionary keyword file chosen in the native sheet, if any.
    pub staged: Option<StagedFileView>,
    /// The active context's applied changes, newest first.
    pub results: Vec<ChangeResultView>,
}

/// What a baseline read produced.
#[derive(Clone, Debug)]
struct Prepared {
    /// Ready to send (a key create carries its auth code).
    mutation: Mutation,
    before: Option<Value>,
    /// v1 review digest: tenant, client, mutation and baseline.
    digest: String,
}

type PrepareSlot = Arc<Mutex<Option<Result<Prepared, ChangeFailure>>>>;

#[derive(Debug)]
enum Phase {
    Preparing {
        operation: OperationId,
        slot: PrepareSlot,
    },
    Ready(Box<Prepared>),
    Confirming {
        nonce: u64,
        prepared: Box<Prepared>,
    },
    Failed(ChangeFailure),
}

pub(super) struct ChangeReview {
    id: ReviewId,
    context: ContextRef,
    tenant: TenantId,
    /// The normalized draft before any server-derived field.
    draft: Mutation,
    phase: Phase,
    created: Instant,
}

/// What an applied change produced.
struct ApplyOutput {
    outcome: Outcome,
    failure: Option<ChangeFailure>,
    response: Option<String>,
    readback: Option<Readback>,
    current: Option<String>,
    secret: Option<Zeroizing<String>>,
}

type ApplySlot = Arc<Mutex<Option<ApplyOutput>>>;

pub(super) struct ChangeRecord {
    operation: OperationId,
    context: ContextRef,
    kind: ChangeKind,
    action: ChangeAction,
    target: String,
    name: Option<String>,
    slot: ApplySlot,
    output: Option<ApplyOutput>,
    secret: SecretState,
}

fn review_digest(
    tenant: &TenantId,
    client_id: &str,
    mutation: &Mutation,
    before: Option<&Value>,
) -> String {
    let baseline = digest(before.unwrap_or(&Value::Null));
    digest(&json!({
        "tenant": tenant.as_str(),
        "identity": client_id,
        "kind": mutation.kind,
        "action": mutation.action,
        "target": mutation.target,
        "body": mutation.body,
        "baseline": baseline,
    }))
}

fn nonce() -> Option<u64> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

impl State {
    /// Prepares a review of `draft` for the active tenant's management
    /// client. A draft that fails v1's checks yields a failed review the
    /// operator can read (no operation); otherwise the baseline read is
    /// admitted as an operation.
    pub(super) fn review_change(
        &mut self,
        context: ContextRef,
        draft: ChangeDraft,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(Option<OperationId>, Vec<crate::runtime::Start>), Refusal> {
        let tenant = self.check_context(&context)?;
        let source = self.binding_source(&context, keychain, session_vault)?;
        // A newly captured draft invalidates a pending confirmation: its sheet
        // is ended and its late completion can no longer apply anything.
        if let Some(Phase::Confirming { nonce, .. }) = self.change_review.as_ref().map(|r| &r.phase)
        {
            self.confirmer.dismiss(*nonce);
        }
        let id = ReviewId::issue().ok_or(Refusal::ShuttingDown)?;
        let resolved = self.resolve_staged(&context, &draft);
        let draft = match resolved.and_then(|d| normalize(&d, &source.client_id)) {
            Ok(mutation) => mutation,
            Err(problem) => {
                // Kept so the operator can see why; nothing is read or sent.
                self.change_review = Some(ChangeReview {
                    id,
                    context,
                    tenant,
                    draft: Mutation {
                        kind: draft.kind,
                        action: draft.action,
                        target: draft.target,
                        body: Value::Null,
                    },
                    phase: Phase::Failed(problem.into()),
                    created: Instant::now(),
                });
                return Ok((None, Vec::new()));
            }
        };
        let slot: PrepareSlot = Arc::default();
        let job = PrepareJob {
            source,
            tenant: tenant.clone(),
            draft: draft.clone(),
            slot: Arc::clone(&slot),
        };
        let (operation, starts) =
            self.operations
                .admit(context.as_str(), 0, job.into_work(), Instant::now())?;
        self.change_review = Some(ChangeReview {
            id,
            context,
            tenant,
            draft,
            phase: Phase::Preparing {
                operation: operation.clone(),
                slot,
            },
            created: Instant::now(),
        });
        Ok((Some(operation), starts))
    }

    /// RequestApply: the freshly captured `draft` and typed `confirmation`
    /// must equal the stored review's; then the review is held for the
    /// native confirmation `nonce`.
    pub(super) fn request_apply(
        &mut self,
        review: &ReviewId,
        draft: &ChangeDraft,
        typed: &str,
    ) -> Result<(u64, ConfirmationRequest), Refusal> {
        if self.consumed_reviews.contains(review) {
            return Err(Refusal::ReviewConsumed);
        }
        let active = self.active_ref.clone();
        let lifetime = self.review_lifetime;
        let label = self
            .change_review
            .as_ref()
            .and_then(|r| self.tenant(&r.tenant))
            .map(|t| t.profile.label.clone())
            .unwrap_or_default();
        let current = self
            .change_review
            .as_mut()
            .filter(|r| &r.id == review)
            .ok_or(Refusal::ReviewInvalid)?;
        if active.as_ref() != Some(&current.context) || current.created.elapsed() > lifetime {
            return Err(Refusal::ReviewInvalid);
        }
        let prepared = match &current.phase {
            Phase::Ready(prepared) => prepared.clone(),
            Phase::Confirming { .. } => return Err(Refusal::Busy),
            _ => return Err(Refusal::ReviewInvalid),
        };
        // Fresh input must be exactly what was reviewed (client authorship
        // is re-applied, so the comparison is of normalized drafts).
        let author = current
            .draft
            .body
            .get(match current.draft.action {
                ChangeAction::Create | ChangeAction::Version => "created_by",
                _ => "updated_by",
            })
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let context = current.context.clone();
        let fresh = match self.resolve_staged(&context, draft) {
            Ok(draft) => normalize(&draft, &author).map_err(|_| Refusal::ReviewInvalid)?,
            Err(_) => return Err(Refusal::ReviewInvalid),
        };
        let Some(current) = self.change_review.as_mut().filter(|r| &r.id == review) else {
            return Err(Refusal::ReviewInvalid);
        };
        if fresh != current.draft {
            return Err(Refusal::ReviewInvalid);
        }
        let expected = confirmation(&prepared.mutation, prepared.before.as_ref());
        if expected.as_deref() != Some(typed) {
            return Err(Refusal::InvalidInput);
        }
        let nonce = nonce().ok_or(Refusal::ShuttingDown)?;
        let request = confirmation_request(&label, &prepared);
        current.phase = Phase::Confirming { nonce, prepared };
        Ok((nonce, request))
    }

    /// The native confirmation's private completion. Only the open
    /// confirmation `nonce` of a current review is honored, once; a denial
    /// returns the review to ready (another RequestApply is needed).
    pub(super) fn settle_confirmation(
        &mut self,
        review: &ReviewId,
        nonce: u64,
        accepted: bool,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Vec<crate::runtime::Start> {
        let active = self.active_ref.clone();
        let Some(current) = self.change_review.as_mut().filter(|r| &r.id == review) else {
            return Vec::new();
        };
        if !matches!(&current.phase, Phase::Confirming { nonce: n, .. } if *n == nonce)
            || active.as_ref() != Some(&current.context)
        {
            return Vec::new();
        }
        let expired = current.created.elapsed() > self.review_lifetime;
        let Phase::Confirming { prepared, .. } =
            std::mem::replace(&mut current.phase, Phase::Failed(ChangeFailure::Conflict))
        else {
            return Vec::new();
        };
        // A denial, or a confirmation that outlived the review, applies
        // nothing; the review stays (an expired one shows as expired).
        if !accepted || expired {
            current.phase = Phase::Ready(prepared);
            return Vec::new();
        }
        let (context, tenant, draft) = (
            current.context.clone(),
            current.tenant.clone(),
            current.draft.clone(),
        );
        // Admission and consumption are one step: if the change cannot be
        // started, the review is not consumed and the operator sees why.
        let source = match self.binding_source(&context, keychain, session_vault) {
            Ok(source) => source,
            Err(_) => {
                if let Some(r) = self.change_review.as_mut() {
                    r.phase = Phase::Failed(ChangeFailure::Service {
                        failure: ManagementFailure::CredentialMissing,
                    });
                }
                return Vec::new();
            }
        };
        let slot: ApplySlot = Arc::default();
        let name = item_name(draft.kind, &prepared.mutation.body).or_else(|| {
            prepared
                .before
                .as_ref()
                .and_then(|b| item_name(draft.kind, b))
        });
        let job = ApplyJob {
            source,
            tenant,
            draft: draft.clone(),
            prepared: *prepared,
            slot: Arc::clone(&slot),
        };
        let (operation, starts) =
            match self
                .operations
                .admit(context.as_str(), 0, job.into_work(), Instant::now())
            {
                Ok(admitted) => admitted,
                Err(_) => {
                    if let Some(r) = self.change_review.as_mut() {
                        r.phase = Phase::Failed(ChangeFailure::Capacity);
                    }
                    return Vec::new();
                }
            };
        if let Some(taken) = self.change_review.take() {
            self.consumed_reviews.push(taken.id);
        }
        self.change_results.insert(
            0,
            ChangeRecord {
                operation,
                context,
                kind: draft.kind,
                action: draft.action,
                target: draft.target,
                name,
                slot,
                output: None,
                secret: SecretState::None,
            },
        );
        starts
    }

    /// Replaces a dictionary draft's `fileRef` with the staged file's text
    /// (the file staged for this context, by its token).
    fn resolve_staged(
        &self,
        context: &ContextRef,
        draft: &ChangeDraft,
    ) -> Result<ChangeDraft, ChangeProblem> {
        if draft.kind != ChangeKind::Dictionaries {
            return Ok(draft.clone());
        }
        let Ok(Value::Object(mut body)) = serde_json::from_str::<Value>(&draft.body) else {
            return Ok(draft.clone());
        };
        let Some(reference) = body.remove("fileRef") else {
            return Ok(draft.clone());
        };
        let staged = match &self.staging {
            Some(Staging::Ready(file))
                if &file.context == context && Some(file.token.as_str()) == reference.as_str() =>
            {
                file
            }
            _ => return Err(ChangeProblem::StagedFile),
        };
        if body.contains_key("file") {
            return Err(ChangeProblem::Body);
        }
        body.insert("file".into(), Value::String(staged.text.clone()));
        Ok(ChangeDraft {
            body: Value::Object(body).to_string(),
            ..draft.clone()
        })
    }

    /// ChooseDictionaryFile: opens the native file sheet (one at a time).
    pub(super) fn choose_dictionary_file(
        &mut self,
        context: ContextRef,
        lease: u64,
    ) -> Result<(), Refusal> {
        self.check_context(&context)?;
        // The native confirmation holds the editor lease: the file a pending
        // change was reviewed with cannot be replaced or cleared meanwhile.
        if matches!(self.staging, Some(Staging::Choosing { .. }))
            || matches!(
                self.change_review.as_ref().map(|r| &r.phase),
                Some(Phase::Confirming { .. })
            )
        {
            return Err(Refusal::Busy);
        }
        self.staging = Some(Staging::Choosing { context, lease });
        Ok(())
    }

    /// The sheet's result: a bounded UTF-8 text file is staged; anything else
    /// is reported. A cancelled sheet keeps nothing.
    pub(super) fn settle_dictionary_file(
        &mut self,
        lease: u64,
        chosen: Option<Result<(String, Vec<u8>), StagedFileFailure>>,
    ) {
        let Some(Staging::Choosing { context, lease: l }) = &self.staging else {
            return;
        };
        if *l != lease || self.active_ref.as_ref() != Some(context) {
            return;
        }
        let context = context.clone();
        // A sheet opened before a confirmation began must not change the file
        // under it: end that confirmation, whose late completion then applies
        // nothing, and require a new review.
        if let Some(review) = self.change_review.as_mut()
            && let Phase::Confirming { nonce, .. } = review.phase
        {
            self.confirmer.dismiss(nonce);
            review.phase = Phase::Failed(ChangeProblem::StagedFile.into());
        }
        self.staging = match chosen {
            None => None,
            Some(Err(failure)) => Some(Staging::Failed(failure)),
            Some(Ok((name, bytes))) => Some(match staged_text(bytes) {
                Err(failure) => Staging::Failed(failure),
                Ok(text) => match nonce() {
                    Some(n) => Staging::Ready(StagedFile {
                        token: format!("sf-{n:016x}"),
                        context,
                        name,
                        text,
                    }),
                    None => Staging::Failed(StagedFileFailure::Unreadable),
                },
            }),
        };
    }

    pub(super) fn discard_change(&mut self, review: &ReviewId) -> Result<(), Refusal> {
        match &self.change_review {
            Some(r) if &r.id == review => {
                if let Phase::Confirming { nonce, .. } = r.phase {
                    self.confirmer.dismiss(nonce);
                }
                self.change_review = None;
                Ok(())
            }
            _ => Err(Refusal::ReviewInvalid),
        }
    }

    /// Hands the held one-time secret of an applied change to the native
    /// sheet, once: the core keeps no copy afterwards, so closing, copying
    /// or the sheet's expiry ends it for good (DESIGN: one reveal).
    pub(super) fn reveal_secret(
        &mut self,
        operation: &OperationId,
    ) -> Result<(String, Zeroizing<String>), Refusal> {
        let active = self.active_ref.clone();
        let record = self
            .change_results
            .iter_mut()
            .find(|r| &r.operation == operation && Some(&r.context) == active.as_ref())
            .ok_or(Refusal::InvalidInput)?;
        let secret = record
            .output
            .as_mut()
            .and_then(|o| o.secret.take())
            .ok_or(Refusal::CapabilityMissing)?;
        record.secret = SecretState::Shown;
        let label = format!(
            "New API key{}",
            record
                .name
                .as_deref()
                .map(|n| format!(" for {n}"))
                .unwrap_or_default()
        );
        Ok((label, secret))
    }

    pub(super) fn forget_secret(&mut self, operation: &OperationId) -> Result<(), Refusal> {
        let record = self
            .change_results
            .iter_mut()
            .find(|r| &r.operation == operation)
            .ok_or(Refusal::InvalidInput)?;
        if let Some(output) = record.output.as_mut() {
            output.secret = None;
        }
        if record.secret == SecretState::Held {
            record.secret = SecretState::Forgotten;
        }
        self.secrets.dismiss();
        Ok(())
    }

    /// Settles prepared reviews and applied changes; drops everything bound
    /// to a retired context (an open confirmation is ended, held secrets
    /// are zeroized with their record).
    pub(super) fn sync_changes(&mut self) {
        let active = self.active_ref.clone();
        if let Some(review) = &mut self.change_review {
            if active.as_ref() != Some(&review.context) {
                if let Phase::Confirming { nonce, .. } = review.phase {
                    self.confirmer.dismiss(nonce);
                }
                self.change_review = None;
            } else if let Phase::Preparing { operation, slot } = &review.phase {
                match self.operations.state(operation) {
                    Some(OperationState::Settled(_)) | None => {
                        let taken = slot.lock().ok().and_then(|mut s| s.take());
                        review.phase = match taken {
                            Some(Ok(prepared)) => Phase::Ready(Box::new(prepared)),
                            Some(Err(failure)) => Phase::Failed(failure),
                            None => Phase::Failed(ChangeFailure::Service {
                                failure: ManagementFailure::Cancelled,
                            }),
                        };
                    }
                    Some(_) => {}
                }
            }
        }
        let stale = match &self.staging {
            Some(Staging::Choosing { context, .. }) => active.as_ref() != Some(context),
            Some(Staging::Ready(file)) => active.as_ref() != Some(&file.context),
            _ => false,
        };
        if stale {
            self.staging = None;
        }
        let before = self.change_results.len();
        self.change_results
            .retain(|r| Some(&r.context) == active.as_ref());
        if self.change_results.len() != before {
            self.secrets.dismiss();
        }
        for record in self
            .change_results
            .iter_mut()
            .filter(|r| r.output.is_none())
        {
            let settled = matches!(
                self.operations.state(&record.operation),
                Some(OperationState::Settled(_)) | None
            );
            if !settled {
                continue;
            }
            let output = record.slot.lock().ok().and_then(|mut s| s.take());
            let output = output.unwrap_or(ApplyOutput {
                outcome: Outcome::Unknown,
                failure: Some(ChangeFailure::Service {
                    failure: ManagementFailure::Cancelled,
                }),
                response: None,
                readback: None,
                current: None,
                secret: None,
            });
            record.secret = if output.secret.is_some() {
                SecretState::Held
            } else {
                SecretState::None
            };
            record.output = Some(output);
        }
        let mut settled = 0;
        self.change_results.retain(|r| {
            if r.output.is_some() {
                settled += 1;
                settled <= RETAINED_RESULTS
            } else {
                true
            }
        });
    }

    pub(super) fn changes_view(&self) -> ChangesView {
        let active = self.active_ref.as_ref();
        let review = self
            .change_review
            .as_ref()
            .filter(|r| Some(&r.context) == active)
            .map(|r| {
                let (state, failure, prepared) = match &r.phase {
                    Phase::Preparing { .. } => (ChangeReviewState::Preparing, None, None),
                    Phase::Ready(p) if r.created.elapsed() > self.review_lifetime => (
                        ChangeReviewState::Expired,
                        Some(ChangeFailure::Expired),
                        Some(p.as_ref()),
                    ),
                    Phase::Ready(p) => (ChangeReviewState::Ready, None, Some(p.as_ref())),
                    Phase::Confirming { prepared, .. } => {
                        (ChangeReviewState::Confirming, None, Some(prepared.as_ref()))
                    }
                    Phase::Failed(f) => (ChangeReviewState::Failed, Some(*f), None),
                };
                let version = r.draft.action == ChangeAction::Version;
                let redacted = |v: &Value| {
                    let mut v = v.clone();
                    crate::management::redact(&mut v);
                    v.to_string()
                };
                ChangeReviewView {
                    review: r.id.clone(),
                    kind: r.draft.kind,
                    action: r.draft.action,
                    target: r.draft.target.clone(),
                    state,
                    failure,
                    before: prepared
                        .filter(|_| !version)
                        .and_then(|p| p.before.as_ref())
                        .map(redacted),
                    after: prepared
                        .and_then(|p| {
                            preview_after(&p.mutation, p.before.as_ref().filter(|_| !version))
                        })
                        .map(|v| v.to_string()),
                    source_key: prepared
                        .filter(|_| version)
                        .and_then(|p| p.before.as_ref())
                        .map(redacted),
                    impact: impact(&r.draft).to_owned(),
                    confirmation: prepared
                        .and_then(|p| confirmation(&p.mutation, p.before.as_ref())),
                }
            });
        let results = self
            .change_results
            .iter()
            .filter(|r| Some(&r.context) == active)
            .map(|r| ChangeResultView {
                operation: r.operation.clone(),
                kind: r.kind,
                action: r.action,
                target: r.target.clone(),
                name: r.name.clone(),
                settled: r.output.is_some(),
                outcome: r.output.as_ref().map(|o| o.outcome),
                failure: r.output.as_ref().and_then(|o| o.failure),
                response: r.output.as_ref().and_then(|o| o.response.clone()),
                readback: r.output.as_ref().and_then(|o| o.readback),
                current: r.output.as_ref().and_then(|o| o.current.clone()),
                secret: r.secret,
            })
            .collect();
        let staged = self.staging.as_ref().map(|s| match s {
            Staging::Choosing { .. } => StagedFileView {
                state: StagedFileState::Choosing,
                token: None,
                name: None,
                bytes: None,
                lines: None,
                failure: None,
            },
            Staging::Ready(file) => StagedFileView {
                state: StagedFileState::Ready,
                token: Some(file.token.clone()),
                name: Some(file.name.clone()),
                bytes: u32::try_from(file.text.len()).ok(),
                lines: u32::try_from(file.text.lines().count()).ok(),
                failure: None,
            },
            Staging::Failed(failure) => StagedFileView {
                state: StagedFileState::Failed,
                token: None,
                name: None,
                bytes: None,
                lines: None,
                failure: Some(*failure),
            },
        });
        ChangesView {
            review,
            staged,
            results,
        }
    }
}

/// A chosen keyword file as text: UTF-8 (a byte-order mark is dropped),
/// non-empty, at most 64 Ki UTF-16 units (v1).
fn staged_text(bytes: Vec<u8>) -> Result<String, StagedFileFailure> {
    let bytes = bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF])
        .map(<[u8]>::to_vec)
        .unwrap_or(bytes);
    let text = String::from_utf8(bytes).map_err(|_| StagedFileFailure::NotText)?;
    if text.contains('\0') {
        return Err(StagedFileFailure::NotText);
    }
    if text.trim().is_empty() {
        return Err(StagedFileFailure::Empty);
    }
    if text.encode_utf16().count() > crate::changes::MAX_DICTIONARY_FILE {
        return Err(StagedFileFailure::TooLarge);
    }
    Ok(text)
}

fn verb(action: ChangeAction) -> &'static str {
    match action {
        ChangeAction::Create => "Create",
        ChangeAction::Update => "Update",
        ChangeAction::Delete => "Delete",
        ChangeAction::Version => "Add a key for",
        ChangeAction::Rotate => "Rotate",
    }
}

fn noun(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Profiles => "security profile",
        ChangeKind::Topics => "custom topic",
        ChangeKind::Applications => "application",
        ChangeKind::ApiKeys => "API key",
        ChangeKind::DataPatterns => "data pattern",
        ChangeKind::DataProfiles => "data profile",
        ChangeKind::FilteringProfiles => "data filtering profile",
        ChangeKind::Dictionaries => "dictionary",
    }
}

/// The native confirmation's text: Rust's stored review, not renderer text.
fn confirmation_request(tenant: &str, prepared: &Prepared) -> ConfirmationRequest {
    let m = &prepared.mutation;
    let name = item_name(m.kind, &m.body)
        .or_else(|| prepared.before.as_ref().and_then(|b| item_name(m.kind, b)))
        .unwrap_or_else(|| m.target.clone());
    let subject = match m.action {
        ChangeAction::Version => format!(
            "application {}",
            m.body["cust_app"].as_str().unwrap_or(&name)
        ),
        _ => format!("{} \u{201c}{name}\u{201d}", noun(m.kind)),
    };
    ConfirmationRequest {
        title: format!("{} {subject}?", verb(m.action)),
        message: format!("Tenant: {tenant}.\n{}", impact(m)),
        confirm_label: match m.action {
            ChangeAction::Delete => "Delete".into(),
            ChangeAction::Rotate => "Rotate key".into(),
            ChangeAction::Create | ChangeAction::Version => "Create".into(),
            ChangeAction::Update => "Apply".into(),
        },
        destructive: matches!(m.action, ChangeAction::Delete | ChangeAction::Rotate),
    }
}

/// The item whose identifier is `id` (v1 bounded walk).
async fn find_id(
    binding: &ManagementBinding,
    resource: ManagementResource,
    id: &str,
    deadline: Instant,
) -> Result<Value, ChangeFailure> {
    let field = resource.id_field();
    match binding
        .walk(resource, deadline, |item| {
            crate::management::identity(item.get(field)).as_deref() == Some(id)
        })
        .await?
    {
        Walk::Found(item) => Ok(item),
        Walk::Ended => Err(ChangeProblem::NotFound.into()),
        Walk::Incomplete => Err(ChangeProblem::InventoryLimit.into()),
    }
}

fn folded(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(|s| s.trim().to_lowercase())
}

/// Fails when an item of `resource` has `field` equal to `wanted`
/// (case-insensitive, v1), or when the inventory could not be read whole.
async fn absent(
    binding: &ManagementBinding,
    resource: ManagementResource,
    field: &str,
    wanted: Option<String>,
    exists: ChangeProblem,
    deadline: Instant,
) -> Result<(), ChangeFailure> {
    match binding
        .walk(resource, deadline, |item| folded(item.get(field)) == wanted)
        .await?
    {
        Walk::Found(_) => Err(exists.into()),
        Walk::Ended => Ok(()),
        Walk::Incomplete => Err(ChangeProblem::InventoryLimit.into()),
    }
}

/// v1's review preparation: key creation checks and auth code, and the
/// baseline (`before`) the review and the apply are bound to.
async fn baseline(
    binding: &ManagementBinding,
    draft: &Mutation,
    deadline: Instant,
) -> Result<(Mutation, Option<Value>), ChangeFailure> {
    use ChangeAction as A;
    let body = &draft.body;
    match (draft.kind, draft.action) {
        (ChangeKind::ApiKeys, A::Create | A::Version) => {
            let source = if draft.action == A::Version {
                let source = find_id(
                    binding,
                    ManagementResource::ApiKeys,
                    &draft.target,
                    deadline,
                )
                .await?;
                if source.get("cust_app") != body.get("cust_app") {
                    return Err(ChangeProblem::SourceMismatch.into());
                }
                Some(source)
            } else {
                absent(
                    binding,
                    ManagementResource::Applications,
                    "app_name",
                    folded(body.get("cust_app")),
                    ChangeProblem::ApplicationExists,
                    deadline,
                )
                .await?;
                None
            };
            absent(
                binding,
                ManagementResource::ApiKeys,
                "api_key_name",
                folded(body.get("api_key_name")),
                ChangeProblem::KeyExists,
                deadline,
            )
            .await?;
            let deployments = binding.deployment_profiles(deadline).await?;
            Ok((with_auth_code(draft, &deployments)?, source))
        }
        (_, A::Create) => Ok((draft.clone(), None)),
        // DLP items are read by their own get (v1 `current`); 404 is absent.
        (kind, _) if kind.is_dlp() => match binding
            .dlp_item(kind.resource(), &draft.target, deadline)
            .await?
        {
            Some(item) => Ok((draft.clone(), Some(item))),
            None => Err(ChangeProblem::NotFound.into()),
        },
        (kind, action) => {
            let mut before = find_id(binding, kind.resource(), &draft.target, deadline).await?;
            if (kind, action) == (ChangeKind::Applications, A::Delete) {
                // The whole matching key inventory is part of the review.
                let app = before.get("app_name").cloned();
                let mut keys = Vec::new();
                let walked = binding
                    .walk(ManagementResource::ApiKeys, deadline, |key| {
                        if key.get("cust_app") == app.as_ref() {
                            keys.push(key.clone());
                        }
                        false
                    })
                    .await?;
                if walked != Walk::Ended {
                    return Err(ChangeProblem::InventoryLimit.into());
                }
                if let Value::Object(map) = &mut before {
                    map.insert("affected_keys".into(), Value::Array(keys));
                }
            }
            Ok((draft.clone(), Some(before)))
        }
    }
}

/// Reads the changed item back (operation-specific).
async fn readback(
    binding: &ManagementBinding,
    mutation: &Mutation,
    before: Option<&Value>,
    response: &Value,
    deadline: Instant,
) -> (Readback, Option<String>) {
    let kind = mutation.kind;
    let resource = kind.resource();
    let id_field = resource.id_field();
    let name_field = kind.name_field();
    let by_name = |name: Option<String>| {
        move |item: &Value| {
            item.get(name_field)
                .and_then(Value::as_str)
                .map(str::to_owned)
                == name
        }
    };
    if kind.is_dlp() {
        // By identifier: the reply's for a create, the target otherwise (a
        // dictionary replace may reply 204 with no body).
        let id = crate::management::identity(response.get("id"))
            .unwrap_or_else(|| mutation.target.clone());
        return match binding.dlp_item(resource, &id, deadline).await {
            Ok(Some(item)) => (Readback::Found, Some(item.to_string())),
            Ok(None) => (Readback::Absent, None),
            Err(_) => (Readback::Failed, None),
        };
    }
    let walked = match mutation.action {
        ChangeAction::Create | ChangeAction::Version => {
            binding
                .walk(resource, deadline, by_name(item_name(kind, &mutation.body)))
                .await
        }
        ChangeAction::Delete if matches!(kind, ChangeKind::Applications | ChangeKind::ApiKeys) => {
            binding
                .walk(
                    resource,
                    deadline,
                    by_name(before.and_then(|b| item_name(kind, b))),
                )
                .await
        }
        _ => {
            // An update may return a new identifier (v1 notes SCM PUT can).
            let id = crate::management::identity(response.get(id_field))
                .unwrap_or_else(|| mutation.target.clone());
            binding
                .walk(resource, deadline, |item| {
                    crate::management::identity(item.get(id_field)).as_deref() == Some(id.as_str())
                })
                .await
        }
    };
    match walked {
        Ok(Walk::Found(item)) => (Readback::Found, Some(item.to_string())),
        Ok(Walk::Ended) => (Readback::Absent, None),
        Ok(Walk::Incomplete) => (Readback::Incomplete, None),
        Err(_) => (Readback::Failed, None),
    }
}

/// The baseline read of one review.
struct PrepareJob {
    source: BindingSource,
    tenant: TenantId,
    draft: Mutation,
    slot: PrepareSlot,
}

impl PrepareJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    async fn run(self, claim: Claim) -> Outcome {
        let result = tokio::select! {
            result = self.prepare(claim.deadline) => result,
            () = claim.cancel.cancelled() => Err(ChangeFailure::Service { failure: ManagementFailure::Cancelled }),
        };
        let outcome = if result.is_ok() {
            Outcome::Completed
        } else {
            Outcome::Unknown
        };
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(result);
        }
        outcome
    }

    async fn prepare(&self, deadline: Instant) -> Result<Prepared, ChangeFailure> {
        let binding = self
            .source
            .bind()
            .await
            .map_err(|failure| ChangeFailure::Service { failure })?;
        let (mutation, before) = baseline(binding, &self.draft, deadline).await?;
        let digest = review_digest(
            &self.tenant,
            &self.source.client_id,
            &mutation,
            before.as_ref(),
        );
        Ok(Prepared {
            mutation,
            before,
            digest,
        })
    }
}

/// One confirmed change: re-read, compare, write once, read back.
struct ApplyJob {
    source: BindingSource,
    tenant: TenantId,
    draft: Mutation,
    prepared: Prepared,
    slot: ApplySlot,
}

impl ApplyJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    fn failed(outcome: Outcome, failure: ChangeFailure) -> ApplyOutput {
        ApplyOutput {
            outcome,
            failure: Some(failure),
            response: None,
            readback: None,
            current: None,
            secret: None,
        }
    }

    async fn run(self, claim: Claim) -> Outcome {
        let output = tokio::select! {
            output = self.apply(claim.deadline) => output,
            // The write may already have reached the service.
            () = claim.cancel.cancelled() => Self::failed(
                Outcome::Unknown,
                ChangeFailure::Service { failure: ManagementFailure::Cancelled },
            ),
        };
        let outcome = output.outcome;
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(output);
        }
        outcome
    }

    async fn apply(&self, deadline: Instant) -> ApplyOutput {
        let binding = match self.source.bind().await {
            Ok(binding) => binding,
            Err(failure) => {
                return Self::failed(Outcome::NotSent, ChangeFailure::Service { failure });
            }
        };
        // v1: the baseline and review digest must still match at apply.
        let fresh = match baseline(binding, &self.draft, deadline).await {
            Ok(fresh) => fresh,
            Err(ChangeFailure::Problem {
                problem: ChangeProblem::NotFound,
            }) => return Self::failed(Outcome::NotSent, ChangeFailure::Conflict),
            Err(failure) => return Self::failed(Outcome::NotSent, failure),
        };
        let digest = review_digest(
            &self.tenant,
            &self.source.client_id,
            &fresh.0,
            fresh.1.as_ref(),
        );
        if digest != self.prepared.digest {
            return Self::failed(Outcome::NotSent, ChangeFailure::Conflict);
        }
        let mutation = &self.prepared.mutation;
        let before = self.prepared.before.as_ref();
        let reply = match binding.write(mutation, before, deadline).await {
            Ok(reply) => reply,
            Err(PageError::Sdk(error)) => {
                let failure = if error.status() == Some(409) {
                    ChangeFailure::Conflict
                } else {
                    ChangeFailure::Service {
                        failure: ManagementFailure::from_sdk(&error),
                    }
                };
                return Self::failed(outcome_of(&error), failure);
            }
            // The service accepted the write but its reply was unusable.
            Err(PageError::Invalid) => {
                return Self::failed(
                    Outcome::Completed,
                    ChangeFailure::Service {
                        failure: ManagementFailure::InvalidResponse,
                    },
                );
            }
        };
        let (response, secret) = take_secret(reply);
        let (readback, current) = readback(binding, mutation, before, &response, deadline).await;
        ApplyOutput {
            outcome: Outcome::Completed,
            failure: None,
            response: Some(response.to_string()),
            readback: Some(readback),
            current,
            secret,
        }
    }
}
