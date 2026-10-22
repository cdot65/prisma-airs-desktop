//! Harness tools, approvals, proposals and project access inside the
//! authority (C21, D048).
//!
//! A tenant may select one project folder for the Harness (through the
//! native folder sheet), read-only unless the operator allows changes with
//! approval. Both are part of a conversation's binding: changing either
//! retires it, and a saved conversation continues only under the same.
//!
//! The engine's typed tool calls run here as reads through the desktop's
//! own management, scanning and case-library paths, admitted under the
//! conversation's context as the operator's own reads would be; results
//! are scrubbed and bounded before the model sees them. A proposal only
//! stages a change: the operator opens it into the existing review, and
//! only the native confirmation of that review applies anything. Approvals
//! for commands and file changes are decided by the operator, by the
//! desktop's own id for this run and turn; a stale id is refused.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use ts_rs::TS;

use super::harness_domain::RenewJob;
use super::{Effect, Inner, Refusal, State, lock};
use crate::changes::ChangeDraft;
use crate::context::{ContextRef, ReviewId};
use crate::harness::HarnessPhase;
use crate::harness::session::{Command, ToolRequest, ToolSink};
use crate::harness::tools::{self, ToolCall, UNAVAILABLE};
use crate::management::ManagementRequest;
use crate::runtime::OperationId;
use crate::store::StoredProject;
use crate::tenants::TenantId;

/// Proposals one conversation may hold.
pub const MAX_PROPOSALS: usize = 8;
/// How long a tool's read may take before it is answered as unavailable.
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);

/// Folders never offered as a project (relative to the home folder, or
/// absolute).
const REFUSED_HOME: [&str; 9] = [
    ".ssh", ".aws", ".gnupg", ".config", ".kube", ".docker", "Library", ".azure", ".netrc",
];
const REFUSED_ROOTS: [&str; 11] = [
    "/System",
    "/usr",
    "/bin",
    "/sbin",
    "/private/etc",
    "/private/var/db",
    "/private/var/root",
    "/Library",
    "/Applications",
    "/dev",
    "/cores",
];

/// Whether `path` (canonical) may be a Harness project: an absolute
/// directory, neither the root, the home folder itself, a credential or
/// configuration folder in it, an operating-system folder, nor the app's
/// own data.
pub fn project_allowed(
    path: &std::path::Path,
    home: Option<&std::path::Path>,
    data: Option<&std::path::Path>,
) -> bool {
    let Some(text) = path.to_str() else {
        return false;
    };
    if !path.is_absolute() || text.len() > 4096 || path.parent().is_none() || !path.is_dir() {
        return false;
    }
    if REFUSED_ROOTS.iter().any(|r| path.starts_with(r)) {
        return false;
    }
    if let Some(home) = home
        && (path == home || REFUSED_HOME.iter().any(|d| path.starts_with(home.join(d))))
    {
        return false;
    }
    if let Some(data) = data
        && (path.starts_with(data) || data.starts_with(path))
    {
        return false;
    }
    true
}

/// The selected project, as the presentation sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessProjectView {
    pub path: String,
    /// Changes in it are allowed, each with the operator's approval.
    pub write: bool,
}

/// Where a proposal stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProposalState {
    /// Staged by the Harness; nothing was sent.
    Staged,
    /// Opened into the operator's change review, which is still open.
    InReview,
    /// Its review ended (applied, discarded, expired or replaced).
    Closed,
    /// The operator dismissed it.
    Dismissed,
}

/// A change the Harness proposed, for the operator to review.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProposalView {
    pub id: String,
    pub summary: String,
    pub draft: ChangeDraft,
    pub state: ProposalState,
    /// The review it was opened into.
    pub review: Option<ReviewId>,
}

/// A staged proposal in a conversation.
pub(super) struct Proposal {
    pub(super) view: ProposalView,
}

impl State {
    /// The active tenant's project, if any.
    pub(super) fn harness_project(&self, tenant: &TenantId) -> Option<StoredProject> {
        self.tenant(tenant)?.harness_project.clone()
    }

    /// The folder just refused as the active tenant's project, if any.
    pub(super) fn harness_project_refused(&self) -> Option<String> {
        let tenant = self.data.active.as_ref()?;
        self.harness_project_refused
            .as_ref()
            .filter(|(t, _)| t == tenant)
            .map(|(_, p)| p.clone())
    }

    pub(super) fn harness_project_view(&self) -> Option<HarnessProjectView> {
        let tenant = self.data.active.as_ref()?;
        self.harness_project(tenant).map(|p| HarnessProjectView {
            path: p.path,
            write: p.write,
        })
    }

