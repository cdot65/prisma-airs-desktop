//! C35 (D059): the catalogue and preference contract the ported shell relies
//! on, through the production authority interface. The sidebar and header
//! are drawn from the prototype (R2App): the seven destinations with ⌘1–⌘7
//! in the prototype's order and titles, Settings catalogued off the sidebar
//! (the status block's button), the six Administration products as the
//! rail's children with the two gated ones named, the Explain switch as the
//! persisted "show the calls" preference, and the rail's theme toggle as
//! the persisted appearance. Nothing here is a new core capability: the
//! suite pins what the shell reads so a presentation change cannot drift
//! from the core's word.

use desktop_core::admin_inventory::{AdminAvailability, AdminProductId};
use desktop_core::features::{Availability, FeatureId};
use desktop_core::fixtures::{self, Handles, ScriptedStore};
use desktop_core::store::PersistedState;
use desktop_core::wire::Theme;
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal,
    ShellSnapshot, ViewScope,
};
use serde_json::json;

struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
}

impl Desk {
    fn from_state(state: PersistedState) -> Self {
        let (adapters, handles) = fixtures::adapters_with(ScriptedStore::with_state(state));
        let core = Core::new(CoreConfig::default(), adapters);
        core.start();
        let handle = core.operator();
        let session = handle.register_delivery_session().expect("session").nonce;
        Self {
            _core: core,
            handle,
            session,
            sequence: 0,
            adapters: handles,
        }
    }

    /// A desk already on the v2 shell.
    async fn v2() -> Self {
        Self::from_state(PersistedState::default())
    }

    fn restart(self) -> Self {
        let state = self.adapters.store.saved().unwrap_or_default();
        drop(self._core);
        Self::from_state(state)
    }

    async fn send(&mut self, intent: serde_json::Value) -> Result<Receipt, Refusal> {
        let intent: Intent = serde_json::from_value(intent).expect("intent");
        self.sequence += 1;
        self.handle
            .dispatch(
                Delivery {
                    session: self.session.clone(),
                    sequence: self.sequence,
                },
                intent,
            )
            .await
    }

    fn snap(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).expect("observe") {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell scope"),
        }
    }
}

/// The prototype's NAV: titles and ⌘ keys in order (R2App `NAV`/`TITLES`).
const NAV: [(FeatureId, &str, u8); 7] = [
    (FeatureId::Home, "Home", 1),
    (FeatureId::Inbox, "Inbox", 2),
    (FeatureId::Harness, "Harness", 3),
    (FeatureId::Academy, "Academy", 4),
    (FeatureId::Library, "Test Cases", 5),
    (FeatureId::ApiClient, "API Client", 6),
    (FeatureId::Administration, "Administration", 7),
];

#[tokio::test]
async fn the_sidebar_is_the_prototypes_nav_and_settings_is_reached_off_it() {
    let mut desk = Desk::v2().await;
    let snap = desk.snap();

    let sidebar: Vec<(FeatureId, &str, Option<u8>)> = snap
        .features
        .iter()
        .filter(|f| f.sidebar)
        .map(|f| (f.id, f.label.as_str(), f.shortcut))
        .collect();
    let expected: Vec<(FeatureId, &str, Option<u8>)> = NAV
        .iter()
        .map(|(id, label, key)| (*id, *label, Some(*key)))
        .collect();
    assert_eq!(
        sidebar, expected,
        "the rail draws the catalogue in the prototype's order"
    );
    // The header's title is the destination's label: no second title table in the presentation.
    for (id, label, _) in NAV {
        let entry = snap
            .features
            .iter()
            .find(|f| f.id == id)
            .expect("catalogued");
        assert_eq!(entry.label, label);
        assert_eq!(
            entry.availability,
            Availability::Available,
            "{id:?} available in this build"
        );
    }
    // Settings is catalogued but off the sidebar: the status block's "Settings ⌘," button
    // and the tenant menu's "Tenant settings and credentials…" navigate there.
    let settings = snap
        .features
        .iter()
        .find(|f| f.id == FeatureId::Settings)
        .expect("settings catalogued");
    assert!(!settings.sidebar);
    assert_eq!(settings.shortcut, None);
    desk.send(json!({"kind": "navigate", "feature": "settings"}))
        .await
        .expect("settings");
    assert_eq!(desk.snap().active_feature, FeatureId::Settings);
    // The Harness button in the header navigates the same way.
    desk.send(json!({"kind": "navigate", "feature": "harness"}))
        .await
        .expect("harness");
    assert_eq!(desk.snap().active_feature, FeatureId::Harness);
}

