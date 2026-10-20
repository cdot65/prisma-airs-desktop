//! API Client Send (C15, ADR 0004). A Send carries the editor's fresh
//! snapshot; Rust renders it against the current environment layers,
//! admits it natively before any secret is read, and builds and sends the
//! actual request itself. Known tenant-credential changes stop at an exact
//! review: Apply re-renders the same snapshot against the current files,
//! must reproduce the reviewed digest, takes the operator's native
//! confirmation, consumes the review and sends once. Transfers run off the
//! workbench lane (at most four at a time, each cancellable); their results
//! go to history and nothing is ever sent again by itself.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::super::{ConfirmationRequest, Inner, ScanCapability, State, lock};
use super::{
    WorkbenchProblem, WorkbenchRequest, WorkbenchResult, blocking, current, ensure_profile,
    refused, with_open,
};
use crate::context::{ContextRef, ReviewId};
use crate::tenants::Region;
use crate::vault::{RecordId, StorageMode, Vault, VaultError};
use crate::workbench::admission::{
    self, Admitted, CredentialClass, Destinations, Network, OriginClass, Registered, SecretFailure,
    SendRefusal,
};
use crate::workbench::cookies::Jar;
use crate::workbench::document::Fields;
use crate::workbench::environments::{self, Layers};
use crate::workbench::execute::{self, Tokens, Transfer};
use crate::workbench::history::{self, BodyWriter, ExportFailure, RunRecord, RunState};
use crate::workbench::profile;
use crate::workbench::project;
use crate::workbench::render::{self, Body, PartValue, Plan, Secrets};

/// Requests running at once (v1: 4).
pub const MAX_ACTIVE: usize = 4;
const REVIEW_BODY: usize = 4 * 1024;

/// A finished (or shown) run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub record: RunRecord,
    /// The body's first 256 KiB as text, when it is text.
    pub preview: Option<String>,
    pub preview_bytes: u32,
    /// A 401 dropped the minted token: Send again for a new one.
    pub token_dropped: bool,
}

/// A request in flight.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunningView {
    pub run: String,
    pub request: String,
    pub method: String,
    /// Bytes sent and received so far.
    #[ts(type = "number")]
    pub bytes: u64,
}

/// A tenant-credential change awaiting the operator (secrets masked).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SendReviewView {
    pub review: ReviewId,
    pub request: String,
    /// The catalogue operation.
    pub operation: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    #[ts(type = "number")]
    pub body_bytes: u64,
    /// Collection files the body sends (bound by their content).
    pub files: Vec<String>,
    /// The origin it goes to.
    pub destination: String,
    /// Seconds since the Unix epoch after which it cannot be applied.
    #[ts(type = "number")]
    pub expires: u64,
}

struct Active {
    cancel: CancellationToken,
    progress: Arc<AtomicU64>,
    request: String,
    method: String,
}

struct Pending {
    view: SendReviewView,
    digest: String,
    context: Option<ContextRef>,
    created: Instant,
}

/// Send state held by the workbench.
#[derive(Default)]
pub(in crate::authority) struct Sending {
    runs: BTreeMap<String, Active>,
    review: Option<Pending>,
    consumed: Vec<ReviewId>,
    tokens: Arc<Tokens>,
    jars: BTreeMap<String, Arc<Mutex<Jar>>>,
    /// Each collection's history once read this session.
    histories: BTreeMap<String, Vec<RunRecord>>,
}

impl Sending {
    /// Reset: running requests are cancelled; tokens, cookies, reviews and
    /// histories are forgotten (their files go with the private folder).
    pub(super) fn clear(&mut self) {
        for active in self.runs.values() {
            active.cancel.cancel();
        }
        self.review = None;
        self.tokens.clear();
        self.jars.clear();
        self.histories.clear();
    }

    /// A collection was closed or another opened.
    pub(super) fn leave(&mut self) {
        self.review = None;
    }

    pub(super) fn running(&self) -> Vec<RunningView> {
        self.runs
            .iter()
            .map(|(run, a)| RunningView {
                run: run.clone(),
                request: a.request.clone(),
                method: a.method.clone(),
                bytes: a.progress.load(Ordering::Relaxed),
            })
            .collect()
    }

    pub(super) fn review(&self) -> Option<SendReviewView> {
        self.review.as_ref().map(|p| p.view.clone())
    }

    /// The active context changed (tenant switch, credential replacement,
    /// reset): a review bound to another context is retired for good.
    pub(in crate::authority) fn retire(&mut self, keep: Option<&ContextRef>) {
        let retired = self
            .review
            .as_ref()
            .is_some_and(|p| p.context.is_some() && p.context.as_ref() != keep);
        if retired && let Some(p) = self.review.take() {
            self.consumed.push(p.view.review);
        }
    }
}

/// What a request does after its turn on the lane.
pub(super) enum Step {
    Done(Box<WorkbenchResult>),
    /// Runs off the lane (confirmation, transfer).
    Later(Pin<Box<dyn Future<Output = WorkbenchResult> + Send>>),
}

fn refusal(refusal: SendRefusal) -> WorkbenchResult {
    WorkbenchResult::SendRefused { refusal }
}

/// The active tenant's credentials a managed request may use.
#[derive(Clone, PartialEq, Eq)]
struct Managed {
    tenant: String,
    client_id: Option<String>,
    management: Option<(RecordId, StorageMode)>,
    scan: Option<(RecordId, StorageMode)>,
}

