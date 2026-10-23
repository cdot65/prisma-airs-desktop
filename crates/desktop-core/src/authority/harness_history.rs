//! Protected Harness history inside the authority (C20, D047).
//!
//! The chosen Gateway workspace's saved conversations are listed, opened,
//! exported and deleted here, and a saved conversation whose binding is the
//! one a new conversation would have now can be continued (restored into a
//! new engine). Records are read and written off the lock with the
//! Keychain-held scope key; each result applies only while its request is
//! still the current one for that scope.

use std::sync::Arc;

use serde::Serialize;
use ts_rs::TS;

use super::harness_domain::{HistoryPlan, Resume};
use super::harness_identity::now;
use super::sessions_domain::ExportKind;
use super::{Effect, Inner, Refusal, State, lock};
use crate::context::{ContextRef, WorkspaceRef};
use crate::harness::history::{
    self, CredentialSource, HistoryFailure, Listed, Record, SavedBinding, Scope,
};
use crate::harness::session::{HistorySink, Replay};
use crate::harness::{HarnessEntry, HarnessRole, HarnessUsage, protocol};
use crate::store::GatewayPreference;
use crate::tenants::TenantId;

/// One saved conversation in the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SavedConversationView {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub updated: u64,
    pub entries: u32,
    pub source: CredentialSource,
}

/// Why a saved conversation can be read but not continued.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum NotContinuable {
    /// It is the conversation running now.
    Running,
    /// It ran with another key, sign-in, endpoint or credential choice.
    OtherCredential,
    /// It ran with another project folder, or with changes allowed or not
    /// (C21): continuing it needs a new conversation.
    OtherProject,
    /// It ran with other workspace MCP servers or sign-ins (C22), or they
    /// cannot be checked because the catalog is not loaded.
    OtherMcp,
    /// The bundled engine is not available.
    EngineUnavailable,
    /// Read from this Mac without its workspace from AI Gateway (as when it
    /// cannot be reached): it can be read and exported, not continued.
    WorkspaceUnavailable,
    /// Cut short before the engine exported anything to restore: it can be
    /// read and exported, never continued (C22 review R2-F1).
    NotRestorable,
    /// Imported from v1 (C23): its transcript can be read and exported; it
    /// never continues (another engine's replay is never restored).
    Imported,
}

/// A saved conversation opened for reading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenedConversationView {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub updated: u64,
    pub entries: Vec<HarnessEntry>,
    pub source: CredentialSource,
    pub not_continuable: Option<NotContinuable>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HistoryListState {
    Loading,
    Ready,
    Failed,
}

/// The chosen workspace's protected history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessHistoryView {
    /// Read from this Mac for the tenant's last chosen workspace, without
    /// that workspace from AI Gateway (T23): read-only.
    pub local: bool,
    pub state: HistoryListState,
    pub conversations: Vec<SavedConversationView>,
    /// Records present that do not open with this workspace's key.
    pub unreadable: Vec<String>,
    pub failure: Option<HistoryFailure>,
    /// The workspace holds as many saved conversations as it may.
    pub full: bool,
    pub opened: Option<OpenedConversationView>,
    /// Opening or deleting is in progress.
    pub busy: bool,
    /// Why the last open or delete did not happen.
    pub problem: Option<HistoryFailure>,
    /// Saving a transcript in the native sheet.
    pub export: Option<super::sessions_domain::ExportView>,
}

/// Export formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HistoryFormat {
    Markdown,
    Text,
    Json,
}

/// The history shown for one scope.
pub(super) struct HistoryState {
    tenant: TenantId,
    workspace: String,
    scope: Scope,
    /// Addressed without the workspace from AI Gateway.
    local: bool,
    /// The newest list request, and the newest open or delete.
    list_lease: u64,
    lease: u64,
    list: Option<Result<Vec<Listed>, HistoryFailure>>,
    opened: Option<Arc<Record>>,
    busy: bool,
    /// Why the last open or delete did not happen.
    problem: Option<HistoryFailure>,
}

/// Work on records, off the lock.
pub(super) enum HistoryJob {
    List {
        lease: u64,
        scope: Scope,
    },
    Open {
        lease: u64,
        scope: Scope,
        id: String,
    },
    Delete {
        lease: u64,
        scope: Scope,
        id: String,
    },
}

impl State {
    pub(super) fn history_root(&self) -> Option<std::path::PathBuf> {
        self.harness_history_root.clone()
    }

