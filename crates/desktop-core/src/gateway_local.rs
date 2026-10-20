//! Gateway configuration work kept on this Mac (C17, T23): each tenant's
//! saved change drafts and the records of changes that settled uncertain.
//! One owner-only file per tenant under the sessions root (named by a
//! digest of the tenant ID; removed with the tenant and by a reset). A
//! draft carries only its workspace label, the change it describes and the
//! editor text (credentials are refused in drafts, as in review); a record
//! carries what the operator was told when it settled. Neither carries a
//! review, Apply authority, a reply or a one-time key, so nothing here can
//! be sent: after a restart each is read and edited locally, and a retry
//! is a fresh review of the draft text against the service as it is then.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::Readback;
use crate::changes::{ChangeAction, ChangeKind, MAX_BODY_BYTES};
use crate::gateway::ListedWorkspace;
use crate::gateway_changes::Provisioning;
use crate::runtime::Outcome;
use crate::sessions::ReportStoreFailure;

/// Schema of a tenant's Gateway local file.
pub const LOCAL_SCHEMA: &str = "prisma-airs-desktop-v2/gateway-local/1";
/// Drafts and records kept per tenant (the oldest go first).
pub const MAX_DRAFTS: usize = 32;
pub const MAX_RECORDS: usize = 32;
/// Largest file.
pub const LOCAL_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ID: usize = 128;

/// What a draft or record edits: its workspace (none for a tenant-wide
/// change), kind, action and target.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayTarget {
    pub workspace: Option<ListedWorkspace>,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    /// The item's ID (empty for a create).
    pub target: String,
}

impl GatewayTarget {
    /// Same change: the same workspace, kind, action and target.
    pub fn same(&self, other: &Self) -> bool {
        self.workspace.as_ref().map(|w| &w.id) == other.workspace.as_ref().map(|w| &w.id)
            && (self.kind, self.action) == (other.kind, other.action)
            && self.target == other.target
    }

    fn valid(&self) -> bool {
        self.kind.gateway().is_some()
            && crate::gateway_changes::allowed(self.kind, self.action)
            && self.target.len() <= MAX_ID
            && self
                .workspace
                .as_ref()
                .is_none_or(|w| crate::gateway::is_id(&w.id) && w.name.len() <= 256)
    }
}

/// An unreviewed change draft saved on this Mac.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayDraft {
    pub id: String,
    #[serde(flatten)]
    pub change: GatewayTarget,
    /// The editor text as the operator left it.
    pub body: String,
    /// When it was saved (RFC 3339, UTC).
    pub saved_at: String,
}

/// A change that settled uncertain: what was known when it settled. The
/// service has not been read since; reading it again shows what exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayRecord {
    pub id: String,
    #[serde(flatten)]
    pub change: GatewayTarget,
    /// The item's name when it was known.
    pub name: Option<String>,
    /// The reviewed draft text (to review again explicitly).
    pub body: String,
    pub outcome: Outcome,
    pub readback: Option<Readback>,
    pub provisioning: Option<Provisioning>,
    pub settled_at: String,
}

/// A tenant's file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LocalBook {
    pub schema: String,
    /// Newest first.
    pub drafts: Vec<GatewayDraft>,
    /// Newest first.
    pub records: Vec<GatewayRecord>,
}

impl Default for LocalBook {
    fn default() -> Self {
        Self {
            schema: LOCAL_SCHEMA.to_owned(),
            drafts: Vec::new(),
            records: Vec::new(),
        }
    }
}

/// Why a draft could not be kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum GatewayDraftProblem {
    /// Not a Gateway change this desktop makes.
    Unsupported,
    /// Larger than a change body may be.
    TooLarge,
    /// It names a credential field; credentials are never kept in drafts.
    Credential,
    /// It is not a complete document yet (kept in the editor only).
    Incomplete,
}

/// Credential fields a draft may never hold (as review refuses them).
fn holds_credentials(body: &serde_json::Value) -> bool {
    body.as_object()
        .is_some_and(|m| m.contains_key("key") || m.contains_key("secret_mappings"))
        || crate::gateway::has_credentials(body)
}

/// Checks a draft's change and text. Only text that reads as a complete
/// document is kept, so the credential check sees every field: text still
/// being typed (which may hold a credential the check cannot see) stays in
/// the editor and is never written to this Mac.
pub fn check(change: &GatewayTarget, body: &str) -> Result<(), GatewayDraftProblem> {
    if !change.valid() {
        return Err(GatewayDraftProblem::Unsupported);
    }
    if body.len() > MAX_BODY_BYTES {
        return Err(GatewayDraftProblem::TooLarge);
    }
    if body.trim().is_empty() {
        return Ok(());
    }
    let parsed = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|_| GatewayDraftProblem::Incomplete)?;
    if holds_credentials(&parsed) {
        return Err(GatewayDraftProblem::Credential);
    }
    Ok(())
}

