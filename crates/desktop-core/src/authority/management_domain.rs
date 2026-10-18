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
use crate::context::ContextRef;
use crate::management::{
    BindingCell, ManagementFailure, ManagementJob, ManagementRequest, ManagementResult, ResultSlot,
};
use crate::runtime::{OperationId, OperationState, Outcome, Settlement};
use crate::vault::SessionVault;

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
}

/// The binding of one credential generation (context): its session and
/// its lazily built clients.
pub(super) struct GenerationBinding {
    context: ContextRef,
    session: Session,
    cell: BindingCell,
}

impl State {
    fn management_binding(&mut self, context: &ContextRef) -> (Session, BindingCell) {
        if let Some(b) = self
            .management_bindings
            .iter()
            .find(|b| &b.context == context)
        {
            return (b.session.clone(), Arc::clone(&b.cell));
        }
        let binding = GenerationBinding {
            context: context.clone(),
            session: Session::new(),
            cell: BindingCell::default(),
        };
        let pair = (binding.session.clone(), Arc::clone(&binding.cell));
        self.management_bindings.push(binding);
        pair
    }

    /// Cancels every binding but the current context's (its session ends
    /// in-flight reads; its token cache is dropped with it).
    pub(super) fn retire_management_bindings(&mut self) {
        let keep = self.active_ref.clone();
        self.management_bindings.retain(|b| {
            let current = Some(&b.context) == keep.as_ref();
            if !current {
                b.session.cancel();
            }
            current
        });
    }

    /// Admits one management read for the active tenant's management
    /// capability. The scan key is never consulted.
    pub(super) fn admit_management(
        &mut self,
        context: ContextRef,
        request: ManagementRequest,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), Refusal> {
        let tenant = self.check_context(&context)?;
        if !request.is_valid() {
            return Err(Refusal::InvalidInput);
        }
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
        let (session, binding) = self.management_binding(&context);
        let slot: ResultSlot = Arc::default();
        let job = ManagementJob {
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
            request,
            slot: Arc::clone(&slot),
        };
        let (operation, starts) =
            self.operations
                .admit(context.as_str(), 0, job.into_work(), Instant::now())?;
        self.management_calls.push(ManagementRecord {
            operation: operation.clone(),
            context,
            slot,
            cell: Arc::default(),
            settled: false,
            observed: false,
        });
        Ok((operation, starts))
    }

    /// Delivers settled management results to their single observation.
    pub(super) fn sync_management_calls(&mut self) {
        let current = self.active_ref.clone();
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
            let retired = current.as_ref() != Some(&record.context);
            let result = match (reply, retired, settlement.stop) {
                // A reply for a retired context is never shown.
                (_, true, _) | (_, _, Some(_)) | (None, _, _) => ManagementResult::Failed {
                    failure: ManagementFailure::Cancelled,
                    outcome: settlement.outcome,
                },
                (Some(result), false, None) => result,
            };
            if let Ok(mut slot) = record.cell.result.lock() {
                *slot = Some(result);
            }
            record.cell.ready.notify_waiters();
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
        let record = self
            .management_calls
            .iter_mut()
            .find(|r| &r.operation == operation && !r.observed)
            .ok_or(Refusal::InvalidInput)?;
        record.observed = true;
        let handle = ManagementHandle(Arc::clone(&record.cell));
        if record.settled {
            self.sync_management_calls();
        }
        Ok(handle)
    }
}
