//! The desktop feature catalogue presented by the shell.
//!
//! Every product feature is listed with an explicit availability. A feature
//! is available only when its owning chunk has shipped; until then the shell
//! shows an unavailable destination naming the chunk, never a stub that looks
//! like working behavior.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which desktop presentation the shell renders (D058). `Legacy` was the
/// C01 shell, removed in C44; `V2` is the "Refined Native" shell introduced
/// in C28 and the only one. The value stays on the wire for compatibility
/// (the snapshot always says `v2`; `Intent::SetUiVariant` is accepted and
/// ignored) and the persisted field is no longer read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum UiVariant {
    Legacy,
    #[default]
    V2,
}

/// First-run state of the v2 onboarding wizard (D058, C29 realises D013).
/// Persisted; it never moves back to `Needed`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OnboardingState {
    #[default]
    Needed,
    InProgress,
    Done,
}

/// A product a tenant reads (C29, DESIGN-V2 §6): only products with an
/// admitted service exist here; AIS and AES are not admitted and have no id.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProductId {
    RuntimeSecurity,
    Gateway,
    RedTeam,
    ModelSkill,
}

/// A tenant's environment, chosen at setup (C29; prototype `env`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TenantEnvironment {
    #[default]
    Production,
    Staging,
    Other,
}

/// Which wizard a setup runs in (C29).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SetupMode {
    FirstRun,
    Add,
}

/// One verification line (C29, screens/wizard.md "Verify").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum VerifyLine {
    ScanKey,
    ManagementClient,
    Gateway,
    RedTeam,
    ModelSkill,
    /// Agentic Identity Security: listed as not available, never checked.
    Ais,
    /// Agentic Endpoint Security: listed as not available, never checked.
    Aes,
}

/// How a verification line ended. `Stored` is an explicit non-result: the
/// credential exists but no admitted read exercises it here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum VerifyState {
    Pending,
    Pass,
    Warning,
    Fail,
    Stored,
    Skipped,
    /// No admitted service exists for this line (AIS, AES); never a result.
    Unavailable,
}

/// One streamed verification result (core-owned).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerifyStep {
    pub line: VerifyLine,
    /// The admitted product the line reports on; none for an unavailable
    /// product's line.
    pub product: Option<ProductId>,
    pub state: VerifyState,
    pub reason: String,
}

/// A warning a verification run produced for an admitted product.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerifyWarning {
    pub product: ProductId,
    pub reason: String,
}

/// The current verification run as the shell shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerifyView {
    /// Counts runs; a later run replaces an earlier one's lines.
    pub generation: u32,
    pub steps: Vec<VerifyStep>,
    /// Every line has settled.
    pub done: bool,
    /// Some line failed.
    pub failed: bool,
    /// A required line failed (the scan key on first run): completion is
    /// refused. An optional line's failure is carried as a warning instead.
    pub blocking: bool,
    /// The inputs changed since this run started: it no longer counts.
    pub stale: bool,
    /// What completion records as the tenant's setup warnings: warning
    /// lines and failed optional lines.
    pub warnings: Vec<VerifyWarning>,
}

/// A starter pack: the bundled cases for one chosen product.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PackView {
    pub product: ProductId,
    pub count: u32,
}

/// The tenant under setup (C29): a draft with its own context, not yet a
/// tenant. Absent when no wizard is open.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetupView {
    pub mode: SetupMode,
    /// The context credential and verify intents use for the draft.
    pub context: crate::context::ContextRef,
    /// The draft's profile and credential states (no secret material).
    pub tenant: crate::TenantView,
    pub products: Vec<ProductId>,
    /// First run requires the scan key; the add sheet does not.
    pub scan_required: bool,
    pub packs: Vec<PackView>,
    /// The Gateway workspace chosen for the draft, with the reference its
    /// inference key is configured under.
    pub workspace: Option<crate::authority::SelectedWorkspace>,
    pub verify: Option<VerifyView>,
    /// The current inputs were verified without a blocking failure:
    /// completion is admitted (optional failures travel as warnings).
    pub completable: bool,
}

