//! Gateway workspace selection inside the authority (C16). The workspaces
//! a tenant context listed are remembered with that context; the operator
//! chooses one of them, and a native [`WorkspaceRef`] names that choice.
//! Workspace reads are admitted only with the current reference and read
//! the workspace it names (never one the presentation supplies), under the
//! reference as their operations domain: choosing another workspace
//! retires them, and their results are withdrawn. A tenant context change
//! (switch, credential replacement, reset) clears the listing and the
//! choice. The choice is remembered per tenant and restored only when a
//! later listing under a new context lists it again. Neither a choice nor a
//! restore touches the tenant context, so Runtime reviews stay protected.

use serde::Serialize;
use tokio::time::Instant;
use ts_rs::TS;

use super::{Refusal, State, StorageHealth};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway::{GatewayList, GatewayRead, GatewayResource, ListedWorkspace, WorkspaceScope};
use crate::management::ManagementRequest;

/// The chosen workspace and the reference reads use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SelectedWorkspace {
    pub reference: WorkspaceRef,
    pub workspace: ListedWorkspace,
    /// Restored from the tenant's remembered choice (not chosen this
    /// session).
    pub restored: bool,
}

/// The active context's Gateway workspace state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayView {
    /// The workspaces last listed under the active context (`None`: not
    /// listed yet).
    pub workspaces: Option<Vec<ListedWorkspace>>,
    /// That listing was complete (absence from it is proven).
    pub complete: bool,
    pub selected: Option<SelectedWorkspace>,
    /// The tenant's remembered workspace, while it is not selected.
    pub remembered: Option<String>,
    /// A complete listing did not list the remembered workspace.
    pub remembered_missing: bool,
    /// The tenant's Gateway drafts and uncertain changes on this Mac
    /// (readable offline; C17).
    pub local: super::gateway_local_domain::GatewayLocalView,
    /// The Playground's run while it is bound to this context and
    /// workspace (C18).
    pub playground: Option<super::inference_domain::PlaygroundView>,
    /// The chosen workspace's files while they are bound to this context,
    /// workspace and inference key (C18).
    pub files: Option<super::gateway_files_domain::GatewayFilesView>,
}

struct Listing {
    context: ContextRef,
    workspaces: Vec<ListedWorkspace>,
    complete: bool,
}

struct Selection {
    reference: WorkspaceRef,
    context: ContextRef,
    workspace: ListedWorkspace,
    restored: bool,
}

/// Where a Gateway read is admitted: its operations domain (the tenant
/// context, or the selection reference for a scoped read), the reference
/// it is bound to and the workspace it reads.
pub(super) struct GatewayAdmission {
    pub domain: String,
    pub workspace: Option<WorkspaceRef>,
    pub scope: Option<WorkspaceScope>,
}

#[derive(Default)]
pub(super) struct GatewayState {
    listing: Option<Listing>,
    selection: Option<Selection>,
    epoch: u64,
    /// Work a retirement inside a settlement freed (started by the next
    /// intent; normally none, since retired work still holds its slot).
    pending: Vec<crate::runtime::Start>,
}

impl State {
    pub(super) fn gateway_view(&self) -> GatewayView {
        let current = |context: &ContextRef| self.active_ref.as_ref() == Some(context);
        let listing = self
            .gateway
            .listing
            .as_ref()
            .filter(|l| current(&l.context));
        let selected = self
            .gateway
            .selection
            .as_ref()
            .filter(|s| current(&s.context))
            .map(|s| SelectedWorkspace {
                reference: s.reference.clone(),
                workspace: s.workspace.clone(),
                restored: s.restored,
            });
        let remembered = self
            .data
            .active
            .as_ref()
            .and_then(|id| self.tenant(id))
            .and_then(|t| t.gateway_workspace.clone())
            .filter(|id| selected.as_ref().is_none_or(|s| &s.workspace.id != id));
        let remembered_missing = remembered.as_ref().is_some_and(|id| {
            listing.is_some_and(|l| l.complete && !l.workspaces.iter().any(|w| &w.id == id))
        });
        GatewayView {
            workspaces: listing.map(|l| l.workspaces.clone()),
            complete: listing.is_some_and(|l| l.complete),
            selected,
            remembered,
            remembered_missing,
            local: self.gateway_local_view(),
            playground: self.playground_view(),
            files: self.files_view(),
        }
    }

    fn issue_workspace(&mut self) -> WorkspaceRef {
        self.gateway.epoch += 1;
        WorkspaceRef::issue(&self.salt, self.gateway.epoch)
    }

