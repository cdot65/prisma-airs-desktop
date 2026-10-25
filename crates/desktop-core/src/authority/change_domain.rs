//! Reviewed Runtime changes inside the authority (C09). A captured draft is
//! normalized in Rust and its baseline read (v1's bounded walk) into an
//! immutable review bound to the context, tenant, management client, draft
//! and baseline digest. RequestApply compares freshly captured input with
//! the stored draft and opens the native operator confirmation; only its
//! private completion admits the write, which re-reads the baseline first
//! and refuses on any change (v1 conflict). Results keep their remote
//! outcome and an operation-specific readback; a one-time API key stays in
//! a zeroizing buffer here and is shown only by the native secret sheet.
//! Gateway changes (C17) take the same path with `gateway_changes`' rules:
//! a change inside a workspace is bound to the selection reference it was
//! reviewed under, deletes list what still references the item, and a
//! workspace provisioning reports each of its steps and reconciles them.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use tokio::time::Instant;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::gateway_domain::ChangeScope;
use super::{ConfirmationRequest, Refusal, State};
use crate::changes::{
    ChangeAction, ChangeDraft, ChangeKind, ChangeProblem, Mutation, confirmation, digest, impact,
    item_name, normalize, preview_after, take_secret, with_app_auth_code, with_auth_code,
};
use crate::context::{ContextRef, ReviewId, WorkspaceRef};
use crate::gateway::ListedWorkspace;
use crate::gateway_changes::{self as gateway, ChangeError, Provisioning, ReadBack};
use crate::management::{
    BindingSource, ManagementBinding, ManagementFailure, ManagementResource, PageError, Walk,
    outcome_of,
};
use crate::runtime::{Claim, OperationId, OperationState, Outcome, Work};
use crate::tenants::TenantId;
use crate::vault::SessionVault;

/// How long a prepared review may be applied (the default of
/// `CoreConfig::review_lifetime`).
pub const REVIEW_LIFETIME: Duration = Duration::from_secs(15 * 60);
/// Settled change results kept for the active context.
const RETAINED_RESULTS: usize = 16;

/// Why a change did not apply (sanitized; no server text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ChangeFailure {
    /// The draft or its checks (v1 validation and key creation rules).
    Problem { problem: ChangeProblem },
    /// The item changed since review, or the service reported a conflict.
    Conflict,
    /// The service or the management credential.
    Service { failure: ManagementFailure },
    /// The review expired before it was confirmed; review it again.
    Expired,
    /// The desktop could not start the change (its work queue was full);
    /// nothing was sent. Review it again.
    Capacity,
    /// Another Gateway workspace was chosen before the change was sent;
    /// nothing was sent (C17).
    WorkspaceChanged,
}

impl From<ChangeProblem> for ChangeFailure {
    fn from(problem: ChangeProblem) -> Self {
        Self::Problem { problem }
    }
}