    /// A project chosen in the folder sheet (checked again here).
    pub(super) fn set_harness_project(
        &mut self,
        context: &ContextRef,
        chosen: std::path::PathBuf,
    ) -> Result<(), Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let allowed = chosen.canonicalize().ok().filter(|canonical| {
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            project_allowed(
                canonical,
                home.as_deref(),
                self.harness_data_root.as_deref(),
            )
        });
        let Some(canonical) = allowed else {
            // Said in the view: the sheet has already closed.
            self.harness_project_refused = Some((tenant, chosen.display().to_string()));
            return Err(Refusal::InvalidInput);
        };
        self.harness_project_refused = None;
        let path = canonical.to_str().ok_or(Refusal::InvalidInput)?.to_owned();
        let before = self.data.clone();
        if let Some(t) = self.tenant_mut(&tenant) {
            // A new project starts read-only.
            t.harness_project = Some(StoredProject {
                tenant: tenant.clone(),
                path,
                write: false,
            });
        }
        self.save_with(&before)
    }

    pub(super) fn clear_harness_project(&mut self, context: &ContextRef) -> Result<(), Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        self.harness_project_refused = None;
        let before = self.data.clone();
        if let Some(t) = self.tenant_mut(&tenant) {
            t.harness_project = None;
        }
        self.save_with(&before)
    }

    /// Allows (or stops allowing) changes in the selected project.
    pub(super) fn set_harness_write(
        &mut self,
        context: &ContextRef,
        write: bool,
    ) -> Result<(), Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let before = self.data.clone();
        let project = self
            .tenant_mut(&tenant)
            .and_then(|t| t.harness_project.as_mut())
            .ok_or(Refusal::CapabilityMissing)?;
        project.write = write;
        self.save_with(&before)
    }

    /// The operator's decision on a pending approval.
    pub(super) fn decide_harness_approval(
        &mut self,
        conversation: &OperationId,
        approval: &str,
        allow: bool,
    ) -> Result<(), Refusal> {
        let c = self.harness_live(conversation)?;
        let pending = c.progress_approvals().iter().any(|a| a.id == approval);
        if !pending {
            return Err(Refusal::StaleContext);
        }
        c.command(Command::Decide {
            approval: approval.to_owned(),
            allow,
        })
    }

    /// Stages a proposal from a typed tool call (never applied here).
    fn stage_proposal(
        &mut self,
        conversation: &OperationId,
        draft: ChangeDraft,
        summary: String,
    ) -> Result<String, Refusal> {
        // The same static checks a review makes before any read.
        let tenant = self.harness_live(conversation)?.tenant_id().clone();
        let client = self
            .tenant(&tenant)
            .and_then(|t| t.management_client.clone())
            .ok_or(Refusal::CapabilityMissing)?;
        crate::changes::normalize(&draft, &client).map_err(|_| Refusal::InvalidInput)?;
        let c = self.harness_live(conversation)?;
        let proposals = c.proposals_mut();
        if proposals.len() >= MAX_PROPOSALS {
            return Err(Refusal::Busy);
        }
        let id = format!("P-{}", proposals.len() + 1);
        proposals.push(Proposal {
            view: ProposalView {
                id: id.clone(),
                summary,
                draft,
                state: ProposalState::Staged,
                review: None,
            },
        });
        Ok(id)
    }

    /// Opens a staged proposal into the operator's change review, unless a
    /// review is already open (the operator's own work is never replaced).
    pub(super) fn review_harness_proposal(
        &mut self,
        conversation: &OperationId,
        proposal: &str,
        keychain: Arc<dyn super::CredentialStore>,
        session: Arc<crate::vault::SessionVault>,
    ) -> Result<(Option<OperationId>, Vec<crate::runtime::Start>), Refusal> {
        if self.change_review_open() {
            return Err(Refusal::Busy);
        }
        let (context, draft) = {
            let c = self.harness_live(conversation)?;
            let context = c.context_ref().clone();
            let p = c
                .proposals_mut()
                .iter()
                .find(|p| p.view.id == proposal && p.view.state == ProposalState::Staged)
                .ok_or(Refusal::InvalidInput)?;
            (context, p.view.draft.clone())
        };
        let started = self.review_change(context, draft, keychain, session)?;
        let review = self.change_review_id();
        if let Ok(c) = self.harness_live(conversation)
            && let Some(p) = c.proposals_mut().iter_mut().find(|p| p.view.id == proposal)
        {
            p.view.state = ProposalState::InReview;
            p.view.review = review;
        }
        Ok(started)
    }

    pub(super) fn dismiss_harness_proposal(
        &mut self,
        conversation: &OperationId,
        proposal: &str,
    ) -> Result<(), Refusal> {
        let c = self.harness_live(conversation)?;
        let p = c
            .proposals_mut()
            .iter_mut()
            .find(|p| p.view.id == proposal && p.view.state == ProposalState::Staged)
            .ok_or(Refusal::InvalidInput)?;
        p.view.state = ProposalState::Dismissed;
        Ok(())
    }

    /// The operator's answer to re-allowing changes before a renewal
    /// restart: allowed, the renewal goes ahead; declined, nothing restarts
    /// and the message is given back.
    pub(super) fn confirm_harness_write(
        &mut self,
        conversation: &OperationId,
        allow: bool,
    ) -> Result<Option<RenewJob>, Refusal> {
        let c = self.harness_live(conversation)?;
        let job = c.take_write_confirmation().ok_or(Refusal::InvalidInput)?;
        if allow {
            c.set_phase(HarnessPhase::Renewing);
            Ok(Some(job))
        } else {
            c.give_back(job);
            Ok(None)
        }
    }
}

