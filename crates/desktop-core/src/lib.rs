//! Prisma AIRS Desktop v2 application core.
//!
//! The core owns native authority: callers hold a principal-bound
//! [`AuthorityHandle`] whose only operations are [`AuthorityHandle::dispatch`]
//! for a finite [`Intent`] and [`AuthorityHandle::observe`] for a
//! [`ViewScope`]. It does not depend on Tauri, React or any OS adapter; the
//! native host creates one [`Core`] and injects adapters as later chunks add
//! them (ADR 0001).

mod authority;
mod delivery;
pub mod features;
mod observe;
pub mod wire;

pub use authority::{
    AuthorityHandle, Core, CoreConfig, Intent, Observation, Principal, Receipt, Refusal,
    ShellSnapshot, ViewScope,
};
pub use delivery::{Delivery, DeliverySession, SEQUENCE_LIMIT};
pub use observe::{Subscription, Update};
pub use wire::Theme;
