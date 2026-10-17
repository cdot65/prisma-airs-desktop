//! Native authority: principal-bound handles, finite intents and observation.
//!
//! One lock serializes authoritative transitions. Admission decides and
//! records under it, then releases it before any adapter work: vault writes,
//! Keychain cleanup, the native import picker, reset and remote operations
//! run as effects on the application runtime and report back as private
//! completions, each checked against the lease it was started under. A late
//! completion therefore cannot overwrite newer state, reactivate a retired
//! credential or attach a stored secret to another context.

use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::Instant;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::context::{ContextRef, EnteredSecret, ReviewId};
use crate::delivery::{Admit, Delivery, DeliverySession, Sessions};
use crate::features::{self, FeatureEntry};
use crate::import::{self, Discarded, ImportCandidate, ImportError, ImportFormat, ImportPreview};
use crate::observe::{Subscribers, Subscription, Update};
use crate::runtime::{
    self, OperationId, OperationLimits, OperationState, Operations, Outcome, StopReason,
};
use crate::store::{AppStore, PersistedState, StoreError, StoredCredential};
use crate::tenants::{Destination, ProfileDraft, TenantId, TenantProfile};
use crate::transport::{PolicyError, TransportFactory, TransportPolicy};
use crate::vault::{MAX_SECRET_BYTES, RecordId, SessionVault, StorageMode, Vault, VaultError};
use crate::wire::Theme;

/// Most tenant profiles kept.
pub const MAX_TENANTS: usize = 256;

/// The native-established identity on whose behalf work is requested.
///
/// It is chosen by the native host when it creates a handle, never by a
/// request payload. Restricted Harness principals arrive with the Harness
/// (C19) and receive no intents they cannot hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    Operator,
}

/// A typed request for one desktop action under the caller's authority.
///
/// The union is finite and grows only with shipped, explicitly typed
/// instructions; it never carries method names or arbitrary SDK payloads.
/// Tenant-scoped instructions carry the ContextRef the caller was viewing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Intent {
    /// Set the desktop-wide appearance preference (not an Academy setting).
    SetAppearance { theme: Theme },
    /// Create a tenant profile with no credential.
    CreateTenant { profile: ProfileDraft },
    /// Make a tenant active; issues a new context reference.
    SelectTenant { tenant: TenantId },
    /// Remove a tenant profile and clean up its credential.
    DeleteTenant { tenant: TenantId },
    /// Store a new scan key for the active tenant, replacing any previous one.
    ConfigureScanCredential {
        context: ContextRef,
        mode: StorageMode,
        key: EnteredSecret,
    },
    /// Sign the active tenant's scan capability out.
    RemoveScanCredential { context: ContextRef },
    /// Prepare a reviewed change of the active tenant's scan destination.
    ReviewEndpointChange {
        context: ContextRef,
        destination: Destination,
    },
    /// Apply a reviewed destination change (the scan key must be re-entered).
    ConfirmEndpointChange {
        context: ContextRef,
        review: ReviewId,
    },
    /// Discard a pending review.
    DiscardReview { review: ReviewId },
    /// Open the native file sheet to choose a profile file to preview.
    ChooseProfileImport,
    /// Create tenants from selected preview entries (no credentials).
    ConfirmProfileImport {
        review: ReviewId,
        selected: Vec<u32>,
    },
    /// Remove all v2 local data and v2 credentials.
    ResetLocalData,
}

/// Native acknowledgement that an Intent was admitted. It is not evidence of
/// operator confirmation or remote completion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Receipt {
    Admitted { request: String },
}

/// Native rejection of an Intent before admission. Sanitized: it names the
/// class of refusal only, never secret or remote content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    #[error("the presented context is no longer current")]
    StaleContext,
    #[error("the caller lacks the required capability")]
    CapabilityMissing,
    #[error("the request is invalid")]
    InvalidInput,
    #[error("the review is no longer valid")]
    ReviewInvalid,
    #[error("the review was already consumed")]
    ReviewConsumed,
    #[error("the desktop is busy")]
    Busy,
    #[error("protected storage is locked")]
    LockedStore,
    #[error("protected storage access was denied")]
    DeniedStore,
    #[error("this delivery was already processed")]
    AlreadyProcessed,
    #[error("a delivery sequence is missing")]
    SequenceGap,
    #[error("the desktop is shutting down")]
    ShuttingDown,
}

/// The product context a caller asks to observe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewScope {
    Shell,
}

/// A scoped, redacted view of desktop state plus its ordered updates.
#[derive(Debug)]
pub enum Observation {
    Shell {
        snapshot: ShellSnapshot,
        updates: Subscription,
    },
}

/// Health of Rust-owned local storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StorageHealth {
    Ready,
    /// The saved document is unreadable; only reset can continue.
    Corrupt,
    /// The data directory cannot be used (for example another instance).
    Unavailable,
    /// A reset is in progress.
    Resetting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScanState {
    Missing,
    Storing,
    Ready,
    Failed,
}

/// Redacted scan-capability state. Never contains key material.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanView {
    pub state: ScanState,
    pub mode: Option<StorageMode>,
    pub failure: Option<VaultError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TenantView {
    pub id: TenantId,
    pub label: String,
    pub tsg_id: Option<String>,
    pub destination: Destination,
    /// The exact origin the scan key may be sent to.
    pub origin: String,
    pub scan: ScanView,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ActiveContext {
    pub tenant: TenantId,
    pub context: ContextRef,
}

/// The stored summary an operator reviews before a destination change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct EndpointReviewView {
    pub review: ReviewId,
    pub tenant: TenantId,
    pub from_origin: String,
    pub to: Destination,
    pub to_origin: String,
    /// The current scan key is removed and must be entered again.
    pub removes_scan_credential: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ImportView {
    Idle,
    Choosing,
    #[serde(rename_all = "camelCase")]
    Review {
        review: ReviewId,
        format: ImportFormat,
        file_name: String,
        candidates: Vec<ImportCandidate>,
        discarded: Discarded,
    },
    Failed {
        error: ImportError,
    },
}

/// Which step of a reset did not complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ResetStage {
    /// Deleting the v2 Keychain credentials (some may already be deleted;
    /// all were retired and stay in the cleanup ledger).
    Credentials,
    /// Removing the local document (the credentials were already deleted).
    Document,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ResetFailure {
    pub stage: ResetStage,
    pub reason: VaultError,
}

/// Redacted shell state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    /// Opaque, monotonically increasing revision (string-encoded u64).
    pub revision: String,
    pub appearance: Theme,
    pub features: Vec<FeatureEntry>,
    /// True only in the distinct test-fixtures identity.
    pub fixture_build: bool,
    pub storage: StorageHealth,
    pub tenants: Vec<TenantView>,
    pub active: Option<ActiveContext>,
    pub endpoint_review: Option<EndpointReviewView>,
    pub import: ImportView,
    /// The last reset stopped at this step. Scan keys were retired when it
    /// started and must be entered again; other data stays until a reset
    /// completes.
    pub reset_failure: Option<ResetFailure>,
    /// The last change was written but the OS did not confirm it durable.
    pub durability_unconfirmed: bool,
}

