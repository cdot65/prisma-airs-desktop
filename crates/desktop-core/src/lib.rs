//! Prisma AIRS Desktop v2 application core.
//!
//! The core owns native authority: callers hold a principal-bound
//! [`AuthorityHandle`] whose only operations are [`AuthorityHandle::dispatch`]
//! for a finite [`Intent`] and [`AuthorityHandle::observe`] for a
//! [`ViewScope`]. It does not depend on Tauri, React or any OS adapter; the
//! native host creates one [`Core`] with its [`Adapters`] (ADR 0001).

mod authority;
pub mod context;
mod delivery;
pub mod features;
#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixtures;
pub mod import;
mod observe;
pub mod runtime;
pub mod scan;
pub mod store;
pub mod tenants;
pub mod transport;
pub mod vault;
pub mod wire;

pub use authority::{
    ActiveContext, Adapters, AuthorityHandle, Core, CoreConfig, CredentialStore,
    EndpointReviewView, ImportFile, ImportPicker, ImportView, Intent, MAX_TENANTS, Observation,
    Principal, Receipt, Refusal, ResetFailure, ResetStage, ScanState, ScanView, ShellSnapshot,
    StorageHealth, TenantView, ViewScope,
};
pub use delivery::{Delivery, DeliverySession, SEQUENCE_LIMIT};
pub use observe::{Subscription, Update};
pub use wire::Theme;