/// A recorded verify warning for one tenant: the Inbox item type C29
/// defines and C30 renders (`InboxItem` kind `setupWarning`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SetupWarning {
    /// `vw-<n>`, unique within the tenant.
    pub id: String,
    pub product: ProductId,
    pub reason: String,
    /// Wall-clock seconds since the Unix epoch when it was recorded
    /// (`u32`: a JSON number, never a bigint, on the TypeScript side).
    pub recorded_secs: u32,
}

/// A Runtime API key named by its identifier and name (never a secret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct KeyRef {
    pub id: String,
    pub name: String,
}

/// Whether the additional key's issuance was confirmed by readback (C31).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RotationIssuance {
    Confirmed,
    /// The version settled unknown: a key is confirmed only once the key
    /// inventory shows one for the application besides the old key.
    Unconfirmed,
}

/// Where the old key's removal stands (C31).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RotationRemoval {
    Pending,
    /// Sent but not read back absent: reconciled by a complete inventory.
    Unconfirmed,
    /// Refused or failed: the reminder stays, the completion time unset.
    Rejected {
        reason: String,
    },
    Done,
}

/// One Runtime API key rotation of a tenant (C31, I7), persisted so its two
/// timestamps are stored once and survive reads, switches and relaunches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Rotation {
    pub application: String,
    pub old_key: KeyRef,
    pub new_key: Option<KeyRef>,
    pub issuance: RotationIssuance,
    /// The change result that issued the key (its one-time secret lives
    /// there, for the context it was applied in, never here). Session-only:
    /// operation IDs restart with the core, so a relaunch never restores
    /// this link (a reused ID must not point at another key; C31).
    #[serde(skip)]
    #[ts(skip)]
    pub issued_by: Option<String>,
    /// The key name the issuing review named: the only key that can confirm
    /// an unconfirmed issuance.
    #[serde(default)]
    pub pending_name: Option<String>,
    pub issued_at: u32,
    /// The operator's local statement that the application uses the new key.
    pub deployment_acknowledged_at: Option<u32>,
    pub removal: RotationRemoval,
    /// When the old key was read back absent (remote readback).
    pub old_key_removed_at: Option<u32>,
}

/// Stable identifier of a top-level desktop feature destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FeatureId {
    /// v2 only (C28): the tenant's Home.
    Home,
    /// v2 only (C28): the Inbox.
    Inbox,
    Runtime,
    Academy,
    Administration,
    Sessions,
    Library,
    RedTeam,
    ApiClient,
    Gateway,
    Harness,
    Import,
    Settings,
}

/// Whether a destination is usable in this build.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Availability {
    Available,
    /// Not implemented yet; `plannedChunk` names the owning chunk.
    #[serde(rename_all = "camelCase")]
    Unavailable {
        reason: String,
        planned_chunk: String,
    },
}

/// One navigation destination in the shell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FeatureEntry {
    pub id: FeatureId,
    pub label: String,
    pub availability: Availability,
    /// The native menu accelerator digit (⌘1–⌘9), when the destination has
    /// one. The menu, the sidebar and the palette all read this one value.
    pub shortcut: Option<u8>,
    /// Whether the v2 sidebar lists it; false entries stay reachable from
    /// the Go menu and the command palette.
    pub sidebar: bool,
}

impl FeatureEntry {
    fn available(id: FeatureId, label: &str, shortcut: Option<u8>, sidebar: bool) -> Self {
        Self {
            id,
            label: label.to_owned(),
            availability: Availability::Available,
            shortcut,
            sidebar,
        }
    }
}

