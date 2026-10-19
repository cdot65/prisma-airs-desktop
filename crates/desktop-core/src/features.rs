//! The desktop feature catalogue presented by the shell.
//!
//! Every product feature is listed with an explicit availability. A feature
//! is available only when its owning chunk has shipped; until then the shell
//! shows an unavailable destination naming the chunk, never a stub that looks
//! like working behavior.

use serde::Serialize;
use ts_rs::TS;

/// Stable identifier of a top-level desktop feature destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FeatureId {
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
}

const PLANNED: [(FeatureId, &str, &str); 5] = [
    (FeatureId::RedTeam, "Red Team & Models", "C13"),
    (FeatureId::ApiClient, "API Client", "C14"),
    (FeatureId::Gateway, "AI Gateway", "C16"),
    (FeatureId::Harness, "Harness", "C19"),
    (FeatureId::Import, "Import from v1", "C23"),
];

/// The catalogue shipped by this build, in navigation order.
pub fn catalogue() -> Vec<FeatureEntry> {
    // Runtime Security shipped in C03, Academy learning in C05, management
    // reads (Administration) in C08, Sessions & Reports in C11, the Case
    // Library in C12.
    let mut entries = vec![
        FeatureEntry {
            id: FeatureId::Runtime,
            label: "Runtime Security".to_owned(),
            availability: Availability::Available,
        },
        FeatureEntry {
            id: FeatureId::Academy,
            label: "Academy".to_owned(),
            availability: Availability::Available,
        },
        FeatureEntry {
            id: FeatureId::Administration,
            label: "Administration".to_owned(),
            availability: Availability::Available,
        },
        FeatureEntry {
            id: FeatureId::Sessions,
            label: "Sessions & Reports".to_owned(),
            availability: Availability::Available,
        },
        FeatureEntry {
            id: FeatureId::Library,
            label: "Case Library".to_owned(),
            availability: Availability::Available,
        },
    ];
    entries.extend(PLANNED.iter().map(|(id, label, chunk)| FeatureEntry {
        id: *id,
        label: (*label).to_owned(),
        availability: Availability::Unavailable {
            reason: "Not available in this build".to_owned(),
            planned_chunk: (*chunk).to_owned(),
        },
    }));
    entries.push(FeatureEntry {
        id: FeatureId::Settings,
        label: "Settings".to_owned(),
        availability: Availability::Available,
    });
    entries
}