impl From<PageError> for ChangeFailure {
    fn from(error: PageError) -> Self {
        match error {
            PageError::Sdk(error) => Self::Service {
                failure: ManagementFailure::from_sdk(&error),
            },
            PageError::Invalid => Self::Service {
                failure: ManagementFailure::InvalidResponse,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ChangeReviewState {
    /// The baseline is being read.
    Preparing,
    Ready,
    /// The native confirmation is open; the editor is held.
    Confirming,
    /// Older than [`REVIEW_LIFETIME`]: it can no longer be applied.
    Expired,
    Failed,
}

/// What the readback after a write found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Readback {
    Found,
    /// Not in the complete inventory (expected after a delete).
    Absent,
    /// The bounded walk ended before the inventory did.
    Incomplete,
    /// The readback itself failed; the write's outcome stands.
    Failed,
    /// Read back, but it does not show the reviewed change (C17: a routing
    /// document or model list the service did not take as reviewed, or an
    /// item still listed after its delete).
    Differs,
    /// The write completed, but the desktop stopped waiting before reading
    /// it back (C17).
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SecretState {
    /// The change returned no one-time secret.
    None,
    /// A one-time API key is held in memory for an explicit reveal.
    Held,
    /// The operator cleared it (or it was never kept).
    Forgotten,
    /// It was handed to the native sheet once and is no longer held here.
    Shown,
}

/// The review the presentation may show (no secrets).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangeReviewView {
    pub review: ReviewId,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub target: String,
    pub state: ChangeReviewState,
    pub failure: Option<ChangeFailure>,
    /// The item as read for review (JSON, secret fields removed).
    pub before: Option<String>,
    /// The expected result (JSON, server-determined fields labelled).
    pub after: Option<String>,
    /// For a key version: the existing key it is added beside.
    pub source_key: Option<String>,
    pub impact: String,
    /// What the operator types to confirm ("CREATE" or the item's name).
    pub confirmation: Option<String>,
    /// The Gateway workspace the change applies to (C17).
    pub workspace: Option<ListedWorkspace>,
    /// Configuration that still references the item; while any is listed
    /// the change cannot be applied (C17).
    pub references: Vec<String>,
}

/// One applied change as the presentation may show it (never the secret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangeResultView {
    pub operation: OperationId,
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub target: String,
    pub name: Option<String>,
    pub settled: bool,
    pub outcome: Option<Outcome>,
    pub failure: Option<ChangeFailure>,
    /// The service's reply (JSON, secret fields removed).
    pub response: Option<String>,
    pub readback: Option<Readback>,
    /// The item as read back (JSON, secret fields removed).
    pub current: Option<String>,
    pub secret: SecretState,
    /// The Gateway workspace the change applied to (C17).
    pub workspace: Option<ListedWorkspace>,
    /// A workspace provisioning's steps and their reconciliation (C17).
    pub provisioning: Option<Provisioning>,
}

/// Why a chosen dictionary file was not staged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StagedFileFailure {
    /// Over v1's 64 Ki UTF-16 units (or the read bound).
    TooLarge,
    /// Not UTF-8 text.
    NotText,
    Empty,
    /// The file could not be read (not a regular file, permissions).
    Unreadable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StagedFileState {
    /// The native file sheet is open.
    Choosing,
    Ready,
    Failed,
}

/// A keyword file the operator chose in the native sheet, read once by the
/// desktop and referenced from a dictionary draft by its token (the source
/// file is never written; the renderer never sees a path).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StagedFileView {
    pub state: StagedFileState,
    /// Use as `fileRef` in a dictionary draft.
    pub token: Option<String>,
    /// Display name only (no directory).
    pub name: Option<String>,
    pub bytes: Option<u32>,
    pub lines: Option<u32>,
    pub failure: Option<StagedFileFailure>,
}

pub(super) struct StagedFile {
    token: String,
    context: ContextRef,
    name: String,
    text: String,
}

pub(super) enum Staging {
    Choosing { context: ContextRef, lease: u64 },
    Ready(StagedFile),
    Failed(StagedFileFailure),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChangesView {
    pub review: Option<ChangeReviewView>,
    /// The dictionary keyword file chosen in the native sheet, if any.
    pub staged: Option<StagedFileView>,
    /// The active context's applied changes, newest first.
    pub results: Vec<ChangeResultView>,
}

/// What a baseline read produced.
#[derive(Clone, Debug)]
struct Prepared {
    /// Ready to send (a key create carries its auth code).
    mutation: Mutation,
    before: Option<Value>,
    /// v1 review digest: tenant, client, mutation and baseline.
    digest: String,
    /// What still references the item (a Gateway delete; C17).
    references: Vec<String>,
}

type PrepareSlot = Arc<Mutex<Option<Result<Prepared, ChangeFailure>>>>;

#[derive(Debug)]
enum Phase {
    Preparing {
        operation: OperationId,
        slot: PrepareSlot,
    },
    Ready(Box<Prepared>),
    Confirming {
        nonce: u64,
        prepared: Box<Prepared>,
    },
    Failed(ChangeFailure),
}

pub(super) struct ChangeReview {
    id: ReviewId,
    context: ContextRef,
    tenant: TenantId,
    /// The normalized draft before any server-derived field.
    draft: Mutation,
    phase: Phase,
    created: Instant,
    /// A Gateway change's scope and the tenant its creates belong to.
    gateway: Option<GatewayBound>,
}

/// Where a Gateway change was reviewed: its scope and the credential's
/// tenant (the organisation of a create, the owner of an integration).
#[derive(Clone, Debug)]
struct GatewayBound {
    scope: ChangeScope,
    tsg: String,
}

impl GatewayBound {
    fn workspace(&self) -> Option<&str> {
        self.scope.scope.as_ref().map(|s| s.id.as_str())
    }
}

/// What an applied change produced.
struct ApplyOutput {
    outcome: Outcome,
    failure: Option<ChangeFailure>,
    response: Option<String>,
    readback: Option<Readback>,
    current: Option<String>,
    secret: Option<Zeroizing<String>>,
    provisioning: Option<Provisioning>,
}

impl Default for ApplyOutput {
    fn default() -> Self {
        Self {
            outcome: Outcome::Unknown,
            failure: None,
            response: None,
            readback: None,
            current: None,
            secret: None,
            provisioning: None,
        }
    }
}

type ApplySlot = Arc<Mutex<Option<ApplyOutput>>>;

/// How far one apply got, shared by the job and its record (C17): a
/// workspace replaced before the write retires it (nothing may be sent),
/// and a cancellation keeps what an answered write established.
#[derive(Default)]
enum Progress {
    #[default]
    Waiting,
    /// Its workspace was replaced before the write: nothing is sent.
    Retired,
    /// The write was dispatched. A provisioning that reuses a scope carries
    /// the workspaces that already used it (`None`: they could not be read).
    Sent { prior: Option<Vec<String>> },
    /// The write was answered; only its readback remained.
    Written(Box<ApplyOutput>),
    /// The job settled; what it held (a key's copy) was dropped.
    Done,
}

type ProgressSlot = Arc<Mutex<Progress>>;

pub(super) struct ChangeRecord {
    operation: OperationId,
    context: ContextRef,
    kind: ChangeKind,
    action: ChangeAction,
    target: String,
    name: Option<String>,
    slot: ApplySlot,
    output: Option<ApplyOutput>,
    secret: SecretState,
    workspace: Option<ListedWorkspace>,
    /// The workspace reference a Gateway change was reviewed under.
    reference: Option<WorkspaceRef>,
    progress: ProgressSlot,
    /// Kept on this Mac when the change settles uncertain (C17, T23).
    local: Option<LocalOrigin>,
}

impl ChangeRecord {
    /// The tenant a Gateway change keeps its local record under.
    pub(super) fn tenant(&self) -> Option<&TenantId> {
        self.local.as_ref().map(|l| &l.tenant)
    }
}

/// What a Gateway change's local record keeps: the tenant, the workspace
/// label and the reviewed draft text (never a key, reply or authority).
pub(super) struct LocalOrigin {
    pub(super) tenant: TenantId,
    pub(super) workspace: Option<ListedWorkspace>,
    pub(super) body: String,
}

/// What a settled Gateway change leaves on this Mac: its draft is dropped
/// once something was sent, and an uncertain one is recorded with the
/// reviewed text (never its reply, key or authority).
fn local_settlement(
    record: &ChangeRecord,
    local: LocalOrigin,
    output: &ApplyOutput,
) -> (TenantId, super::gateway_local_domain::Settled) {
    let change = crate::gateway_local::GatewayTarget {
        workspace: local.workspace,
        kind: record.kind,
        action: record.action,
        target: record.target.clone(),
    };
    let sent = matches!(output.outcome, Outcome::Completed | Outcome::Unknown);
    let confirmed = match &output.provisioning {
        Some(p) => p.verified(),
        None => match output.readback {
            Some(Readback::Found) => record.action != ChangeAction::Delete,
            Some(Readback::Absent) => record.action == ChangeAction::Delete,
            _ => false,
        },
    };
    let uncertain = match output.outcome {
        Outcome::Unknown => true,
        Outcome::Completed => output.failure.is_some() || !confirmed,
        Outcome::NotSent | Outcome::Rejected => false,
    };
    let kept = uncertain.then(|| crate::gateway_local::GatewayRecord {
        id: String::new(),
        change: change.clone(),
        name: record.name.clone(),
        body: local.body,
        outcome: output.outcome,
        readback: output.readback,
        provisioning: output.provisioning.clone(),
        settled_at: String::new(),
    });
    (
        local.tenant,
        super::gateway_local_domain::Settled {
            record: kept,
            change,
            sent,
        },
    )
}

/// v1's review digest; a Gateway review also binds its workspace and what
/// references the item (the baseline without traffic counters).
fn review_digest(
    tenant: &TenantId,
    client_id: &str,
    mutation: &Mutation,
    before: Option<&Value>,
    gateway: Option<(&GatewayBound, &[String])>,
) -> String {
    let mut value = json!({
        "tenant": tenant.as_str(),
        "identity": client_id,
        "kind": mutation.kind,
        "action": mutation.action,
        "target": mutation.target,
        "body": mutation.body,
    });
    match gateway {
        None => value["baseline"] = json!(digest(before.unwrap_or(&Value::Null))),
        Some((bound, references)) => {
            value["baseline"] = json!(digest(&gateway::stable(before.unwrap_or(&Value::Null))));
            value["workspace"] = json!(bound.workspace());
            value["references"] = json!(references);
        }
    }
    digest(&value)
}

fn gateway_failure(error: ChangeError) -> ChangeFailure {
    match error {
        ChangeError::Problem(problem) => problem.into(),
        ChangeError::Page(error) => error.into(),
    }
}

fn nonce() -> Option<u64> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).ok()?;
    Some(u64::from_le_bytes(bytes))
}

impl State {
    /// Prepares a review of `draft` for the active tenant's management
    /// client. A draft that fails v1's checks yields a failed review the
    /// operator can read (no operation); otherwise the baseline read is
    /// admitted as an operation.
    /// A change review exists (prepared, confirming or failed): a Harness
    /// proposal never replaces it (C21).
    pub(super) fn change_review_open(&self) -> bool {
        self.change_review.is_some()
    }

    /// The open review's id.
    pub(super) fn change_review_id(&self) -> Option<ReviewId> {
        self.change_review.as_ref().map(|r| r.id.clone())
    }

