//! API Client documents inside the authority (C14). Every request runs in
//! admission order on the workbench lane (reset takes it too), reads what it
//! needs from the state, does its file work off the lock and settles under
//! it; its result is observed once. The presentation owns the editor text:
//! Save, Protect and Preview carry a fresh full snapshot with the revision
//! it was edited from, and Rust decides whether that snapshot can be written
//! faithfully, whether the files still hold that revision and what the new
//! revision is. Nothing here sends a request or reads a tenant credential.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use ts_rs::TS;

mod send;

pub use send::{RunView, RunningView, SendReviewView};

use super::{Inner, Refusal, State, lock};
use crate::context::{ContextRef, EnteredSecret, ReviewId};
use crate::runtime::OperationId;
use crate::vault::VaultError;
use crate::workbench::admission::{Network, SendRefusal};
use crate::workbench::adoption::{self, AdoptionProblem, CandidateView};
use crate::workbench::curl::{self, CurlProblem, ImportNote};
use crate::workbench::document::{self, Document, Fields, SaveRefusal};
use crate::workbench::environments::{self, Origin, Preview};
use crate::workbench::git::{self, GitView};
use crate::workbench::history::{ExportFailure, RunRecord};
use crate::workbench::insomnia::{self, ExportProblem, ResourceView};
use crate::workbench::profile::{self, Draft, PrivateFailure, Profile, Recent, Scope};
use crate::workbench::project::{self, Catalog, Issue, ProjectProblem, RequestEntry};
use crate::workbench::schema::{self, SchemaProblem, SchemaText};
use crate::workbench::scm;

/// Settled results kept for observation.
const RETAINED_RESULTS: usize = 64;

/// One API Client request. Each yields exactly one [`WorkbenchResult`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WorkbenchRequest {
    /// Read this Mac's private API Client profile (recent collections).
    Load,
    /// Open a collection folder chosen in the native folder sheet.
    OpenFolder,
    /// Create a collection folder named in the native save sheet (inside a
    /// Git repository, as v1) and open it.
    CreateCollection,
    OpenRecent {
        index: u8,
    },
    /// Write (or refresh) and open the built-in Strata Cloud Manager
    /// collection.
    OpenBuiltIn,
    /// Refused with `Unprotected` while an editor has unprotected edits,
    /// unless the operator chose to discard them.
    CloseCollection {
        discard: bool,
    },
    /// Read the folder again and recheck open documents against it.
    Refresh,
    OpenRequest {
        relative: String,
    },
    /// A new unsaved request at `relative` (`.request.toml` added if
    /// missing).
    NewRequest {
        relative: String,
    },
    /// A new unsaved copy of a document's current snapshot.
    Duplicate {
        id: String,
        relative: String,
        fields: Fields,
    },
    NewFolder {
        relative: String,
    },
    /// The editor has edits not yet protected (the close guard asks).
    /// `edit` is the editor's edit generation; a later Protect or Save
    /// clears the mark only when its snapshot covers that generation.
    Editing {
        id: String,
        edit: u32,
    },
    /// Keep the editor's snapshot (edit generation `edit`) as this Mac's
    /// protected draft.
    Protect {
        id: String,
        revision: String,
        fields: Fields,
        edit: u32,
    },
    /// Write the editor's fresh snapshot (edit generation `edit`) to the
    /// collection.
    Save {
        id: String,
        revision: String,
        fields: Fields,
        edit: u32,
    },
    /// Settle a conflict or interrupted save (both versions are archived
    /// privately first).
    Resolve {
        id: String,
        revision: String,
        fields: Fields,
        choice: Resolution,
    },
    SelectTab {
        id: String,
    },
    /// Refused with `Unprotected` while the document has unprotected
    /// edits, unless the operator chose to discard them.
    CloseTab {
        id: String,
        discard: bool,
    },
    /// Drop a document's protected draft (a new request is closed).
    DiscardDraft {
        id: String,
    },
    /// Send the editor's fresh snapshot (C15). `context` is the active
    /// tenant context, needed only when the request uses tenant
    /// credentials; Rust decides that from the request itself.
    Send {
        id: String,
        revision: String,
        fields: Fields,
        context: Option<ContextRef>,
    },
    /// Send a reviewed tenant-credential change: the same snapshot must
    /// reproduce the review, and the operator confirms natively.
    Apply {
        review: ReviewId,
        id: String,
        revision: String,
        fields: Fields,
        context: Option<ContextRef>,
    },
    DismissReview,
    Cancel {
        run: String,
    },
    /// A page of the open collection's history (newest first).
    History {
        before: Option<String>,
    },
    /// A recorded run and its stored body's preview (nothing is sent).
    ShowRun {
        run: String,
    },
    /// Save a stored response body through the native save sheet.
    SaveResponse {
        run: String,
    },
    ClearHistory,
    ClearCookies,
    /// The request as it would be prepared, every secret masked.
    Preview {
        id: String,
        fields: Fields,
    },
    SelectEnvironment {
        name: Option<String>,
    },
    OpenEnvironment {
        name: String,
        private: bool,
    },
    SaveEnvironment {
        name: String,
        private: bool,
        revision: Option<String>,
        text: String,
    },
    DeleteEnvironment {
        name: String,
    },
    /// Store a workbench secret's value in the Keychain (never a tenant
    /// reference; the value is never shown again).
    SetSecret {
        reference: String,
        value: EnteredSecret,
    },
    RemoveSecret {
        reference: String,
    },
    OpenSchema {
        relative: String,
    },
    SaveSchema {
        relative: String,
        revision: Option<String>,
        text: String,
    },
    /// Associate a schema with a document (or clear it); unsaved until the
    /// document is saved.
    AssociateSchema {
        id: String,
        revision: String,
        fields: Fields,
        schema: Option<String>,
    },
    /// Convert one pasted cURL command into a new unsaved request.
    ImportCurl {
        text: String,
        relative: String,
    },
    /// Choose an Insomnia export in the native file sheet and preview it.
    ChooseInsomnia,
    /// Create the selected resources of the previewed export.
    CreateImported {
        selected: Vec<u32>,
    },
    DismissImport,
    ReadGit,
    /// Read the v1 API Client profile for adoption (read-only).
    ReadAdoption,
    Adopt {
        selected: Vec<u32>,
    },
    DismissAdoption,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    /// Discard local edits for the files on disk.
    UseDisk,
    /// Rebase local edits onto the files on disk (Save then writes them).
    KeepMine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    Changed,
    Deleted,
}

/// Why a request was refused (sanitized).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", content = "detail", rename_all = "camelCase")]
pub enum WorkbenchProblem {
    /// No collection is open.
    NoCollection,
    /// The document is not open (or was closed).
    Missing,
    /// The snapshot names a revision that is no longer the document's.
    Stale,
    /// The private API Client folder is unavailable in this build.
    Unavailable,
    /// Invalid input (name, reference, selection).
    Invalid,
    /// `tenant.*` references belong to native tenant authority.
    Managed,
    /// A limit was reached (drafts, private environments).
    Limit,
    Project(ProjectProblem),
    Private(PrivateFailure),
    Schema(SchemaProblem),
    Curl(CurlProblem),
    Export(ExportProblem),
    Adoption(AdoptionProblem),
    /// The Keychain refused the secret.
    Keychain,
    /// Closing would drop edits not yet protected on this Mac.
    Unprotected,
}

/// A document as the editor needs it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DocumentView {
    pub id: String,
    pub relative: String,
    pub revision: String,
    /// What the editor shows: the protected draft, else the saved fields.
    pub fields: Fields,
    pub saved: Fields,
    pub new: bool,
    /// The draft was restored from an earlier session.
    pub recovered: bool,
    pub body_editable: bool,
    pub schema: Option<String>,
    pub conflict: Option<ConflictKind>,
    pub interrupted: bool,
    /// Unsaved metadata (an association) beyond the fields.
    pub metadata_dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WorkbenchResult {
    Done,
    Cancelled,
    Refused {
        problem: WorkbenchProblem,
    },
    Opened {
        name: String,
    },
    Document {
        document: DocumentView,
        notes: Vec<ImportNote>,
    },
    Saved {
        document: DocumentView,
    },
    /// The snapshot already equals the files.
    Unchanged {
        document: DocumentView,
    },
    SaveRefused {
        refusal: SaveRefusal,
    },
    Protected {
        id: String,
    },
    Closed {
        id: String,
    },
    Preview {
        preview: Preview,
    },
    Environment {
        name: String,
        private: bool,
        text: String,
        revision: Option<String>,
    },
    Schema {
        schema: SchemaText,
    },
    Import {
        collection: String,
        resources: Vec<ResourceView>,
    },
    Created {
        requests: u32,
        environments: u32,
        skipped: u32,
    },
    Git {
        git: GitView,
    },
    Adoption {
        candidates: Vec<CandidateView>,
    },
    Adopted {
        collections: u32,
        environments: u32,
    },
    /// A run that ended (or one shown from history).
    Ran {
        run: RunView,
    },
    /// A known tenant-credential change: review it, then Apply.
    ReviewRequired {
        review: SendReviewView,
    },
    SendRefused {
        refusal: SendRefusal,
    },
    History {
        runs: Vec<RunRecord>,
        more: bool,
    },
    ResponseSaved {
        #[ts(type = "number")]
        bytes: u64,
        /// The saved file's name (not its path).
        name: String,
    },
    SaveResponseFailed {
        failure: ExportFailure,
    },
}

fn refused(problem: WorkbenchProblem) -> WorkbenchResult {
    WorkbenchResult::Refused { problem }
}