impl LocalBook {
    /// Saves a draft (replacing the draft for the same change, or the one
    /// with its ID); the newest first, bounded.
    pub fn save(&mut self, draft: GatewayDraft) {
        self.drafts
            .retain(|d| d.id != draft.id && !d.change.same(&draft.change));
        self.drafts.insert(0, draft);
        self.drafts.truncate(MAX_DRAFTS);
    }

    /// Removes a draft or record by ID; true when one was removed.
    pub fn discard(&mut self, id: &str) -> bool {
        let before = self.drafts.len() + self.records.len();
        self.drafts.retain(|d| d.id != id);
        self.records.retain(|r| r.id != id);
        self.drafts.len() + self.records.len() != before
    }

    /// A change settled: one that changed something drops its draft (an
    /// uncertain one is recorded with the reviewed text instead); one that
    /// sent nothing or was refused keeps it for another review.
    pub fn settled(&mut self, record: Option<GatewayRecord>, change: &GatewayTarget, sent: bool) {
        if sent {
            self.drafts.retain(|d| !d.change.same(change));
        }
        if let Some(record) = record {
            self.records.insert(0, record);
            self.records.truncate(MAX_RECORDS);
        }
    }

    fn valid(&self) -> bool {
        self.schema == LOCAL_SCHEMA
            && self.drafts.len() <= MAX_DRAFTS
            && self.records.len() <= MAX_RECORDS
            && self
                .drafts
                .iter()
                .all(|d| d.id.len() <= MAX_ID && check(&d.change, &d.body).is_ok())
            && self.records.iter().all(|r| {
                r.id.len() <= MAX_ID && r.change.valid() && check(&r.change, &r.body).is_ok()
            })
    }
}

/// Where a tenant's Gateway work lives under the sessions root.
pub fn path(root: &Path, tenant: &str) -> PathBuf {
    let digest = crate::offline::digest(&format!("gateway-local:{tenant}"));
    root.join("gateway").join(format!("{}.json", &digest[..32]))
}

/// Reads a tenant's Gateway work (empty when there is no file yet). A file
/// that fails the checks is reported and kept, never reset.
pub fn load(path: &Path) -> Result<LocalBook, ReportStoreFailure> {
    let Some(bytes) = crate::sessions::read_bounded(path, LOCAL_FILE_BYTES)
        .map_err(|_| ReportStoreFailure::Unavailable)?
    else {
        return Ok(LocalBook::default());
    };
    let book: LocalBook =
        serde_json::from_slice(&bytes).map_err(|_| ReportStoreFailure::Unreadable)?;
    book.valid()
        .then_some(book)
        .ok_or(ReportStoreFailure::Unreadable)
}