    pub(super) fn review_change(
        &mut self,
        context: ContextRef,
        draft: ChangeDraft,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(Option<OperationId>, Vec<crate::runtime::Start>), Refusal> {
        let tenant = self.check_context(&context)?;
        let source = self.binding_source(&context, keychain, session_vault)?;
        // A newly captured draft invalidates a pending confirmation: its sheet
        // is ended and its late completion can no longer apply anything.
        if let Some(Phase::Confirming { nonce, .. }) = self.change_review.as_ref().map(|r| &r.phase)
        {
            self.confirmer.dismiss(*nonce);
        }
        let id = ReviewId::issue().ok_or(Refusal::ShuttingDown)?;
        let gateway = match draft.kind.gateway() {
            Some(resource) => Some(GatewayBound {
                scope: self.gateway_change_scope(
                    &context,
                    resource,
                    draft.action,
                    &draft.target,
                    draft.workspace.as_ref(),
                )?,
                tsg: source.tsg.clone(),
            }),
            None => None,
        };
        let resolved = self.resolve_staged(&context, &draft);
        let normalized = match &gateway {
            Some(bound) => gateway::normalize(&draft, &bound.tsg, bound.workspace()),
            None => resolved.and_then(|d| normalize(&d, &source.client_id)),
        };
        let draft = match normalized {
            Ok(mutation) => mutation,
            Err(problem) => {
                // Kept so the operator can see why; nothing is read or sent.
                self.change_review = Some(ChangeReview {
                    id,
                    context,
                    tenant,
                    draft: Mutation {
                        kind: draft.kind,
                        action: draft.action,
                        target: draft.target,
                        body: Value::Null,
                    },
                    phase: Phase::Failed(problem.into()),
                    created: Instant::now(),
                    gateway,
                });
                return Ok((None, Vec::new()));
            }
        };
        let slot: PrepareSlot = Arc::default();
        let job = PrepareJob {
            source,
            tenant: tenant.clone(),
            draft: draft.clone(),
            gateway: gateway.clone(),
            slot: Arc::clone(&slot),
        };
        let (operation, starts) =
            self.operations
                .admit(context.as_str(), 0, job.into_work(), Instant::now())?;
        self.change_review = Some(ChangeReview {
            id,
            context,
            tenant,
            draft,
            phase: Phase::Preparing {
                operation: operation.clone(),
                slot,
            },
            created: Instant::now(),
            gateway,
        });
        Ok((Some(operation), starts))
    }

    /// RequestApply: the freshly captured `draft` and typed `confirmation`
    /// must equal the stored review's; then the review is held for the
    /// native confirmation `nonce`.
    pub(super) fn request_apply(
        &mut self,
        review: &ReviewId,
        draft: &ChangeDraft,
        typed: &str,
    ) -> Result<(u64, ConfirmationRequest), Refusal> {
        if self.consumed_reviews.contains(review) {
            return Err(Refusal::ReviewConsumed);
        }
        let active = self.active_ref.clone();
        let lifetime = self.review_lifetime;
        let label = self
            .change_review
            .as_ref()
            .and_then(|r| self.tenant(&r.tenant))
            .map(|t| t.profile.label.clone())
            .unwrap_or_default();
        let current = self
            .change_review
            .as_mut()
            .filter(|r| &r.id == review)
            .ok_or(Refusal::ReviewInvalid)?;
        if active.as_ref() != Some(&current.context) || current.created.elapsed() > lifetime {
            return Err(Refusal::ReviewInvalid);
        }
        let prepared = match &current.phase {
            Phase::Ready(prepared) => prepared.clone(),
            Phase::Confirming { .. } => return Err(Refusal::Busy),
            _ => return Err(Refusal::ReviewInvalid),
        };
        // Fresh input must be exactly what was reviewed (client authorship
        // is re-applied, so the comparison is of normalized drafts).
        let author = current
            .draft
            .body
            .get(match current.draft.action {
                ChangeAction::Create | ChangeAction::Version => "created_by",
                _ => "updated_by",
            })
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let context = current.context.clone();
        let bound = current.gateway.clone();
        let fresh = match &bound {
            // A Gateway draft must still name the reference it was reviewed
            // under, and that selection must still be current.
            Some(bound) => {
                if draft.workspace != bound.scope.reference
                    || !self.workspace_current(bound.scope.reference.as_ref())
                    || !prepared.references.is_empty()
                {
                    return Err(Refusal::ReviewInvalid);
                }
                gateway::normalize(draft, &bound.tsg, bound.workspace())
                    .map_err(|_| Refusal::ReviewInvalid)?
            }
            None => match self.resolve_staged(&context, draft) {
                Ok(draft) => normalize(&draft, &author).map_err(|_| Refusal::ReviewInvalid)?,
                Err(_) => return Err(Refusal::ReviewInvalid),
            },
        };
        let Some(current) = self.change_review.as_mut().filter(|r| &r.id == review) else {
            return Err(Refusal::ReviewInvalid);
        };
        if fresh != current.draft {
            return Err(Refusal::ReviewInvalid);
        }
        let expected = confirmation(&prepared.mutation, prepared.before.as_ref());
        if expected.as_deref() != Some(typed) {
            return Err(Refusal::InvalidInput);
        }
        let nonce = nonce().ok_or(Refusal::ShuttingDown)?;
        let shown = bound.as_ref().and_then(|b| b.scope.shown.as_ref());
        let request = confirmation_request(&label, &prepared, shown);
        current.phase = Phase::Confirming { nonce, prepared };
        Ok((nonce, request))
    }

    /// The native confirmation's private completion. Only the open
    /// confirmation `nonce` of a current review is honored, once; a denial
    /// returns the review to ready (another RequestApply is needed).
    pub(super) fn settle_confirmation(
        &mut self,
        review: &ReviewId,
        nonce: u64,
        accepted: bool,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Vec<crate::runtime::Start> {
        let active = self.active_ref.clone();
        let Some(current) = self.change_review.as_mut().filter(|r| &r.id == review) else {
            return Vec::new();
        };
        if !matches!(&current.phase, Phase::Confirming { nonce: n, .. } if *n == nonce)
            || active.as_ref() != Some(&current.context)
        {
            return Vec::new();
        }
        let expired = current.created.elapsed() > self.review_lifetime;
        let Phase::Confirming { prepared, .. } =
            std::mem::replace(&mut current.phase, Phase::Failed(ChangeFailure::Conflict))
        else {
            return Vec::new();
        };
        // A denial, or a confirmation that outlived the review, applies
        // nothing; the review stays (an expired one shows as expired).
        if !accepted || expired {
            current.phase = Phase::Ready(prepared);
            return Vec::new();
        }
        let (context, tenant, draft, bound) = (
            current.context.clone(),
            current.tenant.clone(),
            current.draft.clone(),
            current.gateway.clone(),
        );
        // Admission and consumption are one step: if the change cannot be
        // started, the review is not consumed and the operator sees why.
        let source = match self.binding_source(&context, keychain, session_vault) {
            Ok(source) => source,
            Err(_) => {
                if let Some(r) = self.change_review.as_mut() {
                    r.phase = Phase::Failed(ChangeFailure::Service {
                        failure: ManagementFailure::CredentialMissing,
                    });
                }
                return Vec::new();
            }
        };
        let slot: ApplySlot = Arc::default();
        let name = item_name(draft.kind, &prepared.mutation.body).or_else(|| {
            prepared
                .before
                .as_ref()
                .and_then(|b| item_name(draft.kind, b))
        });
        let workspace = bound.as_ref().and_then(|b| b.scope.shown.clone());
        let reference = bound.as_ref().and_then(|b| b.scope.reference.clone());
        let local = bound.as_ref().map(|_| LocalOrigin {
            tenant: tenant.clone(),
            workspace: workspace.clone(),
            body: draft.body.to_string(),
        });
        let progress: ProgressSlot = Arc::default();
        let job = ApplyJob {
            source,
            tenant,
            draft: draft.clone(),
            prepared: *prepared,
            gateway: bound,
            progress: Arc::clone(&progress),
            slot: Arc::clone(&slot),
        };
        let (operation, starts) =
            match self
                .operations
                .admit(context.as_str(), 0, job.into_work(), Instant::now())
            {
                Ok(admitted) => admitted,
                Err(_) => {
                    if let Some(r) = self.change_review.as_mut() {
                        r.phase = Phase::Failed(ChangeFailure::Capacity);
                    }
                    return Vec::new();
                }
            };
        if let Some(taken) = self.change_review.take() {
            self.consumed_reviews.push(taken.id);
        }
        self.change_results.insert(
            0,
            ChangeRecord {
                operation,
                context,
                kind: draft.kind,
                action: draft.action,
                target: draft.target,
                name,
                slot,
                output: None,
                secret: SecretState::None,
                workspace,
                reference,
                progress,
                local,
            },
        );
        starts
    }

    /// Replaces a dictionary draft's `fileRef` with the staged file's text
    /// (the file staged for this context, by its token).
    fn resolve_staged(
        &self,
        context: &ContextRef,
        draft: &ChangeDraft,
    ) -> Result<ChangeDraft, ChangeProblem> {
        if draft.kind != ChangeKind::Dictionaries {
            return Ok(draft.clone());
        }
        let Ok(Value::Object(mut body)) = serde_json::from_str::<Value>(&draft.body) else {
            return Ok(draft.clone());
        };
        let Some(reference) = body.remove("fileRef") else {
            return Ok(draft.clone());
        };
        let staged = match &self.staging {
            Some(Staging::Ready(file))
                if &file.context == context && Some(file.token.as_str()) == reference.as_str() =>
            {
                file
            }
            _ => return Err(ChangeProblem::StagedFile),
        };
        if body.contains_key("file") {
            return Err(ChangeProblem::Body);
        }
        body.insert("file".into(), Value::String(staged.text.clone()));
        Ok(ChangeDraft {
            body: Value::Object(body).to_string(),
            ..draft.clone()
        })
    }

    /// ChooseDictionaryFile: opens the native file sheet (one at a time).
    pub(super) fn choose_dictionary_file(
        &mut self,
        context: ContextRef,
        lease: u64,
    ) -> Result<(), Refusal> {
        self.check_context(&context)?;
        // The native confirmation holds the editor lease: the file a pending
        // change was reviewed with cannot be replaced or cleared meanwhile.
        if matches!(self.staging, Some(Staging::Choosing { .. }))
            || matches!(
                self.change_review.as_ref().map(|r| &r.phase),
                Some(Phase::Confirming { .. })
            )
        {
            return Err(Refusal::Busy);
        }
        self.staging = Some(Staging::Choosing { context, lease });
        Ok(())
    }

    /// The sheet's result: a bounded UTF-8 text file is staged; anything else
    /// is reported. A cancelled sheet keeps nothing.
    pub(super) fn settle_dictionary_file(
        &mut self,
        lease: u64,
        chosen: Option<Result<(String, Vec<u8>), StagedFileFailure>>,
    ) {
        let Some(Staging::Choosing { context, lease: l }) = &self.staging else {
            return;
        };
        if *l != lease || self.active_ref.as_ref() != Some(context) {
            return;
        }
        let context = context.clone();
        // A sheet opened before a confirmation began must not change the file
        // under it: end that confirmation, whose late completion then applies
        // nothing, and require a new review.
        if let Some(review) = self.change_review.as_mut()
            && let Phase::Confirming { nonce, .. } = review.phase
        {
            self.confirmer.dismiss(nonce);
            review.phase = Phase::Failed(ChangeProblem::StagedFile.into());
        }
        self.staging = match chosen {
            None => None,
            Some(Err(failure)) => Some(Staging::Failed(failure)),
            Some(Ok((name, bytes))) => Some(match staged_text(bytes) {
                Err(failure) => Staging::Failed(failure),
                Ok(text) => match nonce() {
                    Some(n) => Staging::Ready(StagedFile {
                        token: format!("sf-{n:016x}"),
                        context,
                        name,
                        text,
                    }),
                    None => Staging::Failed(StagedFileFailure::Unreadable),
                },
            }),
        };
    }

    pub(super) fn discard_change(&mut self, review: &ReviewId) -> Result<(), Refusal> {
        match &self.change_review {
            Some(r) if &r.id == review => {
                if let Phase::Confirming { nonce, .. } = r.phase {
                    self.confirmer.dismiss(nonce);
                }
                self.change_review = None;
                Ok(())
            }
            _ => Err(Refusal::ReviewInvalid),
        }
    }

    /// Hands the held one-time secret of an applied change to the native
    /// sheet, once: the core keeps no copy afterwards, so closing, copying
    /// or the sheet's expiry ends it for good (DESIGN: one reveal).
    pub(super) fn reveal_secret(
        &mut self,
        operation: &OperationId,
    ) -> Result<(String, Zeroizing<String>), Refusal> {
        let active = self.active_ref.clone();
        let record = self
            .change_results
            .iter_mut()
            .find(|r| &r.operation == operation && Some(&r.context) == active.as_ref())
            .ok_or(Refusal::InvalidInput)?;
        let secret = record
            .output
            .as_mut()
            .and_then(|o| o.secret.take())
            .ok_or(Refusal::CapabilityMissing)?;
        record.secret = SecretState::Shown;
        let what = if record.kind.gateway().is_some() {
            "New Gateway key"
        } else {
            "New API key"
        };
        let label = format!(
            "{what}{}",
            record
                .name
                .as_deref()
                .map(|n| format!(" for {n}"))
                .unwrap_or_default()
        );
        Ok((label, secret))
    }

    /// CancelChange: stops an applying change of the active context. The
    /// job reports what was sent; nothing is rolled back.
    pub(super) fn cancel_change(
        &mut self,
        operation: &OperationId,
    ) -> Result<Vec<crate::runtime::Start>, Refusal> {
        let active = self.active_ref.clone();
        self.change_results
            .iter()
            .find(|r| {
                &r.operation == operation
                    && r.output.is_none()
                    && Some(&r.context) == active.as_ref()
            })
            .ok_or(Refusal::InvalidInput)?;
        let (_, starts) = self.operations.cancel(
            operation,
            crate::runtime::StopReason::Cancelled,
            Instant::now(),
        );
        self.sync_changes();
        Ok(starts)
    }

    /// Another workspace was chosen (or none): every Gateway change bound
    /// to a different reference that has not dispatched its write never
    /// will. Queued work settles not sent when it runs; one already writing
    /// keeps its own outcome. A retired reference is never current again.
    pub(super) fn retire_scoped_applies(&mut self, keep: Option<&WorkspaceRef>) {
        for record in self.change_results.iter().filter(|r| r.output.is_none()) {
            if record.reference.as_ref().is_none_or(|r| Some(r) == keep) {
                continue;
            }
            if let Ok(mut progress) = record.progress.lock()
                && matches!(*progress, Progress::Waiting)
            {
                *progress = Progress::Retired;
            }
        }
    }

    pub(super) fn forget_secret(&mut self, operation: &OperationId) -> Result<(), Refusal> {
        let record = self
            .change_results
            .iter_mut()
            .find(|r| &r.operation == operation)
            .ok_or(Refusal::InvalidInput)?;
        if let Some(output) = record.output.as_mut() {
            output.secret = None;
        }
        if record.secret == SecretState::Held {
            record.secret = SecretState::Forgotten;
        }
        self.secrets.dismiss();
        Ok(())
    }

    /// Settles prepared reviews and applied changes; drops everything bound
    /// to a retired context (an open confirmation is ended, held secrets
    /// are zeroized with their record).
    pub(super) fn sync_changes(&mut self) {
        let active = self.active_ref.clone();
        let retired = self.change_review.as_ref().is_some_and(|r| {
            r.gateway
                .as_ref()
                .is_some_and(|g| !self.workspace_current(g.scope.reference.as_ref()))
        });
        if let Some(review) = &mut self.change_review {
            // A Gateway review is bound to the selection it was reviewed
            // under: choosing another workspace ends it (and its sheet).
            if active.as_ref() != Some(&review.context) || retired {
                if let Phase::Confirming { nonce, .. } = review.phase {
                    self.confirmer.dismiss(nonce);
                }
                self.change_review = None;
            } else if let Phase::Preparing { operation, slot } = &review.phase {
                match self.operations.state(operation) {
                    Some(OperationState::Settled(_)) | None => {
                        let taken = slot.lock().ok().and_then(|mut s| s.take());
                        review.phase = match taken {
                            Some(Ok(prepared)) => Phase::Ready(Box::new(prepared)),
                            Some(Err(failure)) => Phase::Failed(failure),
                            None => Phase::Failed(ChangeFailure::Service {
                                failure: ManagementFailure::Cancelled,
                            }),
                        };
                    }
                    Some(_) => {}
                }
            }
        }
        let stale = match &self.staging {
            Some(Staging::Choosing { context, .. }) => active.as_ref() != Some(context),
            Some(Staging::Ready(file)) => active.as_ref() != Some(&file.context),
            _ => false,
        };
        if stale {
            self.staging = None;
        }
        let before = self.change_results.len();
        // A Gateway change whose write may be in flight leaves the active
        // results with its context but is held (without its review or
        // anything shown) until it settles, so its outcome is still kept
        // on this Mac under the tenant it was made in.
        for record in std::mem::take(&mut self.change_results) {
            if Some(&record.context) == active.as_ref() {
                self.change_results.push(record);
            } else if record.output.is_none() && record.local.is_some() {
                self.detached_changes.push(record);
            }
        }
        if self.change_results.len() != before {
            self.secrets.dismiss();
        }
        let mut kept = Vec::new();
        for record in self
            .change_results
            .iter_mut()
            .chain(self.detached_changes.iter_mut())
            .filter(|r| r.output.is_none())
        {
            let settled = match self.operations.state(&record.operation) {
                Some(OperationState::Settled(settlement)) => Some(settlement.outcome),
                None => Some(Outcome::Unknown),
                Some(_) => None,
            };
            let Some(settled) = settled else {
                continue;
            };
            let output = record.slot.lock().ok().and_then(|mut s| s.take());
            // Work that never ran (cancelled while queued) settles NotSent.
            let mut output = output.unwrap_or(ApplyOutput {
                outcome: settled,
                failure: Some(ChangeFailure::Service {
                    failure: ManagementFailure::Cancelled,
                }),
                response: None,
                readback: None,
                current: None,
                secret: None,
                provisioning: None,
            });
            if Some(&record.context) != active.as_ref() {
                // A retired context's reply and one-time key go unread.
                output.secret = None;
                output.response = None;
                output.current = None;
            }
            record.secret = if output.secret.is_some() {
                SecretState::Held
            } else {
                SecretState::None
            };
            if let Some(local) = record.local.take() {
                kept.push(local_settlement(record, local, &output));
            }
            record.output = Some(output);
        }
        self.detached_changes.retain(|r| r.output.is_none());
        for (tenant, settled) in kept {
            self.gateway_local_settled(&tenant, settled);
        }
        let mut settled = 0;
        self.change_results.retain(|r| {
            if r.output.is_some() {
                settled += 1;
                settled <= RETAINED_RESULTS
            } else {
                true
            }
        });
    }

    pub(super) fn changes_view(&self) -> ChangesView {
        let active = self.active_ref.as_ref();
        let review = self
            .change_review
            .as_ref()
            .filter(|r| Some(&r.context) == active)
            .map(|r| {
                let (state, failure, prepared) = match &r.phase {
                    Phase::Preparing { .. } => (ChangeReviewState::Preparing, None, None),
                    Phase::Ready(p) if r.created.elapsed() > self.review_lifetime => (
                        ChangeReviewState::Expired,
                        Some(ChangeFailure::Expired),
                        Some(p.as_ref()),
                    ),
                    Phase::Ready(p) => (ChangeReviewState::Ready, None, Some(p.as_ref())),
                    Phase::Confirming { prepared, .. } => {
                        (ChangeReviewState::Confirming, None, Some(prepared.as_ref()))
                    }
                    Phase::Failed(f) => (ChangeReviewState::Failed, Some(*f), None),
                };
                let version = r.draft.action == ChangeAction::Version;
                let kind = r.draft.kind;
                // Gateway items are shown only as their allow-listed projection.
                let redacted = |v: &Value| match kind.gateway() {
                    Some(_) => gateway::shown(kind, v).unwrap_or_default(),
                    None => {
                        let mut v = v.clone();
                        crate::management::redact(&mut v);
                        v.to_string()
                    }
                };
                ChangeReviewView {
                    review: r.id.clone(),
                    kind: r.draft.kind,
                    action: r.draft.action,
                    target: r.draft.target.clone(),
                    state,
                    failure,
                    before: prepared
                        .filter(|_| !version)
                        .and_then(|p| p.before.as_ref())
                        .map(redacted),
                    after: prepared
                        .and_then(|p| {
                            preview_after(&p.mutation, p.before.as_ref().filter(|_| !version))
                        })
                        .map(|v| v.to_string()),
                    source_key: prepared
                        .filter(|_| version)
                        .and_then(|p| p.before.as_ref())
                        .map(redacted),
                    impact: impact(&r.draft).to_owned(),
                    confirmation: prepared
                        .and_then(|p| confirmation(&p.mutation, p.before.as_ref())),
                    workspace: r.gateway.as_ref().and_then(|g| g.scope.shown.clone()),
                    references: prepared.map(|p| p.references.clone()).unwrap_or_default(),
                }
            });
        let results = self
            .change_results
            .iter()
            .filter(|r| Some(&r.context) == active)
            .map(|r| ChangeResultView {
                operation: r.operation.clone(),
                kind: r.kind,
                action: r.action,
                target: r.target.clone(),
                name: r.name.clone(),
                settled: r.output.is_some(),
                outcome: r.output.as_ref().map(|o| o.outcome),
                failure: r.output.as_ref().and_then(|o| o.failure),
                response: r.output.as_ref().and_then(|o| o.response.clone()),
                readback: r.output.as_ref().and_then(|o| o.readback),
                current: r.output.as_ref().and_then(|o| o.current.clone()),
                secret: r.secret,
                workspace: r.workspace.clone(),
                provisioning: r.output.as_ref().and_then(|o| o.provisioning.clone()),
            })
            .collect();
        let staged = self.staging.as_ref().map(|s| match s {
            Staging::Choosing { .. } => StagedFileView {
                state: StagedFileState::Choosing,
                token: None,
                name: None,
                bytes: None,
                lines: None,
                failure: None,
            },
            Staging::Ready(file) => StagedFileView {
                state: StagedFileState::Ready,
                token: Some(file.token.clone()),
                name: Some(file.name.clone()),
                bytes: u32::try_from(file.text.len()).ok(),
                lines: u32::try_from(file.text.lines().count()).ok(),
                failure: None,
            },
            Staging::Failed(failure) => StagedFileView {
                state: StagedFileState::Failed,
                token: None,
                name: None,
                bytes: None,
                lines: None,
                failure: Some(*failure),
            },
        });
        ChangesView {
            review,
            staged,
            results,
        }
    }
}

/// A chosen keyword file as text: UTF-8 (a byte-order mark is dropped),
/// non-empty, at most 64 Ki UTF-16 units (v1).
fn staged_text(bytes: Vec<u8>) -> Result<String, StagedFileFailure> {
    let bytes = bytes
        .strip_prefix(&[0xEF, 0xBB, 0xBF])
        .map(<[u8]>::to_vec)
        .unwrap_or(bytes);
    let text = String::from_utf8(bytes).map_err(|_| StagedFileFailure::NotText)?;
    if text.contains('\0') {
        return Err(StagedFileFailure::NotText);
    }
    if text.trim().is_empty() {
        return Err(StagedFileFailure::Empty);
    }
    if text.encode_utf16().count() > crate::changes::MAX_DICTIONARY_FILE {
        return Err(StagedFileFailure::TooLarge);
    }
    Ok(text)
}

fn verb(action: ChangeAction) -> &'static str {
    match action {
        ChangeAction::Create => "Create",
        ChangeAction::Update => "Update",
        ChangeAction::Delete => "Delete",
        ChangeAction::Version => "Add a key for",
        ChangeAction::Rotate => "Rotate",
        ChangeAction::Models => "Change the models of",
    }
}

fn noun(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Profiles => "security profile",
        ChangeKind::Topics => "custom topic",
        ChangeKind::Applications => "application",
        ChangeKind::ApiKeys => "API key",
        ChangeKind::DataPatterns => "data pattern",
        ChangeKind::DataProfiles => "data profile",
        ChangeKind::FilteringProfiles => "data filtering profile",
        ChangeKind::Dictionaries => "dictionary",
        ChangeKind::GatewayWorkspaces => "Gateway workspace",
        ChangeKind::GatewayProviders => "Gateway provider",
        ChangeKind::GatewayRoutes => "Gateway route",
        ChangeKind::GatewayGuardrails => "Gateway guardrail",
        ChangeKind::GatewayServiceKeys => "Gateway service key",
        ChangeKind::GatewayUserKeys => "Gateway user key",
        ChangeKind::GatewayIntegrations => "Gateway integration",
    }
}

