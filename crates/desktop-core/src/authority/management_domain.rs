//! Management reads inside the authority (C08): one binding per credential
//! generation (its own SDK session, reused with the SDK's token cache until
//! the context retires), operations admitted against the active context, and
//! a single observation of each result. A result for a context that retired
//! while the read ran is never shown (a replaced credential mid-pagination
//! restarts from the first page).

use std::sync::{Arc, Mutex};

use scm_sdk::Session;
use tokio::sync::Notify;
use tokio::time::Instant;

use super::{Refusal, ScanCapability, State};
use crate::context::{ContextRef, WorkspaceRef};
use crate::management::{
    BindingCell, BindingSource, ManagementFailure, ManagementJob, ManagementRequest,
    ManagementResult, ResultSlot,
};
use crate::runtime::{OperationId, OperationState, Outcome, Settlement};
use crate::sessions::{Investigation, Persistence};
use crate::vault::SessionVault;
use crate::vault::StorageMode;

/// Settled results kept for observation.
const RETAINED_RESULTS: usize = 64;

#[derive(Default)]
struct Cell {
    result: Mutex<Option<ManagementResult>>,
    ready: Notify,
}

/// A single-use wait for one management read's result.
pub struct ManagementHandle(Arc<Cell>);

impl std::fmt::Debug for ManagementHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ManagementHandle")
    }
}

impl ManagementHandle {
    pub async fn result(self) -> ManagementResult {
        loop {
            let notified = self.0.ready.notified();
            if let Some(result) = self.0.result.lock().ok().and_then(|r| r.clone()) {
                return result;
            }
            notified.await;
        }
    }
}

pub(super) struct ManagementRecord {
    operation: OperationId,
    context: ContextRef,
    slot: ResultSlot,
    cell: Arc<Cell>,
    settled: bool,
    observed: bool,
    /// A Runtime overview read: its result also updates the shell's
    /// Home/Inbox state.
    overview: bool,
    /// The Gateway workspace selection a scoped Gateway read was admitted
    /// under: its result is withdrawn once another is chosen (C16).
    workspace: Option<WorkspaceRef>,
    /// A Gateway Workspaces listing: its result updates the choice.
    lists_workspaces: bool,
}

/// The binding of one credential generation (context): its session and
/// its lazily built clients.
pub(super) struct GenerationBinding {
    context: ContextRef,
    session: Session,
    cell: BindingCell,
    pub(super) investigation: Arc<Investigation>,
}

impl State {
    fn management_binding(
        &mut self,
        context: &ContextRef,
        persistence: impl FnOnce() -> Option<Persistence>,
    ) -> (Session, BindingCell, Arc<Investigation>) {
        if let Some(b) = self
            .management_bindings
            .iter()
            .find(|b| &b.context == context)
        {
            return (
                b.session.clone(),
                Arc::clone(&b.cell),
                Arc::clone(&b.investigation),
            );
        }
        let binding = GenerationBinding {
            context: context.clone(),
            session: Session::new(),
            cell: BindingCell::default(),
            investigation: Investigation::new(persistence()),
        };
        let triple = (
            binding.session.clone(),
            Arc::clone(&binding.cell),
            Arc::clone(&binding.investigation),
        );
        self.management_bindings.push(binding);
        triple
    }

    /// The active context's investigation, when its binding exists.
    pub(super) fn investigation(&self, context: &ContextRef) -> Option<Arc<Investigation>> {
        self.management_bindings
            .iter()
            .find(|b| &b.context == context)
            .map(|b| Arc::clone(&b.investigation))
    }

    /// Cancels every binding but the current context's (its session ends
    /// in-flight reads; its token cache is dropped with it).
    pub(super) fn retire_management_bindings(&mut self) {
        let keep = self.active_ref.clone();
        self.management_bindings.retain(|b| {
            let current = Some(&b.context) == keep.as_ref();
            if !current {
                b.session.cancel();
                // A write already admitted for this cache finds it retired
                // and places nothing (the credential's cleanup may follow).
                b.investigation.retire();
            }
            current
        });
    }

