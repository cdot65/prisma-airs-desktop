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
mod diagnostics_domain;
mod gateway_domain;
mod gateway_files_domain;
mod gateway_local_domain;
mod harness_domain;
mod harness_history;
mod harness_identity;
mod harness_mcp;
mod harness_tools;
mod import_v1_domain;
mod inbox_domain;
mod inference_domain;
mod learner_domain;
mod library_domain;
mod management_domain;
mod observation_domain;
mod offline_domain;
mod rotation_domain;
mod sessions_domain;
mod setup_domain;
mod workbench_domain;

pub use change_domain::{
    ChangeFailure, ChangeResultView, ChangeReviewState, ChangeReviewView, ChangesView, Readback,
    SecretState, StagedFileFailure, StagedFileState, StagedFileView,
};
pub use gateway_domain::{GatewayView, SelectedWorkspace};
pub use learner_domain::{AcademyCallHandle, AcademyCallResult};
use learner_domain::{LearnerPhase, LearnerSession};
pub use library_domain::{CatalogSource, LibraryNotice, LibraryState, LibraryView};
pub use management_domain::ManagementHandle;
// Re-exported for the host and tests; the snapshot and intents name them by path.
#[allow(unused_imports)]
pub use inbox_domain::{
    InboxEntry, InboxKind, InboxTarget, InboxView, ProductRead, ProductReadState, ReadState,
    ReadStatus, SwitchResolution,
};
pub use observation_domain::{AttentionFace, HeldItem, HoldKind, OverviewView, SnoozeReceipt};
pub use offline_domain::{DownloadStatus, DownloadView, LocalDraftView, OfflineState, OfflineView};
pub use rotation_domain::{RotationStage, RotationView};
pub use sessions_domain::{
    ExportKind, ExportState, ExportView, InvestigationView, ReportTarget, ReportUndoView,
    ReportsState, ReportsView, SessionsView,
};
pub use workbench_domain::{
    CollectionView, ConflictKind, DocumentView, EnvironmentView, RecentView, Resolution,
    SecretView, TabView, WorkbenchHandle, WorkbenchProblem, WorkbenchRequest, WorkbenchResult,
    WorkbenchView,
};