impl State {
    /// Every registered Prisma origin, whatever tenant is active: the
    /// configured management and token origins, the workbench API origins,
    /// the AIRS scan regions and each tenant's scan destination.
    fn registered_origins(&self) -> Registered {
        let endpoints = &self.management_endpoints;
        let mut registered = Registered::default();
        for base in [
            &endpoints.api,
            &endpoints.dlp,
            &endpoints.red_team_data,
            &endpoints.red_team_mgmt,
            &endpoints.red_team_broker,
            &endpoints.model_data,
            &endpoints.model_mgmt,
            &endpoints.agentguard_data,
            &endpoints.agentguard_mgmt,
        ]
        .into_iter()
        .chain(&self.workbench_api_origins)
        {
            registered.add(base, OriginClass::Api);
        }
        registered.add(&endpoints.token, OriginClass::Auth);
        for region in [
            Region::UnitedStates,
            Region::Europe,
            Region::India,
            Region::Singapore,
        ] {
            registered.add(region.origin(), OriginClass::Scan);
        }
        for tenant in &self.data.tenants {
            registered.add(tenant.profile.destination.origin(), OriginClass::Scan);
        }
        registered
    }

    /// Where `context`'s tenant credentials may go, and their records.
    fn send_destinations(
        &self,
        context: Option<&ContextRef>,
    ) -> Result<(Destinations, Option<Managed>), SendRefusal> {
        let registered = self.registered_origins();
        let Some(context) = context else {
            return Ok((
                Destinations {
                    registered,
                    ..Destinations::default()
                },
                None,
            ));
        };
        let tenant = self
            .check_context(context)
            .map_err(|_| SendRefusal::NoTenant)?;
        let current = self.tenant(&tenant).ok_or(SendRefusal::NoTenant)?;
        let endpoints = &self.management_endpoints;
        let token_origin = endpoints.token_origin();
        let mut destinations = Destinations {
            tsg: current.profile.tsg_id.clone(),
            registered,
            ..Destinations::default()
        };
        let mut managed = Managed {
            tenant: tenant.as_str().to_owned(),
            client_id: None,
            management: None,
            scan: None,
        };
        if let ScanCapability::Ready {
            record,
            mode,
            destination,
        } = &current.management
            && token_origin.as_deref() == Some(destination.as_str())
            && let Some(client) = &current.management_client
        {
            destinations.management = true;
            destinations.token_urls = vec![
                endpoints.token.clone(),
                format!("{destination}{}", admission::CATALOGUE_TOKEN_PATH),
            ];
            destinations.token_origin = Some(destination.clone());
            for base in [
                &endpoints.api,
                &endpoints.dlp,
                &endpoints.red_team_data,
                &endpoints.red_team_mgmt,
                &endpoints.red_team_broker,
                &endpoints.model_data,
                &endpoints.model_mgmt,
                &endpoints.agentguard_data,
                &endpoints.agentguard_mgmt,
            ] {
                if let Ok(u) = url::Url::parse(base) {
                    destinations
                        .api_origins
                        .insert(u.origin().ascii_serialization());
                }
            }
            destinations
                .api_origins
                .extend(self.workbench_api_origins.iter().cloned());
            managed.client_id = Some(client.clone());
            managed.management = Some((record.clone(), *mode));
        }
        if let ScanCapability::Ready {
            record,
            mode,
            destination,
        } = &current.scan
        {
            destinations.scan_origin = Some(destination.clone());
            managed.scan = Some((record.clone(), *mode));
        }
        Ok((destinations, Some(managed)))
    }
}

/// A rendered, admitted request and where it comes from.
#[derive(Clone)]
struct Planned {
    plan: Plan,
    admitted: Admitted,
    request: String,
    relative: String,
    folder: PathBuf,
    collection: String,
    scope: String,
    managed: Option<Managed>,
    context: Option<ContextRef>,
    /// Body files' content as reviewed (empty unless applied).
    pins: BTreeMap<String, String>,
}

/// Renders and admits the snapshot `fields` of document `id` at
/// `revision` (nothing secret is read).
async fn plan(
    inner: &Arc<Inner>,
    id: &str,
    revision: &str,
    fields: Fields,
    context: Option<ContextRef>,
) -> Result<Planned, Box<WorkbenchResult>> {
    let found = with_open(inner, |o| {
        let doc = current(o, id, revision)?;
        let env = o.data.environment.clone();
        Ok((
            doc,
            o.catalog.root.clone(),
            o.catalog.id.clone(),
            o.scope.clone(),
            env.clone(),
            env.and_then(|e| o.data.private.get(&e).cloned()),
            o.data.private.get(environments::DEFAULTS).cloned(),
        ))
    })
    .map_err(|p| Box::new(refused(p)))?;
    let (doc, folder, collection, scope, env, local, defaults) = found;
    let reference = doc.body_reference();
    let stored_body = if fields.body_kind == "file" {
        if fields.body != doc.saved.body {
            return Err(Box::new(refusal(SendRefusal::Body {
                problem: admission::BodyProblem::Unsaved,
            })));
        }
        reference.clone()
    } else if doc.body_editable {
        None
    } else {
        reference.clone()
    };
    let read_folder = folder.clone();
    let rendered = blocking(Err(SendRefusal::Private), move || {
        let read = |relative: &str| {
            project::relative_path(relative, false)
                .and_then(|r| {
                    project::read_optional(&read_folder, &r, environments::ENVIRONMENT_BYTES)
                        .ok()
                        .flatten()
                })
                .and_then(|b| String::from_utf8(b).ok())
        };
        let layers = Layers {
            collection: read("collection.toml"),
            local_defaults: defaults,
            shared: env
                .as_ref()
                .and_then(|e| read(&format!("environments/{e}.toml"))),
            local,
        };
        render::render(&fields, &layers, stored_body.as_deref())
            .map_err(|problem| SendRefusal::Render { problem })
    })
    .await;
    let mut plan = rendered.map_err(|r| Box::new(refusal(r)))?;
    let (destinations, managed, network) = {
        let state = lock(inner).map_err(|_| Box::new(refused(WorkbenchProblem::Unavailable)))?;
        let (d, m) = state
            .send_destinations(context.as_ref())
            .map_err(|r| Box::new(refusal(r)))?;
        (d, m, state.workbench_network)
    };
    if let Some(tsg) = &destinations.tsg
        && plan.references().contains(admission::TSG_ID)
    {
        admission::fill_tsg(&mut plan, tsg);
    }
    let admitted =
        admission::admit(&plan, network, &destinations).map_err(|r| Box::new(refusal(r)))?;
    Ok(Planned {
        relative: doc.relative.clone(),
        request: doc.id,
        plan,
        admitted,
        folder,
        collection,
        scope,
        managed,
        context,
        pins: BTreeMap::new(),
    })
}