// ------------------------------------------------------------------ views

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecentView {
    pub index: u32,
    pub name: String,
    /// The folder's own name (not its path).
    pub folder: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TabView {
    pub id: String,
    pub relative: String,
    pub method: String,
    /// A protected draft differs from the files (or the request is new).
    pub draft: bool,
    pub new: bool,
    pub recovered: bool,
    pub conflict: Option<ConflictKind>,
    pub interrupted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentView {
    pub name: String,
    pub origin: Origin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SecretView {
    pub reference: String,
    /// A value is stored on this Mac.
    pub stored: bool,
    /// A `tenant.*` reference: native tenant authority resolves it (C15).
    pub managed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CollectionView {
    pub id: String,
    pub name: String,
    pub folder: String,
    /// Opaque identity of this root and collection id for presentation caches, not authority.
    pub scope: String,
    pub built_in: bool,
    pub complete: bool,
    pub requests: Vec<RequestEntry>,
    pub folders: Vec<String>,
    pub schemas: Vec<String>,
    pub issues: Vec<Issue>,
    pub environments: Vec<EnvironmentView>,
    pub environment: Option<String>,
    pub tabs: Vec<TabView>,
    pub selected: Option<String>,
    pub secrets: Vec<SecretView>,
    /// The last private write failed (drafts may not be protected).
    pub unsaved: Option<PrivateFailure>,
    pub archives: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkbenchView {
    pub available: bool,
    /// The private profile was read (or failed to open).
    pub loaded: bool,
    pub failure: Option<PrivateFailure>,
    pub recent: Vec<RecentView>,
    pub collection: Option<CollectionView>,
    /// A previewed Insomnia export.
    pub import: Option<Vec<ResourceView>>,
    pub adoption: Option<Vec<CandidateView>>,
    pub adoption_settled: bool,
    pub git: Option<GitView>,
    /// A request is running on the lane.
    pub busy: bool,
    /// Edits exist that are not protected on this Mac yet.
    pub unprotected: bool,
    /// Requests in flight (C15).
    pub running: Vec<RunningView>,
    /// A tenant-credential change awaiting review.
    pub review: Option<SendReviewView>,
    /// Where this build sends API Client requests.
    pub network: Network,
}

// ------------------------------------------------------------------ state

#[derive(Default)]
struct Cell {
    result: Mutex<Option<WorkbenchResult>>,
    ready: Notify,
}

/// A single-use wait for one API Client request's result.
pub struct WorkbenchHandle(Arc<Cell>);

impl std::fmt::Debug for WorkbenchHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WorkbenchHandle")
    }
}

impl WorkbenchHandle {
    pub async fn result(self) -> WorkbenchResult {
        loop {
            let notified = self.0.ready.notified();
            if let Some(result) = self.0.result.lock().ok().and_then(|r| r.clone()) {
                return result;
            }
            notified.await;
        }
    }
}

struct Call {
    operation: OperationId,
    cell: Arc<Cell>,
    settled: bool,
    observed: bool,
    /// Left the lane (a transfer or confirmation in progress).
    later: bool,
}

pub(super) struct Open {
    root: PathBuf,
    scope: String,
    built_in: bool,
    catalog: Catalog,
    data: Scope,
    unsaved: Option<PrivateFailure>,
    documents: BTreeMap<String, Document>,
    recovered: BTreeSet<String>,
    conflicts: BTreeMap<String, ConflictKind>,
    interrupted: BTreeSet<String>,
}

#[derive(Default)]
pub(super) struct Workbench {
    pub(super) lane: Arc<tokio::sync::Mutex<()>>,
    calls: Vec<Call>,
    running: usize,
    profile: Option<Profile>,
    failure: Option<PrivateFailure>,
    open: Option<Open>,
    /// Documents with unprotected edits: the newest edit generation the
    /// editor reported for each.
    editing: BTreeMap<String, u32>,
    import: Option<insomnia::Export>,
    adoption: Option<Vec<adoption::Candidate>>,
    git: Option<GitView>,
    /// Fixture builds: fail a Save after this many file writes.
    pub(super) interrupt: Option<Interrupt>,
    pub(in crate::authority) sending: send::Sending,
}

impl Workbench {
    /// Forgets everything held (reset).
    pub(super) fn clear(&mut self) {
        self.profile = None;
        self.failure = None;
        self.open = None;
        self.editing.clear();
        self.import = None;
        self.adoption = None;
        self.git = None;
        self.sending.clear();
    }
}

fn folder_name(root: &std::path::Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Open {
    fn entry(&self, id: &str) -> Option<&RequestEntry> {
        self.catalog.requests.iter().find(|r| r.id == id)
    }

    fn view(&self, id: &str) -> Option<DocumentView> {
        let doc = self.documents.get(id)?;
        let draft = self.data.drafts.get(id);
        Some(DocumentView {
            id: doc.id.clone(),
            relative: doc.relative.clone(),
            revision: doc.revision(),
            fields: draft.map_or_else(|| doc.saved.clone(), |d| d.fields.clone()),
            saved: doc.saved.clone(),
            new: doc.new,
            recovered: self.recovered.contains(id),
            body_editable: doc.body_editable,
            schema: doc.schema(),
            conflict: self.conflicts.get(id).copied(),
            interrupted: self.interrupted.contains(id),
            metadata_dirty: doc.metadata_dirty,
        })
    }

    fn open_tab(&mut self, id: &str) {
        if !self.data.open.iter().any(|t| t == id) {
            self.data.open.push(id.to_owned());
        }
        self.data.selected = Some(id.to_owned());
    }

    fn environments(&self) -> Vec<EnvironmentView> {
        let mut names: BTreeMap<String, Origin> = BTreeMap::new();
        for name in &self.catalog.environments {
            names.insert(name.clone(), Origin::Shared);
        }
        for name in self
            .data
            .private
            .keys()
            .filter(|n| *n != environments::DEFAULTS)
        {
            names
                .entry(name.clone())
                .and_modify(|o| *o = Origin::Both)
                .or_insert(Origin::Local);
        }
        names
            .into_iter()
            .map(|(name, origin)| EnvironmentView { name, origin })
            .collect()
    }
}

impl State {
    pub(super) fn admit_workbench(&mut self) -> Result<OperationId, Refusal> {
        if self.workbench.calls.iter().filter(|c| !c.settled).count() >= 256 {
            return Err(Refusal::Busy);
        }
        let operation = self.operations.reserve();
        self.workbench.calls.push(Call {
            operation: operation.clone(),
            cell: Arc::default(),
            settled: false,
            observed: false,
            later: false,
        });
        self.workbench.running += 1;
        Ok(operation)
    }

    pub(super) fn settle_workbench(&mut self, operation: &OperationId, result: WorkbenchResult) {
        self.workbench.running = self.workbench.running.saturating_sub(1);
        if let Some(call) = self
            .workbench
            .calls
            .iter_mut()
            .find(|c| &c.operation == operation)
        {
            call.settled = true;
            if let Ok(mut slot) = call.cell.result.lock() {
                *slot = Some(result);
            }
            call.cell.ready.notify_waiters();
        }
        self.prune_workbench_calls();
    }

    fn prune_workbench_calls(&mut self) {
        self.workbench.calls.retain(|c| !(c.settled && c.observed));
        let settled = self.workbench.calls.iter().filter(|c| c.settled).count();
        let mut excess = settled.saturating_sub(RETAINED_RESULTS);
        self.workbench.calls.retain(|c| {
            if excess > 0 && c.settled {
                excess -= 1;
                false
            } else {
                true
            }
        });
    }

    pub(super) fn observe_workbench(
        &mut self,
        operation: &OperationId,
    ) -> Result<WorkbenchHandle, Refusal> {
        let call = self
            .workbench
            .calls
            .iter_mut()
            .find(|c| &c.operation == operation && !c.observed)
            .ok_or(Refusal::InvalidInput)?;
        call.observed = true;
        let handle = WorkbenchHandle(Arc::clone(&call.cell));
        if call.settled {
            self.prune_workbench_calls();
        }
        Ok(handle)
    }

    /// Edits that are not protected on this Mac (the close guard asks).
    pub(super) fn workbench_unprotected(&self) -> bool {
        !self.workbench.editing.is_empty()
            || self
                .workbench
                .open
                .as_ref()
                .is_some_and(|o| o.unsaved.is_some() && !o.data.drafts.is_empty())
    }

    pub(super) fn workbench_view(&self) -> WorkbenchView {
        let w = &self.workbench;
        let collection = w.open.as_ref().map(|o| {
            let tabs = o
                .data
                .open
                .iter()
                .filter_map(|id| {
                    let doc = o.documents.get(id);
                    let (relative, method, new) = match (doc, o.entry(id)) {
                        (Some(d), _) => (
                            d.relative.clone(),
                            o.data.drafts.get(id).map_or_else(
                                || d.saved.method.clone(),
                                |dr| dr.fields.method.clone(),
                            ),
                            d.new,
                        ),
                        (None, Some(e)) => (e.relative.clone(), e.method.clone(), false),
                        (None, None) => return None,
                    };
                    Some(TabView {
                        id: id.clone(),
                        relative,
                        method,
                        draft: o.data.drafts.contains_key(id),
                        new,
                        recovered: o.recovered.contains(id),
                        conflict: o.conflicts.get(id).copied(),
                        interrupted: o.interrupted.contains(id),
                    })
                })
                .collect();
            let profile = w.profile.as_ref();
            let mut references: BTreeSet<String> = profile
                .map(|p| {
                    p.secrets
                        .iter()
                        .filter(|(c, _)| c == &o.catalog.id)
                        .map(|(_, r)| r.clone())
                        .collect()
                })
                .unwrap_or_default();
            let mut texts: Vec<&str> = o.data.private.values().map(String::as_str).collect();
            let settings: Vec<String> = o
                .documents
                .values()
                .map(|d| {
                    o.data
                        .drafts
                        .get(&d.id)
                        .map_or_else(|| d.saved.settings.clone(), |dr| dr.fields.settings.clone())
                })
                .collect();
            texts.extend(settings.iter().map(String::as_str));
            references.extend(environments::references(&texts));
            references.extend(o.catalog.references.iter().cloned());
            let secrets = references
                .into_iter()
                .map(|reference| SecretView {
                    stored: profile.is_some_and(|p| {
                        p.secrets
                            .iter()
                            .any(|(c, r)| c == &o.catalog.id && r == &reference)
                    }),
                    managed: reference.starts_with(scm::MANAGED_PREFIX),
                    reference,
                })
                .collect();
            CollectionView {
                id: o.catalog.id.clone(),
                name: o.catalog.name.clone(),
                folder: folder_name(&o.root),
                scope: profile::scope(&o.root, &o.catalog.id),
                built_in: o.built_in,
                complete: o.catalog.complete,
                requests: o.catalog.requests.clone(),
                folders: o.catalog.folders.clone(),
                schemas: o.catalog.schemas.clone(),
                issues: o.catalog.issues.iter().take(200).cloned().collect(),
                environments: o.environments(),
                environment: o.data.environment.clone(),
                tabs,
                selected: o.data.selected.clone(),
                secrets,
                unsaved: o.unsaved,
                archives: u32::try_from(o.data.archives.len()).unwrap_or(u32::MAX),
            }
        });
        WorkbenchView {
            available: self.workbench_root.is_some(),
            loaded: w.profile.is_some() || w.failure.is_some(),
            failure: w.failure,
            recent: w
                .profile
                .as_ref()
                .map(|p| {
                    // An import's collections show once it commits (D050).
                    p.recent
                        .iter()
                        .enumerate()
                        .filter(|(_, r)| !self.v1_hides_recent(&r.root))
                        .map(|(i, r)| RecentView {
                            index: u32::try_from(i).unwrap_or(u32::MAX),
                            name: r.name.clone(),
                            folder: folder_name(&r.root),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            collection,
            import: w.import.as_ref().map(insomnia::Export::view),
            adoption: w
                .adoption
                .as_ref()
                .map(|c| c.iter().enumerate().map(|(i, c)| c.view(i)).collect()),
            adoption_settled: w.profile.as_ref().is_some_and(|p| p.adoption_settled),
            git: w.git.clone(),
            busy: w.calls.iter().any(|c| !c.settled && !c.later),
            unprotected: self.workbench_unprotected(),
            running: w.sending.running(),
            review: w.sending.review(),
            network: self.workbench_network,
        }
    }
}

// ----------------------------------------------------------------- worker

async fn blocking<T: Send + 'static>(fallback: T, f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.unwrap_or(fallback)
}

/// Runs one admitted request in its turn on the lane and settles it.
pub(super) async fn run(inner: Arc<Inner>, operation: OperationId, request: WorkbenchRequest) {
    let Some(lane) = lock(&inner).ok().map(|s| Arc::clone(&s.workbench.lane)) else {
        return;
    };
    let step = {
        let _turn = lane.lock().await;
        match request {
            WorkbenchRequest::Send { .. } | WorkbenchRequest::Apply { .. } => {
                send::begin(&inner, request).await
            }
            other => send::Step::Done(Box::new(execute(&inner, other).await)),
        }
    };
    let result = match step {
        send::Step::Done(result) => *result,
        send::Step::Later(work) => {
            if let Ok(mut state) = lock(&inner) {
                if let Some(call) = state
                    .workbench
                    .calls
                    .iter_mut()
                    .find(|c| c.operation == operation)
                {
                    call.later = true;
                }
                state.changed();
            }
            work.await
        }
    };
    if let Ok(mut state) = lock(&inner) {
        state.settle_workbench(&operation, result);
        state.changed();
    }
}

async fn execute(inner: &Arc<Inner>, request: WorkbenchRequest) -> WorkbenchResult {
    let Some(root) = inner.adapters.workbench_root.clone() else {
        return refused(WorkbenchProblem::Unavailable);
    };
    let profile = match ensure_profile(inner, &root).await {
        Ok(p) => p,
        Err(problem) => return refused(problem),
    };
    match request {
        WorkbenchRequest::Load => WorkbenchResult::Done,
        WorkbenchRequest::OpenFolder => match inner.adapters.folders.choose_folder().await {
            Some(folder) => open_at(inner, &root, folder, false).await,
            None => WorkbenchResult::Cancelled,
        },
        WorkbenchRequest::CreateCollection => match inner.adapters.folders.choose_new().await {
            Some(folder) => {
                let target = folder.clone();
                let created = blocking(Err(ProjectProblem::Io), move || {
                    project::create_collection(&target)
                })
                .await;
                match created {
                    Ok(_) => open_at(inner, &root, folder, false).await,
                    Err(p) => refused(WorkbenchProblem::Project(p)),
                }
            }
            None => WorkbenchResult::Cancelled,
        },
        WorkbenchRequest::OpenRecent { index } => match profile.recent.get(usize::from(index)) {
            Some(recent) => open_at(inner, &root, recent.root.clone(), false).await,
            None => refused(WorkbenchProblem::Invalid),
        },
        WorkbenchRequest::OpenBuiltIn => open_built_in(inner, &root, profile).await,
        WorkbenchRequest::CloseCollection { discard } => {
            if let Ok(mut state) = lock(inner) {
                if !discard && !state.workbench.editing.is_empty() {
                    return refused(WorkbenchProblem::Unprotected);
                }
                state.workbench.open = None;
                state.workbench.sending.leave();
                state.workbench.editing.clear();
                state.workbench.import = None;
                state.workbench.git = None;
            }
            WorkbenchResult::Done
        }
        WorkbenchRequest::Refresh => refresh(inner).await,
        WorkbenchRequest::OpenRequest { relative } => open_request(inner, &root, relative).await,
        WorkbenchRequest::NewRequest { relative } => {
            new_request(inner, &root, relative, None, Vec::new()).await
        }
        WorkbenchRequest::Duplicate {
            id,
            relative,
            fields,
        } => {
            let known = lock(inner).ok().and_then(|s| {
                s.workbench
                    .open
                    .as_ref()
                    .map(|o| o.documents.contains_key(&id))
            });
            match known {
                Some(true) if document::check(&fields).is_ok() => {
                    new_request(inner, &root, relative, Some(fields), Vec::new()).await
                }
                Some(true) => refused(WorkbenchProblem::Invalid),
                Some(false) => refused(WorkbenchProblem::Missing),
                None => refused(WorkbenchProblem::NoCollection),
            }
        }
        WorkbenchRequest::NewFolder { relative } => new_folder(inner, relative).await,
        WorkbenchRequest::Editing { id, edit } => {
            if let Ok(mut state) = lock(inner)
                && state
                    .workbench
                    .open
                    .as_ref()
                    .is_some_and(|o| o.documents.contains_key(&id))
            {
                let mark = state.workbench.editing.entry(id).or_default();
                *mark = (*mark).max(edit);
                return WorkbenchResult::Done;
            }
            refused(WorkbenchProblem::Missing)
        }
        WorkbenchRequest::Protect {
            id,
            revision,
            fields,
            edit,
        } => protect(inner, &root, id, revision, fields, edit).await,
        WorkbenchRequest::Save {
            id,
            revision,
            fields,
            edit,
        } => save(inner, &root, id, revision, fields, edit).await,
        WorkbenchRequest::Resolve {
            id,
            revision,
            fields,
            choice,
        } => resolve(inner, &root, id, revision, fields, choice).await,
        WorkbenchRequest::SelectTab { id } => {
            let changed = with_open(inner, |o| {
                if !o.data.open.contains(&id) {
                    return Err(WorkbenchProblem::Missing);
                }
                o.data.selected = Some(id.clone());
                Ok(())
            });
            match changed {
                Ok(()) => persist_result(inner, &root, WorkbenchResult::Done).await,
                Err(p) => refused(p),
            }
        }
        WorkbenchRequest::CloseTab { id, discard } => {
            if !discard
                && lock(inner)
                    .ok()
                    .is_some_and(|s| s.workbench.editing.contains_key(&id))
            {
                return refused(WorkbenchProblem::Unprotected);
            }
            let changed = with_open(inner, |o| {
                let before = o.data.open.len();
                o.data.open.retain(|t| t != &id);
                if o.data.open.len() == before {
                    return Err(WorkbenchProblem::Missing);
                }
                if o.data.selected.as_deref() == Some(id.as_str()) {
                    o.data.selected = o.data.open.last().cloned();
                }
                // A closed tab keeps its protected draft (v1).
                if !o.data.drafts.contains_key(&id) {
                    o.documents.remove(&id);
                    o.recovered.remove(&id);
                }
                Ok(())
            });
            if changed.is_ok()
                && let Ok(mut state) = lock(inner)
            {
                state.workbench.editing.remove(&id);
            }
            match changed {
                Ok(()) => persist_result(inner, &root, WorkbenchResult::Closed { id }).await,
                Err(p) => refused(p),
            }
        }
        WorkbenchRequest::DiscardDraft { id } => discard(inner, &root, id).await,
        // Run off the lane by `run`.
        WorkbenchRequest::Send { .. } | WorkbenchRequest::Apply { .. } => {
            refused(WorkbenchProblem::Invalid)
        }
        WorkbenchRequest::DismissReview => send::dismiss_review(inner),
        WorkbenchRequest::Cancel { run } => send::cancel(inner, &run),
        WorkbenchRequest::History { before } => send::history_page(inner, &root, before).await,
        WorkbenchRequest::ShowRun { run } => send::show(inner, &root, run).await,
        WorkbenchRequest::SaveResponse { run } => send::save_response(inner, &root, run).await,
        WorkbenchRequest::ClearHistory => send::clear(inner, &root, false).await,
        WorkbenchRequest::ClearCookies => send::clear(inner, &root, true).await,
        WorkbenchRequest::Preview { id, fields } => preview(inner, &profile, id, fields).await,
        WorkbenchRequest::SelectEnvironment { name } => {
            let changed = with_open(inner, |o| {
                if let Some(n) = &name
                    && !o.environments().iter().any(|e| &e.name == n)
                {
                    return Err(WorkbenchProblem::Invalid);
                }
                o.data.environment = name.clone();
                Ok(())
            });
            match changed {
                Ok(()) => persist_result(inner, &root, WorkbenchResult::Done).await,
                Err(p) => refused(p),
            }
        }
        WorkbenchRequest::OpenEnvironment { name, private } => {
            open_environment(inner, name, private).await
        }
        WorkbenchRequest::SaveEnvironment {
            name,
            private,
            revision,
            text,
        } => save_environment(inner, &root, name, private, revision, text).await,
        WorkbenchRequest::DeleteEnvironment { name } => {
            let changed = with_open(inner, |o| {
                if o.data.private.remove(&name).is_none() {
                    return Err(WorkbenchProblem::Missing);
                }
                if o.data.environment.as_deref() == Some(name.as_str())
                    && !o.catalog.environments.contains(&name)
                {
                    o.data.environment = None;
                }
                Ok(())
            });
            match changed {
                Ok(()) => persist_result(inner, &root, WorkbenchResult::Done).await,
                Err(p) => refused(p),
            }
        }
        WorkbenchRequest::SetSecret { reference, value } => {
            set_secret(inner, &root, profile, reference, Some(value)).await
        }
        WorkbenchRequest::RemoveSecret { reference } => {
            set_secret(inner, &root, profile, reference, None).await
        }
        WorkbenchRequest::OpenSchema { relative } => {
            let Some(collection) = collection_root(inner) else {
                return refused(WorkbenchProblem::NoCollection);
            };
            match blocking(Err(SchemaProblem::Io), move || {
                schema::open(&collection, &relative)
            })
            .await
            {
                Ok(schema) => WorkbenchResult::Schema { schema },
                Err(p) => refused(WorkbenchProblem::Schema(p)),
            }
        }
        WorkbenchRequest::SaveSchema {
            relative,
            revision,
            text,
        } => {
            let Some(collection) = collection_root(inner) else {
                return refused(WorkbenchProblem::NoCollection);
            };
            let target = relative.clone();
            let saved = blocking(Err(SchemaProblem::Io), move || {
                schema::save(&collection, &target, revision.as_deref(), &text)
                    .map(|revision| (revision, text))
            })
            .await;
            match saved {
                Ok((revision, text)) => {
                    rediscover(inner).await;
                    WorkbenchResult::Schema {
                        schema: SchemaText {
                            relative,
                            text,
                            revision: Some(revision),
                        },
                    }
                }
                Err(p) => refused(WorkbenchProblem::Schema(p)),
            }
        }
        WorkbenchRequest::AssociateSchema {
            id,
            revision,
            fields,
            schema,
        } => associate(inner, &root, id, revision, fields, schema).await,
        WorkbenchRequest::ImportCurl { text, relative } => match curl::parse(&text) {
            Ok(conversion) => {
                new_request(
                    inner,
                    &root,
                    relative,
                    Some(conversion.fields),
                    conversion.notes,
                )
                .await
            }
            Err(p) => refused(WorkbenchProblem::Curl(p)),
        },
        WorkbenchRequest::ChooseInsomnia => choose_insomnia(inner).await,
        WorkbenchRequest::CreateImported { selected } => {
            create_imported(inner, &root, selected).await
        }
        WorkbenchRequest::DismissImport => {
            if let Ok(mut state) = lock(inner) {
                state.workbench.import = None;
            }
            WorkbenchResult::Done
        }
        WorkbenchRequest::ReadGit => {
            let Some(collection) = collection_root(inner) else {
                return refused(WorkbenchProblem::NoCollection);
            };
            let program = lock(inner).ok().and_then(|s| s.git.clone());
            let view = blocking(git::read(None, &collection), move || {
                git::read(program.as_deref(), &collection)
            })
            .await;
            if let Ok(mut state) = lock(inner) {
                state.workbench.git = Some(view.clone());
            }
            WorkbenchResult::Git { git: view }
        }
        WorkbenchRequest::ReadAdoption => {
            let Some(source) = lock(inner).ok().and_then(|s| s.adoption_source.clone()) else {
                return refused(WorkbenchProblem::Adoption(AdoptionProblem::Missing));
            };
            match blocking(Err(AdoptionProblem::Unreadable), move || {
                adoption::read(&source)
            })
            .await
            {
                Ok(candidates) => {
                    let views = candidates
                        .iter()
                        .enumerate()
                        .map(|(i, c)| c.view(i))
                        .collect();
                    if let Ok(mut state) = lock(inner) {
                        state.workbench.adoption = Some(candidates);
                    }
                    WorkbenchResult::Adoption { candidates: views }
                }
                Err(p) => refused(WorkbenchProblem::Adoption(p)),
            }
        }
        WorkbenchRequest::Adopt { selected } => adopt(inner, &root, profile, selected).await,
        WorkbenchRequest::DismissAdoption => {
            let mut next = profile;
            next.adoption_settled = true;
            if let Ok(mut state) = lock(inner) {
                state.workbench.adoption = None;
            }
            match store_profile(inner, &root, next).await {
                Ok(()) => WorkbenchResult::Done,
                Err(f) => refused(WorkbenchProblem::Private(f)),
            }
        }
    }
}

async fn ensure_profile(
    inner: &Arc<Inner>,
    root: &std::path::Path,
) -> Result<Profile, WorkbenchProblem> {
    if let Some(p) = lock(inner)
        .map_err(|_| WorkbenchProblem::Unavailable)?
        .workbench
        .profile
        .clone()
    {
        return Ok(p);
    }
    let keychain = Arc::clone(&inner.adapters.keychain);
    let at = root.to_path_buf();
    let loaded = blocking(Err(PrivateFailure::Unavailable), move || {
        profile::load_profile(&at, keychain.as_ref())
    })
    .await;
    let mut state = lock(inner).map_err(|_| WorkbenchProblem::Unavailable)?;
    match loaded {
        Ok(p) => {
            state.workbench.profile = Some(p.clone());
            state.workbench.failure = None;
            Ok(p)
        }
        Err(f) => {
            state.workbench.failure = Some(f);
            Err(WorkbenchProblem::Private(f))
        }
    }
}

async fn store_profile(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    next: Profile,
) -> Result<(), PrivateFailure> {
    let keychain = Arc::clone(&inner.adapters.keychain);
    let at = root.to_path_buf();
    let written = next.clone();
    let stored = blocking(Err(PrivateFailure::Unavailable), move || {
        profile::store_profile(&at, keychain.as_ref(), &written)
    })
    .await;
    if let Ok(mut state) = lock(inner) {
        match stored {
            Ok(()) => {
                state.workbench.profile = Some(next);
                state.workbench.failure = None;
            }
            Err(f) => state.workbench.failure = Some(f),
        }
    }
    stored
}

/// Writes the open collection's private scope as it now is.
async fn persist(inner: &Arc<Inner>, root: &std::path::Path) -> Result<(), PrivateFailure> {
    let Some((scope, data)) = lock(inner).ok().and_then(|s| {
        s.workbench
            .open
            .as_ref()
            .map(|o| (o.scope.clone(), o.data.clone()))
    }) else {
        return Ok(());
    };
    let keychain = Arc::clone(&inner.adapters.keychain);
    let at = root.to_path_buf();
    let written = scope.clone();
    let stored = blocking(Err(PrivateFailure::Unavailable), move || {
        profile::store_scope(&at, keychain.as_ref(), &written, &data)
    })
    .await;
    if let Ok(mut state) = lock(inner)
        && let Some(open) = state.workbench.open.as_mut().filter(|o| o.scope == scope)
    {
        open.unsaved = stored.err();
    }
    stored
}

async fn persist_result(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    result: WorkbenchResult,
) -> WorkbenchResult {
    match persist(inner, root).await {
        Ok(()) => result,
        Err(f) => refused(WorkbenchProblem::Private(f)),
    }
}

fn with_open<T>(
    inner: &Arc<Inner>,
    f: impl FnOnce(&mut Open) -> Result<T, WorkbenchProblem>,
) -> Result<T, WorkbenchProblem> {
    let mut state = lock(inner).map_err(|_| WorkbenchProblem::Unavailable)?;
    let open = state
        .workbench
        .open
        .as_mut()
        .ok_or(WorkbenchProblem::NoCollection)?;
    f(open)
}

fn collection_root(inner: &Arc<Inner>) -> Option<PathBuf> {
    lock(inner)
        .ok()
        .and_then(|s| s.workbench.open.as_ref().map(|o| o.catalog.root.clone()))
}

/// What opening a collection reads from disk.
struct Opening {
    catalog: Catalog,
    data: Scope,
    documents: BTreeMap<String, Document>,
    conflicts: BTreeMap<String, ConflictKind>,
    interrupted: BTreeSet<String>,
    scope: String,
}

fn conflict_of(root: &std::path::Path, doc: &Document) -> Option<ConflictKind> {
    match document::unchanged_on_disk(root, doc) {
        Ok(true) => None,
        _ if !doc.new
            && project::read_optional(
                root,
                &project::relative_path(&doc.relative, false)?,
                project::METADATA_BYTES,
            )
            .ok()
            .flatten()
            .is_none() =>
        {
            Some(ConflictKind::Deleted)
        }
        _ => Some(ConflictKind::Changed),
    }
}

fn read_opening(
    private: &std::path::Path,
    keychain: &dyn crate::vault::Vault,
    folder: &std::path::Path,
) -> Result<Opening, WorkbenchProblem> {
    let catalog = project::discover(folder).map_err(WorkbenchProblem::Project)?;
    let scope = profile::scope(&catalog.root, &catalog.id);
    let mut data =
        profile::load_scope(private, keychain, &scope).map_err(WorkbenchProblem::Private)?;
    let journal =
        profile::load_journal(private, keychain, &scope).map_err(WorkbenchProblem::Private)?;
    let mut interrupted = BTreeSet::new();
    let mut completed = BTreeSet::new();
    let mut pending = Vec::new();
    for entry in journal {
        match profile::journal_state(&catalog.root, &entry) {
            profile::JournalState::Interrupted => {
                interrupted.insert(entry.request.clone());
                pending.push(entry);
            }
            profile::JournalState::Completed => {
                completed.insert(entry.request.clone());
            }
            profile::JournalState::NotStarted => {}
        }
    }
    profile::store_journal(private, keychain, &scope, &pending)
        .map_err(WorkbenchProblem::Private)?;
    let mut documents = BTreeMap::new();
    let mut conflicts = BTreeMap::new();
    let drafts = std::mem::take(&mut data.drafts);
    for (id, mut draft) in drafts {
        // A save that completed before the journal was cleared: its files
        // are the new baseline; the draft's fields stay as edits.
        if completed.contains(&id)
            && let Some(entry) = catalog.requests.iter().find(|r| r.id == id)
            && let Ok(fresh) = document::open(&catalog.root, entry)
        {
            // Save protects its snapshot first: one fully written leaves a
            // draft equal to the files, which is no edit.
            if !fresh.dirty(&draft.fields) {
                continue;
            }
            draft.document = fresh;
        }
        if let Some(kind) = conflict_of(&catalog.root, &draft.document) {
            conflicts.insert(id.clone(), kind);
        }
        documents.insert(id.clone(), draft.document.clone());
        data.drafts.insert(id, draft);
    }
    // Tabs naming nothing that exists any more are dropped.
    data.open
        .retain(|id| documents.contains_key(id) || catalog.requests.iter().any(|r| &r.id == id));
    if data
        .selected
        .as_ref()
        .is_some_and(|s| !data.open.contains(s))
    {
        data.selected = data.open.last().cloned();
    }
    Ok(Opening {
        catalog,
        data,
        documents,
        conflicts,
        interrupted,
        scope,
    })
}

async fn open_at(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    folder: PathBuf,
    built_in: bool,
) -> WorkbenchResult {
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let opened = blocking(Err(WorkbenchProblem::Unavailable), move || {
        read_opening(&private, keychain.as_ref(), &folder)
    })
    .await;
    let opening = match opened {
        Ok(o) => o,
        Err(p) => return refused(p),
    };
    let name = opening.catalog.name.clone();
    let recent = Recent {
        root: opening.catalog.root.clone(),
        id: opening.catalog.id.clone(),
        name: name.clone(),
    };
    let profile = {
        let Ok(mut state) = lock(inner) else {
            return refused(WorkbenchProblem::Unavailable);
        };
        state.workbench.editing.clear();
        state.workbench.sending.leave();
        state.workbench.git = None;
        state.workbench.import = None;
        state.workbench.open = Some(Open {
            root: opening.catalog.root.clone(),
            recovered: opening.documents.keys().cloned().collect(),
            scope: opening.scope,
            built_in,
            catalog: opening.catalog,
            data: opening.data,
            unsaved: None,
            documents: opening.documents,
            conflicts: opening.conflicts,
            interrupted: opening.interrupted,
        });
        state.workbench.profile.clone()
    };
    if !built_in && let Some(mut next) = profile {
        next.remember(recent);
        // The recent list is a convenience: a failed write only loses it.
        let _ = store_profile(inner, root, next).await;
    }
    WorkbenchResult::Opened { name }
}

async fn open_built_in(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    profile: Profile,
) -> WorkbenchResult {
    let folder = scm::folder(root);
    let target = folder.clone();
    let previous = profile.scm.clone();
    let written = blocking(Err(ProjectProblem::Io), move || {
        let _placement = crate::sessions::placement();
        scm::refresh(&target, &previous)
    })
    .await;
    let (record, _) = match written {
        Ok(done) => done,
        Err(p) => return refused(WorkbenchProblem::Project(p)),
    };
    let mut next = profile;
    next.scm = record;
    if let Err(f) = store_profile(inner, root, next).await {
        return refused(WorkbenchProblem::Private(f));
    }
    open_at(inner, root, folder, true).await
}

/// Reads the collection's inventory again (after a write).
async fn rediscover(inner: &Arc<Inner>) {
    let Some(folder) = collection_root(inner) else {
        return;
    };
    let found = blocking(Err(ProjectProblem::Io), move || project::discover(&folder)).await;
    if let (Ok(catalog), Ok(mut state)) = (found, lock(inner))
        && let Some(open) = state.workbench.open.as_mut()
        && open.catalog.id == catalog.id
    {
        open.catalog = catalog;
    }
}

async fn refresh(inner: &Arc<Inner>) -> WorkbenchResult {
    let Some((folder, documents)) = lock(inner).ok().and_then(|s| {
        s.workbench
            .open
            .as_ref()
            .map(|o| (o.catalog.root.clone(), o.documents.clone()))
    }) else {
        return refused(WorkbenchProblem::NoCollection);
    };
    let checked = blocking(Err(ProjectProblem::Io), move || {
        let catalog = project::discover(&folder)?;
        let conflicts: BTreeMap<String, ConflictKind> = documents
            .iter()
            .filter_map(|(id, d)| conflict_of(&catalog.root, d).map(|k| (id.clone(), k)))
            .collect();
        Ok((catalog, conflicts))
    })
    .await;
    match checked {
        Ok((catalog, conflicts)) => {
            if let Ok(mut state) = lock(inner)
                && let Some(open) = state.workbench.open.as_mut()
            {
                if open.catalog.id != catalog.id {
                    return refused(WorkbenchProblem::Project(ProjectProblem::NotCollection));
                }
                open.catalog = catalog;
                open.conflicts = conflicts;
            }
            WorkbenchResult::Done
        }
        Err(p) => refused(WorkbenchProblem::Project(p)),
    }
}

async fn open_request(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    relative: String,
) -> WorkbenchResult {
    let found = lock(inner).ok().and_then(|s| {
        let open = s.workbench.open.as_ref()?;
        let held = open
            .documents
            .values()
            .find(|d| d.relative == relative)
            .map(|d| d.id.clone());
        Some((
            open.catalog.root.clone(),
            held,
            open.catalog
                .requests
                .iter()
                .find(|r| r.relative == relative)
                .cloned(),
        ))
    });
    let Some((folder, held, entry)) = found else {
        return refused(WorkbenchProblem::NoCollection);
    };
    let id = match (held, entry) {
        (Some(id), _) => id,
        (None, Some(entry)) => {
            let read = blocking(Err(ProjectProblem::Io), move || {
                document::open(&folder, &entry)
            })
            .await;
            match read {
                Ok(doc) => {
                    let id = doc.id.clone();
                    let stored = with_open(inner, |o| {
                        o.documents.insert(id.clone(), doc);
                        Ok(())
                    });
                    if let Err(p) = stored {
                        return refused(p);
                    }
                    id
                }
                Err(p) => return refused(WorkbenchProblem::Project(p)),
            }
        }
        (None, None) => return refused(WorkbenchProblem::Missing),
    };
    let view = with_open(inner, |o| {
        o.open_tab(&id);
        o.view(&id).ok_or(WorkbenchProblem::Missing)
    });
    match view {
        Ok(document) => {
            persist_result(
                inner,
                root,
                WorkbenchResult::Document {
                    document,
                    notes: Vec::new(),
                },
            )
            .await
        }
        Err(p) => refused(p),
    }
}

fn request_path(relative: &str) -> String {
    if relative.ends_with(project::REQUEST_SUFFIX) {
        relative.to_owned()
    } else {
        format!("{relative}{}", project::REQUEST_SUFFIX)
    }
}

async fn new_request(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    relative: String,
    fields: Option<Fields>,
    notes: Vec<ImportNote>,
) -> WorkbenchResult {
    let relative = request_path(relative.trim());
    let Some(folder) = collection_root(inner) else {
        return refused(WorkbenchProblem::NoCollection);
    };
    let doc = match document::create(&relative) {
        Ok(d) => d,
        Err(p) => return refused(WorkbenchProblem::Project(p)),
    };
    let check = doc.clone();
    let free = blocking(Ok(false), move || {
        document::unchanged_on_disk(&folder, &check)
    })
    .await;
    if free != Ok(true) {
        return refused(WorkbenchProblem::Project(ProjectProblem::Exists));
    }
    let view = with_open(inner, |o| {
        if !o.catalog.complete {
            return Err(WorkbenchProblem::Project(ProjectProblem::Incomplete));
        }
        if o.documents.values().any(|d| d.relative == relative) {
            return Err(WorkbenchProblem::Project(ProjectProblem::Exists));
        }
        if o.data.drafts.len() >= profile::MAX_DRAFTS {
            return Err(WorkbenchProblem::Limit);
        }
        let id = doc.id.clone();
        let fields = fields.unwrap_or_else(|| doc.saved.clone());
        o.data.drafts.insert(
            id.clone(),
            Draft {
                document: doc.clone(),
                fields,
            },
        );
        o.documents.insert(id.clone(), doc);
        o.open_tab(&id);
        o.view(&id).ok_or(WorkbenchProblem::Missing)
    });
    match view {
        Ok(document) => {
            persist_result(inner, root, WorkbenchResult::Document { document, notes }).await
        }
        Err(p) => refused(p),
    }
}

async fn new_folder(inner: &Arc<Inner>, relative: String) -> WorkbenchResult {
    let Some(catalog) = lock(inner)
        .ok()
        .and_then(|s| s.workbench.open.as_ref().map(|o| o.catalog.clone()))
    else {
        return refused(WorkbenchProblem::NoCollection);
    };
    match blocking(Err(ProjectProblem::Io), move || {
        project::create_folder(&catalog, &relative)
    })
    .await
    {
        Ok(()) => {
            rediscover(inner).await;
            WorkbenchResult::Done
        }
        Err(p) => refused(WorkbenchProblem::Project(p)),
    }
}

/// The document `id` at `revision` (refused when another revision is
/// held: the editor must reload).
fn current(open: &Open, id: &str, revision: &str) -> Result<Document, WorkbenchProblem> {
    let doc = open.documents.get(id).ok_or(WorkbenchProblem::Missing)?;
    if doc.revision() != revision {
        return Err(WorkbenchProblem::Stale);
    }
    Ok(doc.clone())
}

async fn protect(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    id: String,
    revision: String,
    fields: Fields,
    edit: u32,
) -> WorkbenchResult {
    let changed = with_open(inner, |o| {
        let doc = current(o, &id, &revision)?;
        if fields.body.len() > document::BODY_BYTES {
            return Err(WorkbenchProblem::Limit);
        }
        if doc.dirty(&fields) {
            if !o.data.drafts.contains_key(&id) && o.data.drafts.len() >= profile::MAX_DRAFTS {
                return Err(WorkbenchProblem::Limit);
            }
            o.data.drafts.insert(
                id.clone(),
                Draft {
                    document: doc,
                    fields,
                },
            );
        } else {
            o.data.drafts.remove(&id);
            o.recovered.remove(&id);
        }
        Ok(())
    });
    if let Err(p) = changed {
        return refused(p);
    }
    match persist(inner, root).await {
        Ok(()) => {
            settle_editing(inner, &id, edit);
            WorkbenchResult::Protected { id }
        }
        Err(f) => refused(WorkbenchProblem::Private(f)),
    }
}

/// A fixture interruption of the next Save after `files` project writes:
/// an I/O failure, or (`crash`) a stop that skips everything after it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Interrupt {
    pub(crate) files: usize,
    pub(crate) crash: bool,
}

/// Clears `id`'s unprotected mark when the snapshot of edit generation
/// `edit` covers the newest edit the editor reported.
fn settle_editing(inner: &Arc<Inner>, id: &str, edit: u32) {
    if let Ok(mut state) = lock(inner)
        && state.workbench.editing.get(id).is_some_and(|m| *m <= edit)
    {
        state.workbench.editing.remove(id);
    }
}

/// Writes a planned save: journal first, files in order, journal cleared.
fn commit(
    private: &std::path::Path,
    keychain: &dyn crate::vault::Vault,
    scope: &str,
    folder: &std::path::Path,
    doc: &Document,
    changes: &[document::Change],
    interrupt: Option<usize>,
) -> Result<(), SaveRefusal> {
    let mut journal =
        profile::load_journal(private, keychain, scope).map_err(|_| SaveRefusal::Io)?;
    if journal.iter().any(|j| j.request == doc.id) {
        return Err(SaveRefusal::Interrupted);
    }
    journal.push(profile::Journal {
        request: doc.id.clone(),
        relative: doc.relative.clone(),
        files: changes
            .iter()
            .map(|c| (c.relative.clone(), c.before.clone(), c.after.clone()))
            .collect(),
    });
    profile::store_journal(private, keychain, scope, &journal).map_err(|_| SaveRefusal::Io)?;
    for (written, change) in changes.iter().enumerate() {
        if interrupt == Some(written) {
            return Err(SaveRefusal::Io);
        }
        let rel = project::relative_path(&change.relative, false).ok_or(SaveRefusal::Io)?;
        project::replace(folder, &rel, &change.bytes).map_err(|_| SaveRefusal::Io)?;
    }
    journal.retain(|j| j.request != doc.id);
    // The files are written: a failure to clear the journal only leaves an
    // entry that the next opening finds Completed.
    let _ = profile::store_journal(private, keychain, scope, &journal);
    Ok(())
}

async fn save(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    id: String,
    revision: String,
    fields: Fields,
    edit: u32,
) -> WorkbenchResult {
    let prepared = with_open(inner, |o| {
        let doc = current(o, &id, &revision)?;
        if o.interrupted.contains(&id) {
            return Ok(Err(SaveRefusal::Interrupted));
        }
        // The submitted snapshot is protected before any project file is
        // written, so a stop between writes can always be recovered. A
        // snapshot equal to the saved one makes any older draft obsolete,
        // and it is dropped durably before the Save is acknowledged.
        if doc.dirty(&fields) {
            o.data.drafts.insert(
                id.clone(),
                Draft {
                    document: doc.clone(),
                    fields: fields.clone(),
                },
            );
        } else {
            o.data.drafts.remove(&id);
            o.recovered.remove(&id);
        }
        Ok(Ok((
            doc,
            o.catalog.root.clone(),
            o.catalog.id.clone(),
            o.scope.clone(),
        )))
    });
    let (doc, folder, collection, scope) = match prepared {
        Ok(Ok(p)) => p,
        Ok(Err(refusal)) => return WorkbenchResult::SaveRefused { refusal },
        Err(p) => return refused(p),
    };
    if let Err(f) = persist(inner, root).await {
        return refused(WorkbenchProblem::Private(f));
    }
    let interrupt = lock(inner)
        .ok()
        .and_then(|mut s| s.workbench.interrupt.take());
    let crash = interrupt.is_some_and(|i| i.crash);
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let snapshot = fields.clone();
    let planned = doc.clone();
    let written = blocking(Err(SaveRefusal::Io), move || {
        let catalog = project::discover(&folder).map_err(|_| SaveRefusal::Incomplete)?;
        let changes = document::plan(&folder, &catalog, &collection, &planned, &snapshot)?;
        if changes.is_empty() {
            return Ok(None);
        }
        commit(
            &private,
            keychain.as_ref(),
            &scope,
            &folder,
            &planned,
            &changes,
            interrupt.map(|i| i.files),
        )?;
        Ok(Some((
            document::committed(&planned, &snapshot, &changes),
            catalog,
        )))
    })
    .await;
    match written {
        Ok(Some((next, _))) => {
            let view = with_open(inner, |o| {
                o.documents.insert(id.clone(), next);
                o.data.drafts.remove(&id);
                o.recovered.remove(&id);
                o.conflicts.remove(&id);
                o.view(&id).ok_or(WorkbenchProblem::Missing)
            });
            rediscover(inner).await;
            settle_editing(inner, &id, edit);
            match view {
                Ok(document) => {
                    persist_result(inner, root, WorkbenchResult::Saved { document }).await
                }
                Err(p) => refused(p),
            }
        }
        Ok(None) => match with_open(inner, |o| o.view(&id).ok_or(WorkbenchProblem::Missing)) {
            Ok(document) => {
                settle_editing(inner, &id, edit);
                WorkbenchResult::Unchanged { document }
            }
            Err(p) => refused(p),
        },
        // Fixture builds: the process "stopped" mid-save; nothing after
        // the last file write runs.
        Err(refusal) if crash => WorkbenchResult::SaveRefused { refusal },
        Err(refusal) => {
            let _ = with_open(inner, |o| {
                match refusal {
                    SaveRefusal::Conflict => {
                        let kind =
                            conflict_of(&o.catalog.root, &doc).unwrap_or(ConflictKind::Changed);
                        o.conflicts.insert(id.clone(), kind);
                    }
                    SaveRefusal::Io => {
                        o.interrupted.insert(id.clone());
                    }
                    _ => {}
                }
                // The snapshot stays protected whatever happened.
                if doc.dirty(&fields) {
                    o.data.drafts.insert(
                        id.clone(),
                        Draft {
                            document: doc.clone(),
                            fields: fields.clone(),
                        },
                    );
                }
                Ok(())
            });
            // After an I/O failure, whether any file was written is
            // settled from the journal at the next opening; until then
            // Save is refused.
            if persist(inner, root).await.is_ok() {
                settle_editing(inner, &id, edit);
            }
            WorkbenchResult::SaveRefused { refusal }
        }
    }
}

async fn resolve(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    id: String,
    revision: String,
    fields: Fields,
    choice: Resolution,
) -> WorkbenchResult {
    let prepared = with_open(inner, |o| {
        let doc = current(o, &id, &revision)?;
        if !o.conflicts.contains_key(&id) && !o.interrupted.contains(&id) {
            return Err(WorkbenchProblem::Invalid);
        }
        Ok((doc, o.catalog.root.clone(), o.scope.clone()))
    });
    let (doc, folder, scope) = match prepared {
        Ok(p) => p,
        Err(p) => return refused(p),
    };
    let keychain = Arc::clone(&inner.adapters.keychain);
    let private = root.to_path_buf();
    let local = fields.clone();
    // Both versions are kept before either is replaced; the files on disk
    // become the new baseline (re-read), the journal entry is settled.
    let read = blocking(Err(WorkbenchProblem::Unavailable), move || {
        let disk_metadata = project::relative_path(&doc.relative, false)
            .and_then(|r| {
                project::read_optional(&folder, &r, profile::ARCHIVE_BYTES)
                    .ok()
                    .flatten()
            })
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let disk_body = doc
            .body_reference()
            .and_then(|r| project::relative_path(&r, false))
            .and_then(|r| {
                project::read_optional(&folder, &r, profile::ARCHIVE_BYTES)
                    .ok()
                    .flatten()
            })
            .map(|b| String::from_utf8_lossy(&b).into_owned());
        let archive = profile::Archive {
            relative: doc.relative.clone(),
            at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            local,
            disk_metadata,
            disk_body,
        };
        let mut journal = profile::load_journal(&private, keychain.as_ref(), &scope)
            .map_err(WorkbenchProblem::Private)?;
        journal.retain(|j| j.request != doc.id);
        profile::store_journal(&private, keychain.as_ref(), &scope, &journal)
            .map_err(WorkbenchProblem::Private)?;
        let catalog = project::discover(&folder).map_err(WorkbenchProblem::Project)?;
        let fresh = catalog
            .requests
            .iter()
            .find(|r| r.id == doc.id && r.relative == doc.relative)
            .map(|entry| document::open(&catalog.root, entry))
            .transpose()
            .map_err(WorkbenchProblem::Project)?;
        Ok((archive, fresh, catalog))
    })
    .await;
    let (archive, fresh, catalog) = match read {
        Ok(r) => r,
        Err(p) => return refused(p),
    };
    let result = with_open(inner, |o| {
        o.data.archive(archive);
        o.conflicts.remove(&id);
        o.interrupted.remove(&id);
        o.recovered.remove(&id);
        o.catalog = catalog;
        match (choice, fresh) {
            (Resolution::UseDisk, Some(fresh)) => {
                o.data.drafts.remove(&id);
                o.documents.insert(id.clone(), fresh);
            }
            (Resolution::UseDisk, None) => {
                o.data.drafts.remove(&id);
                o.documents.remove(&id);
                o.data.open.retain(|t| t != &id);
                if o.data.selected.as_deref() == Some(id.as_str()) {
                    o.data.selected = o.data.open.last().cloned();
                }
                return Ok(WorkbenchResult::Closed { id: id.clone() });
            }
            (Resolution::KeepMine, Some(fresh)) => {
                o.data.drafts.insert(
                    id.clone(),
                    Draft {
                        document: fresh.clone(),
                        fields: fields.clone(),
                    },
                );
                o.documents.insert(id.clone(), fresh);
            }
            (Resolution::KeepMine, None) => {
                // Deleted on disk: the request becomes new again.
                let mut renewed = o
                    .documents
                    .get(&id)
                    .cloned()
                    .ok_or(WorkbenchProblem::Missing)?;
                renewed.new = true;
                for digest in renewed.baseline.values_mut() {
                    *digest = None;
                }
                o.data.drafts.insert(
                    id.clone(),
                    Draft {
                        document: renewed.clone(),
                        fields: fields.clone(),
                    },
                );
                o.documents.insert(id.clone(), renewed);
            }
        }
        o.view(&id)
            .map(|document| WorkbenchResult::Document {
                document,
                notes: Vec::new(),
            })
            .ok_or(WorkbenchProblem::Missing)
    });
    match result {
        Ok(r) => persist_result(inner, root, r).await,
        Err(p) => refused(p),
    }
}

async fn discard(inner: &Arc<Inner>, root: &std::path::Path, id: String) -> WorkbenchResult {
    let state = with_open(inner, |o| {
        let doc = o
            .documents
            .get(&id)
            .cloned()
            .ok_or(WorkbenchProblem::Missing)?;
        o.data.drafts.remove(&id);
        o.recovered.remove(&id);
        Ok((
            doc,
            o.catalog.root.clone(),
            o.catalog.requests.iter().find(|r| r.id == id).cloned(),
        ))
    });
    let (doc, folder, entry) = match state {
        Ok(s) => s,
        Err(p) => return refused(p),
    };
    if let Ok(mut s) = lock(inner) {
        s.workbench.editing.remove(&id);
    }
    if doc.new || entry.is_none() {
        let closed = with_open(inner, |o| {
            o.documents.remove(&id);
            o.conflicts.remove(&id);
            o.data.open.retain(|t| t != &id);
            if o.data.selected.as_deref() == Some(id.as_str()) {
                o.data.selected = o.data.open.last().cloned();
            }
            Ok(())
        });
        if let Err(p) = closed {
            return refused(p);
        }
        return persist_result(inner, root, WorkbenchResult::Closed { id }).await;
    }
    let entry = entry.expect("checked");
    let fresh = blocking(Err(ProjectProblem::Io), move || {
        document::open(&folder, &entry)
    })
    .await;
    let view = with_open(inner, |o| {
        if let Ok(fresh) = fresh {
            o.documents.insert(id.clone(), fresh);
            o.conflicts.remove(&id);
        }
        o.view(&id).ok_or(WorkbenchProblem::Missing)
    });
    match view {
        Ok(document) => {
            persist_result(
                inner,
                root,
                WorkbenchResult::Document {
                    document,
                    notes: Vec::new(),
                },
            )
            .await
        }
        Err(p) => refused(p),
    }
}

async fn preview(
    inner: &Arc<Inner>,
    profile: &Profile,
    id: String,
    fields: Fields,
) -> WorkbenchResult {
    let found = with_open(inner, |o| {
        if !o.documents.contains_key(&id) {
            return Err(WorkbenchProblem::Missing);
        }
        let env = o.data.environment.clone();
        Ok((
            o.catalog.root.clone(),
            o.catalog.id.clone(),
            env.clone(),
            env.and_then(|e| o.data.private.get(&e).cloned()),
            o.data.private.get(environments::DEFAULTS).cloned(),
        ))
    });
    let (folder, collection, env, local, defaults) = match found {
        Ok(f) => f,
        Err(p) => return refused(p),
    };
    let stored: BTreeSet<String> = profile
        .secrets
        .iter()
        .filter(|(c, _)| c == &collection)
        .map(|(_, r)| r.clone())
        .collect();
    let preview = blocking(None, move || {
        let read = |relative: &str| {
            project::relative_path(relative, false)
                .and_then(|r| {
                    project::read_optional(&folder, &r, environments::ENVIRONMENT_BYTES)
                        .ok()
                        .flatten()
                })
                .and_then(|b| String::from_utf8(b).ok())
        };
        let layers = environments::Layers {
            collection: read("collection.toml"),
            local_defaults: defaults,
            shared: env
                .as_ref()
                .and_then(|e| read(&format!("environments/{e}.toml"))),
            local,
        };
        Some(environments::preview(&fields, &layers, &|r| {
            stored.contains(r) || r.starts_with(scm::MANAGED_PREFIX)
        }))
    })
    .await;
    match preview {
        Some(preview) => WorkbenchResult::Preview { preview },
        None => refused(WorkbenchProblem::Unavailable),
    }
}

async fn open_environment(inner: &Arc<Inner>, name: String, private: bool) -> WorkbenchResult {
    if !environments::valid_name(&name) && name != environments::DEFAULTS {
        return refused(WorkbenchProblem::Invalid);
    }
    let found = with_open(inner, |o| {
        Ok((o.catalog.root.clone(), o.data.private.get(&name).cloned()))
    });
    let (folder, local) = match found {
        Ok(f) => f,
        Err(p) => return refused(p),
    };
    if private {
        let (text, revision) = match local {
            Some(t) => {
                let r = Some(document::hash_bytes(t.as_bytes()));
                (t, r)
            }
            None => (environments::new_text(), None),
        };
        return WorkbenchResult::Environment {
            name,
            private,
            text,
            revision,
        };
    }
    let relative = format!("environments/{name}.toml");
    let read = blocking(Err(ProjectProblem::Io), move || {
        let rel = project::relative_path(&relative, false).ok_or(ProjectProblem::BadPath)?;
        project::read_optional(&folder, &rel, environments::ENVIRONMENT_BYTES)
    })
    .await;
    match read {
        Ok(Some(bytes)) => match String::from_utf8(bytes) {
            Ok(text) => {
                let revision = Some(document::hash_bytes(text.as_bytes()));
                WorkbenchResult::Environment {
                    name,
                    private,
                    text,
                    revision,
                }
            }
            Err(_) => refused(WorkbenchProblem::Project(ProjectProblem::Io)),
        },
        Ok(None) => WorkbenchResult::Environment {
            name,
            private,
            text: environments::new_text(),
            revision: None,
        },
        Err(p) => refused(WorkbenchProblem::Project(p)),
    }
}

async fn save_environment(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    name: String,
    private: bool,
    revision: Option<String>,
    text: String,
) -> WorkbenchResult {
    let reserved = name == environments::DEFAULTS;
    if (!environments::valid_name(&name) && !reserved)
        || (reserved && !private)
        || text.len() > environments::ENVIRONMENT_BYTES
        || environments::text_ok(&text).is_none()
    {
        return refused(WorkbenchProblem::Invalid);
    }
    if private {
        let changed = with_open(inner, |o| {
            let held = o
                .data
                .private
                .get(&name)
                .map(|t| document::hash_bytes(t.as_bytes()));
            if held != revision {
                return Err(WorkbenchProblem::Stale);
            }
            if held.is_none() && o.data.private.len() >= environments::MAX_PRIVATE {
                return Err(WorkbenchProblem::Limit);
            }
            o.data.private.insert(name.clone(), text.clone());
            Ok(())
        });
        if let Err(p) = changed {
            return refused(p);
        }
        let revision = Some(document::hash_bytes(text.as_bytes()));
        return persist_result(
            inner,
            root,
            WorkbenchResult::Environment {
                name,
                private,
                text,
                revision,
            },
        )
        .await;
    }
    let Some(folder) = collection_root(inner) else {
        return refused(WorkbenchProblem::NoCollection);
    };
    let relative = format!("environments/{name}.toml");
    let bytes = text.clone();
    let written = blocking(Err(WorkbenchProblem::Unavailable), move || {
        let current = document::hash_file(&folder, &relative).map_err(WorkbenchProblem::Project)?;
        if current != revision {
            return Err(WorkbenchProblem::Stale);
        }
        let rel = project::relative_path(&relative, false).ok_or(WorkbenchProblem::Invalid)?;
        project::replace(&folder, &rel, bytes.as_bytes()).map_err(WorkbenchProblem::Project)
    })
    .await;
    match written {
        Ok(()) => {
            rediscover(inner).await;
            let revision = Some(document::hash_bytes(text.as_bytes()));
            WorkbenchResult::Environment {
                name,
                private,
                text,
                revision,
            }
        }
        Err(p) => refused(p),
    }
}

async fn set_secret(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    profile: Profile,
    reference: String,
    value: Option<EnteredSecret>,
) -> WorkbenchResult {
    if !document::valid_reference(&reference) {
        return refused(WorkbenchProblem::Invalid);
    }
    if reference.starts_with(scm::MANAGED_PREFIX) {
        return refused(WorkbenchProblem::Managed);
    }
    let Some(collection) = lock(inner)
        .ok()
        .and_then(|s| s.workbench.open.as_ref().map(|o| o.catalog.id.clone()))
    else {
        return refused(WorkbenchProblem::NoCollection);
    };
    if let Some(v) = &value
        && (v.expose().is_empty() || v.expose().len() > crate::vault::MAX_SECRET_BYTES)
    {
        return refused(WorkbenchProblem::Invalid);
    }
    // Before the record changes: anything planned or reviewed with the old
    // value no longer goes.
    if let Ok(mut state) = lock(inner) {
        state
            .workbench
            .sending
            .secret_changed(&collection, &reference);
        state.changed();
    } else {
        return refused(WorkbenchProblem::Unavailable);
    }
    let record = profile::secret_record(&collection, &reference);
    let keychain = Arc::clone(&inner.adapters.keychain);
    let storing = value.is_some();
    let stored = blocking(Err(VaultError::Unavailable), move || {
        // Records are immutable: a new value replaces the old record.
        match keychain.delete(&record) {
            Ok(()) | Err(VaultError::NotFound) => {}
            Err(e) => return Err(e),
        }
        match value {
            Some(v) => keychain.put(&record, v.expose().as_bytes()),
            None => Ok(()),
        }
    })
    .await;
    if stored.is_err() {
        return refused(WorkbenchProblem::Keychain);
    }
    let mut next = profile;
    next.secrets
        .retain(|(c, r)| !(c == &collection && r == &reference));
    if storing {
        next.secrets.push((collection, reference));
    }
    match store_profile(inner, root, next).await {
        Ok(()) => WorkbenchResult::Done,
        Err(f) => refused(WorkbenchProblem::Private(f)),
    }
}

async fn associate(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    id: String,
    revision: String,
    fields: Fields,
    schema: Option<String>,
) -> WorkbenchResult {
    if let Some(s) = &schema {
        let ok = with_open(inner, |o| Ok(o.catalog.schemas.contains(s)));
        if ok != Ok(true) {
            return refused(WorkbenchProblem::Schema(SchemaProblem::Missing));
        }
    }
    let view = with_open(inner, |o| {
        let mut doc = current(o, &id, &revision)?;
        if !doc.associate_schema(schema.as_deref()) {
            return Err(WorkbenchProblem::Invalid);
        }
        o.documents.insert(id.clone(), doc.clone());
        o.data.drafts.insert(
            id.clone(),
            Draft {
                document: doc,
                fields,
            },
        );
        o.view(&id).ok_or(WorkbenchProblem::Missing)
    });
    match view {
        Ok(document) => {
            persist_result(
                inner,
                root,
                WorkbenchResult::Document {
                    document,
                    notes: Vec::new(),
                },
            )
            .await
        }
        Err(p) => refused(p),
    }
}

async fn choose_insomnia(inner: &Arc<Inner>) -> WorkbenchResult {
    if collection_root(inner).is_none() {
        return refused(WorkbenchProblem::NoCollection);
    }
    let Some(chosen) = inner.adapters.workbench_files.choose().await else {
        return WorkbenchResult::Cancelled;
    };
    let file = match chosen {
        Ok(file) => file,
        Err(crate::import::ImportError::TooLarge) => {
            return refused(WorkbenchProblem::Export(ExportProblem::TooLarge));
        }
        Err(_) => return refused(WorkbenchProblem::Export(ExportProblem::Unreadable)),
    };
    let bytes = file.bytes.to_vec();
    let parsed = blocking(Err(ExportProblem::Unreadable), move || {
        if bytes.len() > insomnia::EXPORT_BYTES {
            return Err(ExportProblem::TooLarge);
        }
        let text = String::from_utf8(bytes).map_err(|_| ExportProblem::Unreadable)?;
        insomnia::parse(&text)
    })
    .await;
    match parsed {
        Ok(export) => {
            let resources = export.view();
            let collection = export.collection.clone();
            if let Ok(mut state) = lock(inner) {
                state.workbench.import = Some(export);
            }
            WorkbenchResult::Import {
                collection,
                resources,
            }
        }
        Err(p) => refused(WorkbenchProblem::Export(p)),
    }
}

async fn create_imported(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    selected: Vec<u32>,
) -> WorkbenchResult {
    let Some(export) = lock(inner).ok().and_then(|s| s.workbench.import.clone()) else {
        return refused(WorkbenchProblem::Invalid);
    };
    let Some(folder) = collection_root(inner) else {
        return refused(WorkbenchProblem::NoCollection);
    };
    let wanted: BTreeSet<usize> = selected
        .iter()
        .filter_map(|i| usize::try_from(*i).ok())
        .collect();
    let count = export.requests.len();
    let (mut requests, mut environments_made, mut skipped) = (0u32, 0u32, 0u32);
    for (index, (_, relative, result)) in export.requests.iter().enumerate() {
        if !wanted.contains(&index) {
            continue;
        }
        match result {
            Ok(fields) => match new_request(
                inner,
                root,
                relative.clone(),
                Some(fields.clone()),
                Vec::new(),
            )
            .await
            {
                WorkbenchResult::Document { .. } => requests += 1,
                _ => skipped += 1,
            },
            Err(_) => skipped += 1,
        }
    }
    for (offset, (_, relative, result)) in export.environments.iter().enumerate() {
        if !wanted.contains(&(count + offset)) {
            continue;
        }
        let Ok(text) = result else {
            skipped += 1;
            continue;
        };
        let (target, bytes) = (folder.clone(), text.clone());
        let rel = relative.clone();
        let written = blocking(Err(ProjectProblem::Io), move || {
            let path = project::relative_path(&rel, true).ok_or(ProjectProblem::BadPath)?;
            if document::hash_file(&target, &rel)?.is_some() {
                return Err(ProjectProblem::Exists);
            }
            project::replace(&target, &path, bytes.as_bytes())
        })
        .await;
        match written {
            Ok(()) => environments_made += 1,
            Err(_) => skipped += 1,
        }
    }
    rediscover(inner).await;
    if let Ok(mut state) = lock(inner) {
        state.workbench.import = None;
    }
    WorkbenchResult::Created {
        requests,
        environments: environments_made,
        skipped,
    }
}

async fn adopt(
    inner: &Arc<Inner>,
    root: &std::path::Path,
    profile: Profile,
    selected: Vec<u32>,
) -> WorkbenchResult {
    let Some(candidates) = lock(inner).ok().and_then(|s| s.workbench.adoption.clone()) else {
        return refused(WorkbenchProblem::Invalid);
    };
    let chosen: Vec<adoption::Candidate> = candidates
        .into_iter()
        .enumerate()
        .filter(|(i, _)| selected.contains(&u32::try_from(*i).unwrap_or(u32::MAX)))
        .map(|(_, c)| c)
        .collect();
    let mut next = profile;
    let mut environments_made = 0u32;
    let open_scope = lock(inner)
        .ok()
        .and_then(|s| s.workbench.open.as_ref().map(|o| o.scope.clone()));
    for candidate in &chosen {
        if !next.recent.iter().any(|r| r.root == candidate.root)
            && next.recent.len() < profile::RECENT
        {
            next.recent.push(Recent {
                root: candidate.root.clone(),
                id: candidate.id.clone(),
                name: candidate.name.clone(),
            });
        }
        let scope = profile::scope(&candidate.root, &candidate.id);
        if open_scope.as_deref() == Some(scope.as_str()) {
            let added = with_open(inner, |o| {
                let mut added = 0;
                for (name, text) in &candidate.environments {
                    if !o.data.private.contains_key(name)
                        && o.data.private.len() < environments::MAX_PRIVATE
                    {
                        o.data.private.insert(name.clone(), text.clone());
                        added += 1;
                    }
                }
                Ok(added)
            })
            .unwrap_or(0);
            if persist(inner, root).await.is_err() {
                return refused(WorkbenchProblem::Private(PrivateFailure::Unavailable));
            }
            environments_made += added;
            continue;
        }
        let keychain = Arc::clone(&inner.adapters.keychain);
        let private = root.to_path_buf();
        let envs = candidate.environments.clone();
        let added = blocking(Err(PrivateFailure::Unavailable), move || {
            let mut data = profile::load_scope(&private, keychain.as_ref(), &scope)?;
            let mut added = 0u32;
            for (name, text) in envs {
                if !data.private.contains_key(&name)
                    && data.private.len() < environments::MAX_PRIVATE
                {
                    data.private.insert(name, text);
                    added += 1;
                }
            }
            if added > 0 {
                profile::store_scope(&private, keychain.as_ref(), &scope, &data)?;
            }
            Ok(added)
        })
        .await;
        match added {
            Ok(n) => environments_made += n,
            Err(f) => return refused(WorkbenchProblem::Private(f)),
        }
    }
    next.adoption_settled = true;
    if let Ok(mut state) = lock(inner) {
        state.workbench.adoption = None;
    }
    match store_profile(inner, root, next).await {
        Ok(()) => WorkbenchResult::Adopted {
            collections: u32::try_from(chosen.len()).unwrap_or(u32::MAX),
            environments: environments_made,
        },
        Err(f) => refused(WorkbenchProblem::Private(f)),
    }
}

// ------------------------------------------------- v1 import (C23, D050)

/// The API Client's recent collection roots, on its lane (the profile is
/// read if it was not yet).
pub(super) async fn v1_recent_roots(inner: &Arc<Inner>) -> Option<Vec<std::path::PathBuf>> {
    let (lane, root) = lock(inner)
        .ok()
        .and_then(|s| Some((Arc::clone(&s.workbench.lane), s.workbench_root.clone()?)))?;
    let _turn = lane.lock().await;
    let profile = ensure_profile(inner, &root).await.ok()?;
    Some(profile.recent.iter().map(|r| r.root.clone()).collect())
}

/// Adds v1 collections to the recent list as Adopt from v1 does (C14),
/// on the API Client's lane. `note` is told what is about to be added, on
/// the lane, before the profile is written: when it refuses (`Err(Some)`),
/// nothing is. `Err(None)`: the profile could not be read or written.
/// Private environments are adopted through the API Client itself.
pub(super) async fn add_v1_recents<E>(
    inner: &Arc<Inner>,
    candidates: Vec<adoption::Candidate>,
    note: impl FnOnce(&[(std::path::PathBuf, String)]) -> Result<(), E>,
) -> Result<Vec<(std::path::PathBuf, String)>, Option<E>> {
    let (lane, root) = lock(inner)
        .ok()
        .and_then(|s| Some((Arc::clone(&s.workbench.lane), s.workbench_root.clone()?)))
        .ok_or(None)?;
    let _turn = lane.lock().await;
    let mut next = ensure_profile(inner, &root).await.map_err(|_| None)?;
    let mut added = Vec::new();
    for c in candidates {
        if next.recent.iter().any(|r| r.root == c.root) || next.recent.len() >= profile::RECENT {
            continue;
        }
        added.push((c.root.clone(), c.id.clone()));
        next.recent.push(Recent {
            root: c.root,
            id: c.id,
            name: c.name,
        });
    }
    if !added.is_empty() {
        note(&added).map_err(Some)?;
        store_profile(inner, &root, next).await.map_err(|_| None)?;
    }
    Ok(added)
}

/// Removes an imported collection from the recent list while it is still
/// listed under the same identity (C23 rollback).
pub(super) async fn remove_v1_recent(
    inner: &Arc<Inner>,
    collection: &std::path::Path,
    id: &str,
) -> Result<bool, PrivateFailure> {
    let (lane, root) = lock(inner)
        .ok()
        .and_then(|s| Some((Arc::clone(&s.workbench.lane), s.workbench_root.clone()?)))
        .ok_or(PrivateFailure::Unavailable)?;
    let _turn = lane.lock().await;
    let mut next = ensure_profile(inner, &root)
        .await
        .map_err(|_| PrivateFailure::Unavailable)?;
    let before = next.recent.len();
    next.recent
        .retain(|r| !(r.root == collection && r.id == id));
    if next.recent.len() == before {
        return Ok(false);
    }
    store_profile(inner, &root, next).await?;
    Ok(true)
}