    /// Retires every Gateway workspace read but `keep`'s, and every
    /// change applying under another reference that has not sent its write.
    fn retire_workspace_reads(
        &mut self,
        keep: Option<&WorkspaceRef>,
    ) -> Vec<crate::runtime::Start> {
        self.retire_scoped_applies(keep);
        self.operations.retire_contexts(
            WorkspaceRef::PREFIX,
            keep.map(WorkspaceRef::as_str),
            Instant::now(),
        )
    }

    /// The tenant context changed: its listing and choice go with it.
    pub(super) fn retire_gateway(&mut self) -> Vec<crate::runtime::Start> {
        self.gateway.listing = None;
        self.gateway.selection = None;
        self.retire_workspace_reads(None)
    }

    /// Chooses a workspace the active context listed; remembered for the
    /// tenant (a failed save refuses and leaves the previous choice).
    pub(super) fn select_gateway_workspace(
        &mut self,
        context: &ContextRef,
        workspace: &str,
    ) -> Result<Vec<crate::runtime::Start>, Refusal> {
        let tenant = self.check_context(context)?;
        if self
            .gateway
            .selection
            .as_ref()
            .is_some_and(|s| &s.context == context && s.workspace.id == workspace)
        {
            return Ok(Vec::new());
        }
        let listed = self
            .gateway
            .listing
            .as_ref()
            .filter(|l| &l.context == context)
            .and_then(|l| l.workspaces.iter().find(|w| w.id == workspace))
            .cloned()
            .ok_or(Refusal::InvalidInput)?;
        let remembered = self
            .tenant(&tenant)
            .and_then(|t| t.gateway_workspace.clone());
        if remembered.as_deref() != Some(workspace) {
            let before = self.data.clone();
            if let Some(t) = self
                .data
                .tenants
                .iter_mut()
                .find(|t| t.profile.id == tenant)
            {
                t.gateway_workspace = Some(workspace.to_owned());
            }
            if self.storage == StorageHealth::Ready {
                self.save_with(&before)?;
            }
        }
        Ok(self.choose(context.clone(), listed, false))
    }

    fn choose(
        &mut self,
        context: ContextRef,
        workspace: ListedWorkspace,
        restored: bool,
    ) -> Vec<crate::runtime::Start> {
        let reference = self.issue_workspace();
        let starts = self.retire_workspace_reads(Some(&reference));
        self.gateway.selection = Some(Selection {
            reference,
            context,
            workspace,
            restored,
        });
        starts
    }

    /// A Workspaces listing settled for `context` (still current): it
    /// replaces the last one. A choice it no longer lists (completely) is
    /// cleared; the tenant's remembered workspace is restored when listed.
    pub(super) fn gateway_listed(&mut self, context: &ContextRef, list: &GatewayList) {
        let workspaces = crate::gateway::listed_workspaces(list);
        let mut starts = Vec::new();
        if let Some(selection) = self.gateway.selection.as_mut() {
            match workspaces.iter().find(|w| w.id == selection.workspace.id) {
                Some(listed) => selection.workspace = listed.clone(),
                None if list.complete => {
                    self.gateway.selection = None;
                    starts = self.retire_workspace_reads(None);
                }
                None => {}
            }
        }
        if self.gateway.selection.is_none() {
            let remembered = self
                .data
                .active
                .as_ref()
                .and_then(|id| self.tenant(id))
                .and_then(|t| t.gateway_workspace.clone());
            if let Some(listed) =
                remembered.and_then(|id| workspaces.iter().find(|w| w.id == id).cloned())
            {
                starts.extend(self.choose(context.clone(), listed, true));
            }
        }
        self.gateway.listing = Some(Listing {
            context: context.clone(),
            workspaces,
            complete: list.complete,
        });
        self.gateway.pending.extend(starts);
    }

    /// Work freed by retirements inside settlements.
    pub(super) fn take_gateway_starts(&mut self) -> Vec<crate::runtime::Start> {
        std::mem::take(&mut self.gateway.pending)
    }