/// The native confirmation's text: Rust's stored review, not renderer text.
fn confirmation_request(
    tenant: &str,
    prepared: &Prepared,
    workspace: Option<&ListedWorkspace>,
) -> ConfirmationRequest {
    let m = &prepared.mutation;
    let archives = m.kind == ChangeKind::GatewayWorkspaces && m.action == ChangeAction::Delete;
    let name = item_name(m.kind, &m.body)
        .or_else(|| prepared.before.as_ref().and_then(|b| item_name(m.kind, b)))
        .unwrap_or_else(|| m.target.clone());
    let subject = match m.action {
        ChangeAction::Version => format!(
            "application {}",
            m.body["cust_app"].as_str().unwrap_or(&name)
        ),
        _ => format!("{} \u{201c}{name}\u{201d}", noun(m.kind)),
    };
    // The exact target scope: the Gateway workspace and its IAM scope, or
    // the IAM scope a provisioning creates or reuses.
    let scope = match (workspace, m.body.get("scope_name").and_then(Value::as_str)) {
        (Some(w), _) => format!(
            "\nWorkspace: {} ({}), IAM scope {}.",
            w.name, w.slug, w.scope_name
        ),
        (None, Some(name)) if m.kind == ChangeKind::GatewayWorkspaces => {
            format!("\nIAM scope: {name}.")
        }
        _ => String::new(),
    };
    ConfirmationRequest {
        title: if archives {
            format!("Archive {subject}?")
        } else {
            format!("{} {subject}?", verb(m.action))
        },
        message: format!("Tenant: {tenant}.{scope}\n{}", impact(m)),
        confirm_label: match m.action {
            ChangeAction::Delete if archives => "Archive".into(),
            ChangeAction::Delete => "Delete".into(),
            ChangeAction::Rotate => "Rotate key".into(),
            ChangeAction::Create | ChangeAction::Version => "Create".into(),
            ChangeAction::Update | ChangeAction::Models => "Apply".into(),
        },
        destructive: matches!(m.action, ChangeAction::Delete | ChangeAction::Rotate),
    }
}

