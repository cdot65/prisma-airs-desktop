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

use scm_sdk::Session;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::Instant;
use ts_rs::TS;
use zeroize::Zeroizing;

mod change_domain;
mod learner_domain;
mod library_domain;
mod management_domain;
mod observation_domain;
mod offline_domain;
mod sessions_domain;

pub use change_domain::{
    ChangeFailure, ChangeResultView, ChangeReviewState, ChangeReviewView, ChangesView, Readback,
    SecretState, StagedFileFailure, StagedFileState, StagedFileView,
};
pub use learner_domain::{AcademyCallHandle, AcademyCallResult};
use learner_domain::{LearnerPhase, LearnerSession};
pub use library_domain::{CatalogSource, LibraryNotice, LibraryState, LibraryView};
pub use management_domain::ManagementHandle;
pub use observation_domain::{AttentionFace, HeldItem, HoldKind, OverviewView};
pub use offline_domain::{DownloadStatus, DownloadView, LocalDraftView, OfflineState, OfflineView};
pub use sessions_domain::{
    ExportKind, ExportState, ExportView, InvestigationView, ReportTarget, ReportUndoView,
    ReportsState, ReportsView, SessionsView,
};

use crate::academy::AcademyClient;
use crate::context::{ContextRef, EnteredSecret, LearnerRef, ReviewId};
use crate::delivery::{Admit, Delivery, DeliverySession, Sessions};
use crate::features::{self, FeatureEntry};
use crate::import::{self, Discarded, ImportCandidate, ImportError, ImportFormat, ImportPreview};
use crate::learner::{AcademyView, ProcedureKind};
use crate::library::CaseDraft;
use crate::management::{ManagementEndpoints, ManagementRequest};
use crate::observe::{Subscribers, Subscription, Update};
use crate::runtime::{
    self, OperationId, OperationLimits, OperationState, Operations, Outcome, StopReason,
};
use crate::scanning::{
    HISTORY_PER_TENANT, PayloadSlot, ScanDraft, ScanFailure, ScanJob, ScanPayload, ScanResultView,
};
use crate::sessions::ReportInput;
use crate::store::{
    AppStore, CredentialClass, PersistedState, StoreError, StoredCredential, StoredLearner,
};
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
    SetAppearance {
        theme: Theme,
    },
    /// Create a tenant profile with no credential.
    CreateTenant {
        profile: ProfileDraft,
    },
    /// Make a tenant active; issues a new context reference.
    SelectTenant {
        tenant: TenantId,
    },
    /// Remove a tenant profile and clean up its credential.
    DeleteTenant {
        tenant: TenantId,
    },
    /// Store a new scan key for the active tenant, replacing any previous one.
    ConfigureScanCredential {
        context: ContextRef,
        mode: StorageMode,
        key: EnteredSecret,
    },
    /// Sign the active tenant's scan capability out.
    RemoveScanCredential {
        context: ContextRef,
    },
    /// Store the active tenant's management OAuth client (its own
    /// credential class, never the scan key), replacing any previous one.
    #[serde(rename_all = "camelCase")]
    ConfigureManagementCredential {
        context: ContextRef,
        mode: StorageMode,
        client_id: String,
        secret: EnteredSecret,
    },
    /// Sign the active tenant's management capability out.
    RemoveManagementCredential {
        context: ContextRef,
    },
    /// One management read (verify, a page or a detail) for the active
    /// tenant's management capability.
    Management {
        context: ContextRef,
        request: ManagementRequest,
    },
    /// Prepare a review of a Runtime change (C09): Rust normalizes the
    /// captured draft and reads its baseline.
    ReviewChange {
        context: ContextRef,
        draft: crate::changes::ChangeDraft,
    },
    /// Ask to apply a prepared review: the freshly captured draft and the
    /// typed confirmation must equal the review's. Opens the native operator
    /// confirmation; only its private completion applies the change.
    RequestApply {
        review: ReviewId,
        draft: crate::changes::ChangeDraft,
        confirmation: String,
    },
    /// Discard the open change review (ends its confirmation if shown).
    DiscardChange {
        review: ReviewId,
    },
    /// Choose a dictionary keyword file in the native file sheet (C10); the
    /// desktop reads it once and stages it for `fileRef` in a draft.
    ChooseDictionaryFile {
        context: ContextRef,
    },
    /// Show an applied change's one-time API key in the native secret sheet.
    RevealSecret {
        operation: OperationId,
    },
    /// Clear an applied change's one-time API key from memory.
    ForgetSecret {
        operation: OperationId,
    },
    /// Open the active tenant's FP/FN reports (C11).
    OpenReports {
        context: ContextRef,
    },
    /// Save an FP/FN report of a scan this investigation read (or edit one).
    SaveReport {
        context: ContextRef,
        target: ReportTarget,
        input: ReportInput,
    },
    #[serde(rename_all = "camelCase")]
    RemoveReport {
        context: ContextRef,
        scan_id: String,
        sub_request: u32,
    },
    /// Restore the last removed report.
    UndoReport {
        context: ContextRef,
    },
    /// Export the reports as CSV through the native save sheet.
    ExportReports {
        context: ContextRef,
    },
    /// Empty this credential's investigation cache (sealed copy included).
    ClearInvestigation {
        context: ContextRef,
    },
    /// Export the investigation's metadata (never content) as JSON.
    ExportInvestigation {
        context: ContextRef,
    },
    /// Open the active tenant's case Library (C12).
    OpenLibrary {
        context: ContextRef,
    },
    /// Save a new case authored in the Library or the scan form.
    SaveCase {
        context: ContextRef,
        draft: CaseDraft,
    },
    DeleteCase {
        context: ContextRef,
        id: String,
    },
    /// Import a case export of this tenant through the native file sheet.
    ImportCases {
        context: ContextRef,
    },
    /// Import the bundled or public example catalog (never executed).
    ImportExamples {
        context: ContextRef,
        source: CatalogSource,
    },
    /// Copy a case into another tenant's library as a new case.
    CopyCase {
        context: ContextRef,
        id: String,
        tenant: TenantId,
    },
    /// Export the active tenant's cases through the native save sheet.
    ExportCases {
        context: ContextRef,
    },
    /// Acknowledge or snooze a Runtime attention item (C12).
    HoldInsight {
        context: ContextRef,
        id: String,
        hold: HoldKind,
    },
    /// Reopen an acknowledged or snoozed attention item.
    RestoreInsight {
        context: ContextRef,
        id: String,
    },
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
    DiscardReview {
        review: ReviewId,
    },
    /// Open the native file sheet to choose a profile file to preview.
    ChooseProfileImport,
    /// Create tenants from selected preview entries (no credentials).
    ConfirmProfileImport {
        review: ReviewId,
        selected: Vec<u32>,
    },
    /// Remove all v2 local data and v2 credentials.
    ResetLocalData,
    /// Run a Runtime Security synchronous scan for the active tenant.
    Scan {
        context: ContextRef,
        draft: ScanDraft,
    },
    /// Cancel a scan: queued work is not sent; a running request is
    /// cancelled and its outcome reported as the service left it.
    CancelScan {
        operation: OperationId,
    },
    /// Start Academy device sign-in (the learner account is independent of
    /// the selected tenant).
    StartAcademySignIn,
    /// Abandon an Academy sign-in in progress; a late grant is revoked.
    CancelAcademySignIn,
    /// Open the current sign-in's verification page in the browser. The URL
    /// is the one Academy's issuer returned; the presentation supplies none.
    OpenAcademyVerification,
    /// Sign the Academy learner out: retire learner work, delete the saved
    /// session and revoke its refresh token.
    SignOutAcademy,
    /// Open the native print sheet for the window's current page (where the
    /// operator can print or save a PDF, such as an Academy certificate).
    /// Nothing is printed without the operator's choice in that sheet.
    /// Observed like a learner call: whether the sheet opened.
    #[serde(rename_all = "camelCase")]
    PrintPage {
        learner: LearnerRef,
    },
    /// Replace the signed-in learner's avatar with an image the operator
    /// chooses in the native picker (JPEG, PNG or WebP, at most 2 MiB),
    /// stored at Academy's presigned destination and confirmed with Academy.
    /// Observed like a learner call (`settings.confirmAvatar`'s reply).
    #[serde(rename_all = "camelCase")]
    UploadAvatar {
        learner: LearnerRef,
    },
    /// Read an avatar Academy returned (on Academy's origin, public) for a
    /// presentation that cannot load remote images. Observed like a learner
    /// call: `{"json": {"src": "data:image/..."}}`.
    #[serde(rename_all = "camelCase")]
    LoadAcademyImage {
        learner: LearnerRef,
        url: String,
    },
    /// Download a course for offline reading (Academy's access check and
    /// bundle, kept sealed under its lease in this account's store).
    /// Observed like a learner call.
    #[serde(rename_all = "camelCase")]
    DownloadCourse {
        learner: LearnerRef,
        course_id: String,
    },
    /// Remove a downloaded course from this Mac. Observed like a learner call.
    #[serde(rename_all = "camelCase")]
    RemoveDownload {
        learner: LearnerRef,
        course_id: String,
    },
    /// Keep lab work on this Mac (sealed; never sent anywhere) when Academy
    /// cannot take it. `draft` is the draft as JSON text. Observed like a
    /// learner call.
    #[serde(rename_all = "camelCase")]
    SaveLocalDraft {
        learner: LearnerRef,
        attempt_id: String,
        base_version: u32,
        draft: String,
    },
    /// Read the lab work kept on this Mac for an attempt (`null` when none).
    /// Observed like a learner call.
    #[serde(rename_all = "camelCase")]
    LoadLocalDraft {
        learner: LearnerRef,
        attempt_id: String,
    },
    /// Forget lab work kept on this Mac once Academy holds it. Observed like
    /// a learner call.
    #[serde(rename_all = "camelCase")]
    ClearLocalDraft {
        learner: LearnerRef,
        attempt_id: String,
    },
    /// Call one allowed learner procedure for the current learner account.
    /// `input` is the tRPC client's serialized (SuperJSON) input.
    #[serde(rename_all = "camelCase")]
    AcademyCall {
        learner: LearnerRef,
        procedure: String,
        procedure_kind: ProcedureKind,
        input: Option<String>,
    },
}