    /// The operations domain and workspace a Gateway read is admitted
    /// under: a scoped read needs the current reference for this context
    /// and reads the workspace it names.
    pub(super) fn gateway_admission(
        &self,
        context: &ContextRef,
        request: &ManagementRequest,
    ) -> Result<Option<GatewayAdmission>, Refusal> {
        let ManagementRequest::Gateway { workspace, read } = request else {
            return Ok(None);
        };
        if !read.scoped() {
            return Ok(Some(GatewayAdmission {
                domain: context.as_str().to_owned(),
                workspace: None,
                scope: None,
            }));
        }
        let selection = self
            .gateway
            .selection
            .as_ref()
            .filter(|s| &s.context == context && Some(&s.reference) == workspace.as_ref())
            .ok_or(Refusal::StaleContext)?;
        Ok(Some(GatewayAdmission {
            domain: selection.reference.as_str().to_owned(),
            workspace: Some(selection.reference.clone()),
            scope: Some(WorkspaceScope::from(&selection.workspace)),
        }))
    }

    /// The current selection's reference.
    pub(super) fn gateway_selection(&self) -> Option<WorkspaceRef> {
        self.gateway.selection.as_ref().map(|s| s.reference.clone())
    }

    /// The workspace `reference` names, while it is the current choice
    /// under `context`.
    pub(super) fn selected_workspace(
        &self,
        context: &ContextRef,
        reference: &WorkspaceRef,
    ) -> Option<ListedWorkspace> {
        self.gateway
            .selection
            .as_ref()
            .filter(|s| &s.context == context && &s.reference == reference)
            .map(|s| s.workspace.clone())
    }

    /// The chosen workspace under `context`, if any (an overview reads its
    /// Gateway activity).
    pub(super) fn chosen_scope(
        &self,
        context: &ContextRef,
    ) -> Option<crate::gateway::WorkspaceScope> {
        self.gateway
            .selection
            .as_ref()
            .filter(|s| &s.context == context)
            .map(|s| crate::gateway::WorkspaceScope::from(&s.workspace))
    }

    /// Whether a result read under `workspace` may still be shown.
    pub(super) fn workspace_current(&self, workspace: Option<&WorkspaceRef>) -> bool {
        workspace.is_none_or(|w| {
            self.gateway
                .selection
                .as_ref()
                .is_some_and(|s| &s.reference == w)
        })
    }
}

/// Where a reviewed Gateway change applies (C17): the selection reference
/// it is bound to, the workspace the core reads and writes, and the listed
/// workspace the operator is shown.
#[derive(Clone, Debug)]
pub(super) struct ChangeScope {
    pub reference: Option<WorkspaceRef>,
    pub scope: Option<WorkspaceScope>,
    pub shown: Option<ListedWorkspace>,
}

impl State {
    /// The scope a Gateway change of `kind`/`action` on `target` applies
    /// to. A change inside a workspace needs the current reference for this
    /// context (the workspace is the core's, never the draft's); a
    /// workspace's own update or archive needs a workspace this context
    /// listed; a provisioning and an integration change apply to the tenant
    /// (an integration create may also name the chosen workspace).
    pub(super) fn gateway_change_scope(
        &self,
        context: &ContextRef,
        resource: GatewayResource,
        action: crate::changes::ChangeAction,
        target: &str,
        reference: Option<&WorkspaceRef>,
    ) -> Result<ChangeScope, Refusal> {
        let selection = || {
            self.gateway
                .selection
                .as_ref()
                .filter(|s| &s.context == context && Some(&s.reference) == reference)
        };
        let none = ChangeScope {
            reference: None,
            scope: None,
            shown: None,
        };
        match resource {
            GatewayResource::Workspaces if action == crate::changes::ChangeAction::Create => {
                Ok(none)
            }
            GatewayResource::Workspaces => {
                let listed = self
                    .gateway
                    .listing
                    .as_ref()
                    .filter(|l| &l.context == context)
                    .and_then(|l| l.workspaces.iter().find(|w| w.id == target))
                    .ok_or(Refusal::InvalidInput)?;
                Ok(ChangeScope {
                    reference: None,
                    scope: Some(WorkspaceScope::from(listed)),
                    shown: Some(listed.clone()),
                })
            }
            GatewayResource::Integrations if reference.is_none() => Ok(none),
            _ => {
                let selection = selection().ok_or(Refusal::StaleContext)?;
                Ok(ChangeScope {
                    reference: Some(selection.reference.clone()),
                    scope: Some(WorkspaceScope::from(&selection.workspace)),
                    shown: Some(selection.workspace.clone()),
                })
            }
        }
    }
}

/// Whether `request` lists the workspaces (its result updates the choice).
pub(super) fn lists_workspaces(request: &ManagementRequest) -> bool {
    matches!(
        request,
        ManagementRequest::Gateway {
            read: GatewayRead::Inventory {
                resource: GatewayResource::Workspaces
            },
            ..
        }
    )
}
