//! Gateway configuration work kept on this Mac inside the authority (C17,
//! T23). Each opened tenant keeps its own book of saved drafts and of
//! changes that settled uncertain, read once from its owner-only file and
//! written after every change (only the newest generation). Settlements
//! that arrive before the book is read are queued and applied once it is.
//! Nothing here is sent: a draft becomes a change only through a fresh
//! ReviewChange of its text under the workspace chosen then.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Refusal, State};
use crate::changes::{ChangeAction, ChangeKind};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway_local::{
    GatewayDraft, GatewayDraftProblem, GatewayFileRecord, GatewayRecord, GatewayTarget, LocalBook,
    PlaygroundDraft, PlaygroundRunRecord,
};
use crate::sessions::ReportStoreFailure;
use crate::tenants::TenantId;

use super::sessions_domain::ReportsState as LocalState;

/// A draft to keep: a new one for the chosen workspace (or the tenant), or
/// new text for a kept draft (its change and workspace are kept).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayDraftInput {
    /// A kept draft's ID; absent for a new draft.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub target: String,
    pub body: String,
    /// The current workspace reference, for a new draft inside a workspace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub workspace: Option<WorkspaceRef>,
}

/// The active tenant's Gateway work on this Mac.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayLocalView {
    pub state: LocalState,
    pub failure: Option<ReportStoreFailure>,
    /// Newest first.
    pub drafts: Vec<GatewayDraft>,
    /// Changes that settled uncertain, newest first.
    pub records: Vec<GatewayRecord>,
    /// Playground drafts, one per workspace (C18).
    pub playground: Vec<PlaygroundDraft>,
    /// Playground runs that ended without a known result (C18).
    pub runs: Vec<PlaygroundRunRecord>,
    /// File uploads and deletes without a known result (C18).
    pub files: Vec<GatewayFileRecord>,
    /// Why the last draft was not kept.
    pub problem: Option<GatewayDraftProblem>,
    /// The last change could not be written to this Mac.
    pub unsaved: bool,
}

impl GatewayLocalView {
    fn idle() -> Self {
        Self {
            state: LocalState::Idle,
            failure: None,
            drafts: Vec::new(),
            records: Vec::new(),
            playground: Vec::new(),
            runs: Vec::new(),
            files: Vec::new(),
            problem: None,
            unsaved: false,
        }
    }
}

/// A Playground draft to keep (C18): a new or changed one for the chosen
/// workspace, or (by `id`) an edit of one already kept, which needs no
/// workspace listing and so works offline (T23).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaygroundDraftInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    #[serde(default)]
    pub api: crate::gateway_inference::InferenceApi,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub system: Option<String>,
    pub prompt: String,
    /// The Gateway file to attach (its ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file: Option<String>,
}

/// A run or file record to keep in a tenant's book.
pub(super) enum Kept {
    Run(PlaygroundRunRecord),
    File(GatewayFileRecord),
}

impl LocalBook {
    fn keep(&mut self, kept: Kept) {
        match kept {
            Kept::Run(r) => self.ran(r),
            Kept::File(r) => self.filed(r),
        }
    }
}

/// A settlement to apply to a tenant's book.
pub(super) struct Settled {
    pub(super) record: Option<GatewayRecord>,
    pub(super) change: GatewayTarget,
    pub(super) sent: bool,
}

pub(super) struct LocalWork {
    tenant: TenantId,
    lease: u64,
    state: LocalState,
    failure: Option<ReportStoreFailure>,
    book: LocalBook,
    problem: Option<GatewayDraftProblem>,
    /// Settlements waiting for the book to be read.
    queued: Vec<Settled>,
    /// Run and file records waiting for the book to be read.
    queued_kept: Vec<Kept>,
    /// Issued afresh (from the authority's lease counter, never reused) by
    /// every change; only the newest is written, so no write admitted for
    /// an earlier book, tenant or reset can apply to a later one.
    generation: u64,
    unsaved: bool,
}