/// Native acknowledgement that an Intent was admitted. It is not evidence of
/// operator confirmation or remote completion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Receipt {
    /// `operation` names the admitted operation for operation intents.
    Admitted {
        request: String,
        operation: Option<OperationId>,
    },
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewScope {
    Shell,
    /// The single result of an admitted learner call.
    AcademyCall(OperationId),
    /// The single result of an admitted management read.
    Management(OperationId),
}

/// A scoped, redacted view of desktop state plus its ordered updates.
// Shell is the common observation; the call handle is a small pointer.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Observation {
    Shell {
        snapshot: ShellSnapshot,
        updates: Subscription,
    },
    AcademyCall(AcademyCallHandle),
    Management(ManagementHandle),
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
    /// The separate management OAuth capability (no secret material).
    pub management: ManagementView,
}

/// Redacted management-capability state. Never contains the client secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementView {
    pub state: ScanState,
    pub mode: Option<StorageMode>,
    pub failure: Option<VaultError>,
    /// The OAuth client ID (not a secret).
    pub client_id: Option<String>,
    /// The only origin the client secret may be sent to.
    pub token_origin: Option<String>,
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
    /// The active tenant's scans this session, newest first.
    pub scans: Vec<ScanRecordView>,
    /// The Academy learner account (independent of the tenant).
    pub academy: AcademyView,
    /// The current learner generation: the signed-in account's, or a guest
    /// generation for public catalog reads while signed out.
    pub learner: Option<LearnerRef>,
    /// Academy web's origin, for public links Academy pages show (such as a
    /// certificate's verify URL). The presentation never sends it back.
    pub academy_origin: String,
    /// The signed-in account's offline work: what is kept, its status and
    /// whether Academy was last reachable (never content).
    pub academy_offline: offline_domain::OfflineView,
    /// The active tenant's change review and applied changes (no secrets).
    pub changes: ChangesView,
    /// The active tenant's FP/FN reports, investigation cache and exports.
    pub sessions: SessionsView,
    /// The active tenant's case Library.
    pub library: LibraryView,
    /// The active context's Runtime Home/Inbox observations.
    pub overview: OverviewView,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ScanProgress {
    Queued,
    Running,
    Settled,
}

/// One scan as the presentation may show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanRecordView {
    pub operation: OperationId,
    pub progress: ScanProgress,
    pub outcome: Option<Outcome>,
    pub stop: Option<StopReason>,
    pub result: Option<ScanResultView>,
    pub failure: Option<ScanFailure>,
    /// The tenant or credential changed before the reply; any result was
    /// discarded and is never shown.
    pub discarded: bool,
}

struct ScanRecord {
    operation: OperationId,
    tenant: TenantId,
    context: ContextRef,
    slot: PayloadSlot,
    payload: Option<ScanPayload>,
    discarded: bool,
    /// Copied from the runtime when observed, so history never depends on
    /// how long the runtime retains settled operations.
    settlement: Option<runtime::Settlement>,
}

/// Bounds for the core's in-memory structures.
#[derive(Clone, Debug)]
pub struct CoreConfig {
    /// Settled admission results retained per delivery session.
    pub retained_results: usize,
    /// Notifications queued per observer before it must resynchronize.
    pub observer_queue: usize,
    pub operations: OperationLimits,
    /// The Academy server origin (fixture builds may point it elsewhere).
    pub academy_origin: String,
    /// The management token, API and DLP endpoints (fixture builds may
    /// point them elsewhere).
    pub management: ManagementEndpoints,
    /// How long a prepared change review may be applied.
    pub review_lifetime: std::time::Duration,
    /// The public example catalog (fixture builds may point it at a local
    /// snapshot server).
    pub case_catalog: url::Url,
}

/// The production Academy origin.
pub const ACADEMY_ORIGIN: &str = "https://academy.cdot.io";

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            retained_results: 256,
            observer_queue: 256,
            operations: OperationLimits::default(),
            academy_origin: ACADEMY_ORIGIN.to_owned(),
            management: ManagementEndpoints::default(),
            review_lifetime: change_domain::REVIEW_LIFETIME,
            case_catalog: url::Url::parse(crate::library::PUBLIC_CATALOG).expect("catalog URL"),
        }
    }
}

/// Opens a verified HTTPS page in the operator's browser.
pub trait UrlOpener: Send + Sync + 'static {
    /// True when the page was handed to the browser (or recorded).
    fn open(&self, url: &url::Url) -> bool;
}

/// Opens the native print sheet for the main window's page (host adapter).
pub trait PagePrinter: Send + Sync + 'static {
    /// True when the sheet was opened (or, in fixture builds, recorded).
    fn print(&self) -> bool;
}

/// The native image picker for the learner's avatar (host adapter).
pub trait ImagePicker: Send + Sync + 'static {
    /// `None` when the operator cancelled or no sheet could open. Reads are
    /// bounded by the host to the avatar cap.
    fn choose_image(
        &self,
    ) -> futures_util::future::BoxFuture<'static, Option<Result<ImportFile, ImportError>>>;
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

/// What the native operator confirmation shows: Rust's stored review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationRequest {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    /// Delete and rotate are shown as destructive.
    pub destructive: bool,
}

/// The native operator confirmation (ADR 0001): a sheet on the main window.
/// Its completion is private to the host and never crosses the renderer
/// boundary; the future resolves true only when the operator chose the
/// confirming action.
pub trait OperatorConfirmation: Send + Sync + 'static {
    fn confirm(
        &self,
        nonce: u64,
        request: ConfirmationRequest,
    ) -> futures_util::future::BoxFuture<'static, bool>;
    /// Ends confirmation `nonce` if it is still shown (resolving it false).
    /// Called with the authority locked: it must not block.
    fn dismiss(&self, nonce: u64);
}

/// The transient native sheet for a one-time secret (never a snapshot).
pub trait SecretPresenter: Send + Sync + 'static {
    /// Shows `secret` under `label` with an explicit copy action; the host
    /// owns its expiry and clipboard cleanup.
    fn reveal(&self, label: String, secret: Zeroizing<String>);
    /// Ends any shown secret sheet and zeroizes its copy. Must not block.
    fn dismiss(&self);
}