    /// The history state for the chosen workspace, (re)made when the scope
    /// changed.
    ///
    /// Without a `reference`, the tenant's last chosen workspace on this
    /// Mac, only while no workspace is chosen under `context` (T23).
    fn history_for(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
    ) -> Result<&mut HistoryState, Refusal> {
        let tenant = self.check_context(context)?;
        let workspace = match reference {
            Some(reference) => self.inference_workspace(context, reference)?.id,
            None => {
                if self.chosen_scope(context).is_some() {
                    return Err(Refusal::StaleContext);
                }
                self.tenant(&tenant)
                    .and_then(|t| t.gateway_workspace.clone())
                    .ok_or(Refusal::InvalidInput)?
            }
        };
        let local = reference.is_none();
        let root = self.history_root().ok_or(Refusal::CapabilityMissing)?;
        let same = self
            .harness_history
            .as_ref()
            .is_some_and(|h| h.tenant == tenant && h.workspace == workspace);
        if same {
            if let Some(h) = self.harness_history.as_mut() {
                h.local = local;
            }
        } else {
            let lease = self.lease();
            self.harness_history = Some(HistoryState {
                scope: Scope::new(&root, tenant.as_str(), &workspace),
                local,
                tenant,
                workspace,
                list_lease: lease,
                lease,
                list: None,
                opened: None,
                busy: false,
                problem: None,
            });
        }
        self.harness_history
            .as_mut()
            .ok_or(Refusal::CapabilityMissing)
    }

    /// Lists the chosen workspace's saved conversations.
    pub(super) fn list_harness_history(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
    ) -> Result<Effect, Refusal> {
        let lease = self.lease();
        let h = self.history_for(context, reference)?;
        // Shown as reading until this request's own answer.
        h.list_lease = lease;
        h.list = None;
        Ok(Effect::HarnessHistory(HistoryJob::List {
            lease,
            scope: h.scope.clone(),
        }))
    }

    pub(super) fn open_harness_history(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
        id: &str,
    ) -> Result<Effect, Refusal> {
        if !protocol::is_uuid(id) {
            return Err(Refusal::InvalidInput);
        }
        let lease = self.lease();
        let h = self.history_for(context, reference)?;
        if h.busy {
            return Err(Refusal::Busy);
        }
        h.busy = true;
        h.lease = lease;
        Ok(Effect::HarnessHistory(HistoryJob::Open {
            lease,
            scope: h.scope.clone(),
            id: id.to_owned(),
        }))
    }

    pub(super) fn close_harness_history(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
    ) -> Result<(), Refusal> {
        self.history_for(context, reference)?.opened = None;
        Ok(())
    }

    /// Deletes a saved conversation (never the one running now).
    pub(super) fn delete_harness_history(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
        id: &str,
    ) -> Result<Effect, Refusal> {
        if !protocol::is_uuid(id) {
            return Err(Refusal::InvalidInput);
        }
        if self.harness_history_live() == Some(id) {
            return Err(Refusal::Busy);
        }
        let lease = self.lease();
        let h = self.history_for(context, reference)?;
        if h.busy {
            return Err(Refusal::Busy);
        }
        h.busy = true;
        h.lease = lease;
        Ok(Effect::HarnessHistory(HistoryJob::Delete {
            lease,
            scope: h.scope.clone(),
            id: id.to_owned(),
        }))
    }

    /// The opened conversation's transcript in `format`, for the native
    /// save sheet. The engine's replay is never exported.
    pub(super) fn export_harness_history(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
        id: &str,
        format: HistoryFormat,
    ) -> Result<(u64, String, Vec<u8>), Refusal> {
        let record = self
            .history_for(context, reference)?
            .opened
            .clone()
            .filter(|r| r.id == id)
            .ok_or(Refusal::InvalidInput)?;
        let (text, extension) = match format {
            HistoryFormat::Markdown => (history::export::markdown(&record), "md"),
            HistoryFormat::Text => (history::export::text(&record), "txt"),
            HistoryFormat::Json => (history::export::json(&record), "json"),
        };
        let name = format!(
            "harness-{}-{}.{extension}",
            crate::sessions::slug(&record.title),
            crate::sessions::file_stamp(record.updated)
        );
        let lease = self.begin_export(context, ExportKind::HarnessConversation)?;
        Ok((lease, name, text.into_bytes()))
    }

    /// Continues the opened conversation, when it is continuable.
    pub(super) fn resume_harness(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
        id: &str,
    ) -> Result<Resume, Refusal> {
        let record = self
            .history_for(context, reference)?
            .opened
            .clone()
            .filter(|r| r.id == id)
            .ok_or(Refusal::InvalidInput)?;
        if self.not_continuable(&record).is_some() {
            return Err(Refusal::StaleContext);
        }
        Ok(Resume {
            id: record.id.clone(),
            created: record.created,
            binding: record.binding.clone(),
            entries: record.entries.clone(),
            usage: record.usage,
            replay: Replay {
                context: record.context.clone(),
                replay: record.replay.clone(),
            },
        })
    }