/// Bounds for the core's in-memory structures.
#[derive(Clone, Copy, Debug)]
pub struct CoreConfig {
    /// Settled admission results retained per delivery session.
    pub retained_results: usize,
    /// Notifications queued per observer before it must resynchronize.
    pub observer_queue: usize,
    pub operations: OperationLimits,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            retained_results: 256,
            observer_queue: 256,
            operations: OperationLimits::default(),
        }
    }
}

/// A file chosen in the native import sheet.
pub struct ImportFile {
    /// Display name only (no directory).
    pub name: String,
    pub bytes: Zeroizing<Vec<u8>>,
}

/// The native file sheet for profile import (host adapter).
pub trait ImportPicker: Send + Sync + 'static {
    /// `None` when the operator cancelled. Reads are bounded by the host.
    fn choose(
        &self,
    ) -> futures_util::future::BoxFuture<'static, Option<Result<ImportFile, ImportError>>>;
}

/// Keychain operations the core needs beyond single records.
pub trait CredentialStore: Vault {
    /// Deletes every record in this build's v2 credential namespace.
    fn purge(&self) -> Result<(), VaultError>;
}

/// OS adapters injected by the native host.
pub struct Adapters {
    pub keychain: Arc<dyn CredentialStore>,
    pub store: Arc<dyn AppStore>,
    pub picker: Arc<dyn ImportPicker>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ScanCapability {
    Missing,
    Storing {
        lease: u64,
        record: RecordId,
        mode: StorageMode,
    },
    Ready {
        record: RecordId,
        mode: StorageMode,
        destination: String,
    },
    Failed {
        mode: StorageMode,
        failure: VaultError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Tenant {
    profile: TenantProfile,
    scan: ScanCapability,
}

/// Everything persisted, plus capability state that rides with it.
#[derive(Clone, Debug, Default)]
struct Data {
    appearance: Option<Theme>,
    tenants: Vec<Tenant>,
    active: Option<TenantId>,
    pending_cleanup: Vec<RecordId>,
}

struct EndpointReview {
    id: ReviewId,
    context: ContextRef,
    tenant: TenantId,
    to: Destination,
}

enum ImportState {
    Idle,
    Choosing {
        lease: u64,
    },
    Review {
        id: ReviewId,
        file_name: String,
        preview: ImportPreview,
    },
    Failed(ImportError),
}

struct State {
    revision: u64,
    operator_sessions: Sessions,
    subscribers: Subscribers,
    admitted: u64,
    storage: StorageHealth,
    data: Data,
    salt: [u8; 16],
    epoch: u64,
    active_ref: Option<ContextRef>,
    endpoint_review: Option<EndpointReview>,
    consumed_reviews: Vec<ReviewId>,
    import: ImportState,
    reset_failure: Option<ResetFailure>,
    durability_unconfirmed: bool,
    cleaning: bool,
    /// Keychain records whose write has not settled; cleanup never deletes
    /// them (a delete could precede the write and drop the ledger entry).
    writes_in_flight: Vec<RecordId>,
    next_lease: u64,
    operations: Operations,
    transport: Option<Arc<TransportFactory>>,
    store: Arc<dyn AppStore>,
}

impl State {
    fn appearance(&self) -> Theme {
        self.data.appearance.unwrap_or(Theme::System)
    }

    fn snapshot(&self) -> ShellSnapshot {
        ShellSnapshot {
            revision: self.revision.to_string(),
            appearance: self.appearance(),
            features: features::catalogue(),
            fixture_build: cfg!(feature = "test-fixtures"),
            storage: self.storage,
            tenants: self.data.tenants.iter().map(tenant_view).collect(),
            active: match (&self.data.active, &self.active_ref) {
                (Some(tenant), Some(context)) => Some(ActiveContext {
                    tenant: tenant.clone(),
                    context: context.clone(),
                }),
                _ => None,
            },
            endpoint_review: self.endpoint_review.as_ref().and_then(|r| {
                let tenant = self.tenant(&r.tenant)?;
                Some(EndpointReviewView {
                    review: r.id.clone(),
                    tenant: r.tenant.clone(),
                    from_origin: tenant.profile.destination.origin().to_owned(),
                    to_origin: r.to.origin().to_owned(),
                    to: r.to.clone(),
                    removes_scan_credential: tenant.scan != ScanCapability::Missing,
                })
            }),
            import: match &self.import {
                ImportState::Idle => ImportView::Idle,
                ImportState::Choosing { .. } => ImportView::Choosing,
                ImportState::Review {
                    id,
                    file_name,
                    preview,
                } => ImportView::Review {
                    review: id.clone(),
                    format: preview.format,
                    file_name: file_name.clone(),
                    candidates: preview.candidates.clone(),
                    discarded: preview.discarded,
                },
                ImportState::Failed(error) => ImportView::Failed { error: *error },
            },
            reset_failure: self.reset_failure,
            durability_unconfirmed: self.durability_unconfirmed,
        }
    }

    fn tenant(&self, id: &TenantId) -> Option<&Tenant> {
        self.data.tenants.iter().find(|t| &t.profile.id == id)
    }

    fn tenant_mut(&mut self, id: &TenantId) -> Option<&mut Tenant> {
        self.data.tenants.iter_mut().find(|t| &t.profile.id == id)
    }

    fn persisted(&self) -> PersistedState {
        PersistedState {
            appearance: self.appearance(),
            tenants: self
                .data
                .tenants
                .iter()
                .map(|t| t.profile.clone())
                .collect(),
            active_tenant: self.data.active.clone(),
            credentials: self
                .data
                .tenants
                .iter()
                .filter_map(|t| match &t.scan {
                    ScanCapability::Ready {
                        record,
                        mode: StorageMode::Keychain,
                        destination,
                    } => Some(StoredCredential {
                        record: record.clone(),
                        tenant: t.profile.id.clone(),
                        destination: destination.clone(),
                    }),
                    _ => None,
                })
                .collect(),
            pending_cleanup: self.data.pending_cleanup.clone(),
            ..PersistedState::default()
        }
    }

    fn lease(&mut self) -> u64 {
        self.next_lease += 1;
        self.next_lease
    }

    fn changed(&mut self) {
        self.revision += 1;
        let update = Update::Shell(Box::new(self.snapshot()));
        self.subscribers.publish(&update);
    }

    /// Issues a fresh context reference for the active tenant (or none) and
    /// retires everything bound to the previous one.
    fn reissue_context(&mut self) -> Vec<runtime::Start> {
        self.epoch += 1;
        self.active_ref = self
            .data
            .active
            .as_ref()
            .map(|_| ContextRef::issue(&self.salt, self.epoch));
        if self
            .endpoint_review
            .as_ref()
            .is_some_and(|r| Some(&r.context) != self.active_ref.as_ref())
        {
            self.endpoint_review = None;
        }
        let keep = self.active_ref.as_ref().map(|c| c.as_str().to_owned());
        self.operations
            .retire_contexts(keep.as_deref(), Instant::now())
    }

    fn check_context(&self, context: &ContextRef) -> Result<TenantId, Refusal> {
        if self.storage == StorageHealth::Resetting {
            return Err(Refusal::StaleContext);
        }
        match (&self.active_ref, &self.data.active) {
            (Some(current), Some(tenant)) if current == context => Ok(tenant.clone()),
            _ => Err(Refusal::StaleContext),
        }
    }

    fn require_storage(&self) -> Result<(), Refusal> {
        match self.storage {
            StorageHealth::Ready => Ok(()),
            StorageHealth::Resetting => Err(Refusal::Busy),
            StorageHealth::Corrupt | StorageHealth::Unavailable => Err(Refusal::DeniedStore),
        }
    }

    /// Retires a tenant's scan credential in `data`. Keychain records join
    /// the durable cleanup ledger; a session record is returned so the caller
    /// deletes it once the change is committed.
    fn retire_credential(&mut self, tenant: &TenantId) -> Option<RecordId> {
        let t = self.tenant_mut(tenant)?;
        match std::mem::replace(&mut t.scan, ScanCapability::Missing) {
            ScanCapability::Ready {
                record,
                mode: StorageMode::Keychain,
                ..
            } => {
                self.data.pending_cleanup.push(record);
                None
            }
            ScanCapability::Ready {
                record,
                mode: StorageMode::Session,
                ..
            } => Some(record),
            // An in-flight write is already in the ledger (Keychain) or is
            // discarded by its stale completion (session).
            ScanCapability::Storing { .. }
            | ScanCapability::Failed { .. }
            | ScanCapability::Missing => None,
        }
    }

    /// Saves the current data. A failure before the replacement changed
    /// nothing: memory is restored to `before` and the intent is refused. A
    /// replacement whose durability was not confirmed is kept (it is what a
    /// restart would most likely load) and reported in the snapshot.
    fn save_with(&mut self, before: &Data) -> Result<(), Refusal> {
        let saved = match self.storage {
            StorageHealth::Ready => self.store.save(&self.persisted()),
            StorageHealth::Resetting => {
                self.data = before.clone();
                return Err(Refusal::Busy);
            }
            StorageHealth::Corrupt | StorageHealth::Unavailable => {
                self.data = before.clone();
                return Err(Refusal::DeniedStore);
            }
        };
        match saved {
            Ok(()) => {
                self.durability_unconfirmed = false;
                Ok(())
            }
            Err(StoreError::Unconfirmed) => {
                self.durability_unconfirmed = true;
                Ok(())
            }
            Err(error) => {
                self.data = before.clone();
                Err(store_refusal(error))
            }
        }
    }

    /// Saves again to confirm durability after an unconfirmed save. Only a
    /// confirmed write-ahead may authorize a protected-storage write.
    fn confirm_durable(&mut self) -> bool {
        match self.store.save(&self.persisted()) {
            Ok(()) => {
                self.durability_unconfirmed = false;
                true
            }
            Err(_) => false,
        }
    }

    /// Retires every context reference and operation (reset admission).
    fn retire_all_contexts(&mut self) -> Vec<runtime::Start> {
        self.epoch += 1;
        self.active_ref = None;
        self.endpoint_review = None;
        self.operations.retire_contexts(None, Instant::now())
    }
}

fn tenant_view(t: &Tenant) -> TenantView {
    let (state, mode, failure) = match &t.scan {
        ScanCapability::Missing => (ScanState::Missing, None, None),
        ScanCapability::Storing { mode, .. } => (ScanState::Storing, Some(*mode), None),
        ScanCapability::Ready { mode, .. } => (ScanState::Ready, Some(*mode), None),
        ScanCapability::Failed { mode, failure } => {
            (ScanState::Failed, Some(*mode), Some(*failure))
        }
    };
    TenantView {
        id: t.profile.id.clone(),
        label: t.profile.label.clone(),
        tsg_id: t.profile.tsg_id.clone(),
        origin: t.profile.destination.origin().to_owned(),
        destination: t.profile.destination.clone(),
        scan: ScanView {
            state,
            mode,
            failure,
        },
    }
}

fn store_refusal(error: StoreError) -> Refusal {
    match error {
        StoreError::Corrupt | StoreError::Unavailable | StoreError::Unconfirmed => {
            Refusal::DeniedStore
        }
    }
}

/// Adapter work started after admission, outside the lock.
enum Effect {
    Store {
        tenant: TenantId,
        lease: u64,
        record: RecordId,
        mode: StorageMode,
        key: EnteredSecret,
    },
    Cleanup,
    ChooseImport {
        lease: u64,
    },
    Reset,
    Start(Vec<runtime::Start>),
}

struct Inner {
    state: Mutex<State>,
    adapters: Adapters,
    session: SessionVault,
}

/// The application core. The native host creates exactly one.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

impl Core {
    /// Creates the core and loads persisted state. A corrupt or unavailable
    /// document is reported in the snapshot and never overwritten implicitly.
    pub fn new(config: CoreConfig, adapters: Adapters) -> Self {
        let (storage, data) = match adapters.store.load() {
            Ok(Some(doc)) => (StorageHealth::Ready, data_from(doc)),
            Ok(None) => (StorageHealth::Ready, Data::default()),
            Err(StoreError::Corrupt) => (StorageHealth::Corrupt, Data::default()),
            // Loading never reports Unconfirmed; treat it as unavailable.
            Err(StoreError::Unavailable | StoreError::Unconfirmed) => {
                (StorageHealth::Unavailable, Data::default())
            }
        };
        let mut salt = [0u8; 16];
        // Without entropy every reference would still be unique per epoch;
        // the salt only makes them unguessable across launches.
        let _ = getrandom::fill(&mut salt);
        // The default Direct policy has no failing inputs; if the TLS stack
        // is unusable the scan capability reports missing instead.
        let transport = TransportFactory::new(TransportPolicy::default())
            .ok()
            .map(Arc::new);
        let mut state = State {
            revision: 1,
            operator_sessions: Sessions::new(config.retained_results),
            subscribers: Subscribers::new(config.observer_queue),
            admitted: 0,
            storage,
            data,
            salt,
            epoch: 0,
            active_ref: None,
            endpoint_review: None,
            consumed_reviews: Vec::new(),
            import: ImportState::Idle,
            reset_failure: None,
            durability_unconfirmed: false,
            cleaning: false,
            writes_in_flight: Vec::new(),
            next_lease: 0,
            operations: Operations::new(config.operations),
            transport,
            store: Arc::clone(&adapters.store),
        };
        let _ = state.reissue_context();
        Self {
            inner: Arc::new(Inner {
                state: Mutex::new(state),
                adapters,
                session: SessionVault::new(),
            }),
        }
    }

    /// Resumes durable work after launch (pending Keychain cleanup). Call on
    /// the application runtime.
    pub fn start(&self) {
        run_effects(&self.inner, vec![Effect::Cleanup]);
    }

    /// The operator handle, created by the host for its packaged main window.
    pub fn operator(&self) -> AuthorityHandle {
        AuthorityHandle {
            principal: Principal::Operator,
            inner: Arc::clone(&self.inner),
        }
    }

    /// Installs a new transport policy revision; contexts and their work
    /// bound to the previous revision are retired.
    pub fn set_transport_policy(&self, policy: TransportPolicy) -> Result<(), PolicyError> {
        let factory = Arc::new(TransportFactory::new(policy)?);
        let starts = {
            let mut state = lock(&self.inner).map_err(|_| PolicyError::Client)?;
            state.transport = Some(factory);
            let starts = state.reissue_context();
            state.changed();
            starts
        };
        run_effects(&self.inner, vec![Effect::Start(starts)]);
        Ok(())
    }
}

fn data_from(doc: PersistedState) -> Data {
    let tenants = doc
        .tenants
        .into_iter()
        .map(|profile| {
            let scan = doc
                .credentials
                .iter()
                .find(|c| c.tenant == profile.id && c.destination == profile.destination.origin())
                .map(|c| ScanCapability::Ready {
                    record: c.record.clone(),
                    mode: StorageMode::Keychain,
                    destination: c.destination.clone(),
                })
                .unwrap_or(ScanCapability::Missing);
            Tenant { profile, scan }
        })
        .collect();
    Data {
        appearance: Some(doc.appearance),
        tenants,
        active: doc.active_tenant,
        pending_cleanup: doc.pending_cleanup,
    }
}

fn lock(inner: &Inner) -> Result<MutexGuard<'_, State>, Refusal> {
    // A poisoned lock means an authoritative transition panicked; refuse
    // further work instead of continuing from a possibly torn state.
    inner.state.lock().map_err(|_| Refusal::ShuttingDown)
}

/// Principal-scoped entry to native desktop operations and observations.
#[derive(Clone)]
pub struct AuthorityHandle {
    principal: Principal,
    inner: Arc<Inner>,
}

impl AuthorityHandle {
    pub fn principal(&self) -> Principal {
        self.principal
    }

    /// Registers a new delivery session for this principal, retiring the
    /// previous one (for example after a renderer reload).
    pub fn register_delivery_session(&self) -> Result<DeliverySession, Refusal> {
        lock(&self.inner)?.operator_sessions.register()
    }

    /// Admits or refuses one delivered Intent. Admission never awaits remote
    /// IO, Keychain calls or an operator answer while holding the state.
    pub async fn dispatch(&self, delivery: Delivery, intent: Intent) -> Result<Receipt, Refusal> {
        let (result, effects) = {
            let mut state = lock(&self.inner)?;
            let digest = intent_digest(&state.salt, &intent);
            match state.operator_sessions.check(&delivery, &digest) {
                Admit::Refused(refusal) => return Err(refusal),
                Admit::Duplicate(result) => return result,
                Admit::Fresh => {
                    let mut effects = Vec::new();
                    let result = admit(&mut state, &self.inner.session, intent, &mut effects);
                    if result.is_err() {
                        effects.clear();
                    }
                    state
                        .operator_sessions
                        .record(&delivery, digest, result.clone());
                    (result, effects)
                }
            }
        };
        run_effects(&self.inner, effects);
        result
    }

    /// Returns the current redacted snapshot and subscribes to its updates
    /// atomically, so no update between them is lost.
    pub fn observe(&self, scope: ViewScope) -> Result<Observation, Refusal> {
        let mut state = lock(&self.inner)?;
        match scope {
            ViewScope::Shell => {
                let snapshot = state.snapshot();
                let updates = state.subscribers.subscribe().ok_or(Refusal::Busy)?;
                Ok(Observation::Shell { snapshot, updates })
            }
        }
    }

    /// Live observers registered with the core (retired ones are pruned).
    pub fn observer_count(&self) -> usize {
        lock(&self.inner)
            .map(|mut s| s.subscribers.len())
            .unwrap_or(0)
    }

    /// Admits remote work bound to the current context. Production remote
    /// operations (C03 Scan) are admitted through this path from their
    /// intents; fixtures use it to exercise the shared lifecycle.
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn admit_fixture_operation(
        &self,
        context: &ContextRef,
        bytes: usize,
        work: runtime::Work,
    ) -> Result<OperationId, Refusal> {
        let (id, starts) = {
            let mut state = lock(&self.inner)?;
            state.check_context(context)?;
            state
                .operations
                .admit(context.as_str(), bytes, work, Instant::now())?
        };
        run_effects(&self.inner, vec![Effect::Start(starts)]);
        Ok(id)
    }

    /// Cancels an operation admitted by this principal.
    pub fn cancel_operation(&self, operation: &OperationId) -> Result<bool, Refusal> {
        let (found, starts) = {
            let mut state = lock(&self.inner)?;
            state
                .operations
                .cancel(operation, StopReason::Cancelled, Instant::now())
        };
        run_effects(&self.inner, vec![Effect::Start(starts)]);
        Ok(found)
    }

    pub fn operation_state(&self, operation: &OperationId) -> Option<OperationState> {
        lock(&self.inner).ok()?.operations.state(operation)
    }

    /// A transport for one admitted operation under the current policy.
    pub fn transport_for_admission(
        &self,
    ) -> Result<Arc<crate::transport::NativeTransport>, Refusal> {
        let factory = lock(&self.inner)?
            .transport
            .clone()
            .ok_or(Refusal::CapabilityMissing)?;
        factory
            .for_admission()
            .map_err(|_| Refusal::CapabilityMissing)
    }

    /// Reads the active tenant's scan key for a binding (C03). The secret
    /// never enters a snapshot; the read runs off the transition lock.
    pub async fn read_scan_credential(
        &self,
        context: &ContextRef,
    ) -> Result<(String, Zeroizing<Vec<u8>>), Refusal> {
        let (record, mode, destination) = {
            let state = lock(&self.inner)?;
            let tenant = state.check_context(context)?;
            match &state.tenant(&tenant).ok_or(Refusal::StaleContext)?.scan {
                ScanCapability::Ready {
                    record,
                    mode,
                    destination,
                } => (record.clone(), *mode, destination.clone()),
                _ => return Err(Refusal::CapabilityMissing),
            }
        };
        let inner = Arc::clone(&self.inner);
        let read = tokio::task::spawn_blocking(move || match mode {
            StorageMode::Keychain => inner.adapters.keychain.get(&record),
            StorageMode::Session => inner.session.get(&record),
        })
        .await
        .map_err(|_| Refusal::ShuttingDown)?;
        let secret = read.map_err(|e| match e {
            VaultError::Locked => Refusal::LockedStore,
            VaultError::NotFound => Refusal::CapabilityMissing,
            VaultError::Denied | VaultError::Unavailable | VaultError::Corrupt => {
                Refusal::DeniedStore
            }
        })?;
        // Recheck after the read: a context change meanwhile retires it.
        lock(&self.inner)?.check_context(context)?;
        Ok((destination, secret))
    }

    /// Session-only credentials currently held (for lifecycle tests).
    pub fn session_credentials(&self) -> usize {
        self.inner.session.len()
    }
}

fn admit(
    state: &mut State,
    session: &SessionVault,
    intent: Intent,
    effects: &mut Vec<Effect>,
) -> Result<Receipt, Refusal> {
    match intent {
        Intent::SetAppearance { theme } => {
            if state.appearance() != theme {
                let before = state.data.clone();
                state.data.appearance = Some(theme);
                if state.storage == StorageHealth::Ready
                    && let Err(e) = state.save_with(&before)
                {
                    return Err(e);
                }
                state.changed();
            }
        }
        Intent::CreateTenant { profile } => {
            state.require_storage()?;
            let profile = profile.validate().ok_or(Refusal::InvalidInput)?;
            if state.data.tenants.len() >= MAX_TENANTS {
                return Err(Refusal::Busy);
            }
            let id = TenantId::issue().ok_or(Refusal::ShuttingDown)?;
            let before = state.data.clone();
            let select = state.data.active.is_none();
            state.data.tenants.push(Tenant {
                profile: TenantProfile {
                    id: id.clone(),
                    label: profile.label,
                    tsg_id: profile.tsg_id,
                    destination: profile.destination,
                },
                scan: ScanCapability::Missing,
            });
            if select {
                state.data.active = Some(id);
            }
            state.save_with(&before)?;
            if select {
                effects.push(Effect::Start(state.reissue_context()));
            }
            state.changed();
        }
        Intent::SelectTenant { tenant } => {
            state.require_storage()?;
            if state.tenant(&tenant).is_none() {
                return Err(Refusal::InvalidInput);
            }
            let before = state.data.clone();
            state.data.active = Some(tenant);
            state.save_with(&before)?;
            effects.push(Effect::Start(state.reissue_context()));
            state.changed();
        }
        Intent::DeleteTenant { tenant } => {
            state.require_storage()?;
            if state.tenant(&tenant).is_none() {
                return Err(Refusal::InvalidInput);
            }
            let before = state.data.clone();
            let was_active = state.data.active.as_ref() == Some(&tenant);
            let retired_session = state.retire_credential(&tenant);
            state.data.tenants.retain(|t| t.profile.id != tenant);
            if was_active {
                state.data.active = None;
            }
            state.save_with(&before)?;
            if let Some(record) = retired_session {
                let _ = session.delete(&record);
            }
            if state
                .endpoint_review
                .as_ref()
                .is_some_and(|r| r.tenant == tenant)
            {
                state.endpoint_review = None;
            }
            if was_active {
                effects.push(Effect::Start(state.reissue_context()));
            }
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::ConfigureScanCredential { context, mode, key } => {
            let tenant = state.check_context(&context)?;
            if mode == StorageMode::Keychain {
                state.require_storage()?;
            } else if state.storage == StorageHealth::Resetting {
                return Err(Refusal::Busy);
            }
            if key.expose().is_empty()
                || key.expose().len() > MAX_SECRET_BYTES
                || !crate::scan::key_is_usable(key.expose())
            {
                return Err(Refusal::InvalidInput);
            }
            let current = state.tenant(&tenant).ok_or(Refusal::StaleContext)?;
            if matches!(current.scan, ScanCapability::Storing { .. }) {
                return Err(Refusal::Busy);
            }
            let record = RecordId::issue().ok_or(Refusal::ShuttingDown)?;
            let lease = state.lease();
            let before = state.data.clone();
            let retired_session = state.retire_credential(&tenant);
            if let Some(t) = state.tenant_mut(&tenant) {
                t.scan = ScanCapability::Storing {
                    lease,
                    record: record.clone(),
                    mode,
                };
            }
            if mode == StorageMode::Keychain {
                // Write-ahead: the new record is in the cleanup ledger until
                // it is committed, so a crash never leaves an untracked item.
                state.data.pending_cleanup.push(record.clone());
            }
            if state.storage == StorageHealth::Ready {
                state.save_with(&before)?;
            }
            if let Some(old) = retired_session {
                let _ = session.delete(&old);
            }
            // A Keychain secret may be written only once its write-ahead
            // ledger entry is confirmed durable: if a crash could restore a
            // document without that entry, the secret would be untracked.
            // The retirement of the previous key stays visible either way.
            let authorized = mode == StorageMode::Session
                || !state.durability_unconfirmed
                || state.confirm_durable();
            effects.push(Effect::Start(state.reissue_context()));
            if authorized {
                if mode == StorageMode::Keychain {
                    state.writes_in_flight.push(record.clone());
                }
                effects.push(Effect::Store {
                    tenant,
                    lease,
                    record,
                    mode,
                    key,
                });
            } else if let Some(t) = state.tenant_mut(&tenant) {
                // Nothing was written; the ledger entry settles as absent.
                t.scan = ScanCapability::Failed {
                    mode,
                    failure: VaultError::Unavailable,
                };
            }
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::RemoveScanCredential { context } => {
            let tenant = state.check_context(&context)?;
            let has_keychain = matches!(
                state.tenant(&tenant).map(|t| &t.scan),
                Some(ScanCapability::Ready {
                    mode: StorageMode::Keychain,
                    ..
                })
            );
            if has_keychain {
                state.require_storage()?;
            }
            let before = state.data.clone();
            let retired_session = state.retire_credential(&tenant);
            if state.storage == StorageHealth::Ready {
                state.save_with(&before)?;
            }
            if let Some(old) = retired_session {
                let _ = session.delete(&old);
            }
            effects.push(Effect::Start(state.reissue_context()));
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::ReviewEndpointChange {
            context,
            destination,
        } => {
            state.require_storage()?;
            let tenant = state.check_context(&context)?;
            let to = destination.validate().ok_or(Refusal::InvalidInput)?;
            let current = state.tenant(&tenant).ok_or(Refusal::StaleContext)?;
            if current.profile.destination.origin() == to.origin() {
                return Err(Refusal::InvalidInput);
            }
            state.endpoint_review = Some(EndpointReview {
                id: ReviewId::issue().ok_or(Refusal::ShuttingDown)?,
                context,
                tenant,
                to,
            });
            state.changed();
        }
        Intent::ConfirmEndpointChange { context, review } => {
            if state.consumed_reviews.contains(&review) {
                return Err(Refusal::ReviewConsumed);
            }
            let tenant = state.check_context(&context)?;
            state.require_storage()?;
            let pending = state
                .endpoint_review
                .as_ref()
                .filter(|r| r.id == review && r.context == context && r.tenant == tenant)
                .ok_or(Refusal::ReviewInvalid)?;
            let to = pending.to.clone();
            let before = state.data.clone();
            let retired_session = state.retire_credential(&tenant);
            if let Some(t) = state.tenant_mut(&tenant) {
                t.profile.destination = to;
            }
            state.save_with(&before)?;
            if let Some(old) = retired_session {
                let _ = session.delete(&old);
            }
            state.endpoint_review = None;
            state.consumed_reviews.push(review);
            effects.push(Effect::Start(state.reissue_context()));
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::DiscardReview { review } => {
            let mut found = false;
            if state
                .endpoint_review
                .as_ref()
                .is_some_and(|r| r.id == review)
            {
                state.endpoint_review = None;
                found = true;
            }
            if matches!(&state.import, ImportState::Review { id, .. } if *id == review) {
                state.import = ImportState::Idle;
                found = true;
            }
            if !found {
                return Err(Refusal::ReviewInvalid);
            }
            state.changed();
        }
        Intent::ChooseProfileImport => {
            state.require_storage()?;
            if matches!(state.import, ImportState::Choosing { .. }) {
                return Err(Refusal::Busy);
            }
            let lease = state.lease();
            state.import = ImportState::Choosing { lease };
            effects.push(Effect::ChooseImport { lease });
            state.changed();
        }
        Intent::ConfirmProfileImport { review, selected } => {
            if state.consumed_reviews.contains(&review) {
                return Err(Refusal::ReviewConsumed);
            }
            state.require_storage()?;
            let ImportState::Review { id, preview, .. } = &state.import else {
                return Err(Refusal::ReviewInvalid);
            };
            if *id != review {
                return Err(Refusal::ReviewInvalid);
            }
            let mut chosen = Vec::new();
            for index in &selected {
                let candidate = preview
                    .candidates
                    .iter()
                    .find(|c| c.index == *index)
                    .ok_or(Refusal::InvalidInput)?;
                if candidate.problem.is_some()
                    || chosen.iter().any(|c: &ImportCandidate| c.index == *index)
                {
                    return Err(Refusal::InvalidInput);
                }
                chosen.push(candidate.clone());
            }
            if chosen.is_empty() || state.data.tenants.len() + chosen.len() > MAX_TENANTS {
                return Err(Refusal::InvalidInput);
            }
            let before = state.data.clone();
            for candidate in chosen {
                let profile = ProfileDraft {
                    label: candidate.label,
                    tsg_id: candidate.tsg_id,
                    destination: candidate.destination.ok_or(Refusal::InvalidInput)?,
                }
                .validate()
                .ok_or(Refusal::InvalidInput)?;
                state.data.tenants.push(Tenant {
                    profile: TenantProfile {
                        id: TenantId::issue().ok_or(Refusal::ShuttingDown)?,
                        label: profile.label,
                        tsg_id: profile.tsg_id,
                        destination: profile.destination,
                    },
                    scan: ScanCapability::Missing,
                });
            }
            state.save_with(&before)?;
            state.import = ImportState::Idle;
            state.consumed_reviews.push(review);
            state.changed();
        }
        Intent::ResetLocalData => {
            match state.storage {
                StorageHealth::Resetting => return Err(Refusal::Busy),
                // Another instance may own this data root and namespace.
                StorageHealth::Unavailable => return Err(Refusal::DeniedStore),
                StorageHealth::Ready | StorageHealth::Corrupt => {}
            }
            // Retire every credential first and save that retirement (with
            // the Keychain records in the cleanup ledger) before anything is
            // deleted, so an interrupted reset is resumed by cleanup and never
            // leaves an untracked or still-usable credential.
            let before = state.data.clone();
            let ids: Vec<TenantId> = state
                .data
                .tenants
                .iter()
                .map(|t| t.profile.id.clone())
                .collect();
            let retired_sessions: Vec<RecordId> = ids
                .iter()
                .filter_map(|id| state.retire_credential(id))
                .collect();
            if state.storage == StorageHealth::Ready {
                state.save_with(&before)?;
            }
            for record in retired_sessions {
                let _ = session.delete(&record);
            }
            session.clear();
            state.storage = StorageHealth::Resetting;
            state.reset_failure = None;
            state.import = ImportState::Idle;
            effects.push(Effect::Start(state.retire_all_contexts()));
            effects.push(Effect::Reset);
            state.changed();
        }
    }
    state.admitted += 1;
    Ok(Receipt::Admitted {
        request: format!("rq-{}", state.admitted),
    })
}

fn intent_digest(salt: &[u8; 16], intent: &Intent) -> [u8; 32] {
    // Serialization of this closed enum cannot fail; an empty digest input
    // would still only ever match itself. Entered secrets are covered by the
    // digest but the serialized bytes are zeroized and the digest is salted
    // per launch, so no unsalted hash of a key is retained.
    let bytes = Zeroizing::new(serde_json::to_vec(intent).unwrap_or_default());
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(bytes.as_slice());
    hasher.finalize().into()
}

fn run_effects(inner: &Arc<Inner>, effects: Vec<Effect>) {
    for effect in effects {
        match effect {
            Effect::Start(starts) => {
                for start in starts {
                    let inner = Arc::clone(inner);
                    runtime::spawn(start, move |id, outcome, stop| {
                        complete_operation(&inner, &id, outcome, stop)
                    });
                }
            }
            Effect::Store {
                tenant,
                lease,
                record,
                mode,
                key,
            } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let worker = Arc::clone(&inner);
                    let stored_record = record.clone();
                    let result = tokio::task::spawn_blocking(move || {
                        let written = match mode {
                            StorageMode::Keychain => worker
                                .adapters
                                .keychain
                                .put(&stored_record, key.expose().as_bytes()),
                            StorageMode::Session => {
                                worker.session.put(&stored_record, key.expose().as_bytes())
                            }
                        };
                        drop(key);
                        written
                    })
                    .await
                    .unwrap_or(Err(VaultError::Unavailable));
                    settle_store(&inner, &tenant, lease, record, mode, result);
                });
            }
            Effect::Cleanup => drain_cleanup(inner),
            Effect::ChooseImport { lease } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let chosen = inner.adapters.picker.choose().await;
                    settle_import(&inner, lease, chosen);
                });
            }
            Effect::Reset => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    // No new credential write can start while resetting; wait
                    // for admitted ones to settle so none lands after the purge.
                    loop {
                        let pending = lock(&inner).map(|s| s.writes_in_flight.len()).unwrap_or(0);
                        if pending == 0 {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    let worker = Arc::clone(&inner);
                    let result = tokio::task::spawn_blocking(move || {
                        worker
                            .adapters
                            .keychain
                            .purge()
                            .map_err(|reason| ResetFailure {
                                stage: ResetStage::Credentials,
                                reason,
                            })?;
                        match worker.adapters.store.reset() {
                            Ok(()) => Ok(false),
                            Err(StoreError::Unconfirmed) => Ok(true),
                            Err(_) => Err(ResetFailure {
                                stage: ResetStage::Document,
                                reason: VaultError::Unavailable,
                            }),
                        }
                    })
                    .await
                    .unwrap_or(Err(ResetFailure {
                        stage: ResetStage::Credentials,
                        reason: VaultError::Unavailable,
                    }));
                    settle_reset(&inner, result);
                });
            }
        }
    }
}

