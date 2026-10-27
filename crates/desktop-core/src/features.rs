//! The desktop feature catalogue presented by the shell.
//!
//! Every product feature is listed with an explicit availability. A feature
//! is available only when its owning chunk has shipped; until then the shell
//! shows an unavailable destination naming the chunk, never a stub that looks
//! like working behavior.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Which desktop presentation the shell renders (D058). `Legacy` is the
/// C01 shell; `V2` is the "Refined Native" shell introduced in C28 and the
/// only one after C34. Persisted; changed only by `Intent::SetUiVariant`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum UiVariant {
    #[default]
    Legacy,
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
            FeatureEntry {
                id: FeatureId::Inbox,
                label: "Inbox".to_owned(),
                availability: Availability::Unavailable {
                    reason: "The Inbox arrives in C30".to_owned(),
                    planned_chunk: "C30".to_owned(),
                },
                shortcut: Some(2),
                sidebar: true,
            },
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
            FeatureEntry::available(FeatureId::Import, "Import from v1", None, false),
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
