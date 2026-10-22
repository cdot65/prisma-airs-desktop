//! The native Harness conversation inside the authority (C19, D046).
//!
//! A conversation is admitted under the current tenant context and Gateway
//! workspace reference with the inference key stored for that workspace,
//! exactly as a Playground run is (`admit_key`). It is its own task, not
//! an operation slot: it may last hours. Every published change checks that
//! it is still bound (the same context, the chosen workspace and the key it
//! was started with); the moment it is not, it is retired: its view is
//! withdrawn and its engine is stopped and reaped. Ending it, sleep and
//! quitting stop it too.
//!
//! C20: a conversation runs with either the workspace key or the tenant's
//! company sign-in for that workspace (its grant), whichever is chosen when
//! it starts; choosing the other, signing out or a new grant retires it.
//! Before each message, a sign-in token inside its renewal margin (wall
//! clock) is renewed and the engine restarted with it, the conversation
//! restored; a failed renewal gives the message back and nothing is sent.
//! After every turn the conversation is saved to protected history, and a
//! saved conversation with the same binding can be continued.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::harness_identity::{Grant, now};
use super::{Refusal, ScanCapability, State};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway::ListedWorkspace;
use crate::gateway_inference::InferenceFailure;
use crate::harness::history::{CredentialSource, HistoryFailure, SavedBinding, Scope};
use crate::harness::identity::SignInFailure;
use crate::harness::session::{Command, Inputs, ProgressSlot, ProxyEnv, Replay, TurnEnd};
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
    /// The workspace key it runs with (workspace key).
    record: Option<RecordId>,
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
    source: CredentialSource,
    /// The signed-in grant it runs under (company sign-in).
    grant: Option<Arc<Grant>>,
    /// When the engine's sign-in token must be renewed (wall clock).
    engine_renew_at: Option<u64>,
    /// A message given back because its renewal failed (shown in the box).
    unsent: Option<String>,
    renew_failure: Option<SignInFailure>,
    /// Its protected history record.
    pub(super) history_id: String,
    /// The project and access it runs with (C21).
    project: Option<String>,
    write: bool,
    /// Changes the Harness proposed for the operator's review (C21).
    proposals: Vec<super::harness_tools::Proposal>,
    /// A renewal waiting for the operator to allow changes again (C21).
    write_confirm: Option<RenewJob>,
    /// Its workspace MCP scope and the grants it holds (C22).
    mcp: Vec<crate::harness::mcp::ScopeServer>,
    mcp_grants: Vec<Option<Arc<super::harness_mcp::McpGrant>>>,
    /// When one of the MCP tokens must be renewed (wall clock), and what
    /// the running engine was given.
    mcp_renew_at: Option<u64>,
    mcp_fingerprint: Option<String>,
}

impl Conversation {
    pub(super) fn context_ref(&self) -> &ContextRef {
        &self.context
    }

    pub(super) fn tenant_id(&self) -> &TenantId {
        &self.tenant
    }

    pub(super) fn progress_approvals(&self) -> Vec<crate::harness::HarnessApproval> {
        self.progress
            .lock()
            .map(|p| p.approvals.clone())
            .unwrap_or_default()
    }

    pub(super) fn command(&self, command: Command) -> Result<(), Refusal> {
        self.commands.try_send(command).map_err(|_| Refusal::Busy)
    }

    pub(super) fn proposals_mut(&mut self) -> &mut Vec<super::harness_tools::Proposal> {
        &mut self.proposals
    }

    pub(super) fn take_write_confirmation(&mut self) -> Option<RenewJob> {
        self.write_confirm.take()
    }

    pub(super) fn set_phase(&self, phase: HarnessPhase) {
        if let Ok(mut p) = self.progress.lock() {
            p.phase = phase;
        }
    }

    /// A message that will not be sent goes back to the box.
    pub(super) fn give_back(&mut self, job: RenewJob) {
        self.entries -= 1;
        self.unsent = Some(job.prompt.to_string());
        self.set_phase(HarnessPhase::Ready);
    }
}

/// A saved conversation to continue.
pub(super) struct Resume {
    pub(super) id: String,
    pub(super) created: u64,
    pub(super) binding: SavedBinding,
    pub(super) entries: Vec<HarnessEntry>,
    pub(super) usage: Option<HarnessUsage>,
    pub(super) replay: Replay,
}

/// How a conversation's turns are saved (the sink is made with the core).
pub(super) struct HistoryPlan {
    pub(super) scope: Scope,
    pub(super) id: String,
    pub(super) created: u64,
    pub(super) binding: SavedBinding,
    pub(super) tenant: TenantId,
    pub(super) workspace: String,
}