/// The item whose identifier is `id` (v1 bounded walk).
async fn find_id(
    binding: &ManagementBinding,
    resource: ManagementResource,
    id: &str,
    deadline: Instant,
) -> Result<Value, ChangeFailure> {
    let field = resource.id_field();
    match binding
        .walk(resource, deadline, |item| {
            crate::management::identity(item.get(field)).as_deref() == Some(id)
        })
        .await?
    {
        Walk::Found(item) => Ok(item),
        Walk::Ended => Err(ChangeProblem::NotFound.into()),
        Walk::Incomplete => Err(ChangeProblem::InventoryLimit.into()),
    }
}

fn folded(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(|s| s.trim().to_lowercase())
}

/// Fails when an item of `resource` has `field` equal to `wanted`
/// (case-insensitive, v1), or when the inventory could not be read whole.
async fn absent(
    binding: &ManagementBinding,
    resource: ManagementResource,
    field: &str,
    wanted: Option<String>,
    exists: ChangeProblem,
    deadline: Instant,
) -> Result<(), ChangeFailure> {
    match binding
        .walk(resource, deadline, |item| folded(item.get(field)) == wanted)
        .await?
    {
        Walk::Found(_) => Err(exists.into()),
        Walk::Ended => Ok(()),
        Walk::Incomplete => Err(ChangeProblem::InventoryLimit.into()),
    }
}