/// Replaces a tenant's file (0600 in a 0700 directory, atomic).
pub fn store(path: &Path, book: &LocalBook) -> Result<(), ReportStoreFailure> {
    let bytes = serde_json::to_vec(book).map_err(|_| ReportStoreFailure::Unavailable)?;
    if bytes.len() as u64 > LOCAL_FILE_BYTES {
        return Err(ReportStoreFailure::Unavailable);
    }
    crate::sessions::write_private(path, &bytes).map_err(|_| ReportStoreFailure::Unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(kind: ChangeKind, action: ChangeAction, target: &str) -> GatewayTarget {
        GatewayTarget {
            workspace: Some(ListedWorkspace {
                id: "16f7e90d-382a-4e78-b577-1b01eb5f8297".into(),
                name: "Team A".into(),
                slug: "team-a".into(),
                scope_name: "scope-team-a".into(),
            }),
            kind,
            action,
            target: target.into(),
        }
    }

    fn draft(id: &str, change: GatewayTarget, body: &str) -> GatewayDraft {
        GatewayDraft {
            id: id.into(),
            change,
            body: body.into(),
            saved_at: "2026-10-20T00:00:00Z".into(),
        }
    }

    #[test]
    fn drafts_are_checked_replaced_per_change_and_bounded() {
        let edit = change(ChangeKind::GatewayProviders, ChangeAction::Update, "p1");
        assert_eq!(check(&edit, r#"{"note": "n"}"#), Ok(()));
        assert_eq!(
            check(&edit, r#"{"key": "sk-x"}"#),
            Err(GatewayDraftProblem::Credential)
        );
        assert_eq!(
            check(&edit, &"x".repeat(MAX_BODY_BYTES + 1)),
            Err(GatewayDraftProblem::TooLarge)
        );
        assert_eq!(
            check(
                &change(ChangeKind::Profiles, ChangeAction::Update, "p1"),
                "{}"
            ),
            Err(GatewayDraftProblem::Unsupported)
        );
        // Text still being typed is never kept: the check cannot see
        // every field of it.
        assert_eq!(
            check(&edit, "{ \"note\": "),
            Err(GatewayDraftProblem::Incomplete)
        );
        assert_eq!(check(&edit, "  "), Ok(()));
        let mut book = LocalBook::default();
        book.save(draft("d1", edit.clone(), "{}"));
        book.save(draft("d2", edit.clone(), r#"{"note": "m"}"#));
        assert_eq!(book.drafts.len(), 1, "one draft per change");
        assert_eq!(book.drafts[0].id, "d2");
        for i in 0..40 {
            book.save(draft(
                &format!("x{i}"),
                change(
                    ChangeKind::GatewayProviders,
                    ChangeAction::Update,
                    &format!("p{i}x"),
                ),
                "{}",
            ));
        }
        assert_eq!(book.drafts.len(), MAX_DRAFTS);
        assert!(book.valid());
    }

    #[test]
    fn credential_text_is_never_kept_whole_truncated_nested_or_escaped() {
        let edit = change(ChangeKind::GatewayIntegrations, ChangeAction::Update, "i1");
        let canary = "CANARY-SECRET-7f3a";
        for body in [
            format!(r#"{{"key": "{canary}"}}"#),
            format!(r#"{{"secret_mappings": {{"a": "{canary}"}}}}"#),
            format!(r#"{{"config": {{"headers": {{"x-api-key": "{canary}"}}}}}}"#),
            format!(r#"{{"\u006bey": "{canary}"}}"#),
            format!(r#"{{"config": {{"\u0061pi_key": "{canary}"}}}}"#),
        ] {
            assert_eq!(
                check(&edit, &body),
                Err(GatewayDraftProblem::Credential),
                "{body}"
            );
        }
        for body in [
            format!(r#"{{"key":"{canary}","#),
            format!(r#"{{"key": "{canary}"#),
            format!(r#"{{"config": {{"api_key": "{canary}"}}"#),
            format!(r#"{{"\u006bey": "{canary}""#),
            format!(r#"{{"note": "n"}} {{"key": "{canary}"}}"#),
        ] {
            assert_eq!(
                check(&edit, &body),
                Err(GatewayDraftProblem::Incomplete),
                "{body}"
            );
        }
        // A file holding such text is refused on read, never restored.
        let dir = std::env::temp_dir().join(format!(
            "airs-gateway-local-canary-{}",
            crate::offline::digest(&format!("{:?}", std::time::SystemTime::now()))
        ));
        let file = path(&dir, "tenant-1");
        let planted = LocalBook {
            drafts: vec![draft("d1", edit, &format!(r#"{{"key":"{canary}","#))],
            ..LocalBook::default()
        };
        std::fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        std::fs::write(&file, serde_json::to_vec(&planted).expect("json")).expect("planted");
        assert_eq!(load(&file), Err(ReportStoreFailure::Unreadable));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sent_change_drops_its_draft_and_an_unsent_one_keeps_it() {
        let edit = change(ChangeKind::GatewayProviders, ChangeAction::Update, "p1");
        let mut book = LocalBook::default();
        book.save(draft("d1", edit.clone(), "{}"));
        book.settled(None, &edit, false);
        assert_eq!(book.drafts.len(), 1);
        let record = GatewayRecord {
            id: "r1".into(),
            change: edit.clone(),
            name: Some("openai-prod".into()),
            body: "{}".into(),
            outcome: Outcome::Unknown,
            readback: None,
            provisioning: None,
            settled_at: "2026-10-20T00:00:00Z".into(),
        };
        book.settled(Some(record), &edit, true);
        assert!(book.drafts.is_empty());
        assert_eq!(book.records.len(), 1);
        assert!(book.discard("r1") && !book.discard("r1"));
    }

    #[test]
    fn files_round_trip_owner_only_and_a_bad_file_is_kept() {
        let dir = std::env::temp_dir().join(format!(
            "airs-gateway-local-{}",
            crate::offline::digest(&format!("{:?}", std::time::SystemTime::now()))
        ));
        let file = path(&dir, "tenant-1");
        assert_eq!(load(&file), Ok(LocalBook::default()));
        let mut book = LocalBook::default();
        book.save(draft(
            "d1",
            change(ChangeKind::GatewayRoutes, ChangeAction::Update, "r1"),
            r#"{"config": {}}"#,
        ));
        store(&file, &book).expect("stored");
        assert_eq!(load(&file), Ok(book));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file).expect("file").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::write(
            &file,
            br#"{"schema": "other", "drafts": [], "records": []}"#,
        )
        .expect("planted");
        assert_eq!(load(&file), Err(ReportStoreFailure::Unreadable));
        assert!(file.exists(), "kept, not reset");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