/// The native save sheet (host adapter): writes `bytes` owner-only to the
/// file the operator chooses (replacing one only when the sheet confirmed
/// it). Resolves `Ok(None)` when cancelled, else the saved file's name.
pub trait FileSaver: Send + Sync + 'static {
    fn save(
        &self,
        suggested: String,
        bytes: Vec<u8>,
    ) -> futures_util::future::BoxFuture<'static, Result<Option<String>, ()>>;
}

/// Keychain operations the core needs beyond single records.
pub trait CredentialStore: Vault {
    /// Deletes every record in this build's v2 credential namespace.
    fn purge(&self) -> Result<(), VaultError>;
}

/// OS adapters injected by the native host.
pub struct Adapters {
    pub keychain: Arc<dyn CredentialStore>,
    /// The Academy learner namespace (refresh token of the signed-in
    /// learner); purged with the scan namespace on reset.
    pub learner_keychain: Arc<dyn CredentialStore>,
    pub store: Arc<dyn AppStore>,
    pub picker: Arc<dyn ImportPicker>,
    pub opener: Arc<dyn UrlOpener>,
    pub printer: Arc<dyn PagePrinter>,
    pub images: Arc<dyn ImagePicker>,
    pub confirmer: Arc<dyn OperatorConfirmation>,
    pub secrets: Arc<dyn SecretPresenter>,
    /// The native file sheet for dictionary keyword files (bounded reads).
    pub text_files: Arc<dyn ImportPicker>,
    /// Where account-scoped encrypted offline work lives (None: unavailable).
    pub offline_root: Option<std::path::PathBuf>,
    /// The native save sheet for report and investigation exports (C11).
    pub saver: Arc<dyn FileSaver>,
    /// Where sealed investigations and report files live (None: reports
    /// unavailable, investigations kept in memory only).
    pub sessions_root: Option<std::path::PathBuf>,
    /// The native file sheet for case imports (bounded reads).
    pub case_files: Arc<dyn ImportPicker>,
    /// Where sealed case libraries live (None: the Library is unavailable).
    pub library_root: Option<std::path::PathBuf>,
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
    /// The management OAuth client secret (destination: the token origin).
    management: ScanCapability,
    /// The management OAuth client ID that goes with it.
    management_client: Option<String>,
}

impl Tenant {
    fn new(profile: TenantProfile) -> Self {
        Self {
            profile,
            scan: ScanCapability::Missing,
            management: ScanCapability::Missing,
            management_client: None,
        }
    }

    fn capability(&self, class: CredentialClass) -> &ScanCapability {
        match class {
            CredentialClass::Scan => &self.scan,
            CredentialClass::Management => &self.management,
        }
    }

    fn capability_mut(&mut self, class: CredentialClass) -> &mut ScanCapability {
        match class {
            CredentialClass::Scan => &mut self.scan,
            CredentialClass::Management => &mut self.management,
        }
    }
}

