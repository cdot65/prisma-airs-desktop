//! Prisma AIRS Desktop v2 application core.
//!
//! The core owns native authority: callers hold a principal-bound
//! [`AuthorityHandle`] whose only operations are [`AuthorityHandle::dispatch`]
//! for a finite [`Intent`] and [`AuthorityHandle::observe`] for a
//! [`ViewScope`]. It does not depend on Tauri, React or any OS adapter; the
//! native host creates one [`Core`] with its [`Adapters`] (ADR 0001).

pub mod academy;
mod authority;
pub mod avatar;
pub mod changes;
pub mod context;
mod delivery;
pub mod features;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
pub mod import;
pub mod learner;
pub mod management;
mod observe;
pub mod offline;
pub mod runtime;
pub mod scan;
pub mod scanning;
pub mod store;
pub mod tenants;
pub mod transport;
pub mod vault;
pub mod wire;

pub use authority::{
    ACADEMY_ORIGIN, AcademyCallHandle, AcademyCallResult, ActiveContext, Adapters, AuthorityHandle,
    ChangeFailure, ChangeResultView, ChangeReviewState, ChangeReviewView, ChangesView,
    ConfirmationRequest, Core, CoreConfig, CredentialStore, DownloadStatus, DownloadView,
    EndpointReviewView, ImagePicker, ImportFile, ImportPicker, ImportView, Intent, LocalDraftView,
    MAX_TENANTS, Observation, OfflineState, OfflineView, OperatorConfirmation, PagePrinter,
    Principal, Readback, Receipt, Refusal, ResetFailure, ResetStage, ScanProgress, ScanRecordView,
    ScanState, ScanView, SecretPresenter, SecretState, ShellSnapshot, StagedFileFailure,
    StagedFileState, StagedFileView, StorageHealth, TenantView, UrlOpener, ViewScope,
};
pub use delivery::{Delivery, DeliverySession, SEQUENCE_LIMIT};
pub use observe::{Subscription, Update};
pub use wire::Theme;
