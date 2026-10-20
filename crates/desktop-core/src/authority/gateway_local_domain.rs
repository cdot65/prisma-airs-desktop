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
    GatewayDraft, GatewayDraftProblem, GatewayRecord, GatewayTarget, LocalBook,
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
            problem: None,
            unsaved: false,
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
        let queued = self
            .local_work(&tenant)
            .map(|w| std::mem::take(&mut w.queued))
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
                if !work.queued.is_empty() {
                    for s in std::mem::take(&mut work.queued) {
                        work.book.settled(s.record, &s.change, s.sent);
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
            if work.state == LocalState::Idle && !work.queued.is_empty() {
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
                problem: w.problem,
                unsaved: w.unsaved,
            })
    }
}
