//! The Administration inventory (C31, screens/administration-inventory.md):
//! every product the prototype showed, each tab's disposition, the admitted
//! reads behind it and the admitted reviewed actions it may offer, with the
//! actions the UI must never offer named too. Generated once from the
//! table and pinned by tests against the change matrices
//! (`changes::is_allowed`): a label can never outrun what the core admits.

use serde::Serialize;
use ts_rs::TS;

use crate::changes::{ChangeAction, ChangeKind};

/// A product of the Administration accordion (the prototype's six).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AdminProductId {
    RuntimeSecurity,
    Gateway,
    RedTeam,
    ModelSkill,
    AgenticIdentity,
    AgenticEndpoint,
}

/// Whether a product has an admitted service in this release.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum AdminAvailability {
    Available,
    /// No admitted service: the card and accordion entry name the gate; no
    /// inventory, no action, no review, no verification, no Inbox item.
    Unavailable {
        reason: String,
    },
}

/// What a tab shows and may do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TabDisposition {
    /// An admitted read; rows shown; no change.
    Read,
    /// An admitted read with reviewed changes (the listed actions only).
    Change,
    /// No admitted service: the notice with its gate; no rows, no actions.
    Unavailable { reason: String },
    /// A prototype tab not carried (named with its reason; never rendered).
    Removed { reason: String },
}

/// One reviewed action a tab offers, with the label the UI shows for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminAction {
    pub kind: ChangeKind,
    pub action: ChangeAction,
    /// "New topic…", "Issue additional key…", "Remove…", …
    pub label: String,
    /// A row action (else a creation offered above the table).
    pub row: bool,
}

/// A (kind, action) the UI must never offer on this tab; the core refuses it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DeniedAction {
    pub kind: ChangeKind,
    pub action: ChangeAction,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminTab {
    /// Stable id (kebab-case of the label).
    pub id: String,
    pub label: String,
    pub disposition: TabDisposition,
    /// The admitted reads behind the tab, by name (documentation of
    /// provenance; the presentation reads through the existing views).
    pub reads: Vec<String>,
    pub actions: Vec<AdminAction>,
    pub denied: Vec<DeniedAction>,
    /// The Runtime inventory the tab shows (its `ManagementResource` id in
    /// the presentation), when one.
    pub resource: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminProduct {
    pub id: AdminProductId,
    pub label: String,
    pub availability: AdminAvailability,
    /// Read-only everywhere (Red Team, model/skill): never a change.
    pub read_only: bool,
    pub tabs: Vec<AdminTab>,
}

/// The inventory scope (static per build).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdminInventoryView {
    pub products: Vec<AdminProduct>,
}

const AIS_GATE: &str =
    "Not available in this release · needs an admitted Agentic Identity Security service (D058)";
const AES_GATE: &str =
    "Not available in this release · needs an admitted Agentic Endpoint Security service (D058)";

fn tab(label: &str, disposition: TabDisposition, reads: &[&str]) -> AdminTab {
    AdminTab {
        id: label
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .trim_matches('-')
            .to_owned(),
        label: label.to_owned(),
        disposition,
        reads: reads.iter().map(|r| (*r).to_owned()).collect(),
        actions: Vec::new(),
        denied: Vec::new(),
        resource: None,
    }
}

fn act(kind: ChangeKind, action: ChangeAction, label: &str, row: bool) -> AdminAction {
    AdminAction {
        kind,
        action,
        label: label.to_owned(),
        row,
    }
}

fn deny(kind: ChangeKind, action: ChangeAction, reason: &str) -> DeniedAction {
    DeniedAction {
        kind,
        action,
        reason: reason.to_owned(),
    }
}

fn read_tab(label: &str, reads: &[&str]) -> AdminTab {
    tab(label, TabDisposition::Read, reads)
}

fn change_tab(
    label: &str,
    reads: &[&str],
    resource: Option<&str>,
    actions: Vec<AdminAction>,
    denied: Vec<DeniedAction>,
) -> AdminTab {
    let mut t = tab(label, TabDisposition::Change, reads);
    t.resource = resource.map(str::to_owned);
    t.actions = actions;
    t.denied = denied;
    t
}

fn unavailable_tab(label: &str, reason: &str) -> AdminTab {
    tab(
        label,
        TabDisposition::Unavailable {
            reason: reason.to_owned(),
        },
        &[],
    )
}