/// v1's review preparation: key creation checks and auth code, and the
/// baseline (`before`) the review and the apply are bound to.
async fn baseline(
    binding: &ManagementBinding,
    draft: &Mutation,
    deadline: Instant,
) -> Result<(Mutation, Option<Value>), ChangeFailure> {
    use ChangeAction as A;
    let body = &draft.body;
    let Some(resource) = draft.kind.resource() else {
        return Err(ChangeProblem::Unsupported.into());
    };
    match (draft.kind, draft.action) {
        (ChangeKind::ApiKeys, A::Create | A::Version) => {
            let source = if draft.action == A::Version {
                let source = find_id(
                    binding,
                    ManagementResource::ApiKeys,
                    &draft.target,
                    deadline,
                )
                .await?;
                if source.get("cust_app") != body.get("cust_app") {
                    return Err(ChangeProblem::SourceMismatch.into());
                }
                Some(source)
            } else {
                absent(
                    binding,
                    ManagementResource::Applications,
                    "app_name",
                    folded(body.get("cust_app")),
                    ChangeProblem::ApplicationExists,
                    deadline,
                )
                .await?;
                None
            };
            absent(
                binding,
                ManagementResource::ApiKeys,
                "api_key_name",
                folded(body.get("api_key_name")),
                ChangeProblem::KeyExists,
                deadline,
            )
            .await?;
            let deployments = binding.deployment_profiles(deadline).await?;
            Ok((with_auth_code(draft, &deployments)?, source))
        }
        (_, A::Create) => Ok((draft.clone(), None)),
        // DLP items are read by their own get (v1 `current`); 404 is absent.
        (kind, _) if kind.is_dlp() => {
            match binding.dlp_item(resource, &draft.target, deadline).await? {
                Some(item) if dlp_deleted(&item) => Err(ChangeProblem::NotFound.into()),
                Some(item) => Ok((draft.clone(), Some(item))),
                None => Err(ChangeProblem::NotFound.into()),
            }
        }
        (kind, action) => {
            let mut before = find_id(binding, resource, &draft.target, deadline).await?;
            if (kind, action) == (ChangeKind::Applications, A::Delete) {
                // The whole matching key inventory is part of the review.
                let app = before.get("app_name").cloned();
                let mut keys = Vec::new();
                let walked = binding
                    .walk(ManagementResource::ApiKeys, deadline, |key| {
                        if key.get("cust_app") == app.as_ref() {
                            keys.push(key.clone());
                        }
                        false
                    })
                    .await?;
                if walked != Walk::Ended {
                    return Err(ChangeProblem::InventoryLimit.into());
                }
                if let Value::Object(map) = &mut before {
                    map.insert("affected_keys".into(), Value::Array(keys));
                }
            }
            if (kind, action) == (ChangeKind::Applications, A::Update) {
                let deployments = binding.deployment_profiles(deadline).await?;
                return Ok((
                    with_app_auth_code(draft, &before, &deployments)?,
                    Some(before),
                ));
            }
            Ok((draft.clone(), Some(before)))
        }
    }
}