    /// The binding a new conversation would have now in the record's
    /// workspace, compared with the record's.
    fn not_continuable(&self, record: &Record) -> Option<NotContinuable> {
        if self.harness_history_live() == Some(record.id.as_str()) {
            return Some(NotContinuable::Running);
        }
        if record.origin.is_some() {
            return Some(NotContinuable::Imported);
        }
        if !self.harness_engine {
            return Some(NotContinuable::EngineUnavailable);
        }
        if record.replay.is_null() || record.context.is_null() {
            return Some(NotContinuable::NotRestorable);
        }
        let h = self.harness_history.as_ref()?;
        if h.local {
            return Some(NotContinuable::WorkspaceUnavailable);
        }
        let tenant = self.tenant(&h.tenant)?;
        let b = &record.binding;
        let sign_in = tenant
            .harness_sign_in
            .as_ref()
            .filter(|s| s.selected && s.target.workspace.id == h.workspace);
        let current = match sign_in {
            Some(s) => s.grant.as_ref().map(|g| {
                (
                    protocol::derived_uuid("sign-in-grant", &g.id),
                    s.target.endpoint.clone(),
                    CredentialSource::CompanySignIn,
                )
            }),
            None => match (&tenant.inference, &tenant.inference_target) {
                (super::ScanCapability::Ready { record, .. }, Some(target))
                    if target.workspace.id == h.workspace =>
                {
                    Some((
                        protocol::derived_uuid("credential", record.as_str()),
                        target.endpoint.clone(),
                        CredentialSource::WorkspaceKey,
                    ))
                }
                _ => None,
            },
        };
        let same = current.is_some_and(|(credential, endpoint, source)| {
            b.tenant == protocol::derived_uuid("tenant", h.tenant.as_str())
                && b.workspace == h.workspace
                && b.credential == credential
                && b.credential_epoch == 1
                && b.endpoint == endpoint
                && b.source == source
        });
        if !same {
            return Some(NotContinuable::OtherCredential);
        }
        let project = tenant.harness_project.as_ref();
        if b.project.as_ref() != project.map(|p| &p.path)
            || b.write != project.is_some_and(|p| p.write)
        {
            return Some(NotContinuable::OtherProject);
        }
        (self.mcp_scope_now(&h.tenant, &h.workspace).as_ref() != Some(&b.mcp))
            .then_some(NotContinuable::OtherMcp)
    }

    /// A job's result, applied while it is still current.
    fn history_done(&mut self, lease: u64, scope: &Scope, result: JobResult) {
        let Some(h) = self.harness_history.as_mut().filter(|h| &h.scope == scope) else {
            return;
        };
        match result {
            JobResult::Listed(list) => {
                if h.list_lease == lease {
                    h.list = Some(list);
                }
            }
            JobResult::Opened(record) => {
                if h.lease == lease {
                    h.busy = false;
                    match record {
                        Ok(record) => {
                            h.opened = Some(Arc::new(*record));
                            h.problem = None;
                        }
                        Err(failure) => h.problem = Some(failure),
                    }
                }
            }
            JobResult::Deleted(id, deleted, list) => {
                h.busy = false;
                h.problem = deleted.err();
                if h.opened.as_ref().is_some_and(|r| r.id == id) {
                    h.opened = None;
                }
                h.list = Some(list);
            }
        }
        self.changed();
    }

    /// A save landed: the list shows it at once.
    fn history_saved(&mut self, scope: &Scope, record: &Record) {
        let Some(h) = self.harness_history.as_mut().filter(|h| &h.scope == scope) else {
            return;
        };
        if let Some(Ok(list)) = h.list.as_mut() {
            list.retain(|l| !matches!(l, Listed::Saved { id, .. } if id == &record.id));
            list.insert(
                0,
                Listed::Saved {
                    id: record.id.clone(),
                    title: record.title.clone(),
                    updated: record.updated,
                    entries: record.entries.len(),
                    binding: record.binding.clone(),
                },
            );
        }
        self.changed();
    }

    /// Records that `scope` may hold history (write-ahead, so removing the
    /// tenant removes its key).
    fn note_history_scope(&mut self, tenant: &TenantId, workspace: &str) -> bool {
        let known = self
            .data
            .harness_history
            .iter()
            .any(|p| &p.tenant == tenant && p.workspace == workspace);
        if known {
            return true;
        }
        let before = self.data.clone();
        self.data.harness_history.push(GatewayPreference {
            tenant: tenant.clone(),
            workspace: workspace.to_owned(),
        });
        self.save_with(&before).is_ok()
    }