fn removed_tab(label: &str, reason: &str) -> AdminTab {
    tab(
        label,
        TabDisposition::Removed {
            reason: reason.to_owned(),
        },
        &[],
    )
}

/// The inventory, exactly as screens/administration-inventory.md tables it.
pub fn inventory() -> AdminInventoryView {
    use ChangeAction as A;
    use ChangeKind as K;
    let runtime = AdminProduct {
        id: AdminProductId::RuntimeSecurity,
        label: "AI Runtime Security".to_owned(),
        availability: AdminAvailability::Available,
        read_only: false,
        tabs: vec![
            {
                let mut t = read_tab(
                    "Overview",
                    &["ManagementResource (counts)", "ScanRecordView"],
                );
                t.resource = None;
                t
            },
            read_tab("Playground", &["scan.rs (synchronous scan)"]),
            change_tab(
                "Security profiles",
                &["ManagementResource::Profiles"],
                Some("profiles"),
                vec![
                    act(K::Profiles, A::Create, "New profile…", false),
                    act(K::Profiles, A::Update, "Edit…", true),
                    act(K::Profiles, A::Delete, "Delete…", true),
                ],
                vec![],
            ),
            change_tab(
                "Custom topics",
                &["ManagementResource::Topics"],
                Some("topics"),
                vec![
                    act(K::Topics, A::Create, "New topic…", false),
                    act(K::Topics, A::Update, "Edit…", true),
                    act(K::Topics, A::Delete, "Delete…", true),
                ],
                vec![],
            ),
            change_tab(
                "Applications",
                &["ManagementResource::Applications"],
                Some("applications"),
                vec![
                    act(K::Applications, A::Update, "Edit…", true),
                    act(K::Applications, A::Delete, "Delete…", true),
                ],
                vec![deny(
                    K::Applications,
                    A::Create,
                    "not admitted: applications are created with their first API key",
                )],
            ),
            change_tab(
                "API keys",
                &["ManagementResource::ApiKeys"],
                Some("apiKeys"),
                vec![
                    act(K::ApiKeys, A::Create, "New API key…", false),
                    act(K::ApiKeys, A::Version, "Issue additional key…", true),
                    act(K::ApiKeys, A::Rotate, "Regenerate…", true),
                    act(K::ApiKeys, A::Delete, "Remove…", true),
                ],
                vec![deny(
                    K::ApiKeys,
                    A::Update,
                    "not admitted: a key's fields are not editable",
                )],
            ),
            change_tab(
                "DLP",
                &[
                    "ManagementResource::DlpCatalog",
                    "ManagementResource::DataPatterns",
                    "ManagementResource::DataProfiles",
                    "ManagementResource::FilteringProfiles",
                    "ManagementResource::Dictionaries",
                ],
                Some("dataPatterns"),
                vec![
                    act(K::DataPatterns, A::Create, "New data pattern…", false),
                    act(K::DataPatterns, A::Update, "Edit…", true),
                    act(K::DataPatterns, A::Delete, "Delete…", true),
                    act(K::Dictionaries, A::Create, "New dictionary…", false),
                    act(K::Dictionaries, A::Update, "Edit…", true),
                    act(K::Dictionaries, A::Delete, "Delete…", true),
                    act(K::DataProfiles, A::Create, "New data profile…", false),
                    act(K::DataProfiles, A::Update, "Edit…", true),
                    act(K::FilteringProfiles, A::Update, "Edit…", true),
                ],
                vec![
                    deny(K::DataProfiles, A::Delete, "not admitted"),
                    deny(K::FilteringProfiles, A::Create, "not admitted"),
                    deny(K::FilteringProfiles, A::Delete, "not admitted"),
                ],
            ),
            {
                let mut t = read_tab(
                    "Deployment profiles",
                    &["ManagementResource::DeploymentProfiles"],
                );
                t.resource = Some("deploymentProfiles".to_owned());
                t
            },
        ],
    };
    let gateway = AdminProduct {
        id: AdminProductId::Gateway,
        label: "AI Gateway".to_owned(),
        availability: AdminAvailability::Available,
        read_only: false,
        tabs: vec![
            read_tab(
                "Overview",
                &["GatewayRead::Observe", "GatewayRead::Prerequisites"],
            ),
            change_tab(
                "Workspaces",
                &["GatewayRead::Inventory", "GatewayRead::Detail"],
                None,
                vec![
                    act(K::GatewayWorkspaces, A::Create, "New workspace…", false),
                    act(K::GatewayWorkspaces, A::Update, "Edit…", true),
                    act(K::GatewayWorkspaces, A::Delete, "Archive…", true),
                ],
                vec![deny(K::GatewayWorkspaces, A::Rotate, "not admitted")],
            ),
            change_tab(
                "Routes",
                &["GatewayRead::Inventory", "GatewayRead::Detail"],
                None,
                vec![
                    act(K::GatewayRoutes, A::Create, "New route…", false),
                    act(K::GatewayRoutes, A::Update, "Edit…", true),
                    act(K::GatewayRoutes, A::Delete, "Delete…", true),
                ],
                vec![deny(K::GatewayRoutes, A::Version, "not admitted")],
            ),
            change_tab(
                "Providers",
                &["GatewayRead::Inventory", "GatewayRead::Detail"],
                None,
                vec![
                    act(K::GatewayProviders, A::Create, "New provider…", false),
                    act(K::GatewayProviders, A::Update, "Edit…", true),
                    act(K::GatewayProviders, A::Delete, "Delete…", true),
                ],
                vec![deny(K::GatewayProviders, A::Models, "not admitted")],
            ),
            change_tab(
                "Guardrails",
                &["GatewayRead::Inventory", "GatewayRead::Detail"],
                None,
                vec![
                    act(K::GatewayGuardrails, A::Create, "New guardrail…", false),
                    act(K::GatewayGuardrails, A::Update, "Edit…", true),
                    act(K::GatewayGuardrails, A::Delete, "Delete…", true),
                ],
                vec![],
            ),
            change_tab(
                "Keys",
                &["GatewayRead::Inventory (service keys, user keys)"],
                None,
                vec![
                    act(K::GatewayServiceKeys, A::Create, "New key…", false),
                    act(K::GatewayServiceKeys, A::Update, "Edit…", true),
                    act(K::GatewayServiceKeys, A::Rotate, "Rotate…", true),
                    act(K::GatewayServiceKeys, A::Delete, "Remove…", true),
                    act(K::GatewayUserKeys, A::Create, "New key…", false),
                    act(K::GatewayUserKeys, A::Update, "Edit…", true),
                    act(K::GatewayUserKeys, A::Rotate, "Rotate…", true),
                    act(K::GatewayUserKeys, A::Delete, "Remove…", true),
                ],
                vec![
                    deny(
                        K::GatewayServiceKeys,
                        A::Version,
                        "not admitted: Gateway keys rotate in place (no additional key, no staged rotation, no reminder)",
                    ),
                    deny(
                        K::GatewayUserKeys,
                        A::Version,
                        "not admitted: Gateway keys rotate in place (no additional key, no staged rotation, no reminder)",
                    ),
                ],
            ),
            change_tab(
                "Integrations",
                &["GatewayRead::IntegrationModels"],
                None,
                vec![
                    act(K::GatewayIntegrations, A::Create, "New integration…", false),
                    act(K::GatewayIntegrations, A::Update, "Edit…", true),
                    act(K::GatewayIntegrations, A::Delete, "Delete…", true),
                    act(
                        K::GatewayIntegrations,
                        A::Models,
                        "Replace model list…",
                        true,
                    ),
                ],
                vec![deny(K::GatewayIntegrations, A::Rotate, "not admitted")],
            ),
            removed_tab(
                "Limits",
                "the prototype's route budget limits have no admitted read or change; budgets appear in Routes detail only when the service returns them (C16)",
            ),
            read_tab("Logs", &["GatewayRead::Observe (telemetry)"]),
            read_tab("MCP servers", &["GatewayRead::McpServers"]),
        ],
    };
    let red_team = AdminProduct {
        id: AdminProductId::RedTeam,
        label: "AI Red Teaming".to_owned(),
        availability: AdminAvailability::Available,
        read_only: true,
        tabs: vec![
            read_tab("Overview", &["red_team::statistics"]),
            read_tab(
                "Targets",
                &["red_team::red_team_scans (target_id, target_type only; names never shown, P22)"],
            ),
            read_tab("Scans", &["red_team::red_team_scans"]),
            read_tab("Attack results", &["red_team::attacks (paged)"]),
            read_tab("Reports", &["red_team static reports (metadata only)"]),
        ],
    };
    let model_skill = AdminProduct {
        id: AdminProductId::ModelSkill,
        label: "Model and skill scans".to_owned(),
        availability: AdminAvailability::Available,
        read_only: true,
        tabs: vec![
            read_tab(
                "Overview",
                &[
                    "red_team::skill_statistics",
                    "red_team::model_scans (counts)",
                ],
            ),
            read_tab("Model scans", &["red_team::model_scans"]),
            read_tab("Skill scans", &["red_team::skill_scans (pseudonymised)"]),
            unavailable_tab("Codebase scans", "needs an admitted codebase-scan read"),
            unavailable_tab("Policies", "needs an admitted supply-chain policy service"),
        ],
    };
    let ais = AdminProduct {
        id: AdminProductId::AgenticIdentity,
        label: "Agentic Identity Security".to_owned(),
        availability: AdminAvailability::Unavailable {
            reason: AIS_GATE.to_owned(),
        },
        read_only: true,
        tabs: Vec::new(),
    };
    let aes = AdminProduct {
        id: AdminProductId::AgenticEndpoint,
        label: "Agentic Endpoint Security".to_owned(),
        availability: AdminAvailability::Unavailable {
            reason: AES_GATE.to_owned(),
        },
        read_only: true,
        tabs: Vec::new(),
    };
    AdminInventoryView {
        products: vec![runtime, gateway, red_team, model_skill, ais, aes],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::changes::is_allowed;

    /// V2-T20 / V2-R11 (core half): every offered action is admitted by the
    /// change matrices, every denied pair is refused by them, unavailable
    /// products have no tabs and no actions, and read-only products offer
    /// nothing.
    #[test]
    fn the_inventory_never_outruns_the_change_matrices() {
        let inv = inventory();
        assert_eq!(inv.products.len(), 6);
        for product in &inv.products {
            if let AdminAvailability::Unavailable { .. } = product.availability {
                assert!(product.tabs.is_empty(), "{:?}", product.id);
                continue;
            }
            for tab in &product.tabs {
                if product.read_only {
                    assert!(tab.actions.is_empty(), "{:?} {}", product.id, tab.label);
                }
                match tab.disposition {
                    TabDisposition::Unavailable { .. } | TabDisposition::Removed { .. } => {
                        assert!(tab.actions.is_empty() && tab.reads.is_empty())
                    }
                    TabDisposition::Read => assert!(tab.actions.is_empty()),
                    TabDisposition::Change => assert!(!tab.actions.is_empty()),
                }
                for a in &tab.actions {
                    assert!(
                        is_allowed(a.kind, a.action),
                        "{:?} {:?} offered but not admitted",
                        a.kind,
                        a.action
                    );
                }
                for d in &tab.denied {
                    assert!(
                        !is_allowed(d.kind, d.action),
                        "{:?} {:?} denied but admitted",
                        d.kind,
                        d.action
                    );
                }
            }
        }
        let gateway_keys = inv.products[1]
            .tabs
            .iter()
            .find(|t| t.label == "Keys")
            .expect("Keys");
        assert!(
            gateway_keys
                .actions
                .iter()
                .all(|a| a.action != ChangeAction::Version)
        );
        assert!(gateway_keys.actions.iter().any(|a| a.label == "Rotate…"));
        let runtime_keys = inv.products[0]
            .tabs
            .iter()
            .find(|t| t.label == "API keys")
            .expect("API keys");
        assert!(
            runtime_keys
                .actions
                .iter()
                .any(|a| a.label == "Issue additional key…")
        );
        assert!(
            runtime_keys
                .actions
                .iter()
                .any(|a| a.label == "Regenerate…")
        );
    }

    /// The presentation's fixture copy (apps/desktop/src/v2/admin) is the
    /// table, byte for byte in JSON value terms.
    #[test]
    fn the_presentation_fixture_is_the_table() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../apps/desktop/src/v2/admin/inventory.fixture.json"
        ))
        .expect("fixture json");
        assert_eq!(fixture, serde_json::to_value(inventory()).expect("json"));
    }

    /// Prints the inventory as the wire fixture carries it (run with
    /// `--ignored --nocapture` to refresh `fixtures/wire`).
    #[test]
    #[ignore]
    fn dump_wire_json() {
        println!(
            "ADMIN_INVENTORY_JSON {}",
            serde_json::to_string(&inventory()).expect("json")
        );
    }
}