/// The sink that runs a conversation's typed tool calls.
pub(super) fn sink(
    inner: &Arc<Inner>,
    conversation: OperationId,
    commands: mpsc::Sender<Command>,
) -> ToolSink {
    let inner = Arc::clone(inner);
    Arc::new(move |request: ToolRequest| {
        let inner = Arc::clone(&inner);
        let conversation = conversation.clone();
        let commands = commands.clone();
        tokio::spawn(async move {
            let ToolRequest { request, call } = request;
            let ran = tokio::time::timeout(TOOL_TIMEOUT, run(&inner, &conversation, call)).await;
            let (success, text) = match ran {
                Ok(Ok(text)) => (true, text),
                _ => (false, UNAVAILABLE.to_owned()),
            };
            let _ = commands
                .send(Command::ToolResult {
                    request,
                    success,
                    text,
                })
                .await;
        });
    })
}

/// One tool call, admitted under the conversation's current binding.
async fn run(
    inner: &Arc<Inner>,
    conversation: &OperationId,
    call: ToolCall,
) -> Result<String, Refusal> {
    let context = lock(inner)?
        .harness_live(conversation)?
        .context_ref()
        .clone();
    let management = |request: ManagementRequest| async {
        let (handle, starts) = {
            let mut state = lock(inner)?;
            let (operation, mut starts) = state.admit_management(
                context.clone(),
                request,
                Arc::clone(&inner.adapters.keychain),
                Arc::clone(&inner.session),
            )?;
            starts.extend(state.take_gateway_starts());
            let handle = state.observe_management(&operation)?;
            state.changed();
            (handle, starts)
        };
        super::run_effects(inner, vec![Effect::Start(starts)]);
        let result = handle.result().await;
        Ok::<_, Refusal>(tools::result_text(
            serde_json::to_value(result).unwrap_or(Value::Null),
            &[],
        ))
    };
    match call {
        ToolCall::Inventory {
            resource,
            offset,
            limit,
        } => {
            management(ManagementRequest::Page {
                resource,
                offset,
                limit,
            })
            .await
        }
        ToolCall::Detail { resource, id } => {
            management(ManagementRequest::Detail { resource, id }).await
        }
        ToolCall::Session {
            session_id,
            app_id,
            app_name,
        } => {
            management(ManagementRequest::Session {
                session_id,
                app_id,
                app_name,
            })
            .await
        }
        ToolCall::Search { query } => management(ManagementRequest::Search { query }).await,
        ToolCall::RedTeam { window } => management(ManagementRequest::RedTeam { window }).await,
        ToolCall::SupplyChain => management(ManagementRequest::SupplyChain).await,
        ToolCall::Scan {
            profile,
            prompt,
            response,
        } => {
            let draft = crate::scanning::ScanDraft {
                profile: crate::scanning::ProfileSelector::Name { name: profile },
                prompt: prompt.unwrap_or_default(),
                response: response.unwrap_or_default(),
                app_name: None,
            };
            let (operation, starts) = {
                let mut state = lock(inner)?;
                let admitted = state.admit_scan(
                    context.clone(),
                    draft,
                    Arc::clone(&inner.adapters.keychain),
                    Arc::clone(&inner.session),
                )?;
                state.changed();
                admitted
            };
            super::run_effects(inner, vec![Effect::Start(starts)]);
            loop {
                tokio::time::sleep(Duration::from_millis(50)).await;
                let mut state = lock(inner)?;
                state.sync_scans_now();
                if let Some(view) = state.scan_settled(&operation) {
                    return Ok(tools::result_text(
                        serde_json::to_value(view).unwrap_or(Value::Null),
                        &[],
                    ));
                }
            }
        }
        ToolCall::Cases => {
            let load = lock(inner)?.open_library(&context)?;
            if let Some(lease) = load {
                super::run_effects(inner, vec![Effect::LoadLibrary { lease }]);
            }
            loop {
                {
                    let state = lock(inner)?;
                    state.check_context(&context)?;
                    let view = state.library_view();
                    if !matches!(view.state, super::library_domain::LibraryState::Loading) {
                        let cases: Vec<Value> = view
                            .cases
                            .iter()
                            .take(200)
                            .map(|c| {
                                json!({"id":c.id,"name":c.name,"profile":c.profile,
                                "expected":c.expected,"appName":c.app_name})
                            })
                            .collect();
                        return Ok(tools::result_text(json!({"cases":cases}), &[]));
                    }
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        ToolCall::ProposeChange {
            kind,
            action,
            target,
            body,
            summary,
        } => {
            let draft = ChangeDraft {
                kind,
                action,
                target,
                body,
                workspace: None,
            };
            let id = lock(inner)?.stage_proposal(conversation, draft, summary)?;
            {
                let mut state = lock(inner)?;
                state.changed();
            }
            Ok(format!(
                "Proposal {id} is staged for the operator. It has not been applied: only the operator can review and confirm it in the desktop."
            ))
        }
    }
}