#[tokio::test]
async fn the_rails_product_children_are_the_inventory_with_the_gated_two_named() {
    let desk = Desk::v2().await;
    let snap = desk.snap();
    let inventory = snap
        .administration
        .expect("the v2 shell publishes the Administration inventory");
    let ids: Vec<AdminProductId> = inventory.products.iter().map(|p| p.id).collect();
    assert_eq!(
        ids,
        [
            AdminProductId::RuntimeSecurity,
            AdminProductId::Gateway,
            AdminProductId::RedTeam,
            AdminProductId::ModelSkill,
            AdminProductId::AgenticIdentity,
            AdminProductId::AgenticEndpoint,
        ],
        "six children under Administration, as the prototype's PRODUCTS"
    );
    for product in &inventory.products {
        assert!(!product.label.is_empty());
        match product.id {
            AdminProductId::AgenticIdentity | AdminProductId::AgenticEndpoint => {
                // The rail shows these with the prototype's "New" tag and no count; the
                // gate is the core's reason, never a presentation string.
                match &product.availability {
                    AdminAvailability::Unavailable { reason } => {
                        assert!(reason.contains("Not available in this release"), "{reason}")
                    }
                    AdminAvailability::Available => {
                        panic!("{:?} has no admitted service", product.id)
                    }
                }
            }
            _ => assert_eq!(product.availability, AdminAvailability::Available),
        }
    }
    // C44: there is one presentation; a variant intent is accepted and changes nothing, so the
    // inventory stays published (the legacy shell, which kept its own fixed matrix, is gone).

    assert!(desk.snap().administration.is_some());
}

#[tokio::test]
async fn the_explain_switch_is_the_persisted_show_calls_preference() {
    let mut desk = Desk::v2().await;
    assert!(
        desk.snap().show_calls,
        "on by default: the switch starts checked"
    );
    desk.send(json!({"kind": "setShowCalls", "on": false}))
        .await
        .expect("off");
    assert!(!desk.snap().show_calls);
    assert!(!desk.adapters.store.saved().expect("saved").show_calls);
    desk.send(json!({"kind": "setShowCalls", "on": true}))
        .await
        .expect("on");
    assert!(desk.snap().show_calls);
    // Idempotent: the switch sends the value it shows, never a toggle the core must invert.
    desk.send(json!({"kind": "setShowCalls", "on": true}))
        .await
        .expect("on again");
    assert!(desk.snap().show_calls);
    let desk = desk.restart();
    assert!(desk.snap().show_calls);
}

#[tokio::test]
async fn the_rails_theme_toggle_is_the_persisted_appearance() {
    let mut desk = Desk::v2().await;
    assert_eq!(
        desk.snap().appearance,
        Theme::System,
        "follows macOS until chosen"
    );
    // The toggle flips between light and dark from what is shown; Settings keeps "Match system".
    desk.send(json!({"kind": "setAppearance", "theme": "dark"}))
        .await
        .expect("dark");
    assert_eq!(desk.snap().appearance, Theme::Dark);
    desk.send(json!({"kind": "setAppearance", "theme": "light"}))
        .await
        .expect("light");
    assert_eq!(desk.snap().appearance, Theme::Light);
    assert_eq!(
        desk.adapters.store.saved().expect("saved").appearance,
        Theme::Light
    );
    desk.send(json!({"kind": "setAppearance", "theme": "system"}))
        .await
        .expect("system");
    assert_eq!(desk.snap().appearance, Theme::System);
    desk.send(json!({"kind": "setAppearance", "theme": "dark"}))
        .await
        .expect("dark again");
    let desk = desk.restart();
    assert_eq!(
        desk.snap().appearance,
        Theme::Dark,
        "the choice survives a restart"
    );
    // A theme the wire does not name is not a well-formed intent.
    assert!(
        serde_json::from_value::<Intent>(json!({"kind": "setAppearance", "theme": "sepia"}))
            .is_err()
    );
}