    /// Admits one save of `scope`: only while storage is ready (not
    /// resetting) and the tenant exists, with the scope noted first. The
    /// tenant counts as writing and the scope's key stays in flight until
    /// [`State::history_written`], so tenant removal, cleanup and reset wait
    /// for it and none can place a file or key after them (C20 review F4).
    pub(super) fn admit_history_write(
        &mut self,
        tenant: &TenantId,
        workspace: &str,
        scope: &Scope,
    ) -> bool {
        // The scope's note must be durable before its key may be made:
        // an unconfirmed note (this save's, or an earlier one still known
        // only in memory) is confirmed first or nothing is written, so a
        // crash can never lose the ledger of a key it made (C20 review R2).
        if self.storage != super::StorageHealth::Ready
            || self.tenant(tenant).is_none()
            || !self.note_history_scope(tenant, workspace)
            || (self.durability_unconfirmed && !self.confirm_durable())
        {
            return false;
        }
        self.history_writers.push(tenant.clone());
        self.writes_in_flight.push(scope.key_record.clone());
        true
    }

    /// One admitted save settled (written or not).
    pub(super) fn history_written(&mut self, tenant: &TenantId, scope: &Scope) {
        if let Some(at) = self.history_writers.iter().position(|t| t == tenant) {
            self.history_writers.remove(at);
        }
        if let Some(at) = self
            .writes_in_flight
            .iter()
            .position(|r| r == &scope.key_record)
        {
            self.writes_in_flight.remove(at);
        }
    }

    /// Removes history folders of tenants that no longer exist (a removal
    /// that could not finish before the app ended). Only with the saved
    /// document loaded: an unreadable one names no tenant.
    pub(super) fn sweep_harness_history(&self) {
        let Some(root) = self.history_root() else {
            return;
        };
        if self.storage != super::StorageHealth::Ready {
            return;
        }
        let kept: Vec<std::path::PathBuf> = self
            .data
            .tenants
            .iter()
            .map(|t| history::tenant_dir(&root, t.profile.id.as_str()))
            .collect();
        let Ok(entries) = std::fs::read_dir(&root) else {
            return;
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) && !kept.contains(&path) {
                let _ = std::fs::remove_dir_all(&path);
            }
        }
    }

    /// A tenant's history keys (for the cleanup ledger) and directory, as
    /// it is removed.
    pub(super) fn forget_harness_history(
        &mut self,
        tenant: &TenantId,
    ) -> (Vec<crate::vault::RecordId>, Option<std::path::PathBuf>) {
        let root = self.history_root();
        let keys = self
            .data
            .harness_history
            .iter()
            .filter(|p| &p.tenant == tenant)
            .filter_map(|p| {
                root.as_ref()
                    .map(|r| Scope::new(r, tenant.as_str(), &p.workspace).key_record)
            })
            .collect();
        self.data.harness_history.retain(|p| &p.tenant != tenant);
        if self
            .harness_history
            .as_ref()
            .is_some_and(|h| &h.tenant == tenant)
        {
            self.harness_history = None;
        }
        (keys, root.map(|r| history::tenant_dir(&r, tenant.as_str())))
    }

    pub(super) fn harness_history_view(&self) -> Option<HarnessHistoryView> {
        let h = self.harness_history.as_ref()?;
        // Only the active tenant's chosen workspace.
        let active = self.data.active.as_ref()? == &h.tenant
            && self.tenant(&h.tenant)?.gateway_workspace.as_deref() == Some(h.workspace.as_str());
        if !active {
            return None;
        }
        let (state, listed, failure) = match &h.list {
            None => (HistoryListState::Loading, &[][..], None),
            Some(Ok(list)) => (HistoryListState::Ready, &list[..], None),
            Some(Err(f)) => (HistoryListState::Failed, &[][..], Some(*f)),
        };
        let conversations = listed
            .iter()
            .filter_map(|l| match l {
                // An import's conversations show once it commits (D050).
                Listed::Saved { id, .. }
                    if self.v1_hides_conversation(&h.tenant, &h.workspace, id) =>
                {
                    None
                }
                Listed::Saved {
                    id,
                    title,
                    updated,
                    entries,
                    binding,
                } => Some(SavedConversationView {
                    id: id.clone(),
                    title: title.clone(),
                    updated: *updated,
                    entries: u32::try_from(*entries).unwrap_or(u32::MAX),
                    source: binding.source,
                }),
                Listed::Unreadable { .. } => None,
            })
            .collect();
        let unreadable: Vec<String> = listed
            .iter()
            .filter_map(|l| match l {
                Listed::Unreadable { id } => Some(id.clone()),
                Listed::Saved { .. } => None,
            })
            .collect();
        Some(HarnessHistoryView {
            local: h.local,
            state,
            full: listed.len() >= history::MAX_CONVERSATIONS,
            conversations,
            unreadable,
            failure,
            opened: h.opened.as_ref().map(|r| OpenedConversationView {
                id: r.id.clone(),
                title: r.title.clone(),
                updated: r.updated,
                entries: r.entries.clone(),
                source: r.binding.source,
                not_continuable: self.not_continuable(r),
            }),
            busy: h.busy,
            problem: h.problem,
            export: self
                .export
                .as_ref()
                .filter(|e| {
                    Some(&e.context) == self.active_ref.as_ref()
                        && e.is_kind(ExportKind::HarnessConversation)
                })
                .map(|e| e.view()),
        })
    }
}