    /// Admits one management read for the active tenant's management
    /// capability. The scan key is never consulted.
    /// The active tenant's management binding inputs, checked: a ready
    /// management credential, its client ID and TSG, and the configured
    /// token origin equal to the one the credential was stored for. The scan
    /// key is never consulted.
    pub(super) fn binding_source(
        &mut self,
        context: &ContextRef,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<BindingSource, Refusal> {
        let tenant = self.check_context(context)?;
        let current = self.tenant(&tenant).ok_or(Refusal::StaleContext)?;
        let (record, mode, destination) = match &current.management {
            ScanCapability::Ready {
                record,
                mode,
                destination,
            } => (record.clone(), *mode, destination.clone()),
            _ => return Err(Refusal::CapabilityMissing),
        };
        let client_id = current
            .management_client
            .clone()
            .ok_or(Refusal::CapabilityMissing)?;
        let tsg = current
            .profile
            .tsg_id
            .clone()
            .ok_or(Refusal::CapabilityMissing)?;
        // The credential is bound to the token origin it was stored for; a
        // different configured token origin never receives it.
        let endpoints = self.management_endpoints.clone();
        if endpoints.token_origin().as_deref() != Some(destination.as_str()) {
            return Err(Refusal::CapabilityMissing);
        }
        let transport = self
            .transport
            .clone()
            .ok_or(Refusal::CapabilityMissing)?
            .for_admission()
            .map_err(|_| Refusal::CapabilityMissing)?;
        // Only a Keychain-stored credential's investigation is sealed on
        // disk (its key in the same namespace); a session credential's stays
        // in memory with its generation.
        let root = self.sessions_root.clone();
        let sealing: Arc<dyn crate::vault::Vault> = keychain.clone();
        let credential = record.clone();
        let (session, binding, investigation) = self.management_binding(context, || {
            (mode == StorageMode::Keychain)
                .then_some(root)
                .flatten()
                .map(|root| Persistence {
                    root,
                    credential,
                    keychain: sealing,
                })
        });
        Ok(BindingSource {
            keychain,
            session_vault,
            record,
            mode,
            client_id,
            tsg,
            endpoints,
            transport,
            session,
            binding,
            investigation,
        })
    }

    /// Admits one management read for the active tenant's management
    /// capability.
    pub(super) fn admit_management(
        &mut self,
        context: ContextRef,
        request: ManagementRequest,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), Refusal> {
        self.check_context(&context)?;
        if !request.is_valid() {
            return Err(Refusal::InvalidInput);
        }
        let gateway = self.gateway_admission(&context, &request)?;
        let source = self.binding_source(&context, keychain, session_vault)?;
        let overview = request == ManagementRequest::Overview;
        let lists_workspaces = super::gateway_domain::lists_workspaces(&request);
        let (domain, workspace, scope) = match gateway {
            Some(g) => (g.domain, g.workspace, g.scope),
            // An overview also reads the chosen Gateway workspace's
            // activity (C18); it is labelled with that workspace.
            None if overview => (
                context.as_str().to_owned(),
                None,
                self.chosen_scope(&context),
            ),
            None => (context.as_str().to_owned(), None, None),
        };
        let slot: ResultSlot = Arc::default();
        let job = ManagementJob {
            source,
            request,
            slot: Arc::clone(&slot),
            workspace: scope,
        };
        let (operation, starts) =
            self.operations
                .admit(&domain, 0, job.into_work(), Instant::now())?;
        self.management_calls.push(ManagementRecord {
            operation: operation.clone(),
            context,
            slot,
            cell: Arc::default(),
            settled: false,
            observed: false,
            overview,
            workspace,
            lists_workspaces,
        });
        if overview {
            let context = self.management_calls.last().map(|r| r.context.clone());
            if let Some(context) = context {
                self.overview_started(&context);
            }
        }
        Ok((operation, starts))
    }

    /// Delivers settled management results to their single observation.
    pub(super) fn sync_management_calls(&mut self) {
        let current = self.active_ref.clone();
        let mut overviews = Vec::new();
        let mut listings = Vec::new();
        let selected = self.gateway_selection();
        for record in self.management_calls.iter_mut().filter(|r| !r.settled) {
            let settlement = match self.operations.state(&record.operation) {
                Some(OperationState::Settled(s)) => s,
                None => Settlement {
                    outcome: Outcome::Unknown,
                    stop: None,
                },
                Some(_) => continue,
            };
            record.settled = true;
            let reply = record.slot.lock().ok().and_then(|mut s| s.take());
            let retired = current.as_ref() != Some(&record.context)
                || record
                    .workspace
                    .as_ref()
                    .is_some_and(|w| selected.as_ref() != Some(w));
            let result = match (reply, retired, settlement.stop) {
                // A reply for a retired context is never shown.
                (_, true, _) | (_, _, Some(_)) | (None, _, _) => ManagementResult::Failed {
                    failure: ManagementFailure::Cancelled,
                    outcome: settlement.outcome,
                },
                (Some(result), false, None) => result,
            };
            if record.overview {
                overviews.push((record.context.clone(), result.clone()));
            }
            if record.lists_workspaces
                && let ManagementResult::Gateway {
                    gateway: crate::gateway::GatewayResult::List { list },
                } = &result
            {
                listings.push((record.context.clone(), list.clone()));
            }
            if let Ok(mut slot) = record.cell.result.lock() {
                *slot = Some(result);
            }
            record.cell.ready.notify_waiters();
        }
        for (context, result) in overviews {
            self.overview_settled(&context, result);
        }
        for (context, list) in listings {
            self.gateway_listed(&context, &list);
        }
        let mut settled = 0;
        let mut keep = vec![true; self.management_calls.len()];
        for (i, record) in self.management_calls.iter().enumerate().rev() {
            if record.settled {
                settled += 1;
                keep[i] = !record.observed && settled <= RETAINED_RESULTS;
            }
        }
        let mut index = 0;
        self.management_calls.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }

    /// The single observation of a management read's result.
    pub(super) fn observe_management(
        &mut self,
        operation: &OperationId,
    ) -> Result<ManagementHandle, Refusal> {
        let (context, workspace) = self
            .management_calls
            .iter()
            .find(|r| &r.operation == operation && !r.observed)
            .map(|r| (r.context.clone(), r.workspace.clone()))
            .ok_or(Refusal::InvalidInput)?;
        // A result whose context (or Gateway workspace choice) retired
        // before it was observed is withdrawn (it may have settled while it
        // was current).
        let current = self.active_ref.as_ref() == Some(&context)
            && self.workspace_current(workspace.as_ref());
        let record = self
            .management_calls
            .iter_mut()
            .find(|r| &r.operation == operation)
            .ok_or(Refusal::InvalidInput)?;
        record.observed = true;
        if record.settled
            && !current
            && let Ok(mut slot) = record.cell.result.lock()
            && let Some(result) = slot.take()
        {
            *slot = Some(withdrawn(result));
        }
        let handle = ManagementHandle(Arc::clone(&record.cell));
        if record.settled {
            self.sync_management_calls();
        }
        Ok(handle)
    }
}

/// What a retired context's undelivered result becomes: cancelled, keeping
/// the remote outcome (a completed read did reach the service).
fn withdrawn(result: ManagementResult) -> ManagementResult {
    let outcome = match result {
        ManagementResult::Failed { outcome, .. } => outcome,
        _ => Outcome::Completed,
    };
    ManagementResult::Failed {
        failure: ManagementFailure::Cancelled,
        outcome,
    }
}
