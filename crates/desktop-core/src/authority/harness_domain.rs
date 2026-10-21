//! The native Harness conversation inside the authority (C19, D046).
//!
//! A conversation is admitted under the current tenant context and Gateway
//! workspace reference with the inference key stored for that workspace,
//! exactly as a Playground run is (`admit_key`). It is its own task, not
//! an operation slot: it may last hours. Every published change checks that
//! it is still bound (the same context, the chosen workspace and the key it
//! was started with); the moment it is not, it is retired: its view is
//! withdrawn and its engine is stopped and reaped. Ending it, sleep and
//! quitting stop it too. Nothing of it is kept once it is cleared.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::{Refusal, ScanCapability, State};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway::ListedWorkspace;
use crate::gateway_inference::InferenceFailure;
use crate::harness::session::{Command, Inputs, ProgressSlot, ProxyEnv, TurnEnd};
use crate::harness::sidecar::HarnessAdapters;
use crate::harness::{
    Cleanup, HarnessEntry, HarnessFailure, HarnessPhase, HarnessUsage, engine, protocol,
};
use crate::runtime::{OperationId, Outcome};
use crate::tenants::TenantId;
use crate::transport::ProxyMode;
use crate::vault::{RecordId, SessionVault};

/// Why a conversation was stopped other than by failing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HarnessStop {
    /// The operator ended it.
    Ended,
    /// The Mac went to sleep.
    Sleep,
    /// The app quit.
    Quit,
    /// It reached its eight-hour bound.
    Deadline,
}

pub(super) struct Conversation {
    id: OperationId,
    context: ContextRef,
    tenant: TenantId,
    reference: WorkspaceRef,
    workspace: ListedWorkspace,
    record: RecordId,
    endpoint: String,
    commands: mpsc::Sender<Command>,
    cancel: CancellationToken,
    progress: ProgressSlot,
    stop: Option<HarnessStop>,
    /// Its task returned (the engine is reaped and its home removed, or
    /// cleanup failed and says so).
    ended: Option<Outcome>,
    /// No longer bound: withdrawn from every view at once.
    retired: bool,
    entries: u64,
}

/// What a dispatch starts outside the lock.
pub(super) struct HarnessStart {
    pub(super) id: OperationId,
    pub(super) inputs: Inputs,
    pub(super) cancel: CancellationToken,
}

/// The bundled engine, as the presentation sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessEngineView {
    /// The engine is bundled with this build.
    pub available: bool,
    pub protocol: String,
    pub runtime_version: String,
    pub engine_commit: String,
}

/// The conversation, while it is bound to the active context, the chosen
/// workspace and the key it was started with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ConversationView {
    pub id: OperationId,
    pub workspace: ListedWorkspace,
    /// The only endpoint the engine is given.
    pub endpoint: String,
    pub phase: HarnessPhase,
    pub failure: Option<HarnessFailure>,
    pub stop: Option<HarnessStop>,
    pub entries: Vec<HarnessEntry>,
    pub last_turn: Option<TurnEnd>,
    pub usage: Option<HarnessUsage>,
    pub cleanup: Option<Cleanup>,
    pub runtime_version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessView {
    pub engine: HarnessEngineView,
    pub conversation: Option<ConversationView>,
}

fn key_failure(f: InferenceFailure) -> HarnessFailure {
    match f {
        InferenceFailure::CredentialLocked => HarnessFailure::CredentialLocked,
        InferenceFailure::CredentialDenied => HarnessFailure::CredentialDenied,
        _ => HarnessFailure::CredentialMissing,
    }
}