enum JobResult {
    Listed(Result<Vec<Listed>, HistoryFailure>),
    Opened(Result<Box<Record>, HistoryFailure>),
    Deleted(
        String,
        Result<(), HistoryFailure>,
        Result<Vec<Listed>, HistoryFailure>,
    ),
}

pub(super) async fn run_job(inner: Arc<Inner>, job: HistoryJob) {
    let vault = Arc::clone(&inner.adapters.keychain);
    let (lease, scope, result) = tokio::task::spawn_blocking(move || match job {
        HistoryJob::List { lease, scope } => {
            let list = history::list(&*vault, &scope);
            (lease, scope, JobResult::Listed(list))
        }
        HistoryJob::Open { lease, scope, id } => {
            let record = history::read(&*vault, &scope, &id).map(Box::new);
            (lease, scope, JobResult::Opened(record))
        }
        HistoryJob::Delete { lease, scope, id } => {
            let deleted = history::delete(&scope, &id);
            let list = history::list(&*vault, &scope);
            (lease, scope, JobResult::Deleted(id, deleted, list))
        }
    })
    .await
    .expect("history job");
    if let Ok(mut state) = lock(&inner) {
        state.history_done(lease, &scope, result);
    }
}

/// The sink that saves a conversation after each turn: the scope is noted
/// first, then the record is sealed and replaced, then the list updated.
pub(super) fn sink(inner: &Arc<Inner>, plan: HistoryPlan) -> HistorySink {
    let inner = Arc::clone(inner);
    let HistoryPlan {
        scope,
        id,
        created,
        binding,
        tenant,
        workspace,
    } = plan;
    Arc::new(move |entries: Vec<HarnessEntry>, usage, export: &Replay| {
        let hold = lock(&inner)
            .ok()
            .and_then(|mut state| {
                state
                    .admit_history_write(&tenant, &workspace, &scope)
                    .then(|| Arc::clone(&state.history_hold))
            })
            .ok_or(HistoryFailure::Unavailable)?;
        let saved = {
            let _held = hold.blocking_lock();
            save_record(
                &inner, &scope, &id, created, &binding, entries, usage, export,
            )
        };
        if let Ok(mut state) = lock(&inner) {
            state.history_written(&tenant, &scope);
            if let Ok(record) = &saved {
                state.history_saved(&scope, record);
            }
        }
        // A key settled after its tenant was removed is cleaned up now.
        super::run_effects(&inner, vec![Effect::Cleanup]);
        saved.map(|record| record.updated)
    })
}

/// Seals and replaces one conversation's record.
#[allow(clippy::too_many_arguments)]
fn save_record(
    inner: &Arc<Inner>,
    scope: &Scope,
    id: &str,
    created: u64,
    binding: &SavedBinding,
    entries: Vec<HarnessEntry>,
    usage: Option<HarnessUsage>,
    export: &Replay,
) -> Result<Record, HistoryFailure> {
    let title = entries
        .iter()
        .find(|e| e.role == HarnessRole::Operator)
        .map(|e| history::title(&e.text))
        .unwrap_or_default();
    let record = Record {
        schema: history::SCHEMA.to_owned(),
        id: id.to_owned(),
        title,
        created,
        updated: now().max(created),
        binding: binding.clone(),
        entries,
        usage,
        context: export.context.clone(),
        replay: export.replay.clone(),
        origin: None,
    };
    history::save(&*inner.adapters.keychain, scope, &record)?;
    Ok(record)
}