/// Reads the changed item back (operation-specific).
async fn readback(
    binding: &ManagementBinding,
    mutation: &Mutation,
    before: Option<&Value>,
    response: &Value,
    deadline: Instant,
) -> (Readback, Option<String>) {
    let kind = mutation.kind;
    let Some(resource) = kind.resource() else {
        return (Readback::Failed, None);
    };
    let id_field = resource.id_field();
    let name_field = kind.name_field();
    let by_name = |name: Option<String>| {
        move |item: &Value| {
            item.get(name_field)
                .and_then(Value::as_str)
                .map(str::to_owned)
                == name
        }
    };
    if kind.is_dlp() {
        // By identifier: the reply's for a create, the target otherwise (a
        // dictionary replace may reply 204 with no body).
        let id = crate::management::identity(response.get("id"))
            .unwrap_or_else(|| mutation.target.clone());
        return match binding.dlp_item(resource, &id, deadline).await {
            // The DLP service archives what it deletes: the item stays
            // readable by its ID with status `deleted` and an archived name
            // (C25 live finding), so a deleted item reads back as absent.
            Ok(Some(item)) if dlp_deleted(&item) => (Readback::Absent, None),
            Ok(Some(item)) => (Readback::Found, Some(item.to_string())),
            Ok(None) => (Readback::Absent, None),
            Err(_) => (Readback::Failed, None),
        };
    }
    let walked = match mutation.action {
        ChangeAction::Create | ChangeAction::Version => {
            binding
                .walk(resource, deadline, by_name(item_name(kind, &mutation.body)))
                .await
        }
        ChangeAction::Delete if matches!(kind, ChangeKind::Applications | ChangeKind::ApiKeys) => {
            binding
                .walk(
                    resource,
                    deadline,
                    by_name(before.and_then(|b| item_name(kind, b))),
                )
                .await
        }
        _ => {
            // An update may return a new identifier (v1 notes SCM PUT can).
            let id = crate::management::identity(response.get(id_field))
                .unwrap_or_else(|| mutation.target.clone());
            binding
                .walk(resource, deadline, |item| {
                    crate::management::identity(item.get(id_field)).as_deref() == Some(id.as_str())
                })
                .await
        }
    };
    match walked {
        Ok(Walk::Found(item)) => (Readback::Found, Some(item.to_string())),
        Ok(Walk::Ended) => (Readback::Absent, None),
        Ok(Walk::Incomplete) => (Readback::Incomplete, None),
        Err(_) => (Readback::Failed, None),
    }
}

/// Whether a DLP item read by its ID is one the service has deleted
/// (archived): its `status` is `deleted`.
fn dlp_deleted(item: &Value) -> bool {
    item.get("status")
        .and_then(Value::as_str)
        .is_some_and(|s| s.eq_ignore_ascii_case("deleted"))
}

/// The baseline read of one review.
struct PrepareJob {
    source: BindingSource,
    tenant: TenantId,
    draft: Mutation,
    gateway: Option<GatewayBound>,
    slot: PrepareSlot,
}

impl PrepareJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    async fn run(self, claim: Claim) -> Outcome {
        let result = tokio::select! {
            result = self.prepare(claim.deadline) => result,
            () = claim.cancel.cancelled() => Err(ChangeFailure::Service { failure: ManagementFailure::Cancelled }),
        };
        let outcome = if result.is_ok() {
            Outcome::Completed
        } else {
            Outcome::Unknown
        };
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(result);
        }
        outcome
    }

    async fn prepare(&self, deadline: Instant) -> Result<Prepared, ChangeFailure> {
        let binding = self
            .source
            .bind()
            .await
            .map_err(|failure| ChangeFailure::Service { failure })?;
        if let Some(bound) = &self.gateway {
            let read =
                gateway::baseline(binding, &self.draft, bound.scope.scope.as_ref(), deadline)
                    .await
                    .map_err(gateway_failure)?;
            let digest = review_digest(
                &self.tenant,
                &self.source.client_id,
                &self.draft,
                read.before.as_ref(),
                Some((bound, &read.references)),
            );
            return Ok(Prepared {
                mutation: self.draft.clone(),
                before: read.before,
                digest,
                references: read.references,
            });
        }
        let (mutation, before) = baseline(binding, &self.draft, deadline).await?;
        let digest = review_digest(
            &self.tenant,
            &self.source.client_id,
            &mutation,
            before.as_ref(),
            None,
        );
        Ok(Prepared {
            mutation,
            before,
            digest,
            references: Vec::new(),
        })
    }
}

/// One confirmed change: re-read, compare, write once, read back.
struct ApplyJob {
    source: BindingSource,
    tenant: TenantId,
    draft: Mutation,
    prepared: Prepared,
    gateway: Option<GatewayBound>,
    /// How far it got: a cancellation before the write sent nothing, and
    /// one after an answered write keeps what it established.
    progress: ProgressSlot,
    slot: ApplySlot,
}