fn bound_body(
    body: &Body,
    folder: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<serde_json::Value, SendRefusal> {
    let mut hash = |relative: &str| {
        let digest = crate::workbench::document::hash_file(folder, relative)
            .ok()
            .flatten()
            .ok_or(SendRefusal::Body {
                problem: admission::BodyProblem::Missing,
            })?;
        files.insert(relative.to_owned(), digest.clone());
        Ok::<_, SendRefusal>(digest)
    };
    Ok(match body {
        Body::None => serde_json::Value::Null,
        Body::Text { text, json } => serde_json::json!({"text": text.bound(), "json": json}),
        Body::Form(rows) => {
            serde_json::json!({"form": rows.iter().map(|(n, v)| [n.bound(), v.bound()]).collect::<Vec<_>>()})
        }
        Body::Multipart(parts) => {
            let mut out = Vec::new();
            for p in parts {
                out.push(match &p.value {
                    PartValue::Text(t) => serde_json::json!({"name": p.name, "value": t.bound()}),
                    PartValue::File { relative, mime } => {
                        serde_json::json!({"name": p.name, "file": relative, "sha256": hash(relative)?, "mime": mime})
                    }
                });
            }
            serde_json::json!({"multipart": out})
        }
        Body::File { relative } => serde_json::json!({"file": relative, "sha256": hash(relative)?}),
    })
}

/// What a review is bound to: the tenant credential and destination, and
/// the exact expanded request (secrets by reference, files by content).
/// The files' hashes come back as the pins a send must keep to.
fn digest(p: &Planned) -> Result<(String, BTreeMap<String, String>), SendRefusal> {
    let mut files = BTreeMap::new();
    let plan = &p.plan;
    let managed = p.managed.as_ref();
    let auth = match &plan.auth {
        render::Auth::OAuth2 {
            token_url,
            client_id,
            scope,
            secret,
        } => serde_json::json!([
            "oauth2",
            token_url.bound(),
            client_id.bound(),
            scope.bound(),
            secret
        ]),
        render::Auth::ApiKey {
            name,
            query,
            secret,
        } => serde_json::json!(["api-key", name, query, secret]),
        other => serde_json::json!([other.kind()]),
    };
    let bound = serde_json::json!({
        "tenant": managed.map(|m| m.tenant.clone()),
        "client": managed.and_then(|m| m.client_id.clone()),
        "management": managed.and_then(|m| m.management.as_ref().map(|(r, _)| r.as_str().to_owned())),
        "scan": managed.and_then(|m| m.scan.as_ref().map(|(r, _)| r.as_str().to_owned())),
        "context": p.context.as_ref().map(ContextRef::as_str),
        "operation": p.admitted.operation.as_ref().map(|o| o.index),
        "method": plan.method,
        "url": plan.url.bound(),
        "query": plan.query.iter().map(|(n, v)| [n.bound(), v.bound()]).collect::<Vec<_>>(),
        "headers": plan.headers.iter().map(|(n, v)| [n.clone(), v.bound()]).collect::<Vec<_>>(),
        "auth": auth,
        "body": bound_body(&plan.body, &p.folder, &mut files)?,
        "variables": plan.used,
        "request": p.request,
    });
    Ok((
        hex::encode(Sha256::digest(bound.to_string().as_bytes())),
        files,
    ))
}

fn display_url(plan: &Plan) -> String {
    let mut url = plan.url.masked();
    if !plan.query.is_empty() {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        for (n, v) in &plan.query {
            form.append_pair(&n.masked(), &v.masked());
        }
        url.push(if url.contains('?') { '&' } else { '?' });
        url.push_str(&form.finish());
    }
    url
}

fn review_view(p: &Planned, review: ReviewId, expires: u64) -> SendReviewView {
    let plan = &p.plan;
    let (body, files, bytes) = match &plan.body {
        Body::None => (String::new(), Vec::new(), 0),
        Body::Text { text, .. } => {
            let masked = text.masked();
            let n = masked.len() as u64;
            (masked.chars().take(REVIEW_BODY).collect(), Vec::new(), n)
        }
        Body::Form(rows) => {
            let text = rows
                .iter()
                .map(|(n, v)| format!("{}={}", n.masked(), v.masked()))
                .collect::<Vec<_>>()
                .join("\n");
            let n = text.len() as u64;
            (text, Vec::new(), n)
        }
        Body::Multipart(parts) => {
            let mut files = Vec::new();
            let text = parts
                .iter()
                .map(|part| match &part.value {
                    PartValue::Text(t) => format!("{} = {}", part.name, t.masked()),
                    PartValue::File { relative, .. } => {
                        files.push(relative.clone());
                        format!("{} = file {relative}", part.name)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            (text, files, 0)
        }
        Body::File { relative } => (String::new(), vec![relative.clone()], 0),
    };
    SendReviewView {
        review,
        request: p.request.clone(),
        operation: p
            .admitted
            .operation
            .as_ref()
            .map(|o| o.name.clone())
            .unwrap_or_default(),
        method: plan.method.clone(),
        url: display_url(plan),
        headers: plan
            .headers
            .iter()
            .map(|(n, v)| (n.clone(), v.masked()))
            .collect(),
        body,
        body_bytes: bytes,
        files,
        destination: p.admitted.url.origin().ascii_serialization(),
        expires,
    }
}

/// A Send or Apply in its turn on the lane.
pub(super) async fn begin(inner: &Arc<Inner>, request: WorkbenchRequest) -> Step {
    let (id, revision, fields, context, review) = match request {
        WorkbenchRequest::Send {
            id,
            revision,
            fields,
            context,
        } => (id, revision, fields, context, None),
        WorkbenchRequest::Apply {
            review,
            id,
            revision,
            fields,
            context,
        } => (id, revision, fields, context, Some(review)),
        _ => return Step::Done(Box::new(refused(WorkbenchProblem::Invalid))),
    };
    let Some(root) = inner.adapters.workbench_root.clone() else {
        return Step::Done(Box::new(refused(WorkbenchProblem::Unavailable)));
    };
    if let Err(problem) = ensure_profile(inner, &root).await {
        return Step::Done(Box::new(refused(problem)));
    }
    let planned = match plan(inner, &id, &revision, fields.clone(), context.clone()).await {
        Ok(p) => p,
        Err(result) => return Step::Done(result),
    };
    if !planned.admitted.needs_review() {
        if review.is_some() {
            return Step::Done(Box::new(refusal(SendRefusal::ReviewMissing)));
        }
        return Step::Later(Box::pin(dispatch(Arc::clone(inner), root, planned)));
    }
    let digest = {
        let (p, result) = blocking((None, Err(SendRefusal::Private)), move || {
            let d = digest(&planned);
            (Some(planned), d)
        })
        .await;
        match (p, result) {
            (Some(p), Ok((d, _))) => (p, d),
            (_, Err(r)) => return Step::Done(Box::new(refusal(r))),
            (None, Ok(_)) => return Step::Done(Box::new(refusal(SendRefusal::Private))),
        }
    };
    let (planned, digest) = digest;
    let Ok(mut state) = lock(inner) else {
        return Step::Done(Box::new(refused(WorkbenchProblem::Unavailable)));
    };
    let lifetime = state.review_lifetime;
    let Some(review) = review else {
        let Some(id) = ReviewId::issue() else {
            return Step::Done(Box::new(refusal(SendRefusal::Private)));
        };
        let expires = history::now() + lifetime.as_secs();
        let view = review_view(&planned, id, expires);
        state.workbench.sending.review = Some(Pending {
            view: view.clone(),
            digest,
            context: planned.context.clone(),
            created: Instant::now(),
        });
        state.changed();
        return Step::Done(Box::new(WorkbenchResult::ReviewRequired { review: view }));
    };
    if let Err(r) = check_review(&mut state, &review, &planned, &digest, lifetime) {
        state.changed();
        return Step::Done(Box::new(refusal(r)));
    }
    let view = state
        .workbench
        .sending
        .review
        .as_ref()
        .map(|p| p.view.clone());
    drop(state);
    let Some(view) = view else {
        return Step::Done(Box::new(refusal(SendRefusal::ReviewMissing)));
    };
    let inner = Arc::clone(inner);
    Step::Later(Box::pin(async move {
        apply(inner, root, view, id, revision, fields, context).await
    }))
}

fn check_review(
    state: &mut State,
    review: &ReviewId,
    planned: &Planned,
    digest: &str,
    lifetime: std::time::Duration,
) -> Result<(), SendRefusal> {
    let sending = &mut state.workbench.sending;
    if sending.consumed.contains(review) {
        return Err(SendRefusal::ReviewConsumed);
    }
    let Some(pending) = sending.review.as_ref().filter(|p| &p.view.review == review) else {
        return Err(SendRefusal::ReviewMissing);
    };
    if pending.created.elapsed() > lifetime {
        sending.review = None;
        return Err(SendRefusal::ReviewExpired);
    }
    if pending.digest != digest
        || pending.view.request != planned.request
        || pending.context != planned.context
    {
        return Err(SendRefusal::ReviewStale);
    }
    // Bound to the context active now (a retired one never comes back).
    let context = pending.context.clone();
    if let Some(context) = context
        && state.check_context(&context).is_err()
    {
        state.workbench.sending.retire(None);
        return Err(SendRefusal::NoTenant);
    }
    Ok(())
}

impl Planned {
    /// Whether this request must still be admitted by its context when it
    /// goes: it uses a tenant credential, or it was reviewed for one.
    fn context_bound(&self) -> bool {
        self.context.is_some()
            && (self.admitted.class == CredentialClass::Managed || self.admitted.needs_review())
    }
}

/// Whether `planned` may still go: its context is the active one and the
/// tenant's credentials are those it was planned with.
fn still_admitted(inner: &Inner, planned: &Planned) -> bool {
    if !planned.context_bound() {
        return true;
    }
    let Ok(state) = lock(inner) else {
        return false;
    };
    state
        .send_destinations(planned.context.as_ref())
        .is_ok_and(|(_, managed)| managed == planned.managed)
}

/// The operator confirms natively; the same snapshot is re-planned against
/// the current files and must still match; the review is consumed and the
/// change is sent once.
async fn apply(
    inner: Arc<Inner>,
    root: PathBuf,
    view: SendReviewView,
    id: String,
    revision: String,
    fields: Fields,
    context: Option<ContextRef>,
) -> WorkbenchResult {
    let mut nonce = [0u8; 8];
    if getrandom::fill(&mut nonce).is_err() {
        return refusal(SendRefusal::Private);
    }
    let request = ConfirmationRequest {
        title: format!("Send {} to the tenant?", view.method),
        message: format!(
            "{} {}\n{} · {}\nThis change is sent once. It is never sent again by itself.",
            view.method, view.url, view.operation, view.destination
        ),
        confirm_label: "Send".to_owned(),
        destructive: view.method == "DELETE",
    };
    if !inner
        .adapters
        .confirmer
        .confirm(u64::from_be_bytes(nonce), request)
        .await
    {
        return refusal(SendRefusal::Declined);
    }
    let planned = match plan(&inner, &id, &revision, fields, context).await {
        Ok(p) => p,
        Err(result) => return *result,
    };
    let (planned, digest) = blocking((None, Err(SendRefusal::Private)), move || {
        let d = digest(&planned);
        (Some(planned), d)
    })
    .await;
    let (Some(mut planned), Ok((digest, pins))) = (planned, digest) else {
        return refusal(SendRefusal::ReviewStale);
    };
    // The files go as they were when this matched the review.
    planned.pins = pins;
    {
        let Ok(mut state) = lock(&inner) else {
            return refused(WorkbenchProblem::Unavailable);
        };
        let lifetime = state.review_lifetime;
        if let Err(r) = check_review(&mut state, &view.review, &planned, &digest, lifetime) {
            state.changed();
            return refusal(r);
        }
        // Consumed with admission: a second Apply finds it gone.
        state.workbench.sending.review = None;
        state.workbench.sending.consumed.push(view.review.clone());
        let excess = state.workbench.sending.consumed.len().saturating_sub(256);
        state.workbench.sending.consumed.drain(..excess);
        state.changed();
    }
    dispatch(inner, root, planned).await
}

fn secret_failure(error: VaultError) -> SecretFailure {
    match error {
        VaultError::Locked => SecretFailure::Locked,
        VaultError::Denied => SecretFailure::Denied,
        _ => SecretFailure::Missing,
    }
}

/// Reads each secret the admitted request uses: tenant credentials from
/// their capability records, workbench secrets from the collection's
/// Keychain records.
fn read_secrets(
    planned: &Planned,
    keychain: &dyn Vault,
    session: &dyn Vault,
) -> Result<Secrets, SendRefusal> {
    let mut secrets = Secrets::new();
    for reference in planned.plan.references() {
        let failed = |failure| SendRefusal::Secret {
            reference: reference.clone(),
            failure,
        };
        let value = if reference.starts_with(crate::workbench::scm::MANAGED_PREFIX) {
            let managed = planned.managed.as_ref().ok_or(SendRefusal::NoTenant)?;
            let record = match reference.as_str() {
                admission::CLIENT_ID => {
                    let id = managed.client_id.clone().ok_or(SendRefusal::NoTenant)?;
                    secrets.insert(reference.clone(), Zeroizing::new(id));
                    continue;
                }
                admission::CLIENT_SECRET => managed.management.clone(),
                admission::API_KEY => managed.scan.clone(),
                _ => None,
            };
            let (record, mode) = record.ok_or(SendRefusal::NoTenant)?;
            match mode {
                StorageMode::Keychain => keychain.get(&record),
                StorageMode::Session => session.get(&record),
            }
            .map_err(|e| failed(secret_failure(e)))?
        } else {
            keychain
                .get(&profile::secret_record(&planned.collection, &reference))
                .map_err(|e| failed(secret_failure(e)))?
        };
        let text = String::from_utf8(value.to_vec()).map_err(|_| failed(SecretFailure::Invalid))?;
        execute::secret_ok(&text).map_err(failed)?;
        secrets.insert(reference.clone(), Zeroizing::new(text));
    }
    Ok(secrets)
}

/// The collection's history, read (and recovered) once per session.
fn history_of<'a>(
    sending: &'a mut Sending,
    scope: &str,
    loaded: impl FnOnce() -> Vec<RunRecord>,
) -> &'a mut Vec<RunRecord> {
    sending
        .histories
        .entry(scope.to_owned())
        .or_insert_with(loaded)
}

async fn load_history(inner: &Arc<Inner>, root: &Path, scope: &str) -> Result<(), SendRefusal> {
    let loaded = lock(inner)
        .map(|s| s.workbench.sending.histories.contains_key(scope))
        .unwrap_or(false);
    if loaded {
        return Ok(());
    }
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let scope_name = scope.to_owned();
    let runs = blocking(Err(SendRefusal::Private), move || {
        let mut runs = profile::load_history(&private, keychain.as_ref(), &scope_name)
            .map_err(|_| SendRefusal::Private)?;
        if history::recover(&private, &scope_name, &mut runs) {
            profile::store_history(&private, keychain.as_ref(), &scope_name, &runs)
                .map_err(|_| SendRefusal::Private)?;
        }
        Ok(runs)
    })
    .await?;
    if let Ok(mut state) = lock(inner) {
        history_of(&mut state.workbench.sending, scope, || runs);
    }
    Ok(())
}

async fn store_history(inner: &Arc<Inner>, root: &Path, scope: &str) -> Result<(), SendRefusal> {
    let runs = lock(inner)
        .ok()
        .and_then(|s| s.workbench.sending.histories.get(scope).cloned())
        .unwrap_or_default();
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let scope = scope.to_owned();
    blocking(Err(SendRefusal::Private), move || {
        profile::store_history(&private, keychain.as_ref(), &scope, &runs)
            .map_err(|_| SendRefusal::Private)
    })
    .await
}

async fn jar(inner: &Arc<Inner>, root: &Path, scope: &str) -> Option<Arc<Mutex<Jar>>> {
    if let Some(jar) = lock(inner)
        .ok()
        .and_then(|s| s.workbench.sending.jars.get(scope).cloned())
    {
        return Some(jar);
    }
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let name = scope.to_owned();
    let stored = blocking(Vec::new(), move || {
        profile::load_cookies(&private, keychain.as_ref(), &name).unwrap_or_default()
    })
    .await;
    let mut state = lock(inner).ok()?;
    Some(Arc::clone(
        state
            .workbench
            .sending
            .jars
            .entry(scope.to_owned())
            .or_insert_with(|| {
                Arc::new(Mutex::new(Jar::from_stored(stored, history::now() as i64)))
            }),
    ))
}

fn preview_text(bytes: &[u8]) -> Option<String> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text.to_owned()),
        // A character cut at the preview's end is still text.
        Err(e) if e.error_len().is_none() => {
            Some(String::from_utf8_lossy(&bytes[..e.valid_up_to()]).into_owned())
        }
        Err(_) => None,
    }
}