fn complete_operation(
    inner: &Arc<Inner>,
    id: &OperationId,
    outcome: Outcome,
    stop: Option<StopReason>,
) {
    let starts = match lock(inner) {
        Ok(mut state) => {
            let now = Instant::now();
            if let Some(stop) = stop {
                state.operations.cancel(id, stop, now);
            }
            state.operations.complete(id, outcome, now)
        }
        Err(_) => return,
    };
    run_effects(inner, vec![Effect::Start(starts)]);
}

fn settle_store(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    lease: u64,
    record: RecordId,
    mode: StorageMode,
    result: Result<(), VaultError>,
) {
    let Ok(mut state) = lock(inner) else {
        return;
    };
    state.writes_in_flight.retain(|r| r != &record);
    let current = state.tenant(tenant).is_some_and(
        |t| matches!(&t.scan, ScanCapability::Storing { lease: l, .. } if *l == lease),
    );
    if !current {
        // Stale: replaced, signed out, retargeted, deleted or reset while the
        // write ran. A successful stale write is persisted work: it stays in
        // (or joins) the cleanup ledger and is never attached anywhere.
        if result.is_ok() {
            match mode {
                StorageMode::Keychain => {
                    if !state.data.pending_cleanup.contains(&record) {
                        state.data.pending_cleanup.push(record);
                    }
                }
                StorageMode::Session => {
                    let _ = inner.session.delete(&record);
                }
            }
        }
        drop(state);
        drain_cleanup(inner);
        return;
    }
    let before = state.data.clone();
    let destination = state
        .tenant(tenant)
        .map(|t| t.profile.destination.origin().to_owned())
        .unwrap_or_default();
    let next = match result {
        Ok(()) => {
            if mode == StorageMode::Keychain {
                state.data.pending_cleanup.retain(|r| r != &record);
            }
            ScanCapability::Ready {
                record: record.clone(),
                mode,
                destination,
            }
        }
        Err(failure) => ScanCapability::Failed { mode, failure },
    };
    if let Some(t) = state.tenant_mut(tenant) {
        t.scan = next;
    }
    if mode == StorageMode::Keychain && state.save_with(&before).is_err() {
        // The record could not be committed: it stays in the ledger and the
        // capability fails explicitly instead of becoming usable unsaved.
        if let Some(t) = state.tenant_mut(tenant) {
            t.scan = ScanCapability::Failed {
                mode,
                failure: VaultError::Unavailable,
            };
        }
        if !state.data.pending_cleanup.contains(&record) {
            state.data.pending_cleanup.push(record);
        }
    }
    state.changed();
    drop(state);
    drain_cleanup(inner);
}