impl State {
    /// Admits a conversation in the chosen workspace with its stored key.
    pub(super) fn start_harness(
        &mut self,
        context: ContextRef,
        reference: WorkspaceRef,
        adapters: &HarnessAdapters,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<HarnessStart, Refusal> {
        let admission = self.admit_key(&context, &reference, keychain, session_vault)?;
        let (Some(engine), Some(runtime_root)) =
            (adapters.engine.clone(), adapters.runtime_root.clone())
        else {
            return Err(Refusal::CapabilityMissing);
        };
        // The engine names a workspace by UUID; a Gateway workspace ID is one.
        if !protocol::is_uuid(&admission.workspace.id) {
            return Err(Refusal::InvalidInput);
        }
        if self.harness.as_ref().is_some_and(|c| c.ended.is_none()) {
            return Err(Refusal::Busy);
        }
        let policy = self
            .transport
            .as_ref()
            .ok_or(Refusal::CapabilityMissing)?
            .policy()
            .clone();
        let proxy = match policy.proxy {
            ProxyMode::Direct => ProxyEnv::Direct,
            ProxyMode::System => ProxyEnv::System,
            ProxyMode::Explicit { url } => {
                let parsed = url::Url::parse(&url).map_err(|_| Refusal::CapabilityMissing)?;
                if !parsed.username().is_empty() || parsed.password().is_some() {
                    return Err(Refusal::CapabilityMissing);
                }
                ProxyEnv::Explicit(url)
            }
        };
        let binding = protocol::Binding {
            tenant: protocol::derived_uuid("tenant", admission.tenant.as_str()),
            workspace: admission.workspace.id.clone(),
            credential: protocol::derived_uuid("credential", admission.record.as_str()),
            credential_epoch: 1,
            endpoint: admission.endpoint.clone(),
        };
        let id = self.operations.reserve();
        let (commands, receiver) = mpsc::channel(4);
        let cancel = CancellationToken::new();
        let progress = ProgressSlot::default();
        let read = admission.key;
        let inputs = Inputs {
            engine,
            pin: adapters.pin(),
            launcher: Arc::clone(&adapters.launcher),
            runtime_root,
            roots: policy.extra_roots.clone(),
            proxy,
            binding,
            key: Box::new(move || read().map_err(key_failure)),
            commands: receiver,
            progress: Arc::clone(&progress),
            pulse: Arc::clone(&self.stream_pulse),
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            bootstrap_timeout: adapters.bootstrap_timeout(),
        };
        self.harness = Some(Conversation {
            id: id.clone(),
            context,
            tenant: admission.tenant,
            reference,
            workspace: admission.workspace,
            record: admission.record,
            endpoint: admission.endpoint,
            commands,
            cancel: cancel.clone(),
            progress,
            stop: None,
            ended: None,
            retired: false,
            entries: 0,
        });
        Ok(HarnessStart { id, inputs, cancel })
    }

    /// The bound, unended conversation `id`.
    fn live_harness(&mut self, id: &OperationId) -> Result<&mut Conversation, Refusal> {
        let bound = self.harness.as_ref().is_some_and(|c| self.harness_bound(c));
        let conversation = self
            .harness
            .as_mut()
            .filter(|c| &c.id == id)
            .ok_or(Refusal::InvalidInput)?;
        if !bound {
            return Err(Refusal::StaleContext);
        }
        if conversation.ended.is_some() || conversation.stop.is_some() {
            return Err(Refusal::InvalidInput);
        }
        Ok(conversation)
    }

    /// Sends one prompt; only while the conversation is ready.
    pub(super) fn send_harness(&mut self, id: &OperationId, prompt: String) -> Result<(), Refusal> {
        let prompt = Zeroizing::new(prompt);
        if prompt.trim().is_empty() || prompt.len() > protocol::MAX_PROMPT_BYTES {
            return Err(Refusal::InvalidInput);
        }
        let conversation = self.live_harness(id)?;
        if conversation
            .progress
            .lock()
            .map(|p| p.phase != HarnessPhase::Ready)
            .unwrap_or(true)
        {
            return Err(Refusal::Busy);
        }
        conversation.entries += 1;
        let entry = format!("u-{}", conversation.entries);
        // Ready until the task takes it: nothing else may be queued.
        if let Ok(mut p) = conversation.progress.lock() {
            p.phase = HarnessPhase::Answering;
        }
        conversation
            .commands
            .try_send(Command::Send { id: entry, prompt })
            .map_err(|_| Refusal::Busy)
    }

    pub(super) fn interrupt_harness(&mut self, id: &OperationId) -> Result<(), Refusal> {
        let conversation = self.live_harness(id)?;
        conversation
            .commands
            .try_send(Command::Interrupt)
            .map_err(|_| Refusal::Busy)
    }

    /// Ends the conversation: the engine is stopped and reaped.
    pub(super) fn end_harness(&mut self, id: &OperationId) -> Result<(), Refusal> {
        let conversation = self.live_harness(id)?;
        conversation.stop = Some(HarnessStop::Ended);
        conversation.cancel.cancel();
        Ok(())
    }

    /// Stops whatever conversation runs (sleep, quit), keeping it shown.
    pub(super) fn stop_harness(&mut self, why: HarnessStop) -> bool {
        match self.harness.as_mut().filter(|c| c.ended.is_none()) {
            Some(c) => {
                c.stop.get_or_insert(why);
                c.cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// Clears an ended conversation (nothing of it is kept).
    pub(super) fn clear_harness(&mut self, id: &OperationId) -> Result<(), Refusal> {
        if !self
            .harness
            .as_ref()
            .is_some_and(|c| &c.id == id && c.ended.is_some())
        {
            return Err(Refusal::InvalidInput);
        }
        self.harness = None;
        Ok(())
    }

    /// The conversation's task returned.
    pub(super) fn harness_settled(&mut self, id: &OperationId, outcome: Outcome) {
        if let Some(c) = self.harness.as_mut().filter(|c| &c.id == id) {
            c.ended = Some(outcome);
            // Ended without being asked to and without failing: its
            // eight-hour bound passed.
            let failed = c
                .progress
                .lock()
                .map(|p| p.failure.is_some())
                .unwrap_or(true);
            if c.stop.is_none() && !c.retired && !failed {
                c.stop = Some(HarnessStop::Deadline);
            }
            if c.retired {
                self.harness = None;
            }
        }
    }

    /// Whether the conversation still belongs to the active context, the
    /// chosen workspace and the key it was started with.
    fn harness_bound(&self, c: &Conversation) -> bool {
        !c.retired
            && Some(&c.context) == self.active_ref.as_ref()
            && self.workspace_current(Some(&c.reference))
            && self.tenant(&c.tenant).is_some_and(|t| {
                matches!(&t.inference, ScanCapability::Ready { record, .. } if record == &c.record)
            })
    }

    /// Retires a conversation that is no longer bound (run on every change).
    pub(super) fn sync_harness(&mut self) {
        let bound = self.harness.as_ref().is_some_and(|c| self.harness_bound(c));
        let Some(c) = self.harness.as_mut() else {
            return;
        };
        if !bound {
            c.retired = true;
            c.cancel.cancel();
            if c.ended.is_some() {
                self.harness = None;
            }
        }
    }

    #[cfg(feature = "test-fixtures")]
    pub(super) fn harness_pid(&self) -> Option<u32> {
        self.harness.as_ref()?.progress.lock().ok()?.pid
    }

    /// Whether a conversation's task has not returned yet (quit waits).
    pub(super) fn harness_running(&self) -> bool {
        self.harness.as_ref().is_some_and(|c| c.ended.is_none())
    }

    pub(super) fn harness_view(&self, adapters_engine: bool) -> HarnessView {
        let pin = engine::pin();
        let conversation = self
            .harness
            .as_ref()
            .filter(|c| self.harness_bound(c))
            .and_then(|c| {
                let p = c.progress.lock().ok()?.clone();
                let phase = match (c.ended, p.phase) {
                    // Shown as ending until the task has returned.
                    (None, HarnessPhase::Ended | HarnessPhase::Failed) => HarnessPhase::Ending,
                    (None, _) if c.stop.is_some() => HarnessPhase::Ending,
                    (_, phase) => phase,
                };
                Some(ConversationView {
                    id: c.id.clone(),
                    workspace: c.workspace.clone(),
                    endpoint: c.endpoint.clone(),
                    phase,
                    failure: p.failure,
                    stop: c.stop,
                    entries: p.entries,
                    last_turn: p.last_turn,
                    usage: p.usage,
                    cleanup: p.cleanup,
                    runtime_version: p.runtime_version,
                })
            });
        HarnessView {
            engine: HarnessEngineView {
                available: adapters_engine,
                protocol: pin.protocol,
                runtime_version: pin.runtime_version,
                engine_commit: pin.engine_commit,
            },
            conversation,
        }
    }
}