fn run_view(record: RunRecord, preview: &[u8], token_dropped: bool) -> RunView {
    RunView {
        record,
        preview_bytes: u32::try_from(preview.len()).unwrap_or(u32::MAX),
        preview: preview_text(preview),
        token_dropped,
    }
}

/// Sends an admitted request once, recording it in history.
async fn dispatch(inner: Arc<Inner>, root: PathBuf, planned: Planned) -> WorkbenchResult {
    let Some(run) = history::new_run_id() else {
        return refusal(SendRefusal::Private);
    };
    let cancel = CancellationToken::new();
    let progress = Arc::new(AtomicU64::new(0));
    {
        let Ok(mut state) = lock(&inner) else {
            return refused(WorkbenchProblem::Unavailable);
        };
        if state.workbench.sending.runs.len() >= MAX_ACTIVE {
            return refusal(SendRefusal::Busy);
        }
        state.workbench.sending.runs.insert(
            run.clone(),
            Active {
                cancel: cancel.clone(),
                progress: Arc::clone(&progress),
                request: planned.request.clone(),
                method: planned.plan.method.clone(),
            },
        );
        state.changed();
    }
    let result = transfer(&inner, &root, &planned, &run, cancel, progress).await;
    if let Ok(mut state) = lock(&inner) {
        state.workbench.sending.runs.remove(&run);
        state.changed();
    }
    result
}