/// Deletes ledgered Keychain records off the lock; removes each settled one
/// (deleted or already absent) from the durable ledger.
fn drain_cleanup(inner: &Arc<Inner>) {
    let records = {
        let Ok(mut state) = lock(inner) else {
            return;
        };
        let in_flight = state.writes_in_flight.clone();
        let records: Vec<RecordId> = state
            .data
            .pending_cleanup
            .iter()
            .filter(|r| !in_flight.contains(r))
            .cloned()
            .collect();
        if state.cleaning || records.is_empty() || state.storage == StorageHealth::Resetting {
            return;
        }
        state.cleaning = true;
        records
    };
    let inner = Arc::clone(inner);
    tokio::spawn(async move {
        let worker = Arc::clone(&inner);
        let done = tokio::task::spawn_blocking(move || {
            records
                .into_iter()
                .filter(|r| {
                    matches!(
                        worker.adapters.keychain.delete(r),
                        Ok(()) | Err(VaultError::NotFound)
                    )
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        {
            let Ok(mut state) = lock(&inner) else {
                return;
            };
            state.cleaning = false;
            let before = state.data.clone();
            state.data.pending_cleanup.retain(|r| !done.contains(r));
            if state.data.pending_cleanup != before.pending_cleanup
                && state.storage == StorageHealth::Ready
            {
                // A failed save keeps the entries (in memory and on disk);
                // the next drain finds them absent and settles them.
                let _ = state.save_with(&before);
            }
            state.changed();
        };
    });
}

fn settle_import(inner: &Arc<Inner>, lease: u64, chosen: Option<Result<ImportFile, ImportError>>) {
    let Ok(mut state) = lock(inner) else {
        return;
    };
    if !matches!(state.import, ImportState::Choosing { lease: l } if l == lease) {
        return;
    }
    state.import = match chosen {
        None => ImportState::Idle,
        Some(Err(error)) => ImportState::Failed(error),
        Some(Ok(file)) => {
            let labels: Vec<String> = state
                .data
                .tenants
                .iter()
                .map(|t| t.profile.label.clone())
                .collect();
            match import::preview(file.bytes, &labels) {
                Ok(preview) => match ReviewId::issue() {
                    Some(id) => ImportState::Review {
                        id,
                        file_name: display_name(&file.name),
                        preview,
                    },
                    None => ImportState::Failed(ImportError::Unsupported),
                },
                Err(error) => ImportState::Failed(error),
            }
        }
    };
    state.changed();
}

fn display_name(name: &str) -> String {
    let base = name.rsplit('/').next().unwrap_or(name);
    base.chars().filter(|c| !c.is_control()).take(128).collect()
}

/// `Ok(unconfirmed)` when the reset completed (the document removal's
/// durability may be unconfirmed); otherwise the step that stopped it.
fn settle_reset(inner: &Arc<Inner>, result: Result<bool, ResetFailure>) {
    let (starts, cleanup) = {
        let Ok(mut state) = lock(inner) else {
            return;
        };
        match result {
            Ok(unconfirmed) => {
                inner.session.clear();
                state.data = Data::default();
                state.storage = StorageHealth::Ready;
                state.reset_failure = None;
                state.durability_unconfirmed = unconfirmed;
                state.consumed_reviews.clear();
                let starts = state.reissue_context();
                state.changed();
                (starts, false)
            }
            Err(failure) => {
                // Credentials were retired at admission (and stay in the
                // cleanup ledger); tenant profiles and preferences are kept.
                // A fresh context is issued: no earlier reference revives.
                state.storage = match inner.adapters.store.load() {
                    Ok(_) => StorageHealth::Ready,
                    Err(StoreError::Corrupt) => StorageHealth::Corrupt,
                    Err(StoreError::Unavailable | StoreError::Unconfirmed) => {
                        StorageHealth::Unavailable
                    }
                };
                state.reset_failure = Some(failure);
                let starts = state.reissue_context();
                state.changed();
                (starts, true)
            }
        }
    };
    run_effects(inner, vec![Effect::Start(starts)]);
    if cleanup {
        drain_cleanup(inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delivery(session: &str, sequence: u64) -> Delivery {
        Delivery {
            session: session.to_owned(),
            sequence,
        }
    }

    fn core(config: CoreConfig) -> Core {
        Core::new(config, crate::fixtures::adapters().0)
    }

    #[test]
    fn observers_beyond_the_cap_are_refused_not_evicted() {
        let core = core(CoreConfig::default());
        let handle = core.operator();
        let held: Vec<Observation> = (0..crate::observe::MAX_SUBSCRIBERS)
            .map(|_| handle.observe(ViewScope::Shell).expect("within cap"))
            .collect();
        assert_eq!(handle.observe(ViewScope::Shell).err(), Some(Refusal::Busy));
        assert_eq!(handle.observer_count(), crate::observe::MAX_SUBSCRIBERS);
        drop(held);
        assert_eq!(handle.observer_count(), 0, "closed observers are pruned");
        assert!(handle.observe(ViewScope::Shell).is_ok());
    }

    #[tokio::test]
    async fn observer_queue_overflow_ends_with_resync() {
        let core = core(CoreConfig {
            retained_results: 4,
            observer_queue: 3,
            ..CoreConfig::default()
        });
        let handle = core.operator();
        let Observation::Shell { mut updates, .. } =
            handle.observe(ViewScope::Shell).expect("observe");
        let session = handle.register_delivery_session().expect("session");
        let themes = [Theme::Dark, Theme::Light, Theme::Dark, Theme::Light];
        for (i, theme) in themes.into_iter().enumerate() {
            handle
                .dispatch(
                    delivery(&session.nonce, i as u64 + 1),
                    Intent::SetAppearance { theme },
                )
                .await
                .expect("admitted");
        }
        assert!(matches!(updates.try_next(), Some(Update::Shell(_))));
        assert!(matches!(updates.try_next(), Some(Update::Shell(_))));
        assert_eq!(updates.try_next(), Some(Update::Resync));
        assert_eq!(updates.next().await, None);
        assert_eq!(handle.observer_count(), 0);
    }
}
