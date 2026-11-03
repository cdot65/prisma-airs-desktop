//! C23: reviewed import of v1 work into v2 (D050).
//!
//! v1 is a read-only source: its files are read without following links or
//! creating anything, and its Keychain items through a type with no write
//! method. An inspection reads files only; protected items are read only
//! for the categories the operator selects. The operator reviews a plan of
//! items, each `ready`, `alreadyPresent`, `conflict` or `blocked` with a
//! reason; nothing is silently skipped. Importing writes each ready item
//! through its v2 owner under a restartable receipt, and a rollback removes
//! only what that receipt wrote and nobody changed since. No credential,
//! session, sign-in or approval is ever transferred: imported tenants need
//! fresh credentials, Academy drafts need the same account signed in, and
//! imported conversations are inspect-only.

pub mod parse;
pub mod plan;
pub mod receipt;
pub mod source;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use source::{V1_SERVICE, V1Keychain, V1Source};

/// What can be imported, in the order it is shown and applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// Tenant names and TSG IDs (`tenants.json`): keyless v2 tenants.
    Tenants,
    /// Each tenant's scan endpoint, from its protected v1 credential (only
    /// the endpoint is read out; the item holds secrets).
    Endpoints,
    /// Light, dark or system appearance (`appearance.json`).
    Settings,
    /// Each protected tenant's saved cases (protected).
    Cases,
    /// FP/FN reports (`fpfn-reports.json`).
    Reports,
    /// Unsaved Academy lab drafts of the signed-in account (protected key).
    AcademyDrafts,
    /// API Client recent collections and private environments (C14).
    ApiClient,
    /// Harness conversations (protected), inspect-only.
    Conversations,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Tenants,
        Category::Endpoints,
        Category::Settings,
        Category::Cases,
        Category::Reports,
        Category::AcademyDrafts,
        Category::ApiClient,
        Category::Conversations,
    ];

    /// Whether planning it reads v1 Keychain items.
    pub fn protected(self) -> bool {
        matches!(
            self,
            Category::Endpoints
                | Category::Cases
                | Category::AcademyDrafts
                | Category::Conversations
        )
    }
}

/// Why something cannot be imported. Shown by name; never silent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Problem {
    /// v1 has nothing of this kind here.
    Missing,
    /// v1 is running (it holds its instance lock).
    SourceLocked,
    /// macOS denied reading it.
    AccessDenied,
    /// The Keychain is locked or unavailable.
    KeychainLocked,
    /// It could not be read or authenticated.
    Corrupt,
    /// A version this import does not know.
    UnsupportedVersion,
    /// Over its size or count limit.
    TooLarge,
    /// A link, a non-file or an unexpected name.
    PathRejected,
    /// v1 was retiring it.
    SourceRetired,
    /// A session-only v1 tenant: nothing of it was stored.
    NoStoredWork,
    /// It belongs to another account or TSG.
    AccountMismatch,
    /// Academy drafts need their account signed in.
    SignInRequired,
    /// v1's endpoint is not an HTTPS origin v2 can bind to.
    EndpointNotMappable,
    /// v2 has no equivalent.
    NoV2Equivalent,
    /// The v2 destination is full.
    DestinationFull,
    /// It changed since the review.
    SourceChanged,
    /// Its v1 tenant is not imported (skipped, or no target chosen).
    NoTarget,
    /// The import's own record could not be saved durably: nothing more is
    /// written until it can be.
    NotRecorded,
}

/// A plan item's state at review.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind", content = "problem")]
pub enum ItemStatus {
    Ready,
    /// Already in v2 exactly as v1 has it: nothing to write.
    AlreadyPresent,
    /// v2 holds something different under the same identity: never
    /// overwritten.
    Conflict,
    Blocked(Problem),
}

/// Lowercase hex SHA-256 of canonical JSON (keys sorted by `serde_json`'s
/// map): content identity for duplicates, conflicts and rollback.
pub fn content_hash(value: &impl Serialize) -> String {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_value(value)
        .and_then(|v| serde_json::to_vec(&v))
        .unwrap_or_default();
    hex::encode(Sha256::digest(&canonical))
}

/// Where an import or rollback may be interrupted (fixture builds stop
/// there as if the app ended).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitPoint {
    /// The receipt was written; no item started.
    Created,
    /// An item was noted `writing`; its owner write has not happened.
    Writing,
    /// An item's owner write happened; it is not yet noted `applied`.
    Written,
    /// An item was noted `applied`.
    Applied,
    /// A rollback noted an item `reverting`; nothing was reverted.
    Reverting,
    /// A rollback reverted an item's entries; it is not yet noted.
    Reverted,
}