async fn transfer(
    inner: &Arc<Inner>,
    root: &Path,
    planned: &Planned,
    run: &str,
    cancel: CancellationToken,
    progress: Arc<AtomicU64>,
) -> WorkbenchResult {
    // Secrets are read only now, after admission, and only while the
    // context it was planned for is still the active one.
    if !still_admitted(inner, planned) {
        return refusal(SendRefusal::NoTenant);
    }
    let keychain = Arc::clone(&inner.adapters.keychain);
    let session = Arc::clone(&inner.session);
    let plan = planned.plan.clone();
    let admitted = planned.admitted.clone();
    let probe = planned.clone();
    let prepared = blocking(Err(SendRefusal::Private), move || {
        let secrets = read_secrets(&probe, keychain.as_ref(), session.as_ref())?;
        execute::prepare(
            &probe.plan,
            &probe.admitted,
            &secrets,
            &probe.folder,
            &probe.pins,
        )
    })
    .await;
    let prepared = match prepared {
        Ok(p) => p,
        Err(r) => return refusal(r),
    };
    let (policy, network, registered) = match lock(inner) {
        Ok(s) => (
            s.transport
                .as_ref()
                .map(|f| f.policy().clone())
                .unwrap_or_default(),
            s.workbench_network,
            s.registered_origins(),
        ),
        Err(_) => return refused(WorkbenchProblem::Unavailable),
    };
    let Ok(client) = crate::transport::workbench_client(&policy, network == Network::LoopbackOnly)
    else {
        return refusal(SendRefusal::Network);
    };
    if let Err(r) = load_history(inner, root, &planned.scope).await {
        return refusal(r);
    }
    let record = RunRecord {
        id: run.to_owned(),
        request: planned.request.clone(),
        relative: planned.relative.clone(),
        at: history::now(),
        method: plan.method.clone(),
        url: if prepared.short_secret() {
            "Details withheld: a secret is too short to redact".to_owned()
        } else {
            display_url(&plan)
        },
        class: admitted.class,
        operation: admitted.operation.as_ref().map(|o| o.name.clone()),
        state: RunState::Running,
        failure: None,
        uncertain: false,
        status: None,
        headers: Vec::new(),
        redirect: None,
        elapsed_ms: 0,
        bytes: 0,
        stored: false,
    };
    // Recorded as running first: a stop now is found as interrupted.
    if let Ok(mut state) = lock(inner) {
        let runs = history_of(&mut state.workbench.sending, &planned.scope, Vec::new);
        runs.insert(0, record.clone());
    }
    if let Err(r) = store_history(inner, root, &planned.scope).await {
        if let Ok(mut state) = lock(inner)
            && let Some(runs) = state.workbench.sending.histories.get_mut(&planned.scope)
        {
            runs.retain(|r| r.id != run);
        }
        return refusal(r);
    }
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let scope = planned.scope.clone();
    let id = run.to_owned();
    let sink = blocking(None, move || {
        BodyWriter::create(&private, keychain.as_ref(), &scope, &id).ok()
    })
    .await;
    let cookies = if admitted.class == CredentialClass::Managed {
        None
    } else {
        jar(inner, root, &planned.scope).await
    };
    let tokens = lock(inner)
        .map(|s| Arc::clone(&s.workbench.sending.tokens))
        .unwrap_or_default();
    let ended = match sink {
        Some(sink) => {
            execute::send(
                &prepared,
                Transfer {
                    client,
                    network,
                    cookies: cookies.clone(),
                    tokens,
                    cancel,
                    progress,
                    registered,
                    still: {
                        let inner = Arc::clone(inner);
                        let planned = planned.clone();
                        Arc::new(move || still_admitted(&inner, &planned))
                    },
                },
                sink,
            )
            .await
        }
        None => execute::Ended {
            state: RunState::Failed,
            failure: Some(history::RunFailure::Storage),
            uncertain: false,
            status: None,
            headers: Vec::new(),
            redirect: None,
            elapsed_ms: 0,
            bytes: 0,
            stored: false,
            preview: Vec::new(),
            token_dropped: false,
        },
    };
    let mut finished = record;
    finished.state = ended.state;
    finished.failure = ended.failure;
    finished.uncertain = ended.uncertain;
    finished.status = ended.status;
    finished.headers = ended.headers.clone();
    finished.redirect = ended.redirect.clone();
    finished.elapsed_ms = ended.elapsed_ms;
    finished.bytes = ended.bytes;
    finished.stored = ended.stored;
    let dropped = {
        let Ok(mut state) = lock(inner) else {
            return refused(WorkbenchProblem::Unavailable);
        };
        let runs = history_of(&mut state.workbench.sending, &planned.scope, Vec::new);
        if let Some(slot) = runs.iter_mut().find(|r| r.id == run) {
            *slot = finished.clone();
        }
        history::prune(runs, Some(run), history::now())
    };
    for id in &dropped {
        history::remove_body(root, &planned.scope, id);
    }
    let _ = store_history(inner, root, &planned.scope).await;
    if let Some(jar) = cookies {
        let persistent = jar.lock().ok().and_then(|mut j| {
            let dirty = std::mem::take(&mut j.dirty);
            dirty.then(|| j.persistent(history::now() as i64))
        });
        if let Some(cookies) = persistent {
            let keychain = Arc::clone(&inner.adapters.keychain);
            let private = root.to_path_buf();
            let scope = planned.scope.clone();
            blocking((), move || {
                let _ = profile::store_cookies(&private, keychain.as_ref(), &scope, &cookies);
            })
            .await;
        }
    }
    WorkbenchResult::Ran {
        run: run_view(finished, &ended.preview, ended.token_dropped),
    }
}

