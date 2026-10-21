//! Gateway inference inside the authority (C18, D044): the tenant's
//! workspace inference key (its own credential class, bound to the
//! workspace and runtime endpoint it was stored for) and the Playground's
//! single streamed run. A run is admitted only under the current tenant
//! context and Gateway workspace reference, with the key stored for that
//! workspace; it runs as an operation in the reference's domain, so a
//! context change or another choice of workspace stops it, and replacing
//! or removing the key stops it too. Its output is shown only while it is
//! still bound; nothing of it is kept but a record of how it ended.

use std::sync::Arc;

use tokio::time::Instant;

use serde::Serialize;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::{Refusal, ScanCapability, State};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway::ListedWorkspace;
use crate::gateway_inference::{
    InferenceFailure, InferenceRun, InferenceTarget, ProgressSlot, RunEnd, RunInputs, TokenUsage,
};
use crate::runtime::{OperationId, OperationState, Outcome, StopReason};
use crate::tenants::TenantId;
use crate::vault::{RecordId, SessionVault, StorageMode, Vault, VaultError};

/// The Playground's run.
pub(super) struct Run {
    operation: OperationId,
    context: ContextRef,
    tenant: TenantId,
    reference: WorkspaceRef,
    workspace: ListedWorkspace,
    record: RecordId,
    run: InferenceRun,
    trace: String,
    progress: ProgressSlot,
    settled: Option<Outcome>,
    /// Kept on this Mac once it ended (T23).
    kept: bool,
    /// Its key was replaced or removed: from that moment it and its
    /// output are no longer shown, even before its stream has ended.
    retired: bool,
}

/// Where a run stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PlaygroundState {
    Running,
    Completed,
    /// The Gateway ended the answer before it was complete (its reason is
    /// the view's `finish`).
    Incomplete,
    /// The output reached its bound and the stream was closed.
    Truncated,
    Stopped,
    Failed,
}

/// The Playground's current run, as long as it is bound to the active
/// context and the chosen workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlaygroundView {
    pub operation: OperationId,
    pub workspace: ListedWorkspace,
    pub api: crate::gateway_inference::InferenceApi,
    pub model: String,
    pub route: Option<String>,
    /// The Gateway file attached (a Responses run).
    pub file: Option<String>,
    /// Sent as `x-portkey-trace-id`: find the request in the logs by it.
    pub trace: String,
    pub state: PlaygroundState,
    pub output: String,
    pub outcome: Option<Outcome>,
    pub failure: Option<InferenceFailure>,
    /// The request was handed to the Gateway (it may have been charged).
    pub sent: bool,
    /// The Gateway answered.
    pub answered: bool,
    /// The model the Gateway reported serving.
    pub served_model: Option<String>,
    pub response_id: Option<String>,
    pub finish: Option<String>,
    pub usage: Option<TokenUsage>,
}

/// Reads the inference key off the lock, in the job.
pub(super) type KeyReader = Box<dyn FnOnce() -> Result<Zeroizing<String>, InferenceFailure> + Send>;

/// What a request with the inference key needs, resolved under the current
/// context and workspace reference.
pub(super) struct KeyAdmission {
    pub(super) tenant: TenantId,
    pub(super) workspace: ListedWorkspace,
    pub(super) record: RecordId,
    pub(super) endpoint: String,
    pub(super) transport: Arc<dyn scm_sdk::transport::Transport>,
    pub(super) key: KeyReader,
}

pub(super) fn trace_id() -> Option<String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).ok()?;
    Some(format!("airs-{}", hex::encode(bytes)))
}

impl State {
    /// The chosen workspace an inference key would be stored for: the
    /// current selection, named by its reference.
    pub(super) fn inference_workspace(
        &self,
        context: &ContextRef,
        reference: &WorkspaceRef,
    ) -> Result<ListedWorkspace, Refusal> {
        self.check_context(context)?;
        self.selected_workspace(context, reference)
            .ok_or(Refusal::StaleContext)
    }

    /// Withdraws the tenant's run and files view and stops whatever of
    /// them is still going (its key was replaced or removed).
    pub(super) fn retire_inference(&mut self, tenant: &TenantId) -> Vec<crate::runtime::Start> {
        let mut starts = self.retire_files(tenant);
        let Some(run) = self.playground.as_mut().filter(|r| &r.tenant == tenant) else {
            return starts;
        };
        run.retired = true;
        if run.settled.is_some() {
            return starts;
        }
        let operation = run.operation.clone();
        starts.extend(
            self.operations
                .cancel(&operation, StopReason::Cancelled, Instant::now())
                .1,
        );
        starts
    }