/// Everything persisted, plus capability state that rides with it.
#[derive(Clone, Debug, Default)]
struct Data {
    appearance: Option<Theme>,
    tenants: Vec<Tenant>,
    active: Option<TenantId>,
    pending_cleanup: Vec<RecordId>,
    learner: Option<StoredLearner>,
    learner_cleanup: Vec<RecordId>,
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
    scans: Vec<ScanRecord>,
    /// One SDK session per issued context; cancelled when it retires.
    scan_sessions: Vec<(ContextRef, Session)>,
    /// The management endpoints credentials are bound to.
    management_endpoints: ManagementEndpoints,
    /// One management binding per issued context; cancelled when it retires.
    management_bindings: Vec<management_domain::GenerationBinding>,
    management_calls: Vec<management_domain::ManagementRecord>,
    /// The open Runtime change review (one at a time).
    change_review: Option<change_domain::ChangeReview>,
    change_results: Vec<change_domain::ChangeRecord>,
    /// The dictionary keyword file being chosen or staged.
    staging: Option<change_domain::Staging>,
    confirmer: Arc<dyn OperatorConfirmation>,
    secrets: Arc<dyn SecretPresenter>,
    review_lifetime: std::time::Duration,
    academy_origin: String,
    learner: LearnerPhase,
    learner_ref: Option<LearnerRef>,
    learner_epoch: u64,
    /// Sign-in flows are numbered; a newer one supersedes older tasks.
    learner_flow: u64,
    learner_session: Option<Arc<LearnerSession>>,
    academy_calls: Vec<learner_domain::CallRecord>,
    /// Learner records being written or revoked; cleanup leaves them alone
    /// and reset waits for their writes.
    learner_busy: Vec<RecordId>,
    /// Learner records whose Keychain write has not settled (reset waits).
    learner_writes: Vec<RecordId>,
    /// Admitted offline store operations (reset waits for none).
    offline_writers: usize,
    learner_cleaning: bool,
    /// The signed-in generation's offline work (dropped with it).
    offline: Option<Arc<offline_domain::OfflineGen>>,
    sessions_root: Option<std::path::PathBuf>,
    /// Every opened tenant's FP/FN reports (C11), each with its own pending
    /// writes.
    reports: Vec<sessions_domain::Reports>,
    /// The last report or investigation export.
    export: Option<sessions_domain::Export>,
    /// Report and case Library file writes, one at a time (tenant deletion
    /// and reset take it too).
    report_writer: Arc<tokio::sync::Mutex<()>>,
    library_root: Option<std::path::PathBuf>,
    /// Every opened tenant's case Library (C12).
    libraries: Vec<library_domain::Library>,
    case_catalog: url::Url,
    /// The active context's last Runtime overview (C12).
    overview: Option<observation_domain::OverviewRecord>,
    /// Attention holds per tenant (memory only, like v1).
    holds: Vec<(TenantId, Vec<observation_domain::Hold>)>,
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
            scans: self.scan_views(),
            academy: self.academy_view(),
            learner: self.learner_ref.clone(),
            academy_origin: self.academy_origin.clone(),
            academy_offline: self
                .offline
                .as_ref()
                .map_or_else(offline_domain::OfflineView::signed_out, |o| o.view()),
            changes: self.changes_view(),
            sessions: self.sessions_view(),
            library: self.library_view(),
            overview: self.overview_view(),
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
                .flat_map(|t| {
                    [CredentialClass::Scan, CredentialClass::Management]
                        .into_iter()
                        .filter_map(move |class| match t.capability(class) {
                            ScanCapability::Ready {
                                record,
                                mode: StorageMode::Keychain,
                                destination,
                            } => Some(StoredCredential {
                                record: record.clone(),
                                tenant: t.profile.id.clone(),
                                destination: destination.clone(),
                                class,
                                client_id: match class {
                                    CredentialClass::Scan => None,
                                    CredentialClass::Management => t.management_client.clone(),
                                },
                            }),
                            _ => None,
                        })
                })
                .collect(),
            pending_cleanup: self.data.pending_cleanup.clone(),
            learner: self.data.learner.clone(),
            learner_cleanup: self.data.learner_cleanup.clone(),
            ..PersistedState::default()
        }
    }

    fn lease(&mut self) -> u64 {
        self.next_lease += 1;
        self.next_lease
    }

    fn changed(&mut self) {
        // Every published snapshot reflects settled scans' outcomes.
        self.sync_scans();
        self.sync_academy_calls();
        self.sync_management_calls();
        self.sync_changes();
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
        let starts =
            self.operations
                .retire_contexts(ContextRef::PREFIX, keep.as_deref(), Instant::now());
        self.retire_scan_sessions();
        self.retire_management_bindings();
        self.sync_scans();
        self.sync_management_calls();
        self.sync_changes();
        starts
    }

    fn scan_session(&mut self, context: &ContextRef) -> Session {
        if let Some((_, session)) = self.scan_sessions.iter().find(|(c, _)| c == context) {
            return session.clone();
        }
        let session = Session::new();
        self.scan_sessions.push((context.clone(), session.clone()));
        session
    }

    fn retire_scan_sessions(&mut self) {
        let keep = self.active_ref.clone();
        self.scan_sessions.retain(|(context, session)| {
            let current = Some(context) == keep.as_ref();
            if !current {
                session.cancel();
            }
            current
        });
    }

    /// Moves settled operations' payloads into their records. A reply for a
    /// context that is no longer current is discarded, never shown.
    fn sync_scans(&mut self) {
        let current = self.active_ref.clone();
        for record in self.scans.iter_mut().filter(|r| r.settlement.is_none()) {
            record.settlement = match self.operations.state(&record.operation) {
                Some(OperationState::Settled(s)) => Some(s),
                // Unknown to the runtime: nothing is known of its effect.
                None => Some(runtime::Settlement {
                    outcome: runtime::Outcome::Unknown,
                    stop: None,
                }),
                Some(_) => continue,
            };
            let payload = record.slot.lock().ok().and_then(|mut s| s.take());
            if Some(&record.context) == current.as_ref() {
                record.payload = payload;
            } else {
                record.discarded = payload.is_some();
            }
        }
        // Per tenant: every unsettled scan and the newest settled ones.
        let mut kept: Vec<(TenantId, usize)> = Vec::new();
        let mut keep = vec![true; self.scans.len()];
        for (i, record) in self.scans.iter().enumerate().rev() {
            if record.settlement.is_none() {
                continue;
            }
            let count = match kept.iter_mut().find(|(t, _)| t == &record.tenant) {
                Some((_, n)) => {
                    *n += 1;
                    *n
                }
                None => {
                    kept.push((record.tenant.clone(), 1));
                    1
                }
            };
            keep[i] = count <= HISTORY_PER_TENANT;
        }
        let mut index = 0;
        self.scans.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }

    fn scan_views(&self) -> Vec<ScanRecordView> {
        let Some(active) = &self.data.active else {
            return Vec::new();
        };
        self.scans
            .iter()
            .rev()
            .filter(|r| &r.tenant == active)
            .map(|r| {
                let (progress, outcome, stop) =
                    match (r.settlement, self.operations.state(&r.operation)) {
                        (Some(s), _) => (ScanProgress::Settled, Some(s.outcome), s.stop),
                        (None, Some(OperationState::DispatchClaimed)) => {
                            (ScanProgress::Running, None, None)
                        }
                        (None, _) => (ScanProgress::Queued, None, None),
                    };
                let (result, failure) = match &r.payload {
                    Some(ScanPayload::Result(view)) => (Some((**view).clone()), None),
                    Some(ScanPayload::Failure(failure)) => (None, Some(*failure)),
                    None => (None, None),
                };
                ScanRecordView {
                    operation: r.operation.clone(),
                    progress,
                    outcome,
                    stop,
                    result,
                    failure,
                    discarded: r.discarded,
                }
            })
            .collect()
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
        self.retire_class(tenant, CredentialClass::Scan)
    }

    /// [`State::retire_credential`] for either credential class.
    fn retire_class(&mut self, tenant: &TenantId, class: CredentialClass) -> Option<RecordId> {
        let t = self.tenant_mut(tenant)?;
        if class == CredentialClass::Management {
            t.management_client = None;
        }
        match std::mem::replace(t.capability_mut(class), ScanCapability::Missing) {
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
        let starts = self.operations.retire_contexts("", None, Instant::now());
        self.retire_scan_sessions();
        self.retire_management_bindings();
        self.sync_scans();
        self.sync_management_calls();
        self.sync_changes();
        starts
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
        management: {
            let (state, mode, failure, token_origin) = match &t.management {
                ScanCapability::Missing => (ScanState::Missing, None, None, None),
                ScanCapability::Storing { mode, .. } => {
                    (ScanState::Storing, Some(*mode), None, None)
                }
                ScanCapability::Ready {
                    mode, destination, ..
                } => (
                    ScanState::Ready,
                    Some(*mode),
                    None,
                    Some(destination.clone()),
                ),
                ScanCapability::Failed { mode, failure } => {
                    (ScanState::Failed, Some(*mode), Some(*failure), None)
                }
            };
            ManagementView {
                state,
                mode,
                failure,
                client_id: t.management_client.clone(),
                token_origin,
            }
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
        class: CredentialClass,
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
    AcademySignIn {
        flow: u64,
        client: AcademyClient,
    },
    AcademySignedOut {
        session: Option<Arc<LearnerSession>>,
        record: Option<RecordId>,
    },
    Open(url::Url),
    /// Open a signed-in generation's offline work.
    OpenOffline(Arc<offline_domain::OfflineGen>),
    /// Present the native operator confirmation for a change review.
    Confirm {
        review: ReviewId,
        nonce: u64,
        request: ConfirmationRequest,
    },
    RevealSecret {
        label: String,
        secret: Zeroizing<String>,
    },
    ChooseTextFile {
        lease: u64,
    },
    LoadReports {
        lease: u64,
    },
    StoreReports {
        tenant: TenantId,
        generation: u64,
    },
    /// Remove a deleted tenant's report file.
    ForgetReports(std::path::PathBuf),
    SaveFile {
        lease: u64,
        suggested: String,
        bytes: Vec<u8>,
    },
    ClearInvestigation(Arc<crate::sessions::Investigation>),
    LoadLibrary {
        lease: u64,
    },
    StoreLibrary {
        tenant: TenantId,
        generation: u64,
    },
    /// Remove a deleted tenant's sealed library and its key.
    ForgetLibrary(TenantId),
    ChooseCaseFile {
        lease: u64,
    },
    FetchCatalog {
        lease: u64,
        url: url::Url,
    },
}

struct Inner {
    state: Mutex<State>,
    adapters: Adapters,
    session: Arc<SessionVault>,
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
            scans: Vec::new(),
            scan_sessions: Vec::new(),
            management_endpoints: config.management,
            management_bindings: Vec::new(),
            management_calls: Vec::new(),
            change_review: None,
            change_results: Vec::new(),
            staging: None,
            confirmer: Arc::clone(&adapters.confirmer),
            secrets: Arc::clone(&adapters.secrets),
            review_lifetime: config.review_lifetime,
            academy_origin: config.academy_origin,
            learner: LearnerPhase::SignedOut,
            learner_ref: None,
            learner_epoch: 0,
            learner_flow: 0,
            learner_session: None,
            academy_calls: Vec::new(),
            learner_busy: Vec::new(),
            learner_writes: Vec::new(),
            offline_writers: 0,
            learner_cleaning: false,
            offline: None,
            sessions_root: adapters.sessions_root.clone(),
            reports: Vec::new(),
            export: None,
            report_writer: Arc::default(),
            library_root: adapters.library_root.clone(),
            libraries: Vec::new(),
            case_catalog: config.case_catalog.clone(),
            overview: None,
            holds: Vec::new(),
        };
        let _ = state.reissue_context();
        let inner = Arc::new(Inner {
            state: Mutex::new(state),
            adapters,
            session: Arc::new(SessionVault::new()),
        });
        if let Ok(mut state) = lock(&inner) {
            // A saved learner resumes signed in; its session renews the
            // access token from the Keychain on first use.
            if state.data.learner.is_some() {
                state.learner = LearnerPhase::SignedIn;
                state.restore_learner_session(&inner);
            } else {
                // A guest generation for public catalog reads.
                let _ = state.reissue_learner();
            }
        }
        Self { inner }
    }

    /// Makes the signed-in generation's offline store fail its next write at
    /// `fault` (a `DirectorySync` fault persists until cleared with `None`)
    /// (fixture builds only).
    #[cfg(feature = "test-fixtures")]
    /// Holds every report file write (and reset's purge) until the returned
    /// guard drops (fixture builds only).
    pub async fn hold_report_writes(&self) -> tokio::sync::OwnedMutexGuard<()> {
        let writer = lock(&self.inner)
            .map(|s| Arc::clone(&s.report_writer))
            .expect("authority");
        writer.lock_owned().await
    }

    #[cfg(feature = "test-fixtures")]
    pub fn inject_offline_fault(&self, fault: Option<crate::offline::WriteFault>) {
        let store = lock(&self.inner)
            .ok()
            .and_then(|s| s.offline.clone())
            .and_then(|o| o.store());
        if let Some(store) = store {
            match fault {
                Some(fault) => store.inject(fault),
                None => store.clear_fault(),
            }
        }
    }

    /// Locks offline work as the application exits (its store's instance
    /// lock is released once no job holds it). Call once, at exit.
    pub fn shutdown(&self) {
        if let Ok(mut state) = lock(&self.inner) {
            state.offline = None;
            state.changed();
        }
    }

    /// Resumes durable work after launch (pending Keychain cleanup). Call on
    /// the application runtime.
    pub fn start(&self) {
        let offline = lock(&self.inner).ok().and_then(|s| s.offline.clone());
        let mut effects = vec![Effect::Cleanup];
        effects.extend(offline.map(Effect::OpenOffline));
        run_effects(&self.inner, effects);
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
            let previous = state.offline.clone();
            let mut starts = state.reissue_context();
            // Learner work moves to a client under the new policy.
            if matches!(state.learner, LearnerPhase::SignedIn) {
                starts.extend(state.restore_learner_session(&self.inner));
            }
            state.changed();
            // The same account keeps its open (or, before `start`, its
            // pending) offline work; only a new generation is opened here.
            let fresh = state
                .offline
                .clone()
                .filter(|o| !previous.as_ref().is_some_and(|p| Arc::ptr_eq(p, o)));
            (starts, fresh)
        };
        let (starts, offline) = starts;
        let mut effects = vec![Effect::Start(starts)];
        effects.extend(offline.map(Effect::OpenOffline));
        run_effects(&self.inner, effects);
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
                .find(|c| {
                    c.class == CredentialClass::Scan
                        && c.tenant == profile.id
                        && c.destination == profile.destination.origin()
                })
                .map(|c| ScanCapability::Ready {
                    record: c.record.clone(),
                    mode: StorageMode::Keychain,
                    destination: c.destination.clone(),
                })
                .unwrap_or(ScanCapability::Missing);
            let management = doc
                .credentials
                .iter()
                .find(|c| c.class == CredentialClass::Management && c.tenant == profile.id);
            let mut tenant = Tenant::new(profile);
            tenant.scan = scan;
            if let Some(c) = management {
                tenant.management = ScanCapability::Ready {
                    record: c.record.clone(),
                    mode: StorageMode::Keychain,
                    destination: c.destination.clone(),
                };
                tenant.management_client = c.client_id.clone();
            }
            tenant
        })
        .collect();
    Data {
        appearance: Some(doc.appearance),
        tenants,
        active: doc.active_tenant,
        pending_cleanup: doc.pending_cleanup,
        learner: doc.learner,
        learner_cleanup: doc.learner_cleanup,
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
                    let result = admit(
                        &mut state,
                        &self.inner.session,
                        &self.inner.adapters,
                        intent,
                        &mut effects,
                    );
                    if result.is_err() {
                        effects.clear();
                    }
                    // Learner records wait in the ledger for a confirmed
                    // save; once one has happened, let cleanup resume.
                    if !state.durability_unconfirmed && !state.data.learner_cleanup.is_empty() {
                        effects.push(Effect::Cleanup);
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
            ViewScope::AcademyCall(operation) => Ok(Observation::AcademyCall(
                state.observe_academy_call(&operation)?,
            )),
            ViewScope::Management(operation) => Ok(Observation::Management(
                state.observe_management(&operation)?,
            )),
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
    session: &Arc<SessionVault>,
    adapters: &Adapters,
    intent: Intent,
    effects: &mut Vec<Effect>,
) -> Result<Receipt, Refusal> {
    let mut operation_ref: Option<OperationId> = None;
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
            state.data.tenants.push(Tenant::new(TenantProfile {
                id: id.clone(),
                label: profile.label,
                tsg_id: profile.tsg_id,
                destination: profile.destination,
            }));
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
            let retired_management = state.retire_class(&tenant, CredentialClass::Management);
            state.data.tenants.retain(|t| t.profile.id != tenant);
            if was_active {
                state.data.active = None;
            }
            state.save_with(&before)?;
            // Only an accepted deletion drops the tenant's scan records; a
            // refused one keeps them observable and cancellable.
            state.scans.retain(|r| r.tenant != tenant);
            if let Some(path) = state.forget_reports(&tenant) {
                effects.push(Effect::ForgetReports(path));
            }
            state.forget_library(&tenant);
            state.forget_holds(&tenant);
            effects.push(Effect::ForgetLibrary(tenant.clone()));
            for record in [retired_session, retired_management].into_iter().flatten() {
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
            if key.expose().is_empty()
                || key.expose().len() > MAX_SECRET_BYTES
                || !crate::scan::key_is_usable(key.expose())
            {
                return Err(Refusal::InvalidInput);
            }
            configure_credential(
                state,
                session,
                effects,
                &context,
                CredentialClass::Scan,
                mode,
                key,
                None,
            )?;
        }
        Intent::ConfigureManagementCredential {
            context,
            mode,
            client_id,
            secret,
        } => {
            let tenant = state.check_context(&context)?;
            // Management needs the tenant's TSG and its own OAuth client; the
            // client secret goes only to the configured token origin.
            let tsg_ok = state
                .tenant(&tenant)
                .and_then(|t| t.profile.tsg_id.as_deref())
                .is_some_and(crate::management::tsg_is_valid);
            if !tsg_ok {
                return Err(Refusal::CapabilityMissing);
            }
            if !crate::management::client_id_is_valid(&client_id)
                || secret.expose().is_empty()
                || secret.expose().len() > MAX_SECRET_BYTES
                || secret.expose().chars().any(char::is_control)
                || state.management_endpoints.token_origin().is_none()
            {
                return Err(Refusal::InvalidInput);
            }
            configure_credential(
                state,
                session,
                effects,
                &context,
                CredentialClass::Management,
                mode,
                secret,
                Some(client_id),
            )?;
        }
        Intent::RemoveManagementCredential { context } => {
            remove_credential(
                state,
                session,
                effects,
                &context,
                CredentialClass::Management,
            )?;
        }
        Intent::ReviewChange { context, draft } => {
            let (operation, starts) = state.review_change(
                context,
                draft,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            operation_ref = operation;
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::RequestApply {
            review,
            draft,
            confirmation,
        } => {
            let (nonce, request) = state.request_apply(&review, &draft, &confirmation)?;
            effects.push(Effect::Confirm {
                review,
                nonce,
                request,
            });
            state.changed();
        }
        Intent::ChooseDictionaryFile { context } => {
            let lease = state.lease();
            state.choose_dictionary_file(context, lease)?;
            effects.push(Effect::ChooseTextFile { lease });
            state.changed();
        }
        Intent::DiscardChange { review } => {
            state.discard_change(&review)?;
            state.changed();
        }
        Intent::RevealSecret { operation } => {
            let (label, secret) = state.reveal_secret(&operation)?;
            effects.push(Effect::RevealSecret { label, secret });
            // The key is no longer held: publish that, or the presentation
            // keeps offering a reveal that will be refused.
            state.changed();
        }
        Intent::ForgetSecret { operation } => {
            state.forget_secret(&operation)?;
            state.changed();
        }
        Intent::OpenReports { context } => {
            if let Some(lease) = state.open_reports(&context)? {
                effects.push(Effect::LoadReports { lease });
            }
            state.changed();
        }
        Intent::SaveReport {
            context,
            target,
            input,
        } => {
            let saved = state.save_report(&context, target, input);
            // A refused save still publishes its problem.
            state.changed();
            let (tenant, generation) = saved?;
            effects.push(Effect::StoreReports { tenant, generation });
        }
        Intent::RemoveReport {
            context,
            scan_id,
            sub_request,
        } => {
            let (tenant, generation) = state.remove_report(&context, &scan_id, sub_request)?;
            effects.push(Effect::StoreReports { tenant, generation });
            state.changed();
        }
        Intent::UndoReport { context } => {
            let (tenant, generation) = state.undo_report(&context)?;
            effects.push(Effect::StoreReports { tenant, generation });
            state.changed();
        }
        Intent::ExportReports { context } => {
            let (lease, suggested, bytes) = state.export_reports(&context)?;
            effects.push(Effect::SaveFile {
                lease,
                suggested,
                bytes,
            });
            state.changed();
        }
        Intent::ExportInvestigation { context } => {
            let (lease, suggested, bytes) = state.export_investigation(&context)?;
            effects.push(Effect::SaveFile {
                lease,
                suggested,
                bytes,
            });
            state.changed();
        }
        Intent::OpenLibrary { context } => {
            if let Some(lease) = state.open_library(&context)? {
                effects.push(Effect::LoadLibrary { lease });
            }
            state.changed();
        }
        Intent::SaveCase { context, draft } => {
            let saved = state.save_case(&context, draft);
            state.changed();
            let (write, load) = saved?;
            if let Some((tenant, generation)) = write {
                effects.push(Effect::StoreLibrary { tenant, generation });
            }
            if let Some(lease) = load {
                effects.push(Effect::LoadLibrary { lease });
            }
        }
        Intent::DeleteCase { context, id } => {
            let deleted = state.delete_case(&context, &id);
            state.changed();
            let (tenant, generation) = deleted?;
            effects.push(Effect::StoreLibrary { tenant, generation });
        }
        Intent::ImportCases { context } => {
            let lease = state.begin_case_import(&context)?;
            effects.push(Effect::ChooseCaseFile { lease });
            state.changed();
        }
        Intent::ImportExamples { context, source } => match source {
            CatalogSource::Bundled => {
                let added = state.import_bundled(&context);
                state.changed();
                let (tenant, generation) = added?;
                effects.push(Effect::StoreLibrary { tenant, generation });
            }
            CatalogSource::Public => {
                let (lease, url) = state.begin_catalog(&context)?;
                effects.push(Effect::FetchCatalog { lease, url });
                state.changed();
            }
        },
        Intent::CopyCase {
            context,
            id,
            tenant,
        } => {
            let copied = state.copy_case(&context, &id, &tenant);
            state.changed();
            let (write, load) = copied?;
            if let Some((tenant, generation)) = write {
                effects.push(Effect::StoreLibrary { tenant, generation });
            }
            if let Some(lease) = load {
                effects.push(Effect::LoadLibrary { lease });
            }
        }
        Intent::ExportCases { context } => {
            let (lease, suggested, bytes) = state.export_cases(&context)?;
            effects.push(Effect::SaveFile {
                lease,
                suggested,
                bytes,
            });
            state.changed();
        }
        Intent::HoldInsight { context, id, hold } => {
            state.hold_insight(&context, &id, hold)?;
            state.changed();
        }
        Intent::RestoreInsight { context, id } => {
            state.restore_insight(&context, &id)?;
            state.changed();
        }
        Intent::ClearInvestigation { context } => {
            state.check_context(&context)?;
            let investigation = state.investigation(&context).ok_or(Refusal::InvalidInput)?;
            effects.push(Effect::ClearInvestigation(investigation));
        }
        Intent::Management { context, request } => {
            let (operation, starts) = state.admit_management(
                context,
                request,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::RemoveScanCredential { context } => {
            remove_credential(state, session, effects, &context, CredentialClass::Scan)?;
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
                state.data.tenants.push(Tenant::new(TenantProfile {
                    id: TenantId::issue().ok_or(Refusal::ShuttingDown)?,
                    label: profile.label,
                    tsg_id: profile.tsg_id,
                    destination: profile.destination,
                }));
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
                .flat_map(|id| {
                    [
                        state.retire_credential(id),
                        state.retire_class(id, CredentialClass::Management),
                    ]
                })
                .flatten()
                .collect();
            // The Academy learner is signed out with everything else; its
            // namespace is purged by the reset below (after admitted learner
            // writes settle).
            if let Some(stored) = state.data.learner.take() {
                state.data.learner_cleanup.push(stored.record);
            }
            if state.storage == StorageHealth::Ready {
                state.save_with(&before)?;
            }
            let (learner_starts, _) = state.sign_out_learner();
            effects.push(Effect::Start(learner_starts));
            for record in retired_sessions {
                let _ = session.delete(&record);
            }
            session.clear();
            state.storage = StorageHealth::Resetting;
            state.reset_failure = None;
            state.import = ImportState::Idle;
            // Report books go with the data; pending writes find nothing.
            state.reports.clear();
            state.libraries.clear();
            state.holds.clear();
            state.overview = None;
            state.export = None;
            effects.push(Effect::Start(state.retire_all_contexts()));
            effects.push(Effect::Reset);
            state.changed();
        }
        Intent::StartAcademySignIn => {
            state.require_storage()?;
            if state.learner.flow().is_some() {
                return Err(Refusal::Busy);
            }
            if matches!(state.learner, LearnerPhase::SignedIn) {
                return Err(Refusal::CapabilityMissing);
            }
            let client = state.academy_client().ok_or(Refusal::CapabilityMissing)?;
            // Signing in again over a saved account whose session failed
            // first signs that account out (its record joins the ledger).
            let retired = state.retire_saved_learner()?;
            let (starts, old) = state.sign_out_learner();
            effects.push(Effect::Start(starts));
            if old.is_some() || retired.is_some() {
                effects.push(Effect::AcademySignedOut {
                    session: old,
                    record: retired,
                });
            }
            let flow = state.learner_flow;
            state.learner = LearnerPhase::Starting { flow };
            effects.push(Effect::AcademySignIn { flow, client });
            state.changed();
        }
        Intent::CancelAcademySignIn => {
            if state.learner.flow().is_none() {
                return Err(Refusal::InvalidInput);
            }
            state.learner_flow += 1;
            state.learner = LearnerPhase::SignedOut;
            state.changed();
        }
        Intent::OpenAcademyVerification => match &state.learner {
            LearnerPhase::Awaiting { verification, .. } => {
                effects.push(Effect::Open(verification.clone()));
            }
            _ => return Err(Refusal::StaleContext),
        },
        Intent::PrintPage { learner } => {
            let (operation, starts) = state.admit_print(learner, Arc::clone(&adapters.printer))?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::UploadAvatar { learner } => {
            let (operation, starts) =
                state.admit_avatar_upload(learner, Arc::clone(&adapters.images))?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::DownloadCourse { learner, course_id } => {
            let (operation, starts) =
                state.admit_offline(learner, learner_domain::OfflineOp::Download(course_id))?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::RemoveDownload { learner, course_id } => {
            let (operation, starts) =
                state.admit_offline(learner, learner_domain::OfflineOp::Remove(course_id))?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::SaveLocalDraft {
            learner,
            attempt_id,
            base_version,
            draft,
        } => {
            let (operation, starts) = state.admit_offline(
                learner,
                learner_domain::OfflineOp::SaveDraft {
                    attempt_id,
                    base_version,
                    draft,
                },
            )?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::LoadLocalDraft {
            learner,
            attempt_id,
        } => {
            let (operation, starts) =
                state.admit_offline(learner, learner_domain::OfflineOp::LoadDraft(attempt_id))?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::ClearLocalDraft {
            learner,
            attempt_id,
        } => {
            let (operation, starts) =
                state.admit_offline(learner, learner_domain::OfflineOp::ClearDraft(attempt_id))?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::LoadAcademyImage { learner, url } => {
            let (operation, starts) = state.admit_image_read(learner, &url)?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::SignOutAcademy => {
            let active = state.data.learner.is_some()
                || state.learner_session.is_some()
                || !matches!(state.learner, LearnerPhase::SignedOut);
            if !active {
                return Err(Refusal::InvalidInput);
            }
            let record = state.retire_saved_learner()?;
            let (starts, session) = state.sign_out_learner();
            effects.push(Effect::Start(starts));
            effects.push(Effect::AcademySignedOut { session, record });
            state.changed();
        }
        Intent::AcademyCall {
            learner,
            procedure,
            procedure_kind,
            input,
        } => {
            let (operation, starts) =
                state.admit_academy_call(learner, &procedure, procedure_kind, input)?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
        }
        Intent::Scan { context, draft } => {
            let tenant = state.check_context(&context)?;
            let draft = draft.validate().ok_or(Refusal::InvalidInput)?;
            let (record, mode, origin) =
                match &state.tenant(&tenant).ok_or(Refusal::StaleContext)?.scan {
                    ScanCapability::Ready {
                        record,
                        mode,
                        destination,
                    } => (record.clone(), *mode, destination.clone()),
                    _ => return Err(Refusal::CapabilityMissing),
                };
            // System proxy mode builds its client here, per admission.
            let transport = state
                .transport
                .clone()
                .ok_or(Refusal::CapabilityMissing)?
                .for_admission()
                .map_err(|_| Refusal::CapabilityMissing)?;
            let slot: PayloadSlot = Arc::default();
            let bytes = draft.queued_bytes();
            let job = ScanJob {
                keychain: Arc::clone(&adapters.keychain),
                session_vault: Arc::clone(session),
                record,
                mode,
                origin,
                transport,
                session: state.scan_session(&context),
                draft,
                slot: Arc::clone(&slot),
            };
            let (operation, starts) =
                state
                    .operations
                    .admit(context.as_str(), bytes, job.into_work(), Instant::now())?;
            state.scans.push(ScanRecord {
                operation: operation.clone(),
                tenant,
                context,
                slot,
                payload: None,
                discarded: false,
                settlement: None,
            });
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::CancelScan { operation } => {
            let record = state
                .scans
                .iter()
                .find(|r| r.operation == operation)
                .ok_or(Refusal::InvalidInput)?;
            // Only the current context's scans can be cancelled; an earlier
            // context's work was already retired with it.
            state.check_context(&record.context)?;
            let (_, starts) =
                state
                    .operations
                    .cancel(&operation, StopReason::Cancelled, Instant::now());
            state.sync_scans();
            effects.push(Effect::Start(starts));
            state.changed();
        }
    }
    state.admitted += 1;
    Ok(Receipt::Admitted {
        request: format!("rq-{}", state.admitted),
        operation: operation_ref,
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
                class,
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
                    settle_store(&inner, &tenant, class, lease, record, mode, result);
                });
            }
            Effect::Cleanup => {
                drain_cleanup(inner);
                learner_domain::drain_learner_cleanup(inner);
            }
            Effect::AcademySignIn { flow, client } => {
                tokio::spawn(learner_domain::sign_in(Arc::clone(inner), flow, client));
            }
            Effect::AcademySignedOut { session, record } => {
                tokio::spawn(learner_domain::signed_out(
                    Arc::clone(inner),
                    session,
                    record,
                ));
            }
            Effect::OpenOffline(generation) => {
                tokio::spawn(learner_domain::open_offline(Arc::clone(inner), generation));
            }
            Effect::Open(url) => {
                let inner = Arc::clone(inner);
                tokio::task::spawn_blocking(move || {
                    let _ = inner.adapters.opener.open(&url);
                });
            }
            Effect::Confirm {
                review,
                nonce,
                request,
            } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let accepted = inner.adapters.confirmer.confirm(nonce, request).await;
                    settle_confirmation(&inner, &review, nonce, accepted);
                });
            }
            Effect::ChooseTextFile { lease } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let chosen = inner.adapters.text_files.choose().await.map(|r| {
                        r.map(|file| (display_name(&file.name), file.bytes.to_vec()))
                            .map_err(|error| match error {
                                ImportError::TooLarge => StagedFileFailure::TooLarge,
                                ImportError::Empty => StagedFileFailure::Empty,
                                ImportError::Unsupported => StagedFileFailure::Unreadable,
                            })
                    });
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_dictionary_file(lease, chosen);
                        state.changed();
                    }
                });
            }
            Effect::RevealSecret { label, secret } => {
                inner.adapters.secrets.reveal(label, secret);
            }
            Effect::LoadReports { lease } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let Some(path) = lock(&inner).ok().and_then(|s| s.report_load_target(lease))
                    else {
                        return;
                    };
                    let loaded =
                        tokio::task::spawn_blocking(move || crate::sessions::load_reports(&path))
                            .await
                            .unwrap_or(Err(crate::sessions::ReportStoreFailure::Unavailable));
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_reports(lease, loaded);
                        state.changed();
                    }
                });
            }
            Effect::StoreReports { tenant, generation } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let Some(writer) = lock(&inner).ok().map(|s| Arc::clone(&s.report_writer))
                    else {
                        return;
                    };
                    let _write = writer.lock().await;
                    // Only the newest book is written; an older generation's
                    // write is skipped (the newer one follows it).
                    let Some((path, book)) = lock(&inner)
                        .ok()
                        .and_then(|s| s.report_write(&tenant, generation))
                    else {
                        return;
                    };
                    let stored = tokio::task::spawn_blocking(move || {
                        crate::sessions::store_reports(&path, &book).is_ok()
                    })
                    .await
                    .unwrap_or(false);
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_report_write(&tenant, generation, stored);
                        state.changed();
                    }
                });
            }
            Effect::ForgetReports(path) => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let Some(writer) = lock(&inner).ok().map(|s| Arc::clone(&s.report_writer))
                    else {
                        return;
                    };
                    let _write = writer.lock().await;
                    let _ = tokio::task::spawn_blocking(move || std::fs::remove_file(path)).await;
                });
            }
            Effect::SaveFile {
                lease,
                suggested,
                bytes,
            } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let saved = inner.adapters.saver.save(suggested, bytes).await;
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_export(lease, saved);
                        state.changed();
                    }
                });
            }
            Effect::LoadLibrary { lease } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let Some((root, tenant)) =
                        lock(&inner).ok().and_then(|s| s.library_load_target(lease))
                    else {
                        return;
                    };
                    let worker = Arc::clone(&inner);
                    let loaded = tokio::task::spawn_blocking(move || {
                        crate::library::load(&root, worker.adapters.keychain.as_ref(), &tenant)
                    })
                    .await
                    .unwrap_or(Err(crate::library::LibraryFailure::Unavailable));
                    let write = lock(&inner).ok().and_then(|mut state| {
                        let write = state.settle_library(lease, loaded);
                        state.changed();
                        write
                    });
                    if let Some((tenant, generation)) = write {
                        run_effects(&inner, vec![Effect::StoreLibrary { tenant, generation }]);
                    }
                });
            }
            Effect::StoreLibrary { tenant, generation } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let Some(writer) = lock(&inner).ok().map(|s| Arc::clone(&s.report_writer))
                    else {
                        return;
                    };
                    let _write = writer.lock().await;
                    // Only the newest library is written; a deleted tenant's
                    // or reset data's write finds nothing.
                    let Some(write) = lock(&inner)
                        .ok()
                        .and_then(|s| s.library_write(&tenant, generation))
                    else {
                        return;
                    };
                    let worker = Arc::clone(&inner);
                    let stored = tokio::task::spawn_blocking(move || {
                        crate::library::store(
                            &write.root,
                            worker.adapters.keychain.as_ref(),
                            &write.tenant,
                            &write.library,
                        )
                    })
                    .await
                    .unwrap_or(Err(crate::library::LibraryFailure::Unavailable));
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_library_write(&tenant, generation, stored);
                        state.changed();
                    }
                });
            }
            Effect::ForgetLibrary(tenant) => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let Some(writer) = lock(&inner).ok().map(|s| Arc::clone(&s.report_writer))
                    else {
                        return;
                    };
                    let _write = writer.lock().await;
                    let worker = Arc::clone(&inner);
                    let _ = tokio::task::spawn_blocking(move || {
                        crate::library::forget(
                            worker.adapters.library_root.as_deref(),
                            worker.adapters.keychain.as_ref(),
                            tenant.as_str(),
                        )
                    })
                    .await;
                });
            }
            Effect::ChooseCaseFile { lease } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let chosen = inner
                        .adapters
                        .case_files
                        .choose()
                        .await
                        .map(|r| r.ok().map(|file| file.bytes.to_vec()));
                    let write = lock(&inner).ok().and_then(|mut state| {
                        let write = state.settle_case_import(lease, chosen);
                        state.changed();
                        write
                    });
                    if let Some((tenant, generation)) = write {
                        run_effects(&inner, vec![Effect::StoreLibrary { tenant, generation }]);
                    }
                });
            }
            Effect::FetchCatalog { lease, url } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let transport = lock(&inner)
                        .ok()
                        .and_then(|s| s.transport.clone())
                        .and_then(|t| t.for_admission().ok());
                    let fetched = match transport {
                        Some(transport) => library_domain::fetch_catalog(&transport, &url).await,
                        None => None,
                    };
                    let write = lock(&inner).ok().and_then(|mut state| {
                        let write = state.settle_catalog(lease, fetched);
                        state.changed();
                        write
                    });
                    if let Some((tenant, generation)) = write {
                        run_effects(&inner, vec![Effect::StoreLibrary { tenant, generation }]);
                    }
                });
            }
            Effect::ClearInvestigation(investigation) => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    investigation.clear().await;
                    if let Ok(mut state) = lock(&inner) {
                        state.changed();
                    }
                });
            }
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
                    // No new credential or offline write can start while
                    // resetting; wait for admitted ones to settle so none
                    // lands after the purge.
                    loop {
                        let pending = lock(&inner)
                            .map(|s| {
                                s.writes_in_flight.len()
                                    + s.learner_writes.len()
                                    + s.offline_writers
                            })
                            .unwrap_or(0);
                        if pending == 0 {
                            break;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    // A report write already past its check finishes before the
                    // purge; none starts after it (the books were cleared).
                    let writer = lock(&inner).map(|s| Arc::clone(&s.report_writer)).ok();
                    let _write = match &writer {
                        Some(writer) => Some(writer.lock().await),
                        None => None,
                    };
                    let worker = Arc::clone(&inner);
                    let result = tokio::task::spawn_blocking(move || {
                        // No investigation key or snapshot is placed while the
                        // namespace and the sessions root are removed (every
                        // investigation was retired above).
                        let _placement = crate::sessions::placement();
                        for namespace in
                            [&worker.adapters.keychain, &worker.adapters.learner_keychain]
                        {
                            namespace.purge().map_err(|reason| ResetFailure {
                                stage: ResetStage::Credentials,
                                reason,
                            })?;
                        }
                        // Sealed investigations go with their keys (purged
                        // above), and report files with the tenants.
                        if let Some(root) = &worker.adapters.sessions_root {
                            match std::fs::remove_dir_all(root) {
                                Ok(()) => {}
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                                Err(_) => {
                                    return Err(ResetFailure {
                                        stage: ResetStage::Document,
                                        reason: VaultError::Unavailable,
                                    });
                                }
                            }
                        }
                        // Sealed case libraries go with their keys too.
                        if let Some(root) = &worker.adapters.library_root {
                            match std::fs::remove_dir_all(root) {
                                Ok(()) => {}
                                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                                Err(_) => {
                                    return Err(ResetFailure {
                                        stage: ResetStage::Document,
                                        reason: VaultError::Unavailable,
                                    });
                                }
                            }
                        }
                        // Offline work goes with its keys (purged above).
                        if let Some(root) = &worker.adapters.offline_root {
                            crate::offline::purge(root).map_err(|_| ResetFailure {
                                stage: ResetStage::Document,
                                reason: VaultError::Unavailable,
                            })?;
                        }
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
            let starts = state.operations.complete(id, outcome, now);
            state.sync_scans();
            state.changed();
            starts
        }
        Err(_) => return,
    };
    run_effects(inner, vec![Effect::Start(starts)]);
}

/// Stores a new credential of `class` for the active tenant, replacing any
/// previous one of that class (the other class is untouched). The record is
/// written ahead into the cleanup ledger and its secret stored only once
/// that entry is durable.
#[allow(clippy::too_many_arguments)]
fn configure_credential(
    state: &mut State,
    session: &Arc<SessionVault>,
    effects: &mut Vec<Effect>,
    context: &ContextRef,
    class: CredentialClass,
    mode: StorageMode,
    key: EnteredSecret,
    client_id: Option<String>,
) -> Result<(), Refusal> {
    let tenant = state.check_context(context)?;
    if mode == StorageMode::Keychain {
        state.require_storage()?;
    } else if state.storage == StorageHealth::Resetting {
        return Err(Refusal::Busy);
    }
    let current = state.tenant(&tenant).ok_or(Refusal::StaleContext)?;
    if matches!(current.capability(class), ScanCapability::Storing { .. }) {
        return Err(Refusal::Busy);
    }
    let record = RecordId::issue().ok_or(Refusal::ShuttingDown)?;
    let lease = state.lease();
    let before = state.data.clone();
    let retired_session = state.retire_class(&tenant, class);
    if let Some(t) = state.tenant_mut(&tenant) {
        *t.capability_mut(class) = ScanCapability::Storing {
            lease,
            record: record.clone(),
            mode,
        };
        if class == CredentialClass::Management {
            t.management_client = client_id;
        }
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
    let authorized =
        mode == StorageMode::Session || !state.durability_unconfirmed || state.confirm_durable();
    effects.push(Effect::Start(state.reissue_context()));
    if authorized {
        if mode == StorageMode::Keychain {
            state.writes_in_flight.push(record.clone());
        }
        effects.push(Effect::Store {
            tenant,
            class,
            lease,
            record,
            mode,
            key,
        });
    } else if let Some(t) = state.tenant_mut(&tenant) {
        // Nothing was written; the ledger entry settles as absent.
        *t.capability_mut(class) = ScanCapability::Failed {
            mode,
            failure: VaultError::Unavailable,
        };
    }
    effects.push(Effect::Cleanup);
    state.changed();
    Ok(())
}

/// Signs the active tenant's `class` capability out (the other class is
/// untouched).
/// The native confirmation's private completion: never reachable from the
/// renderer (it is the adapter future's own result).
fn settle_confirmation(inner: &Arc<Inner>, review: &ReviewId, nonce: u64, accepted: bool) {
    let starts = match lock(inner) {
        Ok(mut state) => {
            let starts = state.settle_confirmation(
                review,
                nonce,
                accepted,
                Arc::clone(&inner.adapters.keychain),
                Arc::clone(&inner.session),
            );
            state.changed();
            starts
        }
        Err(_) => return,
    };
    run_effects(inner, vec![Effect::Start(starts)]);
}

fn remove_credential(
    state: &mut State,
    session: &Arc<SessionVault>,
    effects: &mut Vec<Effect>,
    context: &ContextRef,
    class: CredentialClass,
) -> Result<(), Refusal> {
    let tenant = state.check_context(context)?;
    let has_keychain = matches!(
        state.tenant(&tenant).map(|t| t.capability(class)),
        Some(ScanCapability::Ready {
            mode: StorageMode::Keychain,
            ..
        })
    );
    if has_keychain {
        state.require_storage()?;
    }
    let before = state.data.clone();
    let retired_session = state.retire_class(&tenant, class);
    if state.storage == StorageHealth::Ready {
        state.save_with(&before)?;
    }
    if let Some(old) = retired_session {
        let _ = session.delete(&old);
    }
    effects.push(Effect::Start(state.reissue_context()));
    effects.push(Effect::Cleanup);
    state.changed();
    Ok(())
}

fn settle_store(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    class: CredentialClass,
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
        |t| matches!(t.capability(class), ScanCapability::Storing { lease: l, .. } if *l == lease),
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
    let destination = match class {
        CredentialClass::Scan => state
            .tenant(tenant)
            .map(|t| t.profile.destination.origin().to_owned())
            .unwrap_or_default(),
        CredentialClass::Management => state
            .management_endpoints
            .token_origin()
            .unwrap_or_default(),
    };
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
        *t.capability_mut(class) = next;
    }
    if mode == StorageMode::Keychain && state.save_with(&before).is_err() {
        // The record could not be committed: it stays in the ledger and the
        // capability fails explicitly instead of becoming usable unsaved.
        if let Some(t) = state.tenant_mut(tenant) {
            *t.capability_mut(class) = ScanCapability::Failed {
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
                    // A management credential's sealed investigation and its
                    // key go with it (absent for every other record).
                    let investigation = crate::sessions::forget(
                        worker.adapters.sessions_root.as_deref(),
                        worker.adapters.keychain.as_ref(),
                        r,
                    );
                    investigation
                        && matches!(
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
                state.scans.clear();
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
        let Ok(Observation::Shell { mut updates, .. }) = handle.observe(ViewScope::Shell) else {
            panic!("shell observation");
        };
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