/// Cancels a running request (a change it already sent stays uncertain).
pub(super) fn cancel(inner: &Arc<Inner>, run: &str) -> WorkbenchResult {
    match lock(inner)
        .ok()
        .and_then(|s| s.workbench.sending.runs.get(run).map(|a| a.cancel.clone()))
    {
        Some(token) => {
            token.cancel();
            WorkbenchResult::Done
        }
        None => refused(WorkbenchProblem::Missing),
    }
}

pub(super) fn dismiss_review(inner: &Arc<Inner>) -> WorkbenchResult {
    if let Ok(mut state) = lock(inner) {
        state.workbench.sending.review = None;
        state.changed();
    }
    WorkbenchResult::Done
}

fn open_scope(inner: &Arc<Inner>) -> Result<String, Box<WorkbenchResult>> {
    with_open(inner, |o| Ok(o.scope.clone())).map_err(|p| Box::new(refused(p)))
}

/// A page of the open collection's history, newest first.
pub(super) async fn history_page(
    inner: &Arc<Inner>,
    root: &Path,
    before: Option<String>,
) -> WorkbenchResult {
    let scope = match open_scope(inner) {
        Ok(s) => s,
        Err(r) => return *r,
    };
    if let Err(r) = load_history(inner, root, &scope).await {
        return refusal(r);
    }
    let Ok(state) = lock(inner) else {
        return refused(WorkbenchProblem::Unavailable);
    };
    let runs = state
        .workbench
        .sending
        .histories
        .get(&scope)
        .cloned()
        .unwrap_or_default();
    let start = match before {
        Some(id) => match runs.iter().position(|r| r.id == id) {
            Some(i) => i + 1,
            None => return refused(WorkbenchProblem::Missing),
        },
        None => 0,
    };
    let page: Vec<RunRecord> = runs
        .iter()
        .skip(start)
        .take(history::PAGE)
        .cloned()
        .collect();
    WorkbenchResult::History {
        more: runs.len() > start + page.len(),
        runs: page,
    }
}