    /// The key stored for the workspace `reference` names, under the
    /// current context: its record, the endpoint it may be sent to and a
    /// reader for it (both runs and file requests are admitted with this).
    pub(super) fn admit_key(
        &self,
        context: &ContextRef,
        reference: &WorkspaceRef,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<KeyAdmission, Refusal> {
        let tenant = self.check_context(context)?;
        let workspace = self.inference_workspace(context, reference)?;
        let current = self.tenant(&tenant).ok_or(Refusal::StaleContext)?;
        let (record, mode, destination) = match &current.inference {
            ScanCapability::Ready {
                record,
                mode,
                destination,
            } => (record.clone(), *mode, destination.clone()),
            _ => return Err(Refusal::CapabilityMissing),
        };
        // The key goes only to the endpoint it was stored with, and only
        // for the workspace it was stored for.
        let target = current
            .inference_target
            .clone()
            .filter(|t| t.workspace.id == workspace.id && t.endpoint == destination)
            .ok_or(Refusal::CapabilityMissing)?;
        let transport = self
            .transport
            .clone()
            .ok_or(Refusal::CapabilityMissing)?
            .for_admission()
            .map_err(|_| Refusal::CapabilityMissing)?;
        let key_record = record.clone();
        let key = Box::new(move || {
            let read = match mode {
                StorageMode::Keychain => keychain.get(&key_record),
                StorageMode::Session => session_vault.get(&key_record),
            };
            match read {
                Ok(bytes) => std::str::from_utf8(&bytes)
                    .map(|text| Zeroizing::new(text.to_owned()))
                    .map_err(|_| InferenceFailure::CredentialMissing),
                Err(VaultError::Locked) => Err(InferenceFailure::CredentialLocked),
                Err(VaultError::Denied) => Err(InferenceFailure::CredentialDenied),
                Err(_) => Err(InferenceFailure::CredentialMissing),
            }
        });
        Ok(KeyAdmission {
            tenant,
            workspace,
            record,
            endpoint: target.endpoint,
            transport,
            key,
        })
    }

    /// Admits one Playground run under the current context and workspace
    /// reference with the key stored for that workspace. An attached file
    /// must be one the files last read for that workspace, with that key,
    /// listed.
    pub(super) fn run_inference(
        &mut self,
        context: ContextRef,
        reference: WorkspaceRef,
        run: InferenceRun,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), Refusal> {
        let KeyAdmission {
            tenant,
            workspace,
            record,
            endpoint,
            transport,
            key,
        } = self.admit_key(&context, &reference, keychain, session_vault)?;
        if !run.is_valid() {
            return Err(Refusal::InvalidInput);
        }
        if let Some(file) = &run.file
            && !self.file_listed(&reference, &record, file)
        {
            return Err(Refusal::InvalidInput);
        }
        if self
            .playground
            .as_ref()
            .is_some_and(|r| r.settled.is_none() && self.operations.state(&r.operation).is_some())
        {
            return Err(Refusal::Busy);
        }
        let trace = trace_id().ok_or(Refusal::Busy)?;
        let progress = ProgressSlot::default();
        let inputs = RunInputs {
            endpoint,
            trace: trace.clone(),
            run: run.clone(),
            key,
            transport,
            progress: Arc::clone(&progress),
            pulse: Arc::clone(&self.stream_pulse),
        };
        let work: crate::runtime::Work = Box::new(move |claim| {
            Box::pin(crate::gateway_inference::run(
                inputs,
                claim.cancel,
                claim.deadline,
            ))
        });
        let (operation, starts) = self.operations.admit_for(
            reference.as_str(),
            0,
            work,
            Instant::now(),
            crate::gateway_inference::RUN_DEADLINE,
        )?;
        self.playground = Some(Run {
            operation: operation.clone(),
            context,
            tenant,
            reference,
            workspace,
            record,
            run,
            trace,
            progress,
            settled: None,
            kept: false,
            retired: false,
        });
        Ok((operation, starts))
    }

    /// Stops the current run (the stream ends on this Mac; whether the
    /// Gateway charged for it is not established).
    pub(super) fn stop_inference(
        &mut self,
        operation: &OperationId,
    ) -> Result<Vec<crate::runtime::Start>, Refusal> {
        let run = self
            .playground
            .as_ref()
            .filter(|r| &r.operation == operation && r.settled.is_none())
            .ok_or(Refusal::InvalidInput)?;
        if Some(&run.context) != self.active_ref.as_ref() {
            return Err(Refusal::StaleContext);
        }
        Ok(self
            .operations
            .cancel(operation, StopReason::Cancelled, Instant::now())
            .1)
    }

    /// Clears a run that has ended.
    pub(super) fn clear_inference(&mut self, operation: &OperationId) -> Result<(), Refusal> {
        if !self
            .playground
            .as_ref()
            .is_some_and(|r| &r.operation == operation && r.settled.is_some())
        {
            return Err(Refusal::InvalidInput);
        }
        self.playground = None;
        Ok(())
    }

    /// Settles the run once its operation has; a run no longer bound to
    /// the active context, the chosen workspace or the key it was admitted
    /// with is dropped with its output (its end is still kept on this Mac).
    /// Whether `run` may still be shown: under the active context and the
    /// chosen workspace, with the key it was admitted with.
    fn run_bound(&self, run: &Run) -> bool {
        !run.retired
            && Some(&run.context) == self.active_ref.as_ref()
            && self.workspace_current(Some(&run.reference))
            && self.tenant(&run.tenant).is_some_and(|t| {
                matches!(&t.inference, ScanCapability::Ready { record, .. } if record == &run.record)
            })
    }

    pub(super) fn sync_inference(&mut self) {
        let bound = self.playground.as_ref().is_some_and(|r| self.run_bound(r));
        let Some(run) = self.playground.as_mut() else {
            return;
        };
        if run.settled.is_none() {
            match self.operations.state(&run.operation) {
                Some(OperationState::Settled(s)) => run.settled = Some(s.outcome),
                None => run.settled = Some(Outcome::Unknown),
                Some(_) => {}
            }
        }
        let kept = match (&run.settled, run.kept) {
            (Some(outcome), false) => {
                run.kept = true;
                let progress = run.progress.lock().map(|p| p.clone()).unwrap_or_default();
                Some((
                    run.tenant.clone(),
                    run.workspace.clone(),
                    run.run.clone(),
                    run.trace.clone(),
                    *outcome,
                    progress,
                ))
            }
            _ => None,
        };
        // Retirement (a context change, another workspace, the key
        // replaced or removed) has already stopped its operation; once it
        // settles, it and its output go.
        if !bound && run.settled.is_some() {
            self.playground = None;
        }
        if let Some((tenant, workspace, run, trace, outcome, progress)) = kept {
            self.inference_settled(&tenant, workspace, &run, trace, outcome, &progress);
        }
    }

    pub(super) fn playground_view(&self) -> Option<PlaygroundView> {
        let run = self.playground.as_ref().filter(|r| self.run_bound(r))?;
        let p = run.progress.lock().ok()?.clone();
        let state = match (run.settled, p.end) {
            (None, _) => PlaygroundState::Running,
            (Some(_), Some(RunEnd::Completed)) => PlaygroundState::Completed,
            (Some(_), Some(RunEnd::Incomplete)) => PlaygroundState::Incomplete,
            (Some(_), Some(RunEnd::Truncated)) => PlaygroundState::Truncated,
            (Some(_), Some(RunEnd::Failed)) => PlaygroundState::Failed,
            (Some(_), _) => PlaygroundState::Stopped,
        };
        Some(PlaygroundView {
            operation: run.operation.clone(),
            workspace: run.workspace.clone(),
            api: run.run.api,
            model: run.run.model.clone(),
            route: run.run.route.clone(),
            file: run.run.file.clone(),
            trace: run.trace.clone(),
            state,
            output: p.output,
            outcome: run.settled,
            failure: p.failure,
            sent: p.sent,
            answered: p.answered,
            served_model: p.model,
            response_id: p.response_id,
            finish: p.finish,
            usage: p.usage,
        })
    }
}

/// The inference key's target as the presentation sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InferenceKeyView {
    pub state: crate::authority::ScanState,
    pub mode: Option<StorageMode>,
    pub failure: Option<VaultError>,
    pub target: Option<InferenceTarget>,
}