use crate::academy::AcademyClient;
use crate::context::{ContextRef, EnteredSecret, LearnerRef, ReviewId, WorkspaceRef};
use crate::delivery::{Admit, Delivery, DeliverySession, Sessions};
use crate::features::{self, FeatureEntry, FeatureId};
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
    /// Choose the desktop presentation (C28, D058): the legacy shell or the
    /// v2 shell. Persisted; navigation falls back to the new variant's first
    /// sidebar destination.
    SetUiVariant {
        variant: features::UiVariant,
    },
    /// Make a destination the active one (C28). Refused unless the feature
    /// is in the current presentation's catalogue and available; the native
    /// menu, the sidebar and the palette all go through here.
    Navigate {
        feature: FeatureId,
    },
    /// Advance the first-run wizard (C28/C29): never back to `needed`.
    SetOnboarding {
        state: features::OnboardingState,
    },
    /// The "show the calls" preference (C28).
    SetShowCalls {
        on: bool,
    },
    /// Start a tenant setup (C29): a draft with its own context that the
    /// credential intents and `verifyTenantSetup` address. One at a time;
    /// `firstRun` only while no tenant exists.
    BeginTenantSetup {
        mode: features::SetupMode,
        profile: ProfileDraft,
    },
    /// Change the draft's profile. A new destination retires the draft's
    /// credentials (they were bound to the old origin).
    UpdateTenantSetup {
        context: ContextRef,
        profile: ProfileDraft,
    },
    /// Record the draft's chosen products (Runtime Security always).
    ChooseSetupProducts {
        context: ContextRef,
        products: Vec<features::ProductId>,
    },
    /// Verify the draft's current inputs over the admitted reads; the
    /// lines stream through `ShellSnapshot.setup.verify`.
    VerifyTenantSetup {
        context: ContextRef,
    },
    /// Commit the draft as a tenant (products, verify warnings, active
    /// selection, first-run onboarding) and import the chosen starter
    /// packs. Refused unless the current inputs were verified without a
    /// blocking failure (an optional service's failure is carried as a warning).
    CompleteTenantSetup {
        context: ContextRef,
        packs: Vec<features::ProductId>,
        /// Completion is a tenant switch (I1): a pending Harness decision
        /// refuses it until the operator resolves the conversation.
        #[serde(default)]
        resolution: Option<inbox_domain::SwitchResolution>,
    },
    /// Drop the draft and clean up its credentials. Nothing else changes.
    DiscardTenantSetup {
        context: ContextRef,
    },
    /// Create a tenant profile with no credential.
    CreateTenant {
        profile: ProfileDraft,
    },
    /// Make a tenant active; issues a new context reference.
    SelectTenant {
        tenant: TenantId,
        /// How a switch blocked by a pending Harness decision proceeds
        /// (C30, I1); absent, a pending decision refuses the switch.
        #[serde(default)]
        resolution: Option<inbox_domain::SwitchResolution>,
    },
    /// Close the switch guard without switching (C30).
    DismissSwitchGuard,
    /// Read the active tenant now: a finite read of every chosen product
    /// (the Runtime overview with the chosen Gateway workspace's day, the
    /// Red Team scans and statistics, the model and skill scans) (C30, I4).
    ReadTenant {
        context: ContextRef,
    },
    /// Read one chosen product again after a role was granted; the
    /// completion names the product (C30).
    RecheckProduct {
        context: ContextRef,
        product: features::ProductId,
    },
    /// The operator's statement that `application` now uses the key a
    /// rotation issued (C31, I7): recorded once, locally, with its time.
    AcknowledgeDeployment {
        context: ContextRef,
        application: String,
    },
    /// Set an Inbox entry aside until its level rises (C30, I11).
    AcknowledgeItem {
        context: ContextRef,
        id: String,
    },
    /// Set an Inbox entry aside for 24 hours; the return time is computed
    /// here and shown wherever the entry appears (C30, I11).
    SnoozeItem {
        context: ContextRef,
        id: String,
    },
    /// Bring a held entry back.
    UnsnoozeItem {
        context: ContextRef,
        id: String,
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
    /// Choose one of the Gateway workspaces the active context listed
    /// (C16); remembered for the tenant.
    SelectGatewayWorkspace {
        context: ContextRef,
        workspace: String,
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
    /// Stop waiting for an applying change (C17). Nothing sent is undone:
    /// a change cancelled before its write is reported not sent, one
    /// cancelled after as unknown (a provisioning is read back).
    CancelChange {
        operation: OperationId,
    },
    /// Open the active tenant's Gateway work kept on this Mac (C17).
    OpenGatewayLocal {
        context: ContextRef,
    },
    /// Keep a Gateway change draft on this Mac (no review or authority).
    SaveGatewayDraft {
        context: ContextRef,
        draft: gateway_local_domain::GatewayDraftInput,
    },
    /// Remove a kept Gateway draft or uncertain-change record.
    DiscardGatewayLocal {
        context: ContextRef,
        id: String,
    },
    /// Store the chosen workspace's inference key (C18), replacing any
    /// previous one; it is sent only to `endpoint`, only for that
    /// workspace's inference.
    #[serde(rename_all = "camelCase")]
    ConfigureInferenceCredential {
        context: ContextRef,
        mode: StorageMode,
        workspace: WorkspaceRef,
        endpoint: String,
        secret: EnteredSecret,
    },
    /// Remove the tenant's inference key (stops its run).
    RemoveInferenceCredential {
        context: ContextRef,
    },
    /// Run one streamed inference in the Playground (never retried).
    RunInference {
        context: ContextRef,
        workspace: WorkspaceRef,
        run: crate::gateway_inference::InferenceRun,
    },
    /// Stop the Playground's run (whether it was charged is not known).
    StopInference {
        operation: OperationId,
    },
    /// Clear a Playground run that has ended.
    ClearInference {
        operation: OperationId,
    },
    /// Start a native Harness conversation in the chosen workspace with
    /// its stored inference key (C19).
    StartHarness {
        context: ContextRef,
        workspace: WorkspaceRef,
    },
    /// Configure company sign-in for the chosen Gateway workspace: its
    /// runtime endpoint and the identity provider (C20).
    #[serde(rename_all = "camelCase")]
    ConfigureHarnessSignIn {
        context: ContextRef,
        workspace: WorkspaceRef,
        endpoint: String,
        issuer: String,
        client_id: String,
        audience: String,
    },
    /// Sign out and remove the company sign-in configuration.
    RemoveHarnessSignIn {
        context: ContextRef,
    },
    /// Choose the credential new conversations use.
    ChooseHarnessCredential {
        context: ContextRef,
        source: crate::harness::history::CredentialSource,
    },
    /// Sign in through the system browser.
    SignInHarness {
        context: ContextRef,
    },
    CancelHarnessSignIn {
        context: ContextRef,
    },
    /// Sign out: the grant ends and its conversation is retired.
    SignOutHarness {
        context: ContextRef,
    },
    /// Choose the Harness project folder in the native folder sheet (C21).
    ChooseHarnessProject {
        context: ContextRef,
    },
    ClearHarnessProject {
        context: ContextRef,
    },
    /// Allow (or stop allowing) changes in the project, each with approval.
    SetHarnessWrite {
        context: ContextRef,
        write: bool,
    },
    /// Decide one pending approval, by its desktop id.
    DecideHarnessApproval {
        conversation: OperationId,
        approval: String,
        allow: bool,
    },
    /// Open a staged proposal into the change review.
    ReviewHarnessProposal {
        conversation: OperationId,
        proposal: String,
    },
    DismissHarnessProposal {
        conversation: OperationId,
        proposal: String,
    },
    /// Allow changes again before a renewal restart, or keep the message.
    ConfirmHarnessWrite {
        conversation: OperationId,
        allow: bool,
    },
    /// Read the chosen workspace's MCP server catalog (C22).
    ListMcpServers {
        context: ContextRef,
        workspace: WorkspaceRef,
    },
    /// Turn a workspace MCP server on or off for new conversations.
    EnableMcpServer {
        context: ContextRef,
        workspace: WorkspaceRef,
        server: String,
        enabled: bool,
    },
    /// Sign in to a workspace MCP server in the browser.
    SignInMcpServer {
        context: ContextRef,
        workspace: WorkspaceRef,
        server: String,
    },
    CancelMcpSignIn {
        context: ContextRef,
    },
    /// Sign out of a workspace MCP server (its tokens are revoked).
    SignOutMcpServer {
        context: ContextRef,
        workspace: WorkspaceRef,
        server: String,
    },
    /// List the chosen workspace's saved conversations (C20). Without a
    /// workspace (none is chosen, as when AI Gateway cannot be read), the
    /// tenant's last chosen workspace on this Mac, read-only (T23).
    ListHarnessHistory {
        context: ContextRef,
        workspace: Option<WorkspaceRef>,
    },
    /// Open a saved conversation for reading.
    OpenHarnessHistory {
        context: ContextRef,
        workspace: Option<WorkspaceRef>,
        id: String,
    },
    CloseHarnessHistory {
        context: ContextRef,
        workspace: Option<WorkspaceRef>,
    },
    /// Delete a saved conversation (not the one running).
    DeleteHarnessHistory {
        context: ContextRef,
        workspace: Option<WorkspaceRef>,
        id: String,
    },
    /// Save the opened conversation's transcript in the native save sheet.
    ExportHarnessHistory {
        context: ContextRef,
        workspace: Option<WorkspaceRef>,
        id: String,
        format: harness_history::HistoryFormat,
    },
    /// Continue the opened conversation in a new engine (only with the
    /// binding it was saved under).
    ResumeHarness {
        context: ContextRef,
        workspace: WorkspaceRef,
        id: String,
    },
    /// Send one prompt to the conversation (only while it is ready).
    SendHarness {
        conversation: OperationId,
        prompt: String,
        /// The presentation's own ID for this send (32 lowercase hex): the
        /// core takes each at most once and reports which message it became,
        /// so a send whose receipt was lost is settled by the core's word,
        /// never by matching text (C32).
        #[serde(default)]
        #[ts(optional)]
        submission: Option<String>,
    },
    /// Stop the answer being given.
    InterruptHarness {
        conversation: OperationId,
    },
    /// End the conversation: the engine is stopped and reaped.
    EndHarness {
        conversation: OperationId,
    },
    /// Clear a conversation that has ended (nothing of it is kept).
    ClearHarness {
        conversation: OperationId,
    },
    /// Read the chosen workspace's Gateway files with its inference key.
    ReadGatewayFiles {
        context: ContextRef,
        workspace: WorkspaceRef,
    },
    /// Choose a file to upload in the native file sheet; the desktop reads
    /// it once and holds it until one upload takes it.
    ChooseGatewayFile {
        context: ContextRef,
        workspace: WorkspaceRef,
    },
    /// Upload the chosen file (named by its token) once, for the
    /// `assistants` purpose (never retried).
    UploadGatewayFile {
        context: ContextRef,
        workspace: WorkspaceRef,
        file: String,
    },
    /// Delete one file the last read listed, once (never retried).
    DeleteGatewayFile {
        context: ContextRef,
        workspace: WorkspaceRef,
        file: String,
    },
    /// Keep the chosen workspace's Playground draft on this Mac, or (with
    /// no workspace, by the draft's `id`) edit one already kept.
    SavePlaygroundDraft {
        context: ContextRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        workspace: Option<WorkspaceRef>,
        draft: gateway_local_domain::PlaygroundDraftInput,
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
    /// Write the active tenant's case Library again after a failed save.
    SaveLibraryAgain {
        context: ContextRef,
    },
    /// One API Client request (C14): projects and documents on this Mac,
    /// independent of the tenant; observed once by its operation.
    Workbench {
        request: WorkbenchRequest,
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
    /// Read v1's folder, without its Keychain, to show what could be
    /// imported (C23).
    InspectV1Import,
    /// Read the selected categories of v1 (its protected items only for
    /// selected categories and tenants) and review each item against v2.
    PlanV1Import {
        categories: Vec<crate::import_v1::Category>,
        targets: Vec<import_v1_domain::TenantTarget>,
    },
    /// Import the chosen ready items of the reviewed plan.
    ConfirmV1Import {
        review: ReviewId,
        items: Vec<u32>,
    },
    /// Finish an interrupted import (its source is read again).
    ResumeV1Import {
        receipt: String,
    },
    /// Remove what an import added and nobody changed since (asks first).
    RollBackV1Import {
        receipt: String,
    },
    /// Forget a settled import's receipt (it can no longer be rolled back).
    ForgetV1ImportReceipt {
        receipt: String,
    },
    /// Close the import review.
    DismissV1Import,
    /// Save the redacted diagnostics through the native save sheet (C24).
    ExportDiagnostics,
    /// Remove the diagnostics kept on this Mac (C24).
    ClearDiagnostics,
    /// Run a Runtime Security synchronous scan for the active tenant.
    Scan {
        context: ContextRef,
        draft: ScanDraft,
        /// The library case it was opened from (C33): once it settles, its
        /// verdict is recorded for that case, if the case still holds what
        /// was scanned. Never starts anything by itself.
        #[serde(default)]
        #[ts(optional)]
        case: Option<String>,
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

/// Each request's kind as its `kind` tag names it, and the list of every
/// kind (diagnostics record a kind, never the request's contents, and
/// export only kinds on this list).
macro_rules! intent_kinds {
    ($($variant:ident => $name:literal,)*) => {
        impl Intent {
            pub fn kind(&self) -> &'static str {
                match self {
                    $(Intent::$variant { .. } => $name,)*
                }
            }
        }

        /// Every request kind.
        pub(crate) const INTENT_KINDS: &[&str] = &[$($name,)*];
    };
}

intent_kinds! {
    SetAppearance => "setAppearance",
    SetUiVariant => "setUiVariant",
    Navigate => "navigate",
    SetOnboarding => "setOnboarding",
    SetShowCalls => "setShowCalls",
    BeginTenantSetup => "beginTenantSetup",
    UpdateTenantSetup => "updateTenantSetup",
    ChooseSetupProducts => "chooseSetupProducts",
    VerifyTenantSetup => "verifyTenantSetup",
    CompleteTenantSetup => "completeTenantSetup",
    DiscardTenantSetup => "discardTenantSetup",
    CreateTenant => "createTenant",
    SelectTenant => "selectTenant",
    DismissSwitchGuard => "dismissSwitchGuard",
    ReadTenant => "readTenant",
    RecheckProduct => "recheckProduct",
    AcknowledgeDeployment => "acknowledgeDeployment",
    AcknowledgeItem => "acknowledgeItem",
    SnoozeItem => "snoozeItem",
    UnsnoozeItem => "unsnoozeItem",
    DeleteTenant => "deleteTenant",
    ConfigureScanCredential => "configureScanCredential",
    RemoveScanCredential => "removeScanCredential",
    ConfigureManagementCredential => "configureManagementCredential",
    RemoveManagementCredential => "removeManagementCredential",
    Management => "management",
    SelectGatewayWorkspace => "selectGatewayWorkspace",
    ReviewChange => "reviewChange",
    RequestApply => "requestApply",
    DiscardChange => "discardChange",
    ChooseDictionaryFile => "chooseDictionaryFile",
    RevealSecret => "revealSecret",
    ForgetSecret => "forgetSecret",
    CancelChange => "cancelChange",
    OpenGatewayLocal => "openGatewayLocal",
    SaveGatewayDraft => "saveGatewayDraft",
    DiscardGatewayLocal => "discardGatewayLocal",
    ConfigureInferenceCredential => "configureInferenceCredential",
    RemoveInferenceCredential => "removeInferenceCredential",
    RunInference => "runInference",
    StopInference => "stopInference",
    ClearInference => "clearInference",
    StartHarness => "startHarness",
    ConfigureHarnessSignIn => "configureHarnessSignIn",
    RemoveHarnessSignIn => "removeHarnessSignIn",
    ChooseHarnessCredential => "chooseHarnessCredential",
    SignInHarness => "signInHarness",
    CancelHarnessSignIn => "cancelHarnessSignIn",
    SignOutHarness => "signOutHarness",
    ChooseHarnessProject => "chooseHarnessProject",
    ClearHarnessProject => "clearHarnessProject",
    SetHarnessWrite => "setHarnessWrite",
    DecideHarnessApproval => "decideHarnessApproval",
    ReviewHarnessProposal => "reviewHarnessProposal",
    DismissHarnessProposal => "dismissHarnessProposal",
    ConfirmHarnessWrite => "confirmHarnessWrite",
    ListMcpServers => "listMcpServers",
    EnableMcpServer => "enableMcpServer",
    SignInMcpServer => "signInMcpServer",
    CancelMcpSignIn => "cancelMcpSignIn",
    SignOutMcpServer => "signOutMcpServer",
    ListHarnessHistory => "listHarnessHistory",
    OpenHarnessHistory => "openHarnessHistory",
    CloseHarnessHistory => "closeHarnessHistory",
    DeleteHarnessHistory => "deleteHarnessHistory",
    ExportHarnessHistory => "exportHarnessHistory",
    ResumeHarness => "resumeHarness",
    SendHarness => "sendHarness",
    InterruptHarness => "interruptHarness",
    EndHarness => "endHarness",
    ClearHarness => "clearHarness",
    ReadGatewayFiles => "readGatewayFiles",
    ChooseGatewayFile => "chooseGatewayFile",
    UploadGatewayFile => "uploadGatewayFile",
    DeleteGatewayFile => "deleteGatewayFile",
    SavePlaygroundDraft => "savePlaygroundDraft",
    OpenReports => "openReports",
    SaveReport => "saveReport",
    RemoveReport => "removeReport",
    UndoReport => "undoReport",
    ExportReports => "exportReports",
    ClearInvestigation => "clearInvestigation",
    ExportInvestigation => "exportInvestigation",
    OpenLibrary => "openLibrary",
    SaveCase => "saveCase",
    DeleteCase => "deleteCase",
    ImportCases => "importCases",
    ImportExamples => "importExamples",
    CopyCase => "copyCase",
    ExportCases => "exportCases",
    SaveLibraryAgain => "saveLibraryAgain",
    Workbench => "workbench",
    HoldInsight => "holdInsight",
    RestoreInsight => "restoreInsight",
    ReviewEndpointChange => "reviewEndpointChange",
    ConfirmEndpointChange => "confirmEndpointChange",
    DiscardReview => "discardReview",
    ChooseProfileImport => "chooseProfileImport",
    ConfirmProfileImport => "confirmProfileImport",
    ResetLocalData => "resetLocalData",
    InspectV1Import => "inspectV1Import",
    PlanV1Import => "planV1Import",
    ConfirmV1Import => "confirmV1Import",
    ResumeV1Import => "resumeV1Import",
    RollBackV1Import => "rollBackV1Import",
    ForgetV1ImportReceipt => "forgetV1ImportReceipt",
    DismissV1Import => "dismissV1Import",
    Scan => "scan",
    CancelScan => "cancelScan",
    StartAcademySignIn => "startAcademySignIn",
    CancelAcademySignIn => "cancelAcademySignIn",
    OpenAcademyVerification => "openAcademyVerification",
    SignOutAcademy => "signOutAcademy",
    PrintPage => "printPage",
    UploadAvatar => "uploadAvatar",
    LoadAcademyImage => "loadAcademyImage",
    DownloadCourse => "downloadCourse",
    RemoveDownload => "removeDownload",
    SaveLocalDraft => "saveLocalDraft",
    LoadLocalDraft => "loadLocalDraft",
    ClearLocalDraft => "clearLocalDraft",
    AcademyCall => "academyCall",
    ExportDiagnostics => "exportDiagnostics",
    ClearDiagnostics => "clearDiagnostics",
}

/// Every refusal (a new one fails to build until it is listed here).
const REFUSALS: [Refusal; 12] = [
    Refusal::StaleContext,
    Refusal::CapabilityMissing,
    Refusal::InvalidInput,
    Refusal::ReviewInvalid,
    Refusal::ReviewConsumed,
    Refusal::Busy,
    Refusal::LockedStore,
    Refusal::DeniedStore,
    Refusal::AlreadyProcessed,
    Refusal::SequenceGap,
    Refusal::ShuttingDown,
    Refusal::DecisionPending,
];
const _: fn(Refusal) = |r| match r {
    Refusal::StaleContext
    | Refusal::CapabilityMissing
    | Refusal::InvalidInput
    | Refusal::ReviewInvalid
    | Refusal::ReviewConsumed
    | Refusal::Busy
    | Refusal::LockedStore
    | Refusal::DeniedStore
    | Refusal::AlreadyProcessed
    | Refusal::SequenceGap
    | Refusal::ShuttingDown
    | Refusal::DecisionPending => {}
};
const HEALTHS: [StorageHealth; 4] = [
    StorageHealth::Ready,
    StorageHealth::Corrupt,
    StorageHealth::Unavailable,
    StorageHealth::Resetting,
];
const _: fn(StorageHealth) = |h| match h {
    StorageHealth::Ready
    | StorageHealth::Corrupt
    | StorageHealth::Unavailable
    | StorageHealth::Resetting => {}
};
const RESET_STAGES: [ResetStage; 2] = [ResetStage::Credentials, ResetStage::Document];
const _: fn(ResetStage) = |s| match s {
    ResetStage::Credentials | ResetStage::Document => {}
};

/// Whether a diagnostics value is one the app records (D051): a request
/// kind, a refusal code, a storage health or a reset outcome. Anything
/// else (an edited or foreign line) is never exported.
pub(crate) fn known_intent_kind(kind: &str) -> bool {
    INTENT_KINDS.contains(&kind)
}

pub(crate) fn known_refusal(code: &str) -> bool {
    REFUSALS.iter().any(|r| refusal_code(r) == code)
}

pub(crate) fn known_storage_health(health: &str) -> bool {
    HEALTHS.iter().any(|h| refusal_code_of(h) == health)
}

pub(crate) fn known_reset_outcome(outcome: &str) -> bool {
    outcome == "done"
        || RESET_STAGES
            .iter()
            .any(|s| outcome.strip_prefix("failed-") == Some(refusal_code_of(s).as_str()))
}

/// A serializable value's name (an enum's tag), for diagnostics.
fn refusal_code_of(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// A refusal's code (its serialized name), for diagnostics.
fn refusal_code(refusal: &Refusal) -> String {
    refusal_code_of(refusal)
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
    /// A Harness decision is pending: the tenant switch waits for the
    /// operator's resolution (I1); the snapshot's `switch_blocked` says which.
    #[error("a Harness decision is pending")]
    DecisionPending,
}

/// The product context a caller asks to observe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewScope {
    Shell,
    /// The single result of an admitted learner call.
    AcademyCall(OperationId),
    /// The single result of an admitted management read.
    Management(OperationId),
    /// The single result of an admitted API Client request.
    Workbench(OperationId),
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
    Workbench(WorkbenchHandle),
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
    /// Production / staging / other (C29).
    pub environment: features::TenantEnvironment,
    /// The exact origin the scan key may be sent to.
    pub origin: String,
    pub scan: ScanView,
    /// The separate management OAuth capability (no secret material).
    pub management: ManagementView,
    /// The Gateway workspace inference key (C18; no secret material).
    pub inference: inference_domain::InferenceKeyView,
    /// The products this tenant reads (C29); Runtime Security until set.
    pub products: Vec<features::ProductId>,
    /// Verify warnings recorded at wizard completion (C29); the Inbox
    /// renders them (C30).
    pub setup_warnings: Vec<features::SetupWarning>,
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

/// Why a tenant switch is held (C28 contract, raised in C30): a Harness
/// conversation has a decision pending; the operator chooses to archive it,
/// discard it or stay. The shell renders its guard from this, never from a
/// guess of its own (invariant I1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SwitchBlocked {
    /// The active source context and native-issued, single refusal reference.
    pub context: ContextRef,
    pub review: ReviewId,
    /// The exact setup generation when completing a draft; none for selection.
    pub target_context: Option<ContextRef>,
    /// The tenant the switch was asked for (a setup draft's id when the
    /// switch is its completion).
    pub to: TenantId,
    pub pending: PendingDecisionRef,
    /// The switch is the completion of the tenant setup draft `to`: the
    /// resolution is re-sent with `completeTenantSetup` (C30, I1).
    pub completion: bool,
}

/// The conversation and decision that hold a switch (ids only; no content).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PendingDecisionRef {
    pub conversation: String,
    pub decision: String,
}

/// Redacted shell state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    /// Opaque, monotonically increasing revision (string-encoded u64).
    pub revision: String,
    pub appearance: Theme,
    /// Which shell renders (C28, D058); `features` is that shell's catalogue.
    pub ui_variant: features::UiVariant,
    /// First-run wizard state (C28/C29).
    pub onboarding: features::OnboardingState,
    /// The active destination (C28): what `navigate` last set, else the
    /// presentation's first sidebar entry.
    pub active_feature: FeatureId,
    /// "Show the calls" preference (C28).
    pub show_calls: bool,
    /// The active tenant's recorded calls, newest last, while "Show the
    /// calls" is on (C33); absent when it is off or no tenant is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub calls: Option<Vec<crate::transport::CallView>>,
    /// A held tenant switch awaiting the operator's resolution (C30 raises
    /// it; always absent in C28).
    pub switch_blocked: Option<SwitchBlocked>,
    /// The tenant setup in progress (C29), with its draft and verification.
    pub setup: Option<features::SetupView>,
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
    /// The Administration inventory (C31): static per build, published in
    /// the v2 shell only (the legacy shell keeps its own fixed matrix and
    /// literal snapshot).
    pub administration: Option<crate::admin_inventory::AdminInventoryView>,
    /// The active tenant's Runtime key rotations (C31).
    pub rotations: Vec<rotation_domain::RotationView>,
    /// The active tenant's Inbox and read status (C30).
    pub inbox: inbox_domain::InboxView,
    /// API Client projects on this Mac (C14).
    pub workbench: WorkbenchView,
    /// The active context's Gateway workspaces and choice (C16).
    pub gateway: GatewayView,
    pub harness: harness_domain::HarnessView,
    /// Reviewed import of v1 work (C23).
    pub v1_import: import_v1_domain::V1ImportView,
    /// Local redacted diagnostics (C24).
    pub diagnostics: diagnostics_domain::DiagnosticsView,
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
    /// The library case it was opened from, with what was scanned (C33).
    case: Option<ScannedCase>,
}

/// A scan opened from a library case (C33): the case and what was scanned.
struct ScannedCase {
    id: String,
    prompt: String,
    response: String,
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
    /// How often time-based state (overview freshness, snooze ends) is
    /// re-evaluated and published when it changed.
    pub deadline_tick: std::time::Duration,
    /// The Git program the API Client's read-only Git view runs (an
    /// absolute path; None: the view reports Git unavailable).
    pub git: Option<std::path::PathBuf>,
    /// The v1 API Client profile offered for adoption (read only).
    pub adoption_source: Option<std::path::PathBuf>,
    /// Where API Client requests may go (C15). Fails closed: production
    /// hosts set `Live`; fixture builds keep loopback only.
    pub workbench_network: crate::workbench::admission::Network,
    /// API origins tenant management tokens may also reach from the API
    /// Client beside the configured management endpoints (the built-in
    /// catalogue's Strata Cloud Manager host).
    pub workbench_api_origins: Vec<String>,
}

/// The built-in catalogue's API host (C15).
pub const SCM_API_ORIGIN: &str = "https://api.sase.paloaltonetworks.com";

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
            deadline_tick: std::time::Duration::from_secs(15),
            git: crate::workbench::git::locate(),
            adoption_source: None,
            workbench_network: crate::workbench::admission::Network::LoopbackOnly,
            workbench_api_origins: vec![SCM_API_ORIGIN.to_owned()],
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

/// The native folder sheets for API Client collections (host adapter).
pub trait FolderPicker: Send + Sync + 'static {
    /// An existing folder the operator chose (`None`: cancelled).
    fn choose_folder(&self)
    -> futures_util::future::BoxFuture<'static, Option<std::path::PathBuf>>;
    /// A new folder's location and name from the save sheet (`None`:
    /// cancelled). Nothing exists there yet.
    fn choose_new(&self) -> futures_util::future::BoxFuture<'static, Option<std::path::PathBuf>>;
    /// An existing folder for the Harness to work in, from its own open
    /// sheet (`None`: cancelled) (C21).
    fn choose_project(
        &self,
    ) -> futures_util::future::BoxFuture<'static, Option<std::path::PathBuf>>;
    /// Where to save a response body, from the save sheet with `suggested`
    /// as its name (`None`: cancelled). The sheet confirms replacing.
    fn choose_save(
        &self,
        suggested: String,
    ) -> futures_util::future::BoxFuture<'static, Option<std::path::PathBuf>>;
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
    /// The native folder sheets for API Client collections.
    pub folders: Arc<dyn FolderPicker>,
    /// The native file sheet for Insomnia exports (bounded reads).
    pub workbench_files: Arc<dyn ImportPicker>,
    /// The native file sheet for Gateway file uploads (C18; reads bounded
    /// to [`crate::gateway_files::MAX_UPLOAD_BYTES`]).
    pub gateway_files: Arc<dyn ImportPicker>,
    /// The API Client's private folder: its sealed profile and the built-in
    /// collection (None: the API Client is unavailable).
    pub workbench_root: Option<std::path::PathBuf>,
    /// The native Harness engine, its launcher and runtime homes (C19).
    pub harness: crate::harness::sidecar::HarnessAdapters,
    /// v1's folder and Keychain service, read only (C23; None: no import).
    pub v1_source: Option<crate::import_v1::V1Source>,
    /// Where import receipts are kept (C23).
    pub import_root: Option<std::path::PathBuf>,
    /// Where local redacted diagnostics are kept (C24; None: not kept).
    pub logs_root: Option<std::path::PathBuf>,
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
    /// Products chosen in the wizard (C29); empty means Runtime only.
    products: Vec<features::ProductId>,
    /// Verify warnings recorded at wizard completion (C29).
    setup_warnings: Vec<features::SetupWarning>,
    /// Runtime API key rotations (C31).
    rotations: Vec<features::Rotation>,
    scan: ScanCapability,
    /// The management OAuth client secret (destination: the token origin).
    management: ScanCapability,
    /// The management OAuth client ID that goes with it.
    management_client: Option<String>,
    /// The Gateway workspace last chosen for this tenant (C16).
    gateway_workspace: Option<String>,
    /// The workspace inference key (destination: its runtime endpoint)
    /// and the workspace and endpoint it is for (C18).
    inference: ScanCapability,
    inference_target: Option<crate::gateway_inference::InferenceTarget>,
    /// Company sign-in for the Harness Gateway credential (C20).
    harness_sign_in: Option<crate::store::StoredSignIn>,
    /// The Harness project folder (C21).
    harness_project: Option<crate::store::StoredProject>,
    /// Its workspace MCP servers and sign-ins (C22).
    harness_mcp: Vec<crate::store::StoredMcpServer>,
}

impl Tenant {
    fn new(profile: TenantProfile) -> Self {
        Self {
            profile,
            products: Vec::new(),
            setup_warnings: Vec::new(),
            rotations: Vec::new(),
            scan: ScanCapability::Missing,
            management: ScanCapability::Missing,
            management_client: None,
            gateway_workspace: None,
            inference: ScanCapability::Missing,
            inference_target: None,
            harness_sign_in: None,
            harness_project: None,
            harness_mcp: Vec::new(),
        }
    }

    fn capability(&self, class: CredentialClass) -> &ScanCapability {
        match class {
            CredentialClass::Scan => &self.scan,
            CredentialClass::Management => &self.management,
            CredentialClass::Inference => &self.inference,
        }
    }

    fn capability_mut(&mut self, class: CredentialClass) -> &mut ScanCapability {
        match class {
            CredentialClass::Scan => &mut self.scan,
            CredentialClass::Management => &mut self.management,
            CredentialClass::Inference => &mut self.inference,
        }
    }
}

/// Everything persisted, plus capability state that rides with it.
#[derive(Clone, Debug, Default)]
struct Data {
    appearance: Option<Theme>,
    onboarding: Option<features::OnboardingState>,
    show_calls: Option<bool>,
    tenants: Vec<Tenant>,
    active: Option<TenantId>,
    /// The draft under setup (C29): not a tenant until completion.
    setup: Option<setup_domain::Setup>,
    pending_cleanup: Vec<RecordId>,
    learner: Option<StoredLearner>,
    learner_cleanup: Vec<RecordId>,
    /// Workspaces whose protected Harness history may exist (C20).
    harness_history: Vec<crate::store::GatewayPreference>,
    /// Unsettled v1 import receipts, the commit binding (C23, D050).
    open_imports: Vec<crate::import_v1::receipt::Receipt>,
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
    /// Session-only: the destination `navigate` last set (C28).
    active_feature: Option<FeatureId>,
    salt: [u8; 16],
    epoch: u64,
    active_ref: Option<ContextRef>,
    endpoint_review: Option<EndpointReview>,
    consumed_reviews: Vec<ReviewId>,
    import: ImportState,
    reset_failure: Option<ResetFailure>,
    durability_unconfirmed: bool,
    /// A rotation fact the service established (an issuance, a removal, a
    /// reconciliation) is held in memory but not yet saved: every later
    /// publication retries the save; nothing remote is replayed (C31).
    rotation_unsaved: bool,
    /// A Harness decision that may change things, waiting on the native
    /// confirmation: its conversation, approval and nonce (C32).
    harness_confirming: Option<(OperationId, String, u64)>,
    cleaning: bool,
    /// Cleanup was asked for while a pass ran: run another when it ends,
    /// so a record released meanwhile is not left waiting (P06).
    clean_again: bool,
    /// Keychain records whose write has not settled; cleanup never deletes
    /// them (a delete could precede the write and drop the ledger entry).
    writes_in_flight: Vec<RecordId>,
    next_lease: u64,
    operations: Operations,
    transport: Option<Arc<TransportFactory>>,
    store: Arc<dyn AppStore>,
    scans: Vec<ScanRecord>,
    /// Calls made on tenants' behalf, for "Show the calls" (C33).
    calls: crate::transport::CallLog,
    /// Library generations a case result changed, still to be stored (C33).
    case_writes: Vec<(TenantId, u64)>,
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
    /// Gateway changes whose context was retired while their write may be
    /// in flight, held only to keep how they settle on this Mac under
    /// their own tenant (never shown, never applied again; a reply or key
    /// is dropped unread).
    detached_changes: Vec<change_domain::ChangeRecord>,
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
    /// Every opened tenant's Gateway drafts and uncertain changes (C17).
    gateway_local: Vec<gateway_local_domain::LocalWork>,
    /// Gateway work writes collected by settlements, started by the caller.
    gateway_local_writes: Vec<(TenantId, u64)>,
    /// The Playground's run (C18).
    playground: Option<inference_domain::Run>,
    /// The chosen workspace's Gateway files (C18).
    files: Option<gateway_files_domain::Files>,
    /// Woken by a streaming run after each batch of output.
    stream_pulse: Arc<tokio::sync::Notify>,
    /// The last report or investigation export.
    export: Option<sessions_domain::Export>,
    /// Report and case Library file writes, one at a time (tenant deletion
    /// and reset take it too).
    report_writer: Arc<tokio::sync::Mutex<()>>,
    library_root: Option<std::path::PathBuf>,
    /// Every opened tenant's case Library (C12).
    libraries: Vec<library_domain::Library>,
    case_catalog: url::Url,
    /// Every opened tenant's Home reads (C12, C30): kept across a switch
    /// away and back; a retired context's reply is never recorded.
    overviews: Vec<observation_domain::OverviewRecord>,
    /// Attention holds per tenant (memory only, like v1).
    holds: Vec<(TenantId, Vec<observation_domain::Hold>)>,
    /// Seconds added to the wall clock (fixture builds only move it).
    clock_offset: u32,
    /// A tenant switch held by a pending Harness decision (C30, I1).
    switch_blocked: Option<SwitchBlocked>,
    /// Conversations the operator discarded (C30): their records are
    /// removed and no later save of theirs is admitted.
    history_discarded: Vec<(crate::harness::history::Scope, String)>,
    /// History work a resolution queued, taken by the intent's effects.
    history_jobs: Vec<harness_history::HistoryJob>,
    /// Discards whose record removal failed (C30): shown on the workspace's
    /// history until the operator removes the record.
    history_discard_failures: Vec<(
        crate::harness::history::Scope,
        String,
        crate::harness::history::HistoryFailure,
    )>,
    /// The overview's time-dependent state at the last deadline tick.
    overview_published: Option<observation_domain::OverviewSignature>,
    deadline_tick: std::time::Duration,
    /// API Client projects and documents (C14).
    workbench: workbench_domain::Workbench,
    workbench_root: Option<std::path::PathBuf>,
    git: Option<std::path::PathBuf>,
    adoption_source: Option<std::path::PathBuf>,
    workbench_network: crate::workbench::admission::Network,
    workbench_api_origins: Vec<String>,
    /// The active context's Gateway workspaces and choice (C16).
    gateway: gateway_domain::GatewayState,
    /// The native Harness conversation (C19).
    harness: Option<harness_domain::Conversation>,
    /// The engine is bundled with this build.
    harness_engine: bool,
    /// An interactive company sign-in (C20).
    harness_sign_in: Option<harness_identity::SignInFlow>,
    /// Signed-in grants' live tokens (C20).
    harness_grants: Vec<Arc<harness_identity::Grant>>,
    /// The chosen workspace's protected history (C20).
    harness_history: Option<harness_history::HistoryState>,
    harness_history_root: Option<std::path::PathBuf>,
    /// The reviewed v1 import (C23).
    v1_import: import_v1_domain::V1Import,
    /// Local redacted diagnostics and their export (C24).
    diagnostics: diagnostics_domain::DiagnosticsState,
    diagnostics_log: Arc<crate::diagnostics::Diagnostics>,
    v1_source: Option<crate::import_v1::V1Source>,
    import_root: Option<std::path::PathBuf>,
    /// Tenants with a Harness history save under way (one entry each):
    /// removing a tenant waits for its saves before removing its folder,
    /// and reset waits for all of them (C20 review F4).
    history_writers: Vec<TenantId>,
    /// Holds every Harness history save after it is admitted (fixture
    /// builds expose it to tests; otherwise uncontended).
    history_hold: Arc<tokio::sync::Mutex<()>>,
    /// Holds every Keychain write of a sign-in or rotation after its put
    /// and before it is settled (fixture builds expose it; otherwise
    /// uncontended).
    settle_hold: Arc<tokio::sync::Mutex<()>>,
    /// The app's data folder, never a Harness project (C21).
    harness_data_root: Option<std::path::PathBuf>,
    /// The folder last refused as a tenant's project, until the next choice.
    harness_project_refused: Option<(TenantId, String)>,
    /// The chosen workspace's MCP catalog, a sign-in to one of its
    /// servers, and the live grants (C22).
    mcp_catalog: Option<harness_mcp::CatalogState>,
    mcp_flow: Option<harness_mcp::McpFlow>,
    mcp_live: Vec<Arc<harness_mcp::McpGrant>>,
}

impl State {
    fn appearance(&self) -> Theme {
        self.data.appearance.unwrap_or(Theme::System)
    }

    /// C44: the v2 presentation is the only one. The persisted flag is read
    /// no more (an older store's `legacy` is ignored) and `SetUiVariant` is
    /// accepted without effect, so an older presentation or script that
    /// still sends it is not refused.
    fn ui_variant(&self) -> features::UiVariant {
        features::UiVariant::V2
    }

    fn onboarding(&self) -> features::OnboardingState {
        self.data.onboarding.unwrap_or_default()
    }

    /// What the shell shows (C29): a first run is one with no tenant yet. A
    /// desktop that already has a tenant (an upgrade, or a tenant created
    /// before the v2 shell was chosen) is never sent through the wizard.
    /// A desktop with a tenant is never in the wizard: an upgrade, a tenant
    /// created before v2 was chosen, or one imported during first run
    /// (the import flow is the reviewed C23 destination; its tenant's
    /// credentials are added in Administration).
    fn shown_onboarding(&self) -> features::OnboardingState {
        if self.data.tenants.is_empty() {
            self.onboarding()
        } else {
            features::OnboardingState::Done
        }
    }

    /// The operator's choice, else on until onboarding completes (C33).
    fn show_calls(&self) -> bool {
        self.data
            .show_calls
            .unwrap_or(self.shown_onboarding() != features::OnboardingState::Done)
    }

    /// The active destination: the last admitted `navigate` while it stays
    /// in the current catalogue, else the presentation's first sidebar entry.
    fn active_feature(&self) -> FeatureId {
        let variant = self.ui_variant();
        self.active_feature
            .filter(|f| features::catalogue_for(variant).iter().any(|e| e.id == *f))
            .unwrap_or_else(|| features::default_feature(variant))
    }

    fn snapshot(&self) -> ShellSnapshot {
        ShellSnapshot {
            revision: self.revision.to_string(),
            appearance: self.shown_appearance(),
            ui_variant: self.ui_variant(),
            onboarding: self.shown_onboarding(),
            active_feature: self.active_feature(),
            show_calls: self.show_calls(),
            calls: self
                .data
                .active
                .as_ref()
                .filter(|_| self.show_calls())
                .map(|t| self.calls.for_tenant(t.as_str())),
            switch_blocked: self.switch_blocked.clone(),
            setup: self.setup_view(),
            features: features::catalogue_for(self.ui_variant()),
            fixture_build: cfg!(feature = "test-fixtures"),
            storage: self.storage,
            // An import's tenants show once it commits (D050).
            tenants: self
                .data
                .tenants
                .iter()
                .filter(|t| !self.v1_hides_tenant(&t.profile.id))
                .map(tenant_view)
                .collect(),
            active: match (&self.data.active, &self.active_ref) {
                (Some(tenant), Some(context)) if !self.v1_hides_tenant(tenant) => {
                    Some(ActiveContext {
                        tenant: tenant.clone(),
                        context: context.clone(),
                    })
                }
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
            academy_offline: self.offline.as_ref().map_or_else(
                offline_domain::OfflineView::signed_out,
                |o| {
                    let mut view = o.view();
                    // An import's drafts show once it commits (D050).
                    view.local_drafts
                        .retain(|d| !self.v1_hides_draft(&d.attempt_id));
                    view
                },
            ),
            changes: self.changes_view(),
            sessions: self.sessions_view(),
            library: self.library_view(),
            overview: self.overview_view(),
            administration: (self.ui_variant() == features::UiVariant::V2)
                .then(crate::admin_inventory::inventory),
            rotations: self.rotations_view(),
            inbox: self.inbox_view(&self.overview_view()),
            workbench: self.workbench_view(),
            gateway: self.gateway_view(),
            harness: self.harness_view(self.harness_engine),
            v1_import: self.v1_import_view(),
            diagnostics: self.diagnostics_view(
                self.diagnostics_log.count(),
                self.diagnostics_log.available(),
            ),
        }
    }

    /// Whether `id` is a committed tenant (never the setup draft).
    fn committed_tenant(&self, id: &TenantId) -> bool {
        self.data.tenants.iter().any(|t| &t.profile.id == id)
    }

    /// A tenant, or the draft under setup (C29), by id.
    fn tenant(&self, id: &TenantId) -> Option<&Tenant> {
        self.data
            .tenants
            .iter()
            .chain(self.data.setup.as_ref().map(|s| &s.tenant))
            .find(|t| &t.profile.id == id)
    }

    fn tenant_mut(&mut self, id: &TenantId) -> Option<&mut Tenant> {
        self.data
            .tenants
            .iter_mut()
            .chain(self.data.setup.as_mut().map(|s| &mut s.tenant))
            .find(|t| &t.profile.id == id)
    }

    fn persisted(&self) -> PersistedState {
        PersistedState {
            appearance: self.appearance(),
            tenant_products: self
                .data
                .tenants
                .iter()
                .filter(|t| !t.products.is_empty())
                .map(|t| crate::store::TenantProducts {
                    tenant: t.profile.id.clone(),
                    products: t.products.clone(),
                })
                .collect(),
            tenant_warnings: self
                .data
                .tenants
                .iter()
                .filter(|t| !t.setup_warnings.is_empty())
                .map(|t| crate::store::TenantWarnings {
                    tenant: t.profile.id.clone(),
                    warnings: t.setup_warnings.clone(),
                })
                .collect(),
            tenant_rotations: self
                .data
                .tenants
                .iter()
                .filter(|t| !t.rotations.is_empty())
                .map(|t| crate::store::TenantRotations {
                    tenant: t.profile.id.clone(),
                    rotations: t.rotations.clone(),
                })
                .collect(),
            ui_variant: self.ui_variant(),
            onboarding: self.onboarding(),
            show_calls: self.show_calls(),
            show_calls_chosen: self.data.show_calls.is_some(),
            tenants: self
                .data
                .tenants
                .iter()
                .map(|t| t.profile.clone())
                .collect(),
            active_tenant: self.data.active.clone(),
            tenant_draft: self.data.setup.as_ref().map(|s| crate::store::StoredDraft {
                profile: s.tenant.profile.clone(),
                mode: s.mode,
            }),
            credentials: self
                .data
                .tenants
                .iter()
                .chain(self.data.setup.as_ref().map(|s| &s.tenant))
                .flat_map(|t| {
                    [
                        CredentialClass::Scan,
                        CredentialClass::Management,
                        CredentialClass::Inference,
                    ]
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
                                CredentialClass::Management => t.management_client.clone(),
                                _ => None,
                            },
                            inference: match class {
                                CredentialClass::Inference => t.inference_target.clone(),
                                _ => None,
                            },
                        }),
                        _ => None,
                    })
                })
                .collect(),
            pending_cleanup: self.data.pending_cleanup.clone(),
            learner: self.data.learner.clone(),
            learner_cleanup: self.data.learner_cleanup.clone(),
            gateway_workspaces: self
                .data
                .tenants
                .iter()
                .filter_map(|t| {
                    t.gateway_workspace
                        .as_ref()
                        .map(|workspace| crate::store::GatewayPreference {
                            tenant: t.profile.id.clone(),
                            workspace: workspace.clone(),
                        })
                })
                .collect(),
            harness_sign_ins: self
                .data
                .tenants
                .iter()
                .filter_map(|t| t.harness_sign_in.clone())
                .collect(),
            harness_history: self.data.harness_history.clone(),
            open_imports: self.data.open_imports.clone(),
            harness_projects: self
                .data
                .tenants
                .iter()
                .filter_map(|t| t.harness_project.clone())
                .collect(),
            harness_mcp: self
                .data
                .tenants
                .iter()
                .flat_map(|t| t.harness_mcp.iter().cloned())
                .collect(),
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
        self.sync_inference();
        self.sync_files();
        self.sync_harness();
        if self.switch_blocked.as_ref().is_some_and(|guard| {
            self.active_ref.as_ref() != Some(&guard.context)
                || self.pending_decision().as_ref() != Some(&guard.pending)
                || guard.target_context.as_ref().is_some_and(|context| {
                    self.setup_tenant_for(context).as_ref() != Some(&guard.to)
                })
        }) {
            self.switch_blocked = None;
        }
        self.revision += 1;
        let update = Update::Shell(Box::new(self.snapshot()));
        self.subscribers.publish(&update);
    }

    /// Issues a fresh context reference for the active tenant (or none) and
    /// retires everything bound to the previous one.
    fn reissue_context(&mut self) -> Vec<runtime::Start> {
        self.switch_blocked = None;
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
        self.workbench.sending.retire(self.active_ref.as_ref());
        self.retire_scan_sessions();
        self.retire_management_bindings();
        let mut starts = starts;
        starts.extend(self.retire_gateway());
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
    /// Admits one scan of the active tenant with its scan key (the operator's
    /// Scan, and the Harness's typed scan tool, C21).
    fn admit_scan(
        &mut self,
        context: ContextRef,
        draft: crate::scanning::ScanDraft,
        case: Option<String>,
        keychain: Arc<dyn CredentialStore>,
        session: Arc<SessionVault>,
    ) -> Result<(OperationId, Vec<runtime::Start>), Refusal> {
        let state = self;
        let tenant = state.check_context(&context)?;
        let draft = draft.validate().ok_or(Refusal::InvalidInput)?;
        if case
            .as_deref()
            .is_some_and(|id| !crate::scanning::uuid_shaped(id))
        {
            return Err(Refusal::InvalidInput);
        }
        // Only a Runtime Security case of this tenant's open library can be
        // named: a missing, another tenant's or another product's case is
        // refused before anything is sent (C33).
        if let Some(id) = case.as_deref() {
            let runnable = state
                .open_library_cases(&tenant)
                .is_some_and(|cases| cases.iter().any(|c| c.id == id && c.product.can_scan()));
            if !runnable {
                return Err(Refusal::InvalidInput);
            }
        }
        // What was scanned, to compare with the case when it settles.
        let case = case.map(|id| ScannedCase {
            id,
            prompt: draft.prompt.clone(),
            response: draft.response.clone(),
        });
        let (record, mode, origin) = match &state.tenant(&tenant).ok_or(Refusal::StaleContext)?.scan
        {
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
            .for_tenant(
                &state.calls,
                crate::transport::CallTag {
                    tenant: tenant.as_str().to_owned(),
                    purpose: crate::transport::CallPurpose::Scan,
                    credential: crate::transport::CallCredential::ScanKey,
                },
            )
            .map_err(|_| Refusal::CapabilityMissing)?;
        let slot: PayloadSlot = Arc::default();
        let bytes = draft.queued_bytes();
        let job = ScanJob {
            keychain,
            session_vault: session,
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
            case,
        });
        Ok((operation, starts))
    }

    /// Collects settled scans now (C21's scan tool waits on them).
    fn sync_scans_now(&mut self) {
        self.sync_scans();
    }

    /// The library writes case results queued (C33).
    fn take_case_writes(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.case_writes)
            .into_iter()
            .map(|(tenant, generation)| Effect::StoreLibrary { tenant, generation })
            .collect()
    }

    /// A settled scan's result as the presentation sees it (C21's scan
    /// tool waits for it).
    fn scan_settled(&self, operation: &OperationId) -> Option<ScanRecordView> {
        let r = self
            .scans
            .iter()
            .find(|r| &r.operation == operation && r.settlement.is_some())?;
        let s = r.settlement?;
        let (result, failure) = match &r.payload {
            Some(ScanPayload::Result(view)) => (Some((**view).clone()), None),
            Some(ScanPayload::Failure(failure)) => (None, Some(*failure)),
            None => (None, None),
        };
        Some(ScanRecordView {
            operation: r.operation.clone(),
            progress: ScanProgress::Settled,
            outcome: Some(s.outcome),
            stop: s.stop,
            result,
            failure,
            discarded: r.discarded,
        })
    }

    fn sync_scans(&mut self) {
        let current = self.active_ref.clone();
        let mut results = Vec::new();
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
                if let (Some(case), Some(ScanPayload::Result(view))) =
                    (&record.case, &record.payload)
                {
                    results.push((
                        record.tenant.clone(),
                        case.id.clone(),
                        case.prompt.clone(),
                        case.response.clone(),
                        crate::library::CaseResult {
                            verdict: view.verdict,
                            at: offline_domain::now_secs(),
                            scan_id: (!view.scan_id.is_empty()).then(|| view.scan_id.clone()),
                        },
                    ));
                }
            } else {
                record.discarded = payload.is_some();
            }
        }
        // A case's result is its newest Playground run, recorded only while
        // the case still holds what was scanned (C33).
        for (tenant, id, prompt, response, result) in results {
            let changed = self.change_open_library(&tenant, |library| {
                let recorded = library.record_result(&id, &prompt, &response, result);
                (recorded, recorded == crate::library::Recorded::Recorded)
            });
            match changed {
                Some((_, Some(generation))) => self.case_writes.push((tenant, generation)),
                // Said, never silently dropped: the library has no room for it.
                Some((crate::library::Recorded::TooLarge, None)) => {
                    self.library_problem(&tenant, crate::library::CaseProblem::TooLarge);
                }
                _ => {}
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
        if let Some(draft) = self.setup_tenant_for(context) {
            return Ok(draft);
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
        if class == CredentialClass::Inference {
            t.inference_target = None;
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
                // The saved state includes every rotation fact held in memory.
                self.rotation_unsaved = false;
                Ok(())
            }
            Err(StoreError::Unconfirmed) => {
                self.durability_unconfirmed = true;
                self.rotation_unsaved = false;
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
        self.workbench.sending.retire(None);
        self.retire_scan_sessions();
        self.retire_management_bindings();
        let mut starts = starts;
        starts.extend(self.retire_gateway());
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
        environment: t.profile.environment,
        origin: t.profile.destination.origin().to_owned(),
        destination: t.profile.destination.clone(),
        scan: ScanView {
            state,
            mode,
            failure,
        },
        products: if t.products.is_empty() {
            vec![features::ProductId::RuntimeSecurity]
        } else {
            t.products.clone()
        },
        setup_warnings: t.setup_warnings.clone(),
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
        inference: {
            let (state, mode, failure) = match &t.inference {
                ScanCapability::Missing => (ScanState::Missing, None, None),
                ScanCapability::Storing { mode, .. } => (ScanState::Storing, Some(*mode), None),
                ScanCapability::Ready { mode, .. } => (ScanState::Ready, Some(*mode), None),
                ScanCapability::Failed { mode, failure } => {
                    (ScanState::Failed, Some(*mode), Some(*failure))
                }
            };
            inference_domain::InferenceKeyView {
                state,
                mode,
                failure,
                target: t.inference_target.clone(),
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
    /// Builds and saves the diagnostics export (C24).
    ExportDiagnostics {
        lease: u64,
    },
    /// Reviewed v1 import work (C23).
    V1Import(import_v1_domain::Job),
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
    /// The native confirmation of a Harness decision that may change
    /// things (C32).
    ConfirmHarness {
        conversation: OperationId,
        approval: String,
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
    ChooseGatewayFile {
        lease: u64,
    },
    LoadReports {
        lease: u64,
    },
    StoreReports {
        tenant: TenantId,
        generation: u64,
    },
    /// Remove a deleted tenant's report (or Gateway work) file.
    ForgetReports(std::path::PathBuf),
    LoadGatewayLocal {
        lease: u64,
    },
    StoreGatewayLocal {
        tenant: TenantId,
        generation: u64,
    },
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
    ChooseCaseFile {
        lease: u64,
    },
    FetchCatalog {
        lease: u64,
        url: url::Url,
    },
    /// Run an API Client request in its turn.
    Workbench {
        operation: OperationId,
        request: WorkbenchRequest,
    },
    /// Run a Harness conversation as its own task (C19).
    Harness(Box<harness_domain::HarnessStart>),
    /// Company sign-in through the browser (C20).
    HarnessSignIn(Box<harness_identity::SignInJob>),
    /// Revoke a signed-out grant (C20).
    HarnessSignedOut(Arc<harness_identity::Grant>),
    /// Renew a sign-in token before a message is sent (C20).
    HarnessRenew(Box<harness_domain::RenewJob>),
    /// Read, open or delete protected history (C20).
    HarnessHistory(harness_history::HistoryJob),
    /// Remove a removed tenant's history directory once its saves under
    /// way have settled (C20).
    ForgetHarnessHistory(TenantId, std::path::PathBuf),
    /// Choose the Harness project in the native folder sheet (C21).
    ChooseHarnessProject(ContextRef),
    /// Reads the chosen workspace's MCP catalog (C22).
    McpCatalog {
        context: ContextRef,
        reference: WorkspaceRef,
        lease: u64,
    },
    McpSignIn(Box<harness_mcp::McpSignInJob>),
    McpSignedOut(Arc<harness_mcp::McpGrant>),
}

struct Inner {
    state: Mutex<State>,
    adapters: Adapters,
    session: Arc<SessionVault>,
    diagnostics: Arc<crate::diagnostics::Diagnostics>,
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
        let diagnostics = Arc::new(crate::diagnostics::Diagnostics::open(
            adapters.logs_root.clone(),
        ));
        diagnostics.record(crate::diagnostics::Event::Started {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            fixture: cfg!(feature = "test-fixtures"),
        });
        if storage != StorageHealth::Ready {
            diagnostics.record(crate::diagnostics::Event::Storage {
                health: refusal_code_of(&storage),
            });
        }
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
            active_feature: None,
            salt,
            epoch: 0,
            active_ref: None,
            endpoint_review: None,
            consumed_reviews: Vec::new(),
            import: ImportState::Idle,
            reset_failure: None,
            durability_unconfirmed: false,
            rotation_unsaved: false,
            harness_confirming: None,
            cleaning: false,
            clean_again: false,
            writes_in_flight: Vec::new(),
            next_lease: 0,
            operations: Operations::new(config.operations),
            transport,
            store: Arc::clone(&adapters.store),
            scans: Vec::new(),
            calls: crate::transport::CallLog::default(),
            case_writes: Vec::new(),
            scan_sessions: Vec::new(),
            management_endpoints: config.management,
            management_bindings: Vec::new(),
            management_calls: Vec::new(),
            change_review: None,
            change_results: Vec::new(),
            detached_changes: Vec::new(),
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
            gateway_local: Vec::new(),
            gateway_local_writes: Vec::new(),
            playground: None,
            files: None,
            stream_pulse: Arc::default(),
            export: None,
            report_writer: Arc::default(),
            library_root: adapters.library_root.clone(),
            libraries: Vec::new(),
            case_catalog: config.case_catalog.clone(),
            overviews: Vec::new(),
            holds: Vec::new(),
            clock_offset: 0,
            switch_blocked: None,
            history_discarded: Vec::new(),
            history_jobs: Vec::new(),
            history_discard_failures: Vec::new(),
            overview_published: None,
            deadline_tick: config.deadline_tick,
            workbench: workbench_domain::Workbench::default(),
            gateway: gateway_domain::GatewayState::default(),
            workbench_root: adapters.workbench_root.clone(),
            git: config.git.clone(),
            adoption_source: config.adoption_source.clone(),
            workbench_network: config.workbench_network,
            workbench_api_origins: config.workbench_api_origins.clone(),
            harness: None,
            harness_engine: adapters.harness.engine.is_some(),
            harness_sign_in: None,
            harness_grants: Vec::new(),
            harness_history: None,
            harness_history_root: adapters.harness.history_root.clone(),
            v1_import: import_v1_domain::V1Import::default(),
            diagnostics: diagnostics_domain::DiagnosticsState::default(),
            diagnostics_log: Arc::clone(&diagnostics),
            v1_source: adapters.v1_source.clone(),
            import_root: adapters.import_root.clone(),
            history_writers: Vec::new(),
            history_hold: Arc::default(),
            settle_hold: Arc::default(),
            harness_data_root: adapters
                .harness
                .runtime_root
                .as_ref()
                .and_then(|r| r.parent())
                .map(std::path::Path::to_path_buf),
            harness_project_refused: None,
            mcp_catalog: None,
            mcp_flow: None,
            mcp_live: Vec::new(),
        };
        let _ = state.reissue_context();
        let inner = Arc::new(Inner {
            state: Mutex::new(state),
            adapters,
            session: Arc::new(SessionVault::new()),
            diagnostics,
        });
        if let Ok(state) = lock(&inner) {
            state.sweep_harness_history();
        }
        if let Ok(mut state) = lock(&inner) {
            state.load_v1_receipts();
        }
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
    /// Whether API Client edits exist that are not protected on this Mac
    /// (the native close guard asks before the window closes).
    pub fn workbench_unprotected(&self) -> bool {
        lock(&self.inner)
            .map(|s| s.workbench_unprotected())
            .unwrap_or(false)
    }

    /// Records where the app panicked (never the panic's message: it may
    /// hold anything) in the local diagnostics (C24, D051).
    pub fn record_panic(&self, file: &str, line: u32) {
        // A source file of this repository; anything else (a dependency's
        // or the standard library's path, which can name the build
        // machine's folders) is "external", with no line.
        let ours = crate::diagnostics::panic_file_ok(file) && file != "external";
        self.inner
            .diagnostics
            .record(crate::diagnostics::Event::Panicked {
                file: if ours { file } else { "external" }.to_owned(),
                line: if ours { line } else { 0 },
            });
    }

    #[cfg(feature = "test-fixtures")]
    /// Stops the next v1 import, resume or rollback at `point` as if the
    /// app ended there: nothing after it is written (fixture builds only).
    pub fn interrupt_v1_import(&self, point: crate::import_v1::CommitPoint) {
        self.interrupt_v1_import_at(point, 0);
    }

    #[cfg(feature = "test-fixtures")]
    /// As [`Core::interrupt_v1_import`], after `point` has passed `skip`
    /// times (a later item's commit point).
    pub fn interrupt_v1_import_at(&self, point: crate::import_v1::CommitPoint, skip: u32) {
        if let Ok(mut state) = lock(&self.inner) {
            state.v1_import.interrupt_at = Some((point, skip));
        }
    }

    #[cfg(feature = "test-fixtures")]
    /// Holds every API Client request (and reset's purge) until the guard
    /// drops (fixture builds only).
    pub async fn hold_workbench(&self) -> tokio::sync::OwnedMutexGuard<()> {
        let lane = lock(&self.inner)
            .map(|s| Arc::clone(&s.workbench.lane))
            .expect("authority");
        lane.lock_owned().await
    }

    #[cfg(feature = "test-fixtures")]
    /// Holds every API Client Send after it captures its document and
    /// before it renders, until the guard drops (fixture builds only).
    pub async fn hold_send_planning(&self) -> tokio::sync::OwnedMutexGuard<()> {
        let (hold, _) = lock(&self.inner)
            .map(|s| s.workbench.sending.planning())
            .expect("authority");
        hold.lock_owned().await
    }

    #[cfg(feature = "test-fixtures")]
    /// Sends waiting at the planning hold (fixture builds only).
    pub fn sends_held_in_planning(&self) -> u64 {
        lock(&self.inner)
            .map(|s| {
                s.workbench
                    .sending
                    .planning()
                    .1
                    .load(std::sync::atomic::Ordering::SeqCst)
            })
            .unwrap_or(0)
    }

    #[cfg(feature = "test-fixtures")]
    /// Makes the next API Client Save fail with an I/O error after `files`
    /// file writes; its failure handling runs (fixture builds only).
    pub fn interrupt_next_save(&self, files: usize) {
        if let Ok(mut s) = lock(&self.inner) {
            s.workbench.interrupt = Some(workbench_domain::Interrupt {
                files,
                crash: false,
            });
        }
    }

    #[cfg(feature = "test-fixtures")]
    /// Stops the next Save after `files` project writes as if the process
    /// ended there: no failure handling runs (fixture builds only).
    pub fn crash_next_save(&self, files: usize) {
        if let Ok(mut s) = lock(&self.inner) {
            s.workbench.interrupt = Some(workbench_domain::Interrupt { files, crash: true });
        }
    }

    #[cfg(feature = "test-fixtures")]
    /// Holds every admitted Harness history save until the guard drops
    /// (fixture builds only).
    pub async fn hold_history_writes(&self) -> tokio::sync::OwnedMutexGuard<()> {
        let hold = lock(&self.inner)
            .map(|s| Arc::clone(&s.history_hold))
            .expect("authority");
        hold.lock_owned().await
    }

    #[cfg(feature = "test-fixtures")]
    /// Holds every sign-in or rotation Keychain write after its put and
    /// before it is settled, until the guard drops (fixture builds only).
    pub async fn hold_write_settlement(&self) -> tokio::sync::OwnedMutexGuard<()> {
        let hold = lock(&self.inner)
            .map(|s| Arc::clone(&s.settle_hold))
            .expect("authority");
        hold.lock_owned().await
    }

    #[cfg(feature = "test-fixtures")]
    /// Runs the cleanup ledger now (fixture builds only).
    pub fn clean_up_now(&self) {
        run_effects(&self.inner, vec![Effect::Cleanup]);
    }

    #[cfg(feature = "test-fixtures")]
    /// Keychain writes in flight (fixture builds only).
    pub fn writes_in_flight(&self) -> usize {
        lock(&self.inner)
            .map(|s| s.writes_in_flight.len())
            .unwrap_or(0)
    }

    #[cfg(feature = "test-fixtures")]
    /// Harness history saves admitted and not yet settled (fixture builds
    /// only).
    pub fn history_writes_under_way(&self) -> usize {
        lock(&self.inner)
            .map(|s| s.history_writers.len())
            .unwrap_or(0)
    }

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

    /// The Mac is going to sleep: a Harness conversation is ended (its
    /// engine stopped and reaped) rather than left running unattended.
    pub fn system_will_sleep(&self) {
        if let Ok(mut state) = lock(&self.inner)
            && state.stop_harness(harness_domain::HarnessStop::Sleep)
        {
            state.changed();
        }
    }

    /// The app is quitting: ends a Harness conversation and waits up to
    /// `wait` for its engine to be reaped. Blocking; call once, at exit.
    /// Returns whether nothing is left running.
    pub fn end_harness_for_exit(&self, wait: std::time::Duration) -> bool {
        if let Ok(mut state) = lock(&self.inner)
            && state.stop_harness(harness_domain::HarnessStop::Quit)
        {
            state.changed();
        }
        let until = std::time::Instant::now() + wait;
        loop {
            let running = lock(&self.inner)
                .map(|s| s.harness_running())
                .unwrap_or(false);
            if !running {
                return true;
            }
            if std::time::Instant::now() >= until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The running Harness engine's process id (fixture builds only).
    #[cfg(feature = "test-fixtures")]
    pub fn harness_pid_for_tests(&self) -> Option<u32> {
        lock(&self.inner).ok()?.harness_pid()
    }

    /// Marks every live workspace MCP sign-in expired, as a refused renewal
    /// would (fixture builds only).
    #[cfg(feature = "test-fixtures")]
    pub fn expire_mcp_grants_for_tests(&self) {
        if let Ok(state) = lock(&self.inner) {
            state.mcp_live.iter().for_each(|g| g.expire_for_tests());
        }
    }

    /// Resumes durable work after launch (pending Keychain cleanup). Call on
    /// the application runtime.
    pub fn start(&self) {
        let (offline, recovery) = lock(&self.inner)
            .ok()
            .map(|mut s| (s.offline.clone(), s.take_v1_recovery()))
            .unwrap_or_default();
        let mut effects = vec![Effect::Cleanup];
        effects.extend(offline.map(Effect::OpenOffline));
        // An import the app ended during is rolled back first (D050).
        effects.extend(recovery);
        run_effects(&self.inner, effects);
        // Deadlines pass without any event (a read going stale, a snooze
        // ending, sleep and wake): re-evaluate them and publish a change.
        // A streaming Playground run wakes this after each batch of
        // output; snapshots follow at most every 40 ms.
        let weak = Arc::downgrade(&self.inner);
        let pulse = lock(&self.inner).ok().map(|s| Arc::clone(&s.stream_pulse));
        if let Some(pulse) = pulse {
            let weak = weak.clone();
            tokio::spawn(async move {
                loop {
                    pulse.notified().await;
                    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
                    let Some(inner) = weak.upgrade() else {
                        return;
                    };
                    let effects = match lock(&inner) {
                        Ok(mut state) => {
                            state.changed();
                            gateway_local_effects(&mut state)
                        }
                        Err(_) => return,
                    };
                    run_effects(&inner, effects);
                }
            });
        }
        tokio::spawn(async move {
            loop {
                let Some(tick) = weak
                    .upgrade()
                    .and_then(|inner| lock(&inner).ok().map(|s| s.deadline_tick))
                else {
                    return;
                };
                tokio::time::sleep(tick).await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                if let Ok(mut state) = lock(&inner) {
                    let now = Some(state.overview_signature());
                    if state.overview_published != now {
                        state.overview_published = now;
                        state.changed();
                    }
                };
            }
        });
    }

    /// Moves this core's clock forward (fixture builds only).
    #[cfg(feature = "test-fixtures")]
    pub fn advance_clock(&self, secs: u32) {
        if let Ok(mut state) = lock(&self.inner) {
            state.clock_offset = state.clock_offset.saturating_add(secs);
        }
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
    // A setup that was in progress is not restored: its credentials are
    // cleaned up (C29).
    let draft_records: Vec<RecordId> = doc
        .tenant_draft
        .as_ref()
        .map(|d| {
            doc.credentials
                .iter()
                .filter(|c| c.tenant == d.profile.id)
                .map(|c| c.record.clone())
                .collect()
        })
        .unwrap_or_default();
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
            let inference = doc
                .credentials
                .iter()
                .find(|c| c.class == CredentialClass::Inference && c.tenant == profile.id);
            let gateway_workspace = doc
                .gateway_workspaces
                .iter()
                .find(|p| p.tenant == profile.id)
                .map(|p| p.workspace.clone());
            let harness_sign_in = doc
                .harness_sign_ins
                .iter()
                .find(|s| s.tenant == profile.id)
                .cloned();
            let mut tenant = Tenant::new(profile);
            tenant.scan = scan;
            tenant.gateway_workspace = gateway_workspace;
            tenant.harness_sign_in = harness_sign_in;
            tenant.harness_project = doc
                .harness_projects
                .iter()
                .find(|p| p.tenant == tenant.profile.id)
                .cloned();
            tenant.harness_mcp = doc
                .harness_mcp
                .iter()
                .filter(|m| m.tenant == tenant.profile.id)
                .cloned()
                .collect();
            if let Some(c) = management {
                tenant.management = ScanCapability::Ready {
                    record: c.record.clone(),
                    mode: StorageMode::Keychain,
                    destination: c.destination.clone(),
                };
                tenant.management_client = c.client_id.clone();
            }
            if let Some(c) = inference
                && let Some(target) = c.inference.clone()
            {
                tenant.inference = ScanCapability::Ready {
                    record: c.record.clone(),
                    mode: StorageMode::Keychain,
                    destination: c.destination.clone(),
                };
                tenant.inference_target = Some(target);
            }
            if let Some(tp) = doc
                .tenant_products
                .iter()
                .find(|p| p.tenant == tenant.profile.id)
            {
                tenant.products = tp.products.clone();
            }
            if let Some(tw) = doc
                .tenant_warnings
                .iter()
                .find(|w| w.tenant == tenant.profile.id)
            {
                tenant.setup_warnings = tw.warnings.clone();
            }
            if let Some(tr) = doc
                .tenant_rotations
                .iter()
                .find(|r| r.tenant == tenant.profile.id)
            {
                tenant.rotations = tr.rotations.clone();
            }
            tenant
        })
        .collect();
    Data {
        appearance: Some(doc.appearance),
        onboarding: Some(doc.onboarding),
        show_calls: doc.show_calls_chosen.then_some(doc.show_calls),
        tenants,
        active: doc.active_tenant,
        setup: None,
        pending_cleanup: {
            let mut pending = doc.pending_cleanup;
            for record in draft_records {
                if !pending.contains(&record) {
                    pending.push(record);
                }
            }
            pending
        },
        learner: doc.learner,
        learner_cleanup: doc.learner_cleanup,
        harness_history: doc.harness_history,
        open_imports: doc.open_imports,
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
        let kind = intent.kind();
        let (result, effects) = {
            let mut state = lock(&self.inner)?;
            let digest = intent_digest(&state.salt, &intent);
            match state.operator_sessions.check(&delivery, &digest) {
                Admit::Refused(refusal) => return Err(refusal),
                Admit::Duplicate(result) => return result,
                Admit::Fresh => {
                    let mut effects = Vec::new();
                    // What an import writes waits while it (or its
                    // rollback) runs, until every receipt has settled, and
                    // while a receipt cannot be read (D050).
                    let result =
                        if state.v1_import_holds() && import_v1_domain::waits_for_import(&intent) {
                            Err(Refusal::Busy)
                        } else {
                            admit(
                                &mut state,
                                &self.inner.session,
                                &self.inner.adapters,
                                intent,
                                &mut effects,
                            )
                        };
                    if result.is_err() {
                        effects.clear();
                    }
                    // Settlements published by this intent are kept on
                    // this Mac whether or not the intent itself succeeded.
                    effects.extend(gateway_local_effects(&mut state));
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
        // Only the request's kind and the refusal's code (D051).
        self.inner
            .diagnostics
            .record(crate::diagnostics::Event::Intent {
                kind: kind.to_owned(),
                refusal: result.as_ref().err().map(refusal_code),
            });
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
            ViewScope::Workbench(operation) => {
                Ok(Observation::Workbench(state.observe_workbench(&operation)?))
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
        Intent::SetUiVariant { variant: _ } => {
            // C44: one presentation. Accepted and ignored (see `ui_variant`).
        }
        Intent::Navigate { feature } => {
            let entry = features::catalogue_for(state.ui_variant())
                .into_iter()
                .find(|e| e.id == feature)
                .ok_or(Refusal::InvalidInput)?;
            if entry.availability != features::Availability::Available {
                return Err(Refusal::InvalidInput);
            }
            if state.active_feature != Some(feature) {
                state.active_feature = Some(feature);
                state.changed();
            }
        }
        Intent::SetOnboarding { state: next } => {
            use features::OnboardingState as O;
            let current = state.onboarding();
            let allowed = matches!(
                (current, next),
                (O::Needed, O::InProgress | O::Done) | (O::InProgress, O::Done)
            ) || current == next;
            if !allowed {
                return Err(Refusal::InvalidInput);
            }
            if current != next {
                let before = state.data.clone();
                state.data.onboarding = Some(next);
                if state.storage == StorageHealth::Ready
                    && let Err(e) = state.save_with(&before)
                {
                    return Err(e);
                }
                state.changed();
            }
        }
        Intent::SetShowCalls { on } => {
            // The operator's choice holds from now on, onboarding or not (C33).
            if state.data.show_calls != Some(on) {
                let before = state.data.clone();
                state.data.show_calls = Some(on);
                if state.storage == StorageHealth::Ready
                    && let Err(e) = state.save_with(&before)
                {
                    return Err(e);
                }
                state.changed();
            }
        }
        Intent::BeginTenantSetup { mode, profile } => {
            state.require_storage()?;
            let before = state.data.clone();
            state.begin_setup(mode, profile)?;
            state.save_with(&before)?;
            state.changed();
        }
        Intent::UpdateTenantSetup { context, profile } => {
            state.require_storage()?;
            let before = state.data.clone();
            let retired = state.update_setup(&context, profile)?;
            state.save_with(&before)?;
            for old in retired {
                let _ = session.delete(&old);
            }
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::ChooseSetupProducts { context, products } => {
            state.require_storage()?;
            let before = state.data.clone();
            state.choose_setup_products(&context, products)?;
            state.save_with(&before)?;
            state.changed();
        }
        Intent::VerifyTenantSetup { context } => {
            let starts = state.begin_verify(
                &context,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            effects.push(Effect::Start(starts));
            effects.push(Effect::Start(state.take_gateway_starts()));
            state.changed();
        }
        Intent::CompleteTenantSetup {
            context,
            packs,
            resolution,
        } => {
            state.require_storage()?;
            let draft = state.completable_setup(&context, &packs)?;
            // Completion makes the new tenant active: the one guarded switch
            // path (I1). A pending Harness decision refuses it, with the
            // draft, the active tenant and the decision all untouched.
            if let Err(refusal) = state.guard_switch(&draft, resolution, Some(&context)) {
                state.changed();
                return Err(refusal);
            }
            effects.extend(state.take_history_jobs());
            let before = state.data.clone();
            let recorded_secs = wall_secs(state.clock_offset);
            let done = state.complete_setup(&context, packs, recorded_secs)?;
            state.save_with(&before)?;
            effects.push(Effect::Start(state.reissue_context()));
            effects.push(Effect::Start(state.take_gateway_starts()));
            if !done.packs.is_empty() {
                let (lease, write) = state.queue_examples(&done.tenant, &done.packs);
                if let Some(lease) = lease {
                    effects.push(Effect::LoadLibrary { lease });
                }
                if let Some((tenant, generation)) = write {
                    effects.push(Effect::StoreLibrary { tenant, generation });
                }
            }
            let _ = done.first_run;
            state.changed();
        }
        Intent::DiscardTenantSetup { context } => {
            if state.setup_has_keychain_credential() {
                state.require_storage()?;
            }
            let before = state.data.clone();
            let retired = state.discard_setup(&context)?;
            if state.storage == StorageHealth::Ready {
                state.save_with(&before)?;
            }
            for old in retired {
                let _ = session.delete(&old);
            }
            effects.push(Effect::Start(state.take_gateway_starts()));
            effects.push(Effect::Cleanup);
            state.changed();
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
                environment: profile.environment,
            }));
            if select {
                state.data.active = Some(id);
            }
            // A first-run draft is over once the desktop has a tenant (C29).
            let retired = state.end_first_run_setup();
            state.save_with(&before)?;
            for old in retired {
                let _ = session.delete(&old);
            }
            effects.push(Effect::Cleanup);
            if select {
                effects.push(Effect::Start(state.reissue_context()));
            }
            state.changed();
        }
        Intent::SelectTenant { tenant, resolution } => {
            state.require_storage()?;
            // A tenant an unsettled import wrote is not there yet (D050); a
            // setup draft is not a tenant until it completes (C29).
            if !state.committed_tenant(&tenant) || state.v1_hides_tenant(&tenant) {
                return Err(Refusal::InvalidInput);
            }
            if resolution.is_some() && state.data.active.as_ref() == Some(&tenant) {
                return Err(Refusal::StaleContext);
            }
            // The one switch path (I1): a pending Harness decision refuses
            // it until the operator resolves the conversation.
            if let Err(refusal) = state.guard_switch(&tenant, resolution, None) {
                state.changed();
                return Err(refusal);
            }
            effects.extend(state.take_history_jobs());
            let before = state.data.clone();
            state.data.active = Some(tenant);
            state.save_with(&before)?;
            effects.push(Effect::Start(state.reissue_context()));
            // The new tenant's Home is what a switch shows (C30, shell.md).
            if state.ui_variant() == features::UiVariant::V2 {
                state.active_feature = Some(FeatureId::Home);
            }
            state.changed();
        }
        Intent::DismissSwitchGuard => {
            state.switch_blocked = None;
            state.changed();
        }
        Intent::ReadTenant { context } => {
            let tenant = state.check_context(&context)?;
            let products = state.selected_products(&tenant);
            // The overview (Runtime sources and the chosen Gateway workspace)
            // is admitted first; the other chosen products' reads follow,
            // each a finite part of the same read.
            let (operation, mut starts) = state.admit_management(
                context.clone(),
                ManagementRequest::Overview,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            for product in products.into_iter().filter(|p| {
                matches!(
                    p,
                    features::ProductId::RedTeam | features::ProductId::ModelSkill
                )
            }) {
                match state.admit_read_part(
                    context.clone(),
                    product,
                    Arc::clone(&adapters.keychain),
                    Arc::clone(session),
                ) {
                    Ok((_, more)) => starts.extend(more),
                    Err(refusal) => state.product_read_refused(&tenant, product, refusal),
                }
            }
            state.set_rechecking(&tenant, None);
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            effects.push(Effect::Start(state.take_gateway_starts()));
            state.changed();
        }
        Intent::RecheckProduct { context, product } => {
            let tenant = state.check_context(&context)?;
            if !state.selected_products(&tenant).contains(&product) {
                return Err(Refusal::InvalidInput);
            }
            let (operation, starts) = match product {
                features::ProductId::RuntimeSecurity | features::ProductId::Gateway => state
                    .admit_management(
                        context,
                        ManagementRequest::Overview,
                        Arc::clone(&adapters.keychain),
                        Arc::clone(session),
                    )?,
                features::ProductId::RedTeam | features::ProductId::ModelSkill => state
                    .admit_read_part(
                        context,
                        product,
                        Arc::clone(&adapters.keychain),
                        Arc::clone(session),
                    )?,
            };
            state.set_rechecking(&tenant, Some(product));
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            effects.push(Effect::Start(state.take_gateway_starts()));
            state.changed();
        }
        Intent::AcknowledgeItem { context, id } => {
            state.hold_insight(&context, &id, HoldKind::Acknowledge)?;
            state.changed();
        }
        Intent::SnoozeItem { context, id } => {
            state.hold_insight(&context, &id, HoldKind::Snooze)?;
            state.changed();
        }
        Intent::UnsnoozeItem { context, id } => {
            state.restore_insight(&context, &id)?;
            state.changed();
        }
        Intent::AcknowledgeDeployment {
            context,
            application,
        } => {
            state.require_storage()?;
            let before = state.data.clone();
            let issued_by = state.acknowledge_deployment(&context, &application)?;
            state.save_with(&before)?;
            // Only a recorded acknowledgement retires the held copy (I6).
            if let Some((operation, old_key)) = issued_by {
                state.forget_issuance(&operation, &old_key);
            }
            state.changed();
        }
        Intent::DeleteTenant { tenant } => {
            state.require_storage()?;
            if !state.committed_tenant(&tenant) {
                return Err(Refusal::InvalidInput);
            }
            let before = state.data.clone();
            let was_active = state.data.active.as_ref() == Some(&tenant);
            state.calls.forget(tenant.as_str());
            let retired_session = state.retire_credential(&tenant);
            let retired_management = state.retire_class(&tenant, CredentialClass::Management);
            let retired_inference = state.retire_class(&tenant, CredentialClass::Inference);
            // Its company sign-in ends: the refresh token item is deleted
            // through the ledger and revoked at the provider.
            let signed_in = state.grant(&tenant);
            if let Some(record) = state
                .tenant(&tenant)
                .and_then(|t| t.harness_sign_in.as_ref())
                .and_then(|s| s.grant.as_ref())
                .map(|g| g.record.clone())
            {
                state.data.pending_cleanup.push(record);
            }
            // Its workspace MCP sign-ins end too: their token items are
            // deleted through the ledger and revoked at each server.
            let mcp_signed_in = state.mcp_grants_of(&tenant);
            let mcp_records: Vec<_> = state
                .tenant(&tenant)
                .map(|t| {
                    t.harness_mcp
                        .iter()
                        .filter_map(|m| m.grant.as_ref().map(|g| g.record.clone()))
                        .collect()
                })
                .unwrap_or_default();
            for record in mcp_records {
                if !state.data.pending_cleanup.contains(&record) {
                    state.data.pending_cleanup.push(record);
                }
            }
            // Its protected history keys go through the ledger too (the
            // sealed records are removed with its directory below).
            let (history_keys, history_dir) = state.forget_harness_history(&tenant);
            for key in history_keys {
                if !state.data.pending_cleanup.contains(&key) {
                    state.data.pending_cleanup.push(key);
                }
            }
            state.data.tenants.retain(|t| t.profile.id != tenant);
            if was_active {
                state.data.active = None;
            }
            // The tenant's case Library key (and with it its sealed file)
            // goes through the durable cleanup ledger, retried until done.
            let library_key = crate::library::key_record(tenant.as_str());
            if !state.data.pending_cleanup.contains(&library_key) {
                state.data.pending_cleanup.push(library_key);
            }
            state.save_with(&before)?;
            // Only an accepted deletion drops the tenant's scan records; a
            // refused one keeps them observable and cancellable.
            state.scans.retain(|r| r.tenant != tenant);
            if let Some(path) = state.forget_reports(&tenant) {
                effects.push(Effect::ForgetReports(path));
            }
            if let Some(path) = state.forget_gateway_local(&tenant) {
                effects.push(Effect::ForgetReports(path));
            }
            state.forget_library(&tenant);
            state.forget_holds(&tenant);
            if let Some(dir) = history_dir {
                effects.push(Effect::ForgetHarnessHistory(tenant.clone(), dir));
            }
            state.harness_grants.retain(|g| g.tenant != tenant);
            if let Some(grant) = signed_in {
                effects.push(Effect::HarnessSignedOut(grant));
            }
            state.mcp_live.retain(|g| g.tenant != tenant);
            for grant in mcp_signed_in {
                effects.push(Effect::McpSignedOut(grant));
            }
            for record in [retired_session, retired_management, retired_inference]
                .into_iter()
                .flatten()
            {
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
                None,
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
        Intent::CancelChange { operation } => {
            let starts = state.cancel_change(&operation)?;
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::OpenGatewayLocal { context } => {
            if let Some(lease) = state.open_gateway_local(&context)? {
                effects.push(Effect::LoadGatewayLocal { lease });
            }
            state.changed();
        }
        Intent::SaveGatewayDraft { context, draft } => {
            let saved = state.save_gateway_draft(&context, draft);
            // A refused draft still publishes its problem.
            state.changed();
            let (tenant, generation) = saved?;
            effects.push(Effect::StoreGatewayLocal { tenant, generation });
        }
        Intent::DiscardGatewayLocal { context, id } => {
            let (tenant, generation) = state.discard_gateway_local(&context, &id)?;
            effects.push(Effect::StoreGatewayLocal { tenant, generation });
            state.changed();
        }
        Intent::ConfigureInferenceCredential {
            context,
            mode,
            workspace,
            endpoint,
            secret,
        } => {
            // The key is bound to the workspace chosen now (natively) and
            // to the runtime endpoint entered with it.
            let workspace = state.inference_workspace(&context, &workspace)?;
            let target = crate::gateway_inference::InferenceTarget {
                workspace,
                endpoint,
            };
            if !target.is_valid()
                || secret.expose().is_empty()
                || secret.expose().len() > MAX_SECRET_BYTES
                || !secret.expose().bytes().all(|b| b.is_ascii_graphic())
            {
                return Err(Refusal::InvalidInput);
            }
            configure_credential(
                state,
                session,
                effects,
                &context,
                CredentialClass::Inference,
                mode,
                secret,
                None,
                Some(target),
            )?;
        }
        Intent::RemoveInferenceCredential { context } => {
            remove_credential(
                state,
                session,
                effects,
                &context,
                CredentialClass::Inference,
            )?;
        }
        Intent::RunInference {
            context,
            workspace,
            run,
        } => {
            let (operation, starts) = state.run_inference(
                context,
                workspace,
                run,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::ReadGatewayFiles { context, workspace } => {
            let (operation, starts) = state.read_files(
                context,
                workspace,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::ChooseGatewayFile { context, workspace } => {
            let lease = state.choose_file(
                context,
                workspace,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            effects.push(Effect::ChooseGatewayFile { lease });
            state.changed();
        }
        Intent::UploadGatewayFile {
            context,
            workspace,
            file,
        } => {
            let (operation, starts) = state.upload_file(
                context,
                workspace,
                &file,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::DeleteGatewayFile {
            context,
            workspace,
            file,
        } => {
            let (operation, starts) = state.delete_file(
                context,
                workspace,
                &file,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            operation_ref = Some(operation);
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::StopInference { operation } => {
            let starts = state.stop_inference(&operation)?;
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::ClearInference { operation } => {
            state.clear_inference(&operation)?;
            state.changed();
        }
        Intent::StartHarness { context, workspace } => {
            let start = state.start_harness(
                context,
                workspace,
                &adapters.harness,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
                None,
            )?;
            operation_ref = Some(start.id.clone());
            effects.push(Effect::Harness(Box::new(start)));
            state.changed();
        }
        Intent::ChooseHarnessProject { context } => {
            state.check_context(&context)?;
            state.harness_project_refused = None;
            effects.push(Effect::ChooseHarnessProject(context));
        }
        Intent::ClearHarnessProject { context } => {
            state.clear_harness_project(&context)?;
            state.changed();
        }
        Intent::SetHarnessWrite { context, write } => {
            state.set_harness_write(&context, write)?;
            state.changed();
        }
        Intent::DecideHarnessApproval {
            conversation,
            approval,
            allow,
        } => {
            // Allowing a decision that may change things opens the native
            // confirmation; only its private completion lets it run (C32).
            if let Some((nonce, request)) =
                state.decide_harness_approval(&conversation, &approval, allow)?
            {
                effects.push(Effect::ConfirmHarness {
                    conversation,
                    approval,
                    nonce,
                    request,
                });
            }
            state.changed();
        }
        Intent::ReviewHarnessProposal {
            conversation,
            proposal,
        } => {
            let (operation, starts) = state.review_harness_proposal(
                &conversation,
                &proposal,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
            operation_ref = operation;
            effects.push(Effect::Start(starts));
            state.changed();
        }
        Intent::DismissHarnessProposal {
            conversation,
            proposal,
        } => {
            state.dismiss_harness_proposal(&conversation, &proposal)?;
            state.changed();
        }
        Intent::ConfirmHarnessWrite {
            conversation,
            allow,
        } => {
            if let Some(job) = state.confirm_harness_write(&conversation, allow)? {
                effects.push(Effect::HarnessRenew(Box::new(job)));
            }
            state.changed();
        }
        Intent::ListMcpServers { context, workspace } => {
            effects.push(state.list_mcp_servers(&context, &workspace)?);
            state.changed();
        }
        Intent::EnableMcpServer {
            context,
            workspace,
            server,
            enabled,
        } => {
            state.enable_mcp_server(&context, &workspace, &server, enabled)?;
            state.changed();
        }
        Intent::SignInMcpServer {
            context,
            workspace,
            server,
        } => {
            effects.push(state.begin_mcp_sign_in(&context, &workspace, &server)?);
            state.changed();
        }
        Intent::CancelMcpSignIn { context } => {
            state.cancel_mcp_sign_in(&context)?;
            state.changed();
        }
        Intent::SignOutMcpServer {
            context,
            workspace,
            server,
        } => {
            if let Some(grant) = state.sign_out_mcp_server(&context, &workspace, &server)? {
                effects.push(Effect::McpSignedOut(grant));
            }
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::ListHarnessHistory { context, workspace } => {
            effects.push(state.list_harness_history(&context, workspace.as_ref())?);
            state.changed();
        }
        Intent::OpenHarnessHistory {
            context,
            workspace,
            id,
        } => {
            effects.push(state.open_harness_history(&context, workspace.as_ref(), &id)?);
            state.changed();
        }
        Intent::CloseHarnessHistory { context, workspace } => {
            state.close_harness_history(&context, workspace.as_ref())?;
            state.changed();
        }
        Intent::DeleteHarnessHistory {
            context,
            workspace,
            id,
        } => {
            effects.push(state.delete_harness_history(&context, workspace.as_ref(), &id)?);
            state.changed();
        }
        Intent::ExportHarnessHistory {
            context,
            workspace,
            id,
            format,
        } => {
            let (lease, suggested, bytes) =
                state.export_harness_history(&context, workspace.as_ref(), &id, format)?;
            effects.push(Effect::SaveFile {
                lease,
                suggested,
                bytes,
            });
            state.changed();
        }
        Intent::ResumeHarness {
            context,
            workspace,
            id,
        } => {
            let resume = state.resume_harness(&context, Some(&workspace), &id)?;
            let start = state.start_harness(
                context,
                workspace,
                &adapters.harness,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
                Some(resume),
            )?;
            operation_ref = Some(start.id.clone());
            effects.push(Effect::Harness(Box::new(start)));
            state.changed();
        }
        Intent::ConfigureHarnessSignIn {
            context,
            workspace,
            endpoint,
            issuer,
            client_id,
            audience,
        } => {
            state.configure_harness_sign_in(
                &context, &workspace, endpoint, &issuer, &client_id, &audience,
            )?;
            state.changed();
        }
        Intent::RemoveHarnessSignIn { context } => {
            if let Some(grant) = state.remove_harness_sign_in(&context)? {
                effects.push(Effect::HarnessSignedOut(grant));
            }
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::ChooseHarnessCredential { context, source } => {
            state.choose_harness_credential(&context, source)?;
            state.changed();
        }
        Intent::SignInHarness { context } => {
            effects.push(state.begin_harness_sign_in(&context)?);
            state.changed();
        }
        Intent::CancelHarnessSignIn { context } => {
            state.cancel_harness_sign_in(&context)?;
            state.changed();
        }
        Intent::SignOutHarness { context } => {
            if let Some(grant) = state.sign_out_harness(&context)? {
                effects.push(Effect::HarnessSignedOut(grant));
            }
            effects.push(Effect::Cleanup);
            state.changed();
        }
        Intent::SendHarness {
            conversation,
            prompt,
            submission,
        } => {
            if let Some(job) = state.send_harness(&conversation, prompt, submission)? {
                effects.push(Effect::HarnessRenew(Box::new(job)));
            }
            state.changed();
        }
        Intent::InterruptHarness { conversation } => {
            state.interrupt_harness(&conversation)?;
            state.changed();
        }
        Intent::EndHarness { conversation } => {
            state.end_harness(&conversation)?;
            state.changed();
        }
        Intent::ClearHarness { conversation } => {
            state.clear_harness(&conversation)?;
            state.changed();
        }
        Intent::SavePlaygroundDraft {
            context,
            workspace,
            draft,
        } => {
            let (tenant, generation) =
                state.save_playground_draft(&context, workspace.as_ref(), draft)?;
            effects.push(Effect::StoreGatewayLocal { tenant, generation });
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
        Intent::SaveLibraryAgain { context } => {
            let (tenant, generation) = state.retry_library(&context)?;
            effects.push(Effect::StoreLibrary { tenant, generation });
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
        Intent::Workbench { request } => {
            if state.storage == StorageHealth::Resetting {
                return Err(Refusal::Busy);
            }
            let operation = state.admit_workbench()?;
            operation_ref = Some(operation.clone());
            effects.push(Effect::Workbench { operation, request });
            state.changed();
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
            effects.push(Effect::Start(state.take_gateway_starts()));
            state.changed();
        }
        Intent::SelectGatewayWorkspace { context, workspace } => {
            let starts = state.select_gateway_workspace(&context, &workspace)?;
            effects.push(Effect::Start(starts));
            effects.push(Effect::Start(state.take_gateway_starts()));
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
                    environment: Default::default(),
                }
                .validate()
                .ok_or(Refusal::InvalidInput)?;
                state.data.tenants.push(Tenant::new(TenantProfile {
                    id: TenantId::issue().ok_or(Refusal::ShuttingDown)?,
                    label: profile.label,
                    tsg_id: profile.tsg_id,
                    destination: profile.destination,
                    environment: profile.environment,
                }));
            }
            state.save_with(&before)?;
            state.import = ImportState::Idle;
            state.consumed_reviews.push(review);
            state.changed();
        }
        Intent::ExportDiagnostics => diagnostics_domain::admit_export(state, effects)?,
        Intent::ClearDiagnostics => {
            state.clear_diagnostics();
            state.changed();
        }
        intent @ (Intent::InspectV1Import
        | Intent::PlanV1Import { .. }
        | Intent::ConfirmV1Import { .. }
        | Intent::ResumeV1Import { .. }
        | Intent::RollBackV1Import { .. }
        | Intent::ForgetV1ImportReceipt { .. }
        | Intent::DismissV1Import) => import_v1_domain::admit(state, intent, effects)?,
        Intent::ResetLocalData => {
            // An import or rollback under way settles first: its receipt
            // must never outlive (or be written after) the reset.
            if state.v1_import_busy() {
                return Err(Refusal::Busy);
            }
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
                        state.retire_class(id, CredentialClass::Inference),
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
            state.workbench.clear();
            state.forget_all_gateway_local();
            state.holds.clear();
            state.overviews.clear();
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
            let hidden = state.v1_hides_draft(&attempt_id);
            let (operation, starts) = state.admit_offline(
                learner,
                learner_domain::OfflineOp::LoadDraft(attempt_id, hidden),
            )?;
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
        Intent::Scan {
            context,
            draft,
            case,
        } => {
            let (operation, starts) = state.admit_scan(
                context,
                draft,
                case,
                Arc::clone(&adapters.keychain),
                Arc::clone(session),
            )?;
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
            effects.extend(state.take_case_writes());
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
            Effect::Harness(start) => {
                let inner = Arc::clone(inner);
                let harness_domain::HarnessStart {
                    id,
                    mut inputs,
                    cancel,
                    grant,
                    mcp,
                    history,
                    commands,
                } = *start;
                inputs.history = history.map(|plan| harness_history::sink(&inner, plan));
                inputs.tools = Some(harness_tools::sink(&inner, id.clone(), commands));
                tokio::spawn(async move {
                    let deadline =
                        tokio::time::Instant::now() + crate::harness::session::SESSION_DEADLINE;
                    // Company sign-in: the token the engine is given, renewed
                    // first if it is inside its margin; none is never sent.
                    if let Some(grant) = grant {
                        match grant.access(&inner).await {
                            Ok((token, renew_at)) => {
                                if let Ok(mut state) = lock(&inner) {
                                    state.harness_keyed(&id, renew_at);
                                }
                                inputs.key = Box::new(move || Ok(token));
                            }
                            Err(_) => {
                                inputs.key =
                                    Box::new(|| Err(crate::harness::HarnessFailure::SignInNeeded))
                            }
                        }
                    }
                    // Workspace MCP tokens (C22): a server without a current
                    // sign-in gets the signed-out placeholder.
                    if !mcp.is_empty() {
                        let (tokens, renew_at) = harness_mcp::scope_tokens(&inner, &mcp).await;
                        let print = crate::harness::mcp::fingerprint(&inputs.binding.mcp, &tokens);
                        if let Ok(mut state) = lock(&inner) {
                            state.harness_mcp_keyed(&id, renew_at, print);
                        }
                        inputs.mcp = tokens;
                    }
                    let outcome = crate::harness::session::run(inputs, cancel, deadline).await;
                    if let Ok(mut state) = lock(&inner) {
                        state.harness_settled(&id, outcome);
                        state.changed();
                    }
                });
            }
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
            Effect::V1Import(job) => import_v1_domain::run(inner, job),
            Effect::ExportDiagnostics { lease } => diagnostics_domain::export(inner, lease),
            Effect::Cleanup => {
                drain_cleanup(inner);
                learner_domain::drain_learner_cleanup(inner);
            }
            Effect::HarnessSignIn(job) => {
                tokio::spawn(harness_identity::sign_in(Arc::clone(inner), *job));
            }
            Effect::HarnessSignedOut(grant) => {
                tokio::spawn(harness_identity::signed_out(Arc::clone(inner), grant));
            }
            Effect::HarnessRenew(job) => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let renewed = match &job.grant {
                        Some(grant) => grant.access(&inner).await.map(Some),
                        None => Ok(None),
                    };
                    let mcp = match (&renewed, &job.mcp) {
                        (Ok(_), Some(grants)) => {
                            Some(harness_mcp::scope_tokens(&inner, grants).await)
                        }
                        _ => None,
                    };
                    if let Ok(mut state) = lock(&inner) {
                        state.harness_renewed(*job, renewed, mcp);
                        state.changed();
                    }
                });
            }
            Effect::HarnessHistory(job) => {
                tokio::spawn(harness_history::run_job(Arc::clone(inner), job));
            }
            Effect::ChooseHarnessProject(context) => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let chosen = inner.adapters.folders.choose_project().await;
                    if let (Some(path), Ok(mut state)) = (chosen, lock(&inner)) {
                        let _ = state.set_harness_project(&context, path);
                        state.changed();
                    }
                });
            }
            Effect::McpCatalog {
                context,
                reference,
                lease,
            } => {
                tokio::spawn(harness_mcp::read_catalog(
                    Arc::clone(inner),
                    context,
                    reference,
                    lease,
                ));
            }
            Effect::McpSignIn(job) => {
                tokio::spawn(harness_mcp::sign_in(Arc::clone(inner), *job));
            }
            Effect::McpSignedOut(grant) => {
                tokio::spawn(harness_mcp::signed_out(Arc::clone(inner), grant));
            }
            Effect::ForgetHarnessHistory(tenant, dir) => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    // A save admitted before the removal finishes first (its
                    // key stays in flight until then, so cleanup waits too);
                    // none is admitted after it. A folder that cannot be
                    // removed now is removed at the next launch.
                    while lock(&inner).is_ok_and(|s| s.history_writers.contains(&tenant)) {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    }
                    let _ = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(dir)).await;
                });
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
            Effect::ConfirmHarness {
                conversation,
                approval,
                nonce,
                request,
            } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let accepted = inner.adapters.confirmer.confirm(nonce, request).await;
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_harness_confirmation(
                            &conversation,
                            &approval,
                            nonce,
                            accepted,
                        );
                        state.changed();
                    }
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
            Effect::ChooseGatewayFile { lease } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let chosen = inner.adapters.gateway_files.choose().await.map(|r| {
                        r.map(|file| (file.name.clone(), file.bytes.to_vec()))
                            .map_err(|error| match error {
                                ImportError::TooLarge => StagedFileFailure::TooLarge,
                                ImportError::Empty => StagedFileFailure::Empty,
                                ImportError::Unsupported => StagedFileFailure::Unreadable,
                            })
                    });
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_file_choice(lease, chosen);
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
            Effect::LoadGatewayLocal { lease } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    // Reads take their turn with the writes (and with
                    // tenant deletion and reset): a read admitted before a
                    // reset finds its lease gone once its turn comes.
                    let Some(writer) = lock(&inner).ok().map(|s| Arc::clone(&s.report_writer))
                    else {
                        return;
                    };
                    let _turn = writer.lock().await;
                    let Some(path) = lock(&inner)
                        .ok()
                        .and_then(|s| s.gateway_local_target(lease))
                    else {
                        return;
                    };
                    let loaded =
                        tokio::task::spawn_blocking(move || crate::gateway_local::load(&path))
                            .await
                            .unwrap_or(Err(crate::sessions::ReportStoreFailure::Unavailable));
                    let effects = match lock(&inner) {
                        Ok(mut state) => {
                            state.settle_gateway_local(lease, loaded);
                            state.changed();
                            gateway_local_effects(&mut state)
                        }
                        Err(_) => return,
                    };
                    run_effects(&inner, effects);
                });
            }
            Effect::StoreGatewayLocal { tenant, generation } => {
                let inner = Arc::clone(inner);
                tokio::spawn(async move {
                    let Some(writer) = lock(&inner).ok().map(|s| Arc::clone(&s.report_writer))
                    else {
                        return;
                    };
                    let _write = writer.lock().await;
                    // Only the newest book is written (under the writer
                    // lock tenant deletion and reset take too).
                    let Some((path, book)) = lock(&inner)
                        .ok()
                        .and_then(|s| s.gateway_local_write(&tenant, generation))
                    else {
                        return;
                    };
                    let stored = tokio::task::spawn_blocking(move || {
                        crate::gateway_local::store(&path, &book).is_ok()
                    })
                    .await
                    .unwrap_or(false);
                    if let Ok(mut state) = lock(&inner) {
                        state.settle_gateway_local_write(&tenant, generation, stored);
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
            Effect::Workbench { operation, request } => {
                tokio::spawn(workbench_domain::run(Arc::clone(inner), operation, request));
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
                    let worker = Arc::clone(&inner);
                    let check = tenant.clone();
                    let stored = tokio::task::spawn_blocking(move || {
                        // Checked and written under the placement lock that
                        // the cleanup ledger takes to remove a deleted
                        // tenant's library: a write either lands before the
                        // removal or finds the tenant gone. Only the newest
                        // generation is written.
                        let _placement = crate::sessions::placement();
                        let write = lock(&worker)
                            .ok()
                            .and_then(|s| s.library_write(&check, generation))?;
                        Some(crate::library::store(
                            &write.root,
                            worker.adapters.keychain.as_ref(),
                            &write.tenant,
                            &write.library,
                        ))
                    })
                    .await
                    .unwrap_or(Some(Err(crate::library::LibraryFailure::Unavailable)));
                    if let (Some(stored), Ok(mut state)) = (stored, lock(&inner)) {
                        state.settle_library_write(&tenant, generation, stored);
                        state.changed();
                    }
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
                                    + s.history_writers.len()
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
                    // API Client work in its turn finishes first; none runs
                    // during the purge.
                    let lane = lock(&inner).map(|s| Arc::clone(&s.workbench.lane)).ok();
                    let _turn = match &lane {
                        Some(lane) => Some(lane.lock().await),
                        None => None,
                    };
                    // Sends run outside the lane. Their epoch was advanced
                    // when reset was admitted; a storage step already past
                    // its check finishes before the purge, and none runs
                    // during or after it.
                    let fence = lock(&inner).map(|s| s.workbench.sending.fence()).ok();
                    let _fenced = match &fence {
                        Some(fence) => Some(fence.write().await),
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
                        // The API Client's private folder (sealed profile and
                        // the built-in collection) goes with its key too;
                        // collections in the operator's folders stay.
                        if let Some(root) = &worker.adapters.workbench_root {
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
                        // Harness history goes with its keys (purged above).
                        if let Some(root) = &worker.adapters.harness.history_root {
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
                        // Diagnostics go with the data (D051).
                        if !worker.diagnostics.purge() {
                            return Err(ResetFailure {
                                stage: ResetStage::Document,
                                reason: VaultError::Unavailable,
                            });
                        }
                        // Import receipts go with the work they describe.
                        if let Some(root) = &worker.adapters.import_root
                            && !crate::import_v1::receipt::purge(root)
                        {
                            return Err(ResetFailure {
                                stage: ResetStage::Document,
                                reason: VaultError::Unavailable,
                            });
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
    let effects = match lock(inner) {
        Ok(mut state) => {
            let now = Instant::now();
            if let Some(stop) = stop {
                state.operations.cancel(id, stop, now);
            }
            let starts = state.operations.complete(id, outcome, now);
            state.sync_scans();
            state.changed();
            let mut effects = vec![Effect::Start(starts)];
            effects.extend(state.take_case_writes());
            effects.extend(gateway_local_effects(&mut state));
            effects
        }
        Err(_) => return,
    };
    run_effects(inner, effects);
}

/// Gateway work to keep on this Mac after settlements: the newest writes,
/// and reads of books that settlements wait for.
fn gateway_local_effects(state: &mut State) -> Vec<Effect> {
    let (writes, reads) = state.take_gateway_local();
    writes
        .into_iter()
        .map(|(tenant, generation)| Effect::StoreGatewayLocal { tenant, generation })
        .chain(
            reads
                .into_iter()
                .map(|lease| Effect::LoadGatewayLocal { lease }),
        )
        .collect()
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
    inference: Option<crate::gateway_inference::InferenceTarget>,
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
        if class == CredentialClass::Inference {
            t.inference_target = inference;
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
    // An inference key is not part of the tenant context: replacing it
    // stops only the run that used the old one (reviews, reads and the
    // workspace choice are untouched).
    if class == CredentialClass::Inference {
        effects.push(Effect::Start(state.retire_inference(&tenant)));
    } else if state.data.active.as_ref() == Some(&tenant) {
        effects.push(Effect::Start(state.reissue_context()));
    } else if state.is_setup_tenant(&tenant) {
        // A draft under setup (C29) has its own context: replacing its
        // credential retires that context (and the binding cached under it)
        // without touching the shell's active context.
        state.reissue_setup_context();
    }
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
    if class == CredentialClass::Inference {
        effects.push(Effect::Start(state.retire_inference(&tenant)));
    } else if state.data.active.as_ref() == Some(&tenant) {
        effects.push(Effect::Start(state.reissue_context()));
    } else if state.is_setup_tenant(&tenant) {
        state.reissue_setup_context();
    }
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
        CredentialClass::Inference => state
            .tenant(tenant)
            .and_then(|t| t.inference_target.as_ref())
            .map(|t| t.endpoint.clone())
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
        if state.cleaning {
            state.clean_again = true;
            return;
        }
        if records.is_empty() || state.storage == StorageHealth::Resetting {
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
                    let library =
                        crate::library::forget_record(worker.adapters.library_root.as_deref(), r);
                    investigation
                        && library
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
        let again = lock(&inner).is_ok_and(|mut s| std::mem::take(&mut s.clean_again));
        if again {
            drain_cleanup(&inner);
        }
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
    inner.diagnostics.record(crate::diagnostics::Event::Reset {
        outcome: match &result {
            Ok(_) => "done".to_owned(),
            Err(failure) => format!("failed-{}", refusal_code_of(&failure.stage)),
        },
    });
    let (starts, cleanup) = {
        let Ok(mut state) = lock(inner) else {
            return;
        };
        match result {
            Ok(unconfirmed) => {
                inner.session.clear();
                state.data = Data::default();
                // Receipts and diagnostics were purged with the data.
                state.v1_import = import_v1_domain::V1Import::default();
                state.diagnostics = diagnostics_domain::DiagnosticsState::default();
                state.storage = StorageHealth::Ready;
                state.reset_failure = None;
                state.durability_unconfirmed = unconfirmed;
                state.consumed_reviews.clear();
                state.scans.clear();
                state.forget_all_gateway_local();
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

/// Wall-clock seconds since the Unix epoch, moved by the fixture clock offset.
fn wall_secs(clock_offset: u32) -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
        .saturating_add(clock_offset)
}

#[cfg(feature = "test-fixtures")]
impl Core {
    /// Whether an armed fixture interruption is still waiting for its exact
    /// commit point. Read-only; tests must not simulate exit before it fires.
    pub fn v1_import_interruption_pending(&self) -> bool {
        lock(&self.inner)
            .map(|state| state.v1_import.interrupt_at.is_some())
            .unwrap_or(true)
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