fn find_run(inner: &Arc<Inner>, scope: &str, run: &str) -> Option<RunRecord> {
    lock(inner).ok().and_then(|s| {
        s.workbench
            .sending
            .histories
            .get(scope)
            .and_then(|runs| runs.iter().find(|r| r.id == run).cloned())
    })
}

/// One recorded run with its stored body's preview (nothing is sent).
pub(super) async fn show(inner: &Arc<Inner>, root: &Path, run: String) -> WorkbenchResult {
    let scope = match open_scope(inner) {
        Ok(s) => s,
        Err(r) => return *r,
    };
    if let Err(r) = load_history(inner, root, &scope).await {
        return refusal(r);
    }
    let Some(record) = find_run(inner, &scope, &run) else {
        return refused(WorkbenchProblem::Missing);
    };
    let preview = if record.stored {
        let keychain = Arc::clone(&inner.adapters.keychain);
        let private = root.to_path_buf();
        blocking(Vec::new(), move || {
            history::preview(
                &private,
                keychain.as_ref(),
                &scope,
                &run,
                history::PREVIEW_BYTES,
            )
            .unwrap_or_default()
        })
        .await
    } else {
        Vec::new()
    };
    WorkbenchResult::Ran {
        run: run_view(record, &preview, false),
    }
}