/// The catalogue for a presentation, in navigation order: the single
/// inventory the native menu, the sidebar and the command palette read.
pub fn catalogue_for(variant: UiVariant) -> Vec<FeatureEntry> {
    match variant {
        UiVariant::Legacy => catalogue(),
        UiVariant::V2 => vec![
            FeatureEntry::available(FeatureId::Home, "Home", Some(1), true),
            // Listed with its shortcut from the start so ⌘2 never moves; the
            // Inbox view itself lands in C30 and is unavailable until then.
            FeatureEntry::available(FeatureId::Inbox, "Inbox", Some(2), true),
            FeatureEntry::available(FeatureId::Harness, "Harness", Some(3), true),
            FeatureEntry::available(FeatureId::Academy, "Academy", Some(4), true),
            FeatureEntry::available(FeatureId::Library, "Test Cases", Some(5), true),
            FeatureEntry::available(FeatureId::ApiClient, "API Client", Some(6), true),
            FeatureEntry::available(FeatureId::Administration, "Administration", Some(7), true),
            // Inside Administration's accordion in v2; still direct Go/palette
            // destinations so nothing the legacy shell reached is lost.
            FeatureEntry::available(FeatureId::Runtime, "Runtime Security", None, false),
            FeatureEntry::available(FeatureId::Gateway, "AI Gateway", None, false),
            FeatureEntry::available(FeatureId::RedTeam, "Red Team & Models", None, false),
            FeatureEntry::available(FeatureId::Sessions, "Sessions & Reports", None, false),
            FeatureEntry::available(FeatureId::Settings, "Settings", None, false),
        ],
    }
}

/// The first sidebar destination of a presentation: where a fresh session
/// lands and where navigation falls back after a variant change.
pub fn default_feature(variant: UiVariant) -> FeatureId {
    catalogue_for(variant)
        .into_iter()
        .find(|e| e.sidebar)
        .map(|e| e.id)
        .unwrap_or(FeatureId::Settings)
}

/// The catalogue shipped by this build, in navigation order.
pub fn catalogue() -> Vec<FeatureEntry> {
    // Runtime Security shipped in C03, Academy learning in C05, management
    // reads (Administration) in C08, Sessions & Reports in C11, the Case
    // Library in C12, Red Team and Model/skill observation in C13, API
    // Client projects and documents in C14, AI Gateway workspace and
    // configuration reads in C16, the native Harness in C19, and the
    // reviewed import of v1 work in C23.
    let mut entries = vec![
        FeatureEntry {
            id: FeatureId::Runtime,
            label: "Runtime Security".to_owned(),
            availability: Availability::Available,
            shortcut: Some(1),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::Academy,
            label: "Academy".to_owned(),
            availability: Availability::Available,
            shortcut: Some(2),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::Administration,
            label: "Administration".to_owned(),
            availability: Availability::Available,
            shortcut: Some(3),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::Sessions,
            label: "Sessions & Reports".to_owned(),
            availability: Availability::Available,
            shortcut: Some(4),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::Library,
            label: "Case Library".to_owned(),
            availability: Availability::Available,
            shortcut: Some(5),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::RedTeam,
            label: "Red Team & Models".to_owned(),
            availability: Availability::Available,
            shortcut: Some(6),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::ApiClient,
            label: "API Client".to_owned(),
            availability: Availability::Available,
            shortcut: Some(7),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::Gateway,
            label: "AI Gateway".to_owned(),
            availability: Availability::Available,
            shortcut: Some(8),
            sidebar: true,
        },
        FeatureEntry {
            id: FeatureId::Harness,
            label: "Harness".to_owned(),
            availability: Availability::Available,
            shortcut: Some(9),
            sidebar: true,
        },
    ];
    entries.push(FeatureEntry {
        id: FeatureId::Import,
        label: "Import from v1".to_owned(),
        availability: Availability::Available,
        shortcut: None,
        sidebar: true,
    });
    entries.push(FeatureEntry {
        id: FeatureId::Settings,
        label: "Settings".to_owned(),
        availability: Availability::Available,
        shortcut: None,
        sidebar: true,
    });
    entries
}