/// A message waiting for its sign-in to be renewed.
pub(super) struct RenewJob {
    pub(super) conversation: OperationId,
    /// The Gateway sign-in to renew (company sign-in), if due.
    pub(super) grant: Option<Arc<Grant>>,
    /// The MCP scope's grants, when one of their tokens is due (C22).
    pub(super) mcp: Option<Vec<Option<Arc<super::harness_mcp::McpGrant>>>>,
    pub(super) id: String,
    pub(super) prompt: Zeroizing<String>,
}

/// What a dispatch starts outside the lock.
pub(super) struct HarnessStart {
    pub(super) id: OperationId,
    pub(super) inputs: Inputs,
    pub(super) cancel: CancellationToken,
    /// Company sign-in: the token is obtained (renewed if needed) first.
    pub(super) grant: Option<Arc<Grant>>,
    /// The MCP scope's grants: their tokens are obtained first (C22).
    pub(super) mcp: Vec<Option<Arc<super::harness_mcp::McpGrant>>>,
    pub(super) history: Option<HistoryPlan>,
    /// For the tool sink's results (C21).
    pub(super) commands: mpsc::Sender<Command>,
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
    /// The credential it runs with.
    pub source: CredentialSource,
    /// Its protected history record (a UUID).
    pub history_id: String,
    /// When it was last saved (wall-clock seconds), and why the last save
    /// did not happen.
    #[ts(type = "number | null")]
    pub saved_at: Option<u64>,
    pub save_failure: Option<HistoryFailure>,
    /// A message given back because renewing the sign-in failed.
    pub unsent: Option<String>,
    pub sign_in_failure: Option<SignInFailure>,
    /// The project and access it runs with (C21).
    pub project: Option<String>,
    pub write: bool,
    /// Approvals the engine is waiting for.
    pub approvals: Vec<crate::harness::HarnessApproval>,
    /// Changes it proposed for the operator's review.
    pub proposals: Vec<super::harness_tools::ProposalView>,
    /// Its workspace MCP servers' startup in the running engine (C22).
    pub mcp: Vec<crate::harness::McpServerStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessView {
    pub engine: HarnessEngineView,
    pub conversation: Option<ConversationView>,
    /// The active tenant's company sign-in (C20).
    pub sign_in: Option<super::harness_identity::HarnessSignInView>,
    /// The chosen workspace's protected history (C20).
    pub history: Option<super::harness_history::HarnessHistoryView>,
    /// The active tenant's project for the Harness (C21).
    pub project: Option<super::harness_tools::HarnessProjectView>,
    /// A folder just refused as its project, and so not chosen.
    pub project_refused: Option<String>,
    /// The chosen workspace's MCP servers (C22).
    pub mcp: Option<super::harness_mcp::HarnessMcpView>,
}

fn key_failure(f: InferenceFailure) -> HarnessFailure {
    match f {
        InferenceFailure::CredentialLocked => HarnessFailure::CredentialLocked,
        InferenceFailure::CredentialDenied => HarnessFailure::CredentialDenied,
        _ => HarnessFailure::CredentialMissing,
    }
}

impl State {
    /// Admits a conversation in the chosen workspace with the chosen
    /// credential: the company sign-in's grant when it is chosen for this
    /// workspace, otherwise the workspace's stored key. `resume` continues
    /// a saved conversation with exactly this binding.
    pub(super) fn start_harness(
        &mut self,
        context: ContextRef,
        reference: WorkspaceRef,
        adapters: &HarnessAdapters,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
        resume: Option<Resume>,
    ) -> Result<HarnessStart, Refusal> {
        let tenant = self.check_context(&context)?;
        let workspace = self.inference_workspace(&context, &reference)?;
        let (Some(engine), Some(runtime_root)) =
            (adapters.engine.clone(), adapters.runtime_root.clone())
        else {
            return Err(Refusal::CapabilityMissing);
        };
        // The engine names a workspace by UUID; a Gateway workspace ID is one.
        if !protocol::is_uuid(&workspace.id) {
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
        let (credential, endpoint, record, grant, key, source) =
            match self.harness_source(&tenant, &workspace) {
                Some((sign_in, grant)) => {
                    // Read in the job, after the token is renewed if needed.
                    let key: crate::harness::session::KeyReader =
                        Box::new(|| Err(HarnessFailure::SignInNeeded));
                    (
                        protocol::derived_uuid("sign-in-grant", &grant.id),
                        sign_in.target.endpoint,
                        None,
                        Some(grant),
                        key,
                        CredentialSource::CompanySignIn,
                    )
                }
                None => {
                    let admission =
                        self.admit_key(&context, &reference, keychain.clone(), session_vault)?;
                    let read = admission.key;
                    let key: crate::harness::session::KeyReader =
                        Box::new(move || read().map_err(key_failure));
                    (
                        protocol::derived_uuid("credential", admission.record.as_str()),
                        admission.endpoint,
                        Some(admission.record),
                        None,
                        key,
                        CredentialSource::WorkspaceKey,
                    )
                }
            };
        // The selected project (checked again: it may have moved since).
        let project = self.harness_project(&tenant);
        if let Some(p) = &project {
            let path = std::path::Path::new(&p.path);
            if path.canonicalize().ok().as_deref() != Some(path) || !path.is_dir() {
                return Err(Refusal::InvalidInput);
            }
        }
        // The workspace MCP scope, fixed for the conversation (C22).
        let (mcp_scope, mcp_grants) = self.mcp_scope(&tenant, &workspace.id)?;
        let binding = protocol::Binding {
            tenant: protocol::derived_uuid("tenant", tenant.as_str()),
            workspace: workspace.id.clone(),
            credential,
            credential_epoch: 1,
            endpoint: endpoint.clone(),
            project: project.as_ref().map(|p| p.path.clone()),
            write: project.as_ref().is_some_and(|p| p.write),
            mcp: mcp_scope.clone(),
        };
        let saved = SavedBinding {
            tenant: binding.tenant.clone(),
            workspace: binding.workspace.clone(),
            credential: binding.credential.clone(),
            credential_epoch: binding.credential_epoch,
            endpoint: binding.endpoint.clone(),
            source,
            project: binding.project.clone(),
            write: binding.write,
            mcp: mcp_scope.clone(),
        };
        // Only exactly the binding it was saved under continues it.
        if resume.as_ref().is_some_and(|r| r.binding != saved) {
            return Err(Refusal::StaleContext);
        }
        let (history_id, created, entries, usage, restore) = match resume {
            Some(r) => (r.id, r.created, r.entries, r.usage, Some(r.replay)),
            None => (
                new_history_id().ok_or(Refusal::CapabilityMissing)?,
                now(),
                Vec::new(),
                None,
                None,
            ),
        };
        let history_plan = adapters.history_root.as_ref().map(|root| HistoryPlan {
            scope: Scope::new(root, tenant.as_str(), &workspace.id),
            id: history_id.clone(),
            created,
            binding: saved.clone(),
            tenant: tenant.clone(),
            workspace: workspace.id.clone(),
        });
        let operator_entries = entries
            .iter()
            .filter(|e| e.role == crate::harness::HarnessRole::Operator)
            .count() as u64;
        let (binding_project, binding_write) = (binding.project.clone(), binding.write);
        let id = self.operations.reserve();
        let (commands, receiver) = mpsc::channel(4);
        let tool_commands = commands.clone();
        let cancel = CancellationToken::new();
        let progress = ProgressSlot::default();
        if let Ok(mut p) = progress.lock() {
            p.entries = entries;
            p.usage = usage;
        }
        let inputs = Inputs {
            engine,
            pin: adapters.pin(),
            launcher: Arc::clone(&adapters.launcher),
            runtime_root,
            roots: policy.extra_roots.clone(),
            proxy,
            binding,
            key,
            // Obtained in the job, off the lock.
            mcp: vec![None; mcp_grants.len()],
            commands: receiver,
            progress: Arc::clone(&progress),
            pulse: Arc::clone(&self.stream_pulse),
            client_version: env!("CARGO_PKG_VERSION").to_owned(),
            bootstrap_timeout: adapters.bootstrap_timeout(),
            history: None,
            restore,
            tools: None,
        };
        self.harness = Some(Conversation {
            id: id.clone(),
            context,
            tenant,
            reference,
            workspace,
            record,
            endpoint,
            commands,
            cancel: cancel.clone(),
            progress,
            stop: None,
            ended: None,
            retired: false,
            entries: operator_entries,
            source,
            grant: grant.clone(),
            engine_renew_at: None,
            unsent: None,
            renew_failure: None,
            history_id,
            project: binding_project,
            write: binding_write,
            proposals: Vec::new(),
            write_confirm: None,
            mcp: mcp_scope,
            mcp_grants: mcp_grants.clone(),
            mcp_renew_at: None,
            mcp_fingerprint: None,
        });
        Ok(HarnessStart {
            id,
            inputs,
            cancel,
            grant,
            mcp: mcp_grants,
            history: history_plan,
            commands: tool_commands,
        })
    }

    /// The engine's sign-in token was obtained (company sign-in).
    pub(super) fn harness_keyed(&mut self, id: &OperationId, renew_at: u64) {
        if let Some(c) = self.harness.as_mut().filter(|c| &c.id == id) {
            c.engine_renew_at = Some(renew_at);
        }
    }

    /// The engine's MCP tokens were obtained (C22): what it was given and
    /// when one must be renewed.
    pub(super) fn harness_mcp_keyed(
        &mut self,
        id: &OperationId,
        renew_at: Option<u64>,
        fingerprint: String,
    ) {
        if let Some(c) = self.harness.as_mut().filter(|c| &c.id == id) {
            c.mcp_renew_at = renew_at;
            c.mcp_fingerprint = Some(fingerprint);
        }
    }

    /// The tenant, workspace and MCP scope of conversation `id` (for an
    /// MCP tool approval's native check, C22).
    pub(super) fn harness_mcp_scope(
        &self,
        id: &OperationId,
    ) -> Option<(TenantId, String, Vec<crate::harness::mcp::ScopeServer>)> {
        let c = self.harness.as_ref().filter(|c| &c.id == id)?;
        Some((c.tenant.clone(), c.workspace.id.clone(), c.mcp.clone()))
    }

    /// The bound, unended conversation `id`.
    /// The bound, unended conversation `id` (for C21's tools and decisions).
    pub(super) fn harness_live(&mut self, id: &OperationId) -> Result<&mut Conversation, Refusal> {
        self.live_harness(id)
    }

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

    /// Sends one prompt; only while the conversation is ready. With
    /// company sign-in, a token inside its renewal margin is renewed first
    /// (the returned job): nothing is sent with it.
    pub(super) fn send_harness(
        &mut self,
        id: &OperationId,
        prompt: String,
    ) -> Result<Option<RenewJob>, Refusal> {
        let prompt = Zeroizing::new(prompt);
        if prompt.trim().is_empty() || prompt.len() > protocol::MAX_PROMPT_BYTES {
            return Err(Refusal::InvalidInput);
        }
        // A sign-in again to the same account replaces the live grant: the
        // current one renews.
        let signed_in = self
            .harness
            .as_ref()
            .filter(|c| &c.id == id && c.grant.is_some())
            .map(|c| c.tenant.clone());
        let live = signed_in.and_then(|tenant| self.grant(&tenant));
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
        conversation.unsent = None;
        conversation.renew_failure = None;
        let renew = match (live, conversation.engine_renew_at) {
            (Some(grant), Some(at)) if now() >= at => Some(grant),
            _ => None,
        };
        // The MCP tokens are renewed before a turn too (C22); only if one
        // changes is the engine restarted.
        let mcp_due = conversation.mcp_renew_at.is_some_and(|at| now() >= at);
        if renew.is_some() || mcp_due {
            if let Some(grant) = &renew {
                conversation.grant = Some(Arc::clone(grant));
            }
            let job = RenewJob {
                conversation: id.clone(),
                // A due Gateway renewal renews the MCP tokens with it.
                mcp: (mcp_due || renew.is_some())
                    .then(|| conversation.mcp_grants.clone())
                    .filter(|g| !g.is_empty()),
                grant: renew,
                id: entry,
                prompt,
            };
            // A Gateway credential restart re-grants changes in the project
            // only when the operator says so again (DESIGN, C21); an
            // MCP-only rotation keeps the confirmed access (C22).
            if job.grant.is_some() && conversation.write {
                conversation.write_confirm = Some(job);
                if let Ok(mut p) = conversation.progress.lock() {
                    p.phase = HarnessPhase::ConfirmWrite;
                }
                return Ok(None);
            }
            if let Ok(mut p) = conversation.progress.lock() {
                p.phase = HarnessPhase::Renewing;
            }
            return Ok(Some(job));
        }
        // Ready until the task takes it: nothing else may be queued.
        if let Ok(mut p) = conversation.progress.lock() {
            p.phase = HarnessPhase::Answering;
        }
        conversation
            .commands
            .try_send(Command::Send { id: entry, prompt })
            .map_err(|_| Refusal::Busy)?;
        Ok(None)
    }

    /// The renewal for a message ended: restart with the new token, or
    /// give the message back and say why.
    pub(super) fn harness_renewed(
        &mut self,
        job: RenewJob,
        renewed: Result<Option<(Zeroizing<String>, u64)>, SignInFailure>,
        mcp: Option<super::harness_mcp::ScopeTokens>,
    ) {
        let bound = self.harness.as_ref().is_some_and(|c| self.harness_bound(c));
        let Some(c) = self
            .harness
            .as_mut()
            .filter(|c| c.id == job.conversation && c.ended.is_none() && c.stop.is_none())
        else {
            return;
        };
        match renewed {
            Ok(key) if bound => {
                if let Some((_, renew_at)) = &key {
                    c.engine_renew_at = Some(*renew_at);
                }
                // MCP tokens restart the engine only when what it holds
                // changed; their next renewal time is kept either way.
                let mcp = mcp.and_then(|(tokens, renew_at)| {
                    c.mcp_renew_at = renew_at;
                    let print = crate::harness::mcp::fingerprint(&c.mcp, &tokens);
                    (c.mcp_fingerprint.as_deref() != Some(print.as_str())).then(|| {
                        c.mcp_fingerprint = Some(print);
                        tokens
                    })
                });
                let command = match (key, mcp) {
                    (None, None) => {
                        if let Ok(mut p) = c.progress.lock() {
                            p.phase = HarnessPhase::Answering;
                        }
                        Command::Send {
                            id: job.id,
                            prompt: job.prompt,
                        }
                    }
                    (key, mcp) => Command::Renew {
                        key: key.map(|(k, _)| k),
                        mcp,
                        id: job.id,
                        prompt: job.prompt,
                    },
                };
                if c.commands.try_send(command).is_err() {
                    c.cancel.cancel();
                }
            }
            Ok(_) => {}
            Err(failure) => {
                c.entries -= 1;
                c.unsent = Some(job.prompt.to_string());
                c.renew_failure = Some(failure);
                if let Ok(mut p) = c.progress.lock() {
                    p.phase = HarnessPhase::Ready;
                }
            }
        }
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
        let Some(tenant) = self.tenant(&c.tenant) else {
            return false;
        };
        // The credential chosen for its workspace now, and its identity.
        let sign_in = tenant
            .harness_sign_in
            .as_ref()
            .filter(|s| s.selected && s.target.workspace.id == c.workspace.id);
        let credential_ok = match (&c.grant, sign_in) {
            (Some(grant), Some(s)) => s
                .grant
                .as_ref()
                .is_some_and(|g| g.id == grant.id && s.target.endpoint == c.endpoint),
            (None, None) => matches!(
                &tenant.inference,
                ScanCapability::Ready { record, .. } if Some(record) == c.record.as_ref()
            ),
            _ => false,
        };
        // The project and its access as selected now (C21).
        let project = tenant.harness_project.as_ref();
        let project_ok = project.map(|p| &p.path) == c.project.as_ref()
            && project.is_some_and(|p| p.write) == c.write;
        // Its MCP servers: still on, with the same sign-ins and revisions
        // (C22). An expired sign-in alone keeps it.
        let mcp_ok = self.mcp_scope_current(&c.tenant, &c.workspace.id, &c.mcp);
        !c.retired
            && credential_ok
            && project_ok
            && mcp_ok
            && Some(&c.context) == self.active_ref.as_ref()
            && self.workspace_current(Some(&c.reference))
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
        let current_review = self.change_review_id();
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
                    source: c.source,
                    history_id: c.history_id.clone(),
                    saved_at: p.saved_at,
                    save_failure: p.save_failure,
                    unsent: c.unsent.clone(),
                    sign_in_failure: c.renew_failure,
                    project: c.project.clone(),
                    write: c.write,
                    approvals: p.approvals,
                    mcp: p.mcp,
                    proposals: c
                        .proposals
                        .iter()
                        .map(|x| {
                            let mut v = x.view.clone();
                            // In review only while that review is the open one.
                            if v.state == super::harness_tools::ProposalState::InReview
                                && v.review != current_review
                            {
                                v.state = super::harness_tools::ProposalState::Closed;
                            }
                            v
                        })
                        .collect(),
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
            sign_in: self.harness_sign_in_view(),
            history: self.harness_history_view(),
            project: self.harness_project_view(),
            project_refused: self.harness_project_refused(),
            mcp: self.harness_mcp_view(),
        }
    }

    /// The live conversation's history record, if it is in this scope.
    pub(super) fn harness_history_live(&self) -> Option<&str> {
        self.harness
            .as_ref()
            .filter(|c| c.ended.is_none())
            .map(|c| c.history_id.as_str())
    }
}

fn new_history_id() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(protocol::derived_uuid("conversation", &hex::encode(bytes)))
}