/// Saves a stored response body where the operator chooses.
pub(super) async fn save_response(inner: &Arc<Inner>, root: &Path, run: String) -> WorkbenchResult {
    let scope = match open_scope(inner) {
        Ok(s) => s,
        Err(r) => return *r,
    };
    if let Err(r) = load_history(inner, root, &scope).await {
        return refusal(r);
    }
    let Some(record) = find_run(inner, &scope, &run) else {
        return refused(WorkbenchProblem::Missing);
    };
    if !record.stored {
        return WorkbenchResult::SaveResponseFailed {
            failure: ExportFailure::Body,
        };
    }
    let name = record
        .relative
        .rsplit('/')
        .next()
        .unwrap_or("response")
        .trim_end_matches(".request.toml");
    let json = record.headers.iter().any(|(n, v)| {
        n.eq_ignore_ascii_case("content-type") && v.to_ascii_lowercase().contains("json")
    });
    let suggested = format!("{name}-response.{}", if json { "json" } else { "txt" });
    let Some(destination) = inner.adapters.folders.choose_save(suggested).await else {
        return WorkbenchResult::Cancelled;
    };
    let name = destination
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let written = blocking(Err(ExportFailure::Io), move || {
        history::export(&private, keychain.as_ref(), &scope, &run, &destination)
    })
    .await;
    match written {
        Ok(bytes) => WorkbenchResult::ResponseSaved { bytes, name },
        Err(failure) => WorkbenchResult::SaveResponseFailed { failure },
    }
}

/// Clears the open collection's history (not while one of its requests
/// runs) and, separately, its cookies.
pub(super) async fn clear(inner: &Arc<Inner>, root: &Path, cookies: bool) -> WorkbenchResult {
    let scope = match open_scope(inner) {
        Ok(s) => s,
        Err(r) => return *r,
    };
    if cookies {
        if let Some(jar) = jar(inner, root, &scope).await
            && let Ok(mut j) = jar.lock()
        {
            j.clear();
        }
        let keychain = Arc::clone(&inner.adapters.keychain);
        let private = root.to_path_buf();
        let stored = blocking(false, move || {
            profile::store_cookies(&private, keychain.as_ref(), &scope, &[]).is_ok()
        })
        .await;
        return if stored {
            WorkbenchResult::Done
        } else {
            refusal(SendRefusal::Private)
        };
    }
    if let Err(r) = load_history(inner, root, &scope).await {
        return refusal(r);
    }
    let removed = {
        let Ok(mut state) = lock(inner) else {
            return refused(WorkbenchProblem::Unavailable);
        };
        if !state.workbench.sending.runs.is_empty() {
            return refusal(SendRefusal::Busy);
        }
        let runs = history_of(&mut state.workbench.sending, &scope, Vec::new);
        std::mem::take(runs)
    };
    for r in &removed {
        history::remove_body(root, &scope, &r.id);
    }
    match store_history(inner, root, &scope).await {
        Ok(()) => WorkbenchResult::Done,
        Err(r) => refusal(r),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_text_unless_binary() {
        assert_eq!(preview_text(b"plain").as_deref(), Some("plain"));
        // A character cut at the preview's end.
        assert_eq!(preview_text(&"é".as_bytes()[..1]).as_deref(), Some(""));
        assert_eq!(preview_text(&[0xff, 0x00, 0x41]), None);
    }

    #[test]
    fn display_urls_mask_secrets() {
        let plan = Plan {
            method: "GET".into(),
            url: render::Template(vec![
                render::Piece::Text("http://127.0.0.1/a?k=".into()),
                render::Piece::Secret("dev".into()),
            ]),
            query: vec![(
                render::Template::text("q"),
                render::Template(vec![render::Piece::Secret("dev".into())]),
            )],
            headers: Vec::new(),
            auth: render::Auth::None,
            body: Body::None,
            used: BTreeMap::new(),
        };
        assert_eq!(
            display_url(&plan),
            format!(
                "http://127.0.0.1/a?k={0}&q={1}",
                environments::MASK,
                url::form_urlencoded::byte_serialize(environments::MASK.as_bytes())
                    .collect::<String>()
            )
        );
    }
}