impl ApplyJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    fn failed(outcome: Outcome, failure: ChangeFailure) -> ApplyOutput {
        ApplyOutput {
            outcome,
            failure: Some(failure),
            response: None,
            readback: None,
            current: None,
            secret: None,
            provisioning: None,
        }
    }

    async fn run(self, claim: Claim) -> Outcome {
        let output = tokio::select! {
            output = self.apply(claim.deadline) => output,
            () = claim.cancel.cancelled() => self.cancelled(claim.deadline).await,
        };
        let outcome = output.outcome;
        // The answered write's copy (and its key) is dropped, zeroized.
        if let Ok(mut progress) = self.progress.lock() {
            *progress = Progress::Done;
        }
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(output);
        }
        outcome
    }

    /// Marks the write as dispatched, unless its workspace was replaced
    /// first (checked and marked under one lock that retirement takes too).
    fn dispatch(&self, prior: Option<Vec<String>>) -> bool {
        let Ok(mut progress) = self.progress.lock() else {
            return false;
        };
        if matches!(*progress, Progress::Retired) {
            return false;
        }
        *progress = Progress::Sent { prior };
        true
    }

    /// The write was answered: what it established survives a cancelled
    /// readback.
    fn answered(&self, output: &ApplyOutput) {
        if let Ok(mut progress) = self.progress.lock() {
            *progress = Progress::Written(Box::new(ApplyOutput {
                outcome: output.outcome,
                failure: output.failure,
                response: output.response.clone(),
                readback: Some(Readback::Stopped),
                current: None,
                secret: output.secret.clone(),
                provisioning: None,
            }));
        }
    }

    /// A cancelled apply: not sent if the write had not started (or its
    /// workspace was replaced); an answered write keeps its outcome and key
    /// with its readback stopped; otherwise it may have reached the service
    /// (unknown), and a provisioning reads back what exists. A local cancel
    /// never claims a rollback.
    async fn cancelled(&self, deadline: Instant) -> ApplyOutput {
        let cancelled = ChangeFailure::Service {
            failure: ManagementFailure::Cancelled,
        };
        let progress = self
            .progress
            .lock()
            .map(|mut p| std::mem::take(&mut *p))
            .unwrap_or_default();
        let prior = match progress {
            Progress::Waiting => return Self::failed(Outcome::NotSent, cancelled),
            Progress::Retired => {
                return Self::failed(Outcome::NotSent, ChangeFailure::WorkspaceChanged);
            }
            Progress::Written(output) => return *output,
            Progress::Sent { prior } => prior,
            Progress::Done => None,
        };
        let mut output = Self::failed(Outcome::Unknown, cancelled);
        let m = &self.prepared.mutation;
        if (m.kind, m.action) == (ChangeKind::GatewayWorkspaces, ChangeAction::Create)
            && let Ok(binding) = self.source.bind().await
        {
            output.provisioning =
                Some(gateway::cancelled(binding, m, prior.as_deref(), deadline).await);
        }
        output
    }

    async fn apply(&self, deadline: Instant) -> ApplyOutput {
        let binding = match self.source.bind().await {
            Ok(binding) => binding,
            Err(failure) => {
                return Self::failed(Outcome::NotSent, ChangeFailure::Service { failure });
            }
        };
        if let Some(bound) = &self.gateway {
            return self.apply_gateway(binding, bound, deadline).await;
        }
        // v1: the baseline and review digest must still match at apply.
        let fresh = match baseline(binding, &self.draft, deadline).await {
            Ok(fresh) => fresh,
            Err(ChangeFailure::Problem {
                problem: ChangeProblem::NotFound,
            }) => return Self::failed(Outcome::NotSent, ChangeFailure::Conflict),
            Err(failure) => return Self::failed(Outcome::NotSent, failure),
        };
        let digest = review_digest(
            &self.tenant,
            &self.source.client_id,
            &fresh.0,
            fresh.1.as_ref(),
            None,
        );
        if digest != self.prepared.digest {
            return Self::failed(Outcome::NotSent, ChangeFailure::Conflict);
        }
        let mutation = &self.prepared.mutation;
        let before = self.prepared.before.as_ref();
        self.dispatch(None);
        let reply = match binding.write(mutation, before, deadline).await {
            Ok(reply) => reply,
            Err(PageError::Sdk(error)) => {
                let failure = if error.status() == Some(409) {
                    ChangeFailure::Conflict
                } else {
                    ChangeFailure::Service {
                        failure: ManagementFailure::from_sdk(&error),
                    }
                };
                return Self::failed(outcome_of(&error), failure);
            }
            // The service accepted the write but its reply was unusable.
            Err(PageError::Invalid) => {
                return Self::failed(
                    Outcome::Completed,
                    ChangeFailure::Service {
                        failure: ManagementFailure::InvalidResponse,
                    },
                );
            }
        };
        let (response, secret) = take_secret(reply);
        let mut output = ApplyOutput {
            outcome: Outcome::Completed,
            response: Some(response.to_string()),
            secret,
            ..ApplyOutput::default()
        };
        self.answered(&output);
        let (readback, current) = readback(binding, mutation, before, &response, deadline).await;
        output.readback = Some(readback);
        output.current = current;
        output
    }

    /// A Gateway change (C17): the baseline, the references and the review
    /// digest must still match; then exactly one write (or the bounded
    /// provisioning) and its readback. A one-time key is taken out of the
    /// reply before anything else.
    async fn apply_gateway(
        &self,
        binding: &ManagementBinding,
        bound: &GatewayBound,
        deadline: Instant,
    ) -> ApplyOutput {
        let scope = bound.scope.scope.as_ref();
        let fresh = match gateway::baseline(binding, &self.draft, scope, deadline).await {
            Ok(fresh) => fresh,
            Err(ChangeError::Problem(ChangeProblem::NotFound)) => {
                return Self::failed(Outcome::NotSent, ChangeFailure::Conflict);
            }
            Err(error) => return Self::failed(Outcome::NotSent, gateway_failure(error)),
        };
        let digest = review_digest(
            &self.tenant,
            &self.source.client_id,
            &self.draft,
            fresh.before.as_ref(),
            Some((bound, &fresh.references)),
        );
        if !fresh.references.is_empty() {
            return Self::failed(Outcome::NotSent, ChangeProblem::Referenced.into());
        }
        if digest != self.prepared.digest {
            return Self::failed(Outcome::NotSent, ChangeFailure::Conflict);
        }
        let mutation = &self.prepared.mutation;
        let provisioning = (mutation.kind, mutation.action)
            == (ChangeKind::GatewayWorkspaces, ChangeAction::Create);
        // A reused scope's existing workspaces are read first, so the one
        // this attempt creates is never confused with them.
        let prior = if provisioning && mutation.body[gateway::EXISTING_SCOPE] == json!(true) {
            gateway::prior_workspaces(binding, &mutation.body, deadline).await
        } else {
            Some(Vec::new())
        };
        if !self.dispatch(prior.clone()) {
            return Self::failed(Outcome::NotSent, ChangeFailure::WorkspaceChanged);
        }
        if provisioning {
            let (report, failure) =
                gateway::provision(binding, mutation, prior.as_deref(), deadline).await;
            let steps = [report.scope, report.workspace, report.binding];
            let outcome = match &failure {
                None => Outcome::Completed,
                Some(_) if steps.contains(&gateway::StepState::Unknown) => Outcome::Unknown,
                // Partially completed: something it created remains (a
                // scope deleted again after a refused workspace does not).
                Some(_) if steps.contains(&gateway::StepState::Completed) => Outcome::Completed,
                Some(error) => outcome_of(error),
            };
            return ApplyOutput {
                outcome,
                failure: failure.map(|error| ChangeFailure::Service {
                    failure: ManagementFailure::from_sdk(&error),
                }),
                response: None,
                readback: Some(match report.workspace_read {
                    gateway::Reconciled::Found => Readback::Found,
                    gateway::Reconciled::Absent => Readback::Absent,
                    // Neither presence nor absence of this attempt's
                    // workspace is claimed.
                    gateway::Reconciled::Incomplete | gateway::Reconciled::Ambiguous => {
                        Readback::Incomplete
                    }
                    gateway::Reconciled::Failed => Readback::Failed,
                }),
                current: None,
                secret: None,
                provisioning: Some(report),
            };
        }
        let written = match gateway::write(binding, mutation, &bound.tsg, deadline).await {
            Ok(written) => written,
            Err(PageError::Sdk(error)) => {
                let failure = if error.status() == Some(409) {
                    ChangeFailure::Conflict
                } else {
                    ChangeFailure::Service {
                        failure: ManagementFailure::from_sdk(&error),
                    }
                };
                return Self::failed(outcome_of(&error), failure);
            }
            Err(PageError::Invalid) => {
                return Self::failed(
                    Outcome::Completed,
                    ChangeFailure::Service {
                        failure: ManagementFailure::InvalidResponse,
                    },
                );
            }
        };
        let before = self.prepared.before.as_ref();
        let mut output = ApplyOutput {
            outcome: Outcome::Completed,
            response: Some(written.reply.to_string()),
            secret: written.secret,
            ..ApplyOutput::default()
        };
        self.answered(&output);
        let (readback, current) =
            match gateway::readback(binding, mutation, before, &written.reply, scope, deadline)
                .await
            {
                ReadBack::Found(item) => (Readback::Found, Some(item)),
                ReadBack::Differs(item) => {
                    (Readback::Differs, Some(item).filter(|i| !i.is_empty()))
                }
                ReadBack::Absent => (Readback::Absent, None),
                ReadBack::Incomplete => (Readback::Incomplete, None),
                ReadBack::Failed => (Readback::Failed, None),
            };
        output.readback = Some(readback);
        output.current = current;
        output
    }
}
