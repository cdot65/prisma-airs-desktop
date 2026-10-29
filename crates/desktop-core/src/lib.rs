//! Prisma AIRS Desktop v2 application core.
//!
//! The core owns native authority: callers hold a principal-bound
//! [`AuthorityHandle`] whose only operations are [`AuthorityHandle::dispatch`]
//! for a finite [`Intent`] and [`AuthorityHandle::observe`] for a
//! [`ViewScope`]. It does not depend on Tauri, React or any OS adapter; the
//! native host creates one [`Core`] with its [`Adapters`] (ADR 0001).

pub mod academy;
pub mod admin_inventory;
mod authority;
pub mod avatar;
pub mod changes;
pub mod context;
mod delivery;
pub mod diagnostics;
pub mod features;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
pub mod gateway;
pub mod gateway_changes;
pub mod gateway_files;
pub mod gateway_inference;
pub mod gateway_local;
pub mod gateway_observe;
pub mod harness;
pub mod import;
pub mod import_v1;
pub mod learner;
pub mod library;
pub mod management;
pub mod observation;
mod observe;
pub mod offline;
pub mod red_team;
pub mod runtime;
pub mod scan;
pub mod scanning;
pub mod sessions;
pub mod store;
pub mod tenants;
pub mod transport;
pub mod vault;
pub mod wire;
pub mod workbench;

pub use authority::{
    ACADEMY_ORIGIN, AcademyCallHandle, AcademyCallResult, ActiveContext, Adapters, AttentionFace,
    AuthorityHandle, CatalogSource, ChangeFailure, ChangeResultView, ChangeReviewState,
    ChangeReviewView, ChangesView, ConfirmationRequest, Core, CoreConfig, CredentialStore,
    DownloadStatus, DownloadView, EndpointReviewView, ExportKind, ExportState, ExportView,
    FileSaver, GatewayView, HeldItem, HoldKind, ImagePicker, ImportFile, ImportPicker, ImportView,
    InboxEntry, InboxKind, InboxTarget, InboxView, Intent, InvestigationView, LibraryNotice,
    LibraryState, LibraryView, LocalDraftView, MAX_TENANTS, Observation, OfflineState, OfflineView,
    OperatorConfirmation, OverviewView, PagePrinter, Principal, ProductRead, ProductReadState,
    ReadState, ReadStatus, Readback, Receipt, Refusal, ReportTarget, ReportUndoView, ReportsState,
    ReportsView, ResetFailure, ResetStage, RotationStage, RotationView, ScanProgress,
    ScanRecordView, ScanState, ScanView, SecretPresenter, SecretState, SelectedWorkspace,
    SessionsView, ShellSnapshot, SnoozeReceipt, StagedFileFailure, StagedFileState, StagedFileView,
    StorageHealth, SwitchResolution, TenantView, UrlOpener, ViewScope,
};
pub use authority::{
    CollectionView, ConflictKind, DocumentView, EnvironmentView, FolderPicker, RecentView,
    Resolution, SecretView, TabView, WorkbenchHandle, WorkbenchProblem, WorkbenchRequest,
    WorkbenchResult, WorkbenchView,
};
pub use delivery::{Delivery, DeliverySession, SEQUENCE_LIMIT};
pub use observe::{Subscription, Update};
pub use wire::Theme;