fn now() -> String {
    crate::sessions::rfc3339(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
}

fn local_id(prefix: &str) -> Option<String> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).ok()?;
    Some(format!("{prefix}-{}", hex::encode(bytes)))
}

impl State {
    fn local_work(&mut self, tenant: &TenantId) -> Option<&mut LocalWork> {
        self.gateway_local.iter_mut().find(|w| &w.tenant == tenant)
    }

    /// Opens the active tenant's Gateway work (once; a failed read is
    /// retried); returns the lease when a read must start.
    pub(super) fn open_gateway_local(
        &mut self,
        context: &ContextRef,
    ) -> Result<Option<u64>, Refusal> {
        let tenant = self.check_context(context)?;
        self.sessions_root
            .as_ref()
            .ok_or(Refusal::CapabilityMissing)?;
        if self.gateway_local.iter().any(|w| {
            w.tenant == tenant && matches!(w.state, LocalState::Loading | LocalState::Ready)
        }) {
            return Ok(None);
        }
        let lease = self.lease();
        let (queued, queued_kept) = self
            .local_work(&tenant)
            .map(|w| {
                (
                    std::mem::take(&mut w.queued),
                    std::mem::take(&mut w.queued_kept),
                )
            })
            .unwrap_or_default();
        self.gateway_local.retain(|w| w.tenant != tenant);
        self.gateway_local.push(LocalWork {
            tenant,
            lease,
            state: LocalState::Loading,
            failure: None,
            book: LocalBook::default(),
            problem: None,
            queued,
            queued_kept,
            generation: 0,
            unsaved: false,
        });
        Ok(Some(lease))
    }

    pub(super) fn gateway_local_target(&self, lease: u64) -> Option<std::path::PathBuf> {
        let work = self.gateway_local.iter().find(|w| w.lease == lease)?;
        let root = self.sessions_root.as_ref()?;
        Some(crate::gateway_local::path(root, work.tenant.as_str()))
    }

    /// The read settled; queued settlements are applied (and written).
    pub(super) fn settle_gateway_local(
        &mut self,
        lease: u64,
        loaded: Result<LocalBook, ReportStoreFailure>,
    ) {
        let generation = self.lease();
        let Some(work) = self
            .gateway_local
            .iter_mut()
            .find(|w| w.lease == lease && w.state == LocalState::Loading)
        else {
            return;
        };
        match loaded {
            Ok(book) => {
                work.book = book;
                work.state = LocalState::Ready;
                if !work.queued.is_empty() || !work.queued_kept.is_empty() {
                    for s in std::mem::take(&mut work.queued) {
                        work.book.settled(s.record, &s.change, s.sent);
                    }
                    for k in std::mem::take(&mut work.queued_kept) {
                        work.book.keep(k);
                    }
                    work.generation = generation;
                    let write = (work.tenant.clone(), work.generation);
                    self.gateway_local_writes.push(write);
                }
            }
            Err(failure) => {
                work.state = LocalState::Failed;
                work.failure = Some(failure);
            }
        }
    }

    fn ready_local(&mut self, tenant: &TenantId) -> Result<&mut LocalWork, Refusal> {
        let work = self.local_work(tenant).ok_or(Refusal::CapabilityMissing)?;
        match work.state {
            LocalState::Ready => Ok(work),
            LocalState::Loading => Err(Refusal::Busy),
            _ => Err(Refusal::CapabilityMissing),
        }
    }

    /// Keeps a draft. A new one is labelled with the workspace a review of
    /// it would apply to now (the core's, never the presentation's); a kept
    /// one keeps its change and label and takes the new text. Returns the
    /// tenant and generation to write; a refused draft publishes why.
    pub(super) fn save_gateway_draft(
        &mut self,
        context: &ContextRef,
        input: GatewayDraftInput,
    ) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let kept = {
            let work = self.ready_local(&tenant)?;
            match &input.id {
                Some(id) => Some(
                    work.book
                        .drafts
                        .iter()
                        .find(|d| &d.id == id)
                        .map(|d| d.change.clone())
                        .ok_or(Refusal::InvalidInput)?,
                ),
                None => None,
            }
        };
        let change = match kept {
            Some(change) => {
                if (change.kind, change.action, change.target.as_str())
                    != (input.kind, input.action, input.target.as_str())
                {
                    return Err(Refusal::InvalidInput);
                }
                change
            }
            None => {
                let resource = input.kind.gateway().ok_or(Refusal::InvalidInput)?;
                let scope = self.gateway_change_scope(
                    context,
                    resource,
                    input.action,
                    &input.target,
                    input.workspace.as_ref(),
                )?;
                GatewayTarget {
                    workspace: scope.shown,
                    kind: input.kind,
                    action: input.action,
                    target: input.target.clone(),
                }
            }
        };
        let work = self.ready_local(&tenant)?;
        if let Err(problem) = crate::gateway_local::check(&change, &input.body) {
            work.problem = Some(problem);
            return Err(Refusal::InvalidInput);
        }
        let id = match input.id {
            Some(id) => id,
            None => local_id("gd").ok_or(Refusal::Busy)?,
        };
        let generation = self.lease();
        let work = self.ready_local(&tenant)?;
        work.book.save(GatewayDraft {
            id,
            change,
            body: input.body,
            saved_at: now(),
        });
        work.problem = None;
        work.generation = generation;
        Ok((tenant, generation))
    }

    /// Keeps a Playground draft (one per workspace; its text is kept as
    /// typed, within the run bounds): by `id`, an edit of one already kept,
    /// under the workspace it was kept for; otherwise the draft of the
    /// workspace `reference` names.
    pub(super) fn save_playground_draft(
        &mut self,
        context: &ContextRef,
        reference: Option<&WorkspaceRef>,
        input: PlaygroundDraftInput,
    ) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let workspace = match (&input.id, reference) {
            (Some(id), None) => self
                .ready_local(&tenant)?
                .book
                .playground
                .iter()
                .find(|d| &d.id == id)
                .map(|d| d.workspace.clone())
                .ok_or(Refusal::InvalidInput)?,
            (None, Some(reference)) => self.inference_workspace(context, reference)?,
            _ => return Err(Refusal::InvalidInput),
        };
        let input_bytes = input.prompt.len() + input.system.as_deref().map_or(0, str::len);
        if input.model.len() > 256
            || input.model.chars().any(char::is_control)
            || input
                .route
                .as_deref()
                .is_some_and(|r| !crate::gateway::is_id(r))
            || input
                .file
                .as_deref()
                .is_some_and(|f| !crate::gateway::is_id(f))
            || input_bytes > crate::gateway_inference::MAX_INPUT_BYTES
        {
            return Err(Refusal::InvalidInput);
        }
        let generation = self.lease();
        let work = self.ready_local(&tenant)?;
        let id = work
            .book
            .playground
            .iter()
            .find(|d| d.workspace.id == workspace.id)
            .map(|d| d.id.clone());
        let id = match id {
            Some(id) => id,
            None => local_id("gp").ok_or(Refusal::Busy)?,
        };
        work.book.save_playground(PlaygroundDraft {
            id,
            workspace,
            api: input.api,
            model: input.model,
            route: input.route,
            system: input.system,
            prompt: input.prompt,
            file: input.file,
            saved_at: now(),
        });
        work.generation = generation;
        Ok((tenant, generation))
    }

    /// A Playground run ended: one that was sent and did not complete is
    /// kept (without its output) under its tenant.
    pub(super) fn inference_settled(
        &mut self,
        tenant: &TenantId,
        workspace: crate::gateway::ListedWorkspace,
        run: &crate::gateway_inference::InferenceRun,
        trace: String,
        outcome: crate::runtime::Outcome,
        progress: &crate::gateway_inference::RunProgress,
    ) {
        use crate::gateway_inference::RunEnd;
        let uncertain = progress.sent
            && matches!(progress.end, Some(RunEnd::Stopped | RunEnd::Failed) | None)
            && matches!(
                outcome,
                crate::runtime::Outcome::Unknown | crate::runtime::Outcome::Completed
            );
        if !uncertain
            || self.tenant(tenant).is_none()
            || self.sessions_root.is_none()
            || self.storage == super::StorageHealth::Resetting
        {
            return;
        }
        let Some(id) = local_id("gi") else {
            return;
        };
        self.keep_local(
            tenant,
            Kept::Run(PlaygroundRunRecord {
                id,
                workspace,
                api: run.api,
                model: run.model.clone(),
                route: run.route.clone(),
                trace,
                end: progress.end.unwrap_or(RunEnd::Stopped),
                outcome,
                failure: progress.failure,
                answered: progress.answered,
                output_bytes: u32::try_from(progress.output.len()).unwrap_or(u32::MAX),
                usage: progress.usage,
                settled_at: now(),
            }),
        );
    }

    /// A file upload or delete was sent and ended without a known result:
    /// it is kept (without the file's content) under its tenant.
    pub(super) fn file_settled(
        &mut self,
        tenant: &TenantId,
        workspace: crate::gateway::ListedWorkspace,
        action: &super::gateway_files_domain::FileActionView,
    ) {
        if self.tenant(tenant).is_none()
            || self.sessions_root.is_none()
            || self.storage == super::StorageHealth::Resetting
        {
            return;
        }
        let Some(id) = local_id("gf") else {
            return;
        };
        self.keep_local(
            tenant,
            Kept::File(GatewayFileRecord {
                id,
                workspace,
                action: action.action,
                filename: action.filename.clone(),
                file: action.file.clone(),
                bytes: action.bytes,
                trace: action.trace.clone(),
                outcome: action.outcome.unwrap_or(crate::runtime::Outcome::Unknown),
                failure: action.failure,
                settled_at: now(),
            }),
        );
    }

    /// Keeps a run or file record in the tenant's book (queued until the
    /// book is read).
    fn keep_local(&mut self, tenant: &TenantId, kept: Kept) {
        let generation = self.lease();
        let write = match self.local_work(tenant) {
            Some(work) if work.state == LocalState::Ready => {
                work.book.keep(kept);
                work.generation = generation;
                Some((tenant.clone(), generation))
            }
            Some(work) => {
                work.queued_kept.push(kept);
                None
            }
            None => {
                let lease = self.lease();
                self.gateway_local.push(LocalWork {
                    tenant: tenant.clone(),
                    lease,
                    state: LocalState::Idle,
                    failure: None,
                    book: LocalBook::default(),
                    problem: None,
                    queued: Vec::new(),
                    queued_kept: vec![kept],
                    generation: 0,
                    unsaved: false,
                });
                None
            }
        };
        self.gateway_local_writes.extend(write);
    }

    /// Removes a kept draft or record of the active tenant.
    pub(super) fn discard_gateway_local(
        &mut self,
        context: &ContextRef,
        id: &str,
    ) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let generation = self.lease();
        let work = self.ready_local(&tenant)?;
        if !work.book.discard(id) {
            return Err(Refusal::InvalidInput);
        }
        work.generation = generation;
        Ok((tenant, generation))
    }

    /// A Gateway change settled for `tenant`: applied to its book (queued
    /// until the book is read). The write is collected for the caller.
    /// Nothing is kept for a tenant that was deleted or reset away.
    pub(super) fn gateway_local_settled(&mut self, tenant: &TenantId, mut settled: Settled) {
        if self.tenant(tenant).is_none()
            || self.sessions_root.is_none()
            || self.storage == super::StorageHealth::Resetting
        {
            return;
        }
        let generation = self.lease();
        if let Some(record) = settled.record.as_mut() {
            match local_id("gr") {
                Some(id) => record.id = id,
                None => settled.record = None,
            }
            if let Some(record) = settled.record.as_mut() {
                record.settled_at = now();
            }
        }
        let write = match self.local_work(tenant) {
            Some(work) if work.state == LocalState::Ready => {
                work.book
                    .settled(settled.record, &settled.change, settled.sent);
                work.generation = generation;
                Some((tenant.clone(), generation))
            }
            Some(work) => {
                work.queued.push(settled);
                None
            }
            None => {
                // Not opened yet: read it now and apply this once it is.
                let lease = self.lease();
                self.gateway_local.push(LocalWork {
                    tenant: tenant.clone(),
                    lease,
                    state: LocalState::Idle,
                    failure: None,
                    book: LocalBook::default(),
                    problem: None,
                    queued: vec![settled],
                    queued_kept: Vec::new(),
                    generation: 0,
                    unsaved: false,
                });
                None
            }
        };
        self.gateway_local_writes.extend(write);
    }

    /// The writes (and reads of books that settlements are queued for) to
    /// start now.
    pub(super) fn take_gateway_local(&mut self) -> (Vec<(TenantId, u64)>, Vec<u64>) {
        let writes = std::mem::take(&mut self.gateway_local_writes);
        let mut reads = Vec::new();
        for work in &mut self.gateway_local {
            if work.state == LocalState::Idle
                && (!work.queued.is_empty() || !work.queued_kept.is_empty())
            {
                work.state = LocalState::Loading;
                reads.push(work.lease);
            }
        }
        (writes, reads)
    }

    /// The newest book to write for `tenant` (nothing once the tenant was
    /// deleted, the data reset or a newer generation exists).
    pub(super) fn gateway_local_write(
        &self,
        tenant: &TenantId,
        generation: u64,
    ) -> Option<(std::path::PathBuf, LocalBook)> {
        if self.storage == super::StorageHealth::Resetting || self.tenant(tenant).is_none() {
            return None;
        }
        let work = self.gateway_local.iter().find(|w| {
            &w.tenant == tenant && w.generation == generation && w.state == LocalState::Ready
        })?;
        let root = self.sessions_root.as_ref()?;
        Some((
            crate::gateway_local::path(root, tenant.as_str()),
            work.book.clone(),
        ))
    }

    pub(super) fn settle_gateway_local_write(
        &mut self,
        tenant: &TenantId,
        generation: u64,
        stored: bool,
    ) {
        if let Some(work) = self
            .gateway_local
            .iter_mut()
            .find(|w| &w.tenant == tenant && w.generation == generation)
        {
            work.unsaved = !stored;
        }
    }

    /// A reset was admitted: every book, queued settlement, pending write
    /// and read lease is dropped. A read or write admitted before finds
    /// nothing to settle or write, and later ones carry new leases and
    /// generations, so nothing purged reappears.
    pub(super) fn forget_all_gateway_local(&mut self) {
        self.gateway_local.clear();
        self.gateway_local_writes.clear();
        self.detached_changes.clear();
    }

    /// Drops a deleted tenant's Gateway work; returns the file to remove.
    pub(super) fn forget_gateway_local(&mut self, tenant: &TenantId) -> Option<std::path::PathBuf> {
        self.gateway_local.retain(|w| &w.tenant != tenant);
        self.gateway_local_writes.retain(|(t, _)| t != tenant);
        self.detached_changes.retain(|d| d.tenant() != Some(tenant));
        self.sessions_root
            .as_ref()
            .map(|root| crate::gateway_local::path(root, tenant.as_str()))
    }

    pub(super) fn gateway_local_view(&self) -> GatewayLocalView {
        let Some(tenant) = self.data.active.as_ref() else {
            return GatewayLocalView::idle();
        };
        self.gateway_local
            .iter()
            .find(|w| &w.tenant == tenant)
            .map_or_else(GatewayLocalView::idle, |w| GatewayLocalView {
                state: w.state,
                failure: w.failure,
                drafts: w.book.drafts.clone(),
                records: w.book.records.clone(),
                playground: w.book.playground.clone(),
                runs: w.book.runs.clone(),
                files: w.book.files.clone(),
                problem: w.problem,
                unsaved: w.unsaved,
            })
    }
}
