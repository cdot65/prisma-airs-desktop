//! C44: one presentation. Through the production authority interface: the
//! snapshot always says the v2 presentation with the v2 catalogue, whatever
//! an older store persisted; `SetUiVariant` (sent by older presentations
//! and drivers) is accepted and changes nothing; navigation resolves against
//! the v2 catalogue only, so every destination the legacy shell reached is
//! still a destination here (the product entries open Administration's
//! products in the shell); a fresh store starts in the first-run wizard.

use desktop_core::features::{FeatureId, OnboardingState, UiVariant};
use desktop_core::fixtures::{self, ScriptedStore};
use desktop_core::store::PersistedState;
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
}

impl Desk {
    fn from_state(state: PersistedState) -> Self {
        let (adapters, _handles) = fixtures::adapters_with(ScriptedStore::with_state(state));
        let core = Core::new(CoreConfig::default(), adapters);
        core.start();
        let handle = core.operator();
        let session = handle.register_delivery_session().expect("session").nonce;
        Self {
            _core: core,
            handle,
            session,
            sequence: 0,
        }
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

fn v2_ids(snap: &ShellSnapshot) -> Vec<FeatureId> {
    snap.features.iter().map(|f| f.id).collect()
}

#[tokio::test]
async fn an_older_store_that_persisted_the_legacy_presentation_still_gets_v2() {
    let mut state = PersistedState::default();
    state.ui_variant = UiVariant::Legacy;
    state.onboarding = OnboardingState::Done;
    let mut desk = Desk::from_state(state);
    let snap = desk.snap();
    assert_eq!(snap.ui_variant, UiVariant::V2);
    assert_eq!(
        v2_ids(&snap)[..7],
        [
            FeatureId::Home,
            FeatureId::Inbox,
            FeatureId::Harness,
            FeatureId::Academy,
            FeatureId::Library,
            FeatureId::ApiClient,
            FeatureId::Administration
        ]
    );
    // The switch intent is accepted and changes nothing.
    desk.send(json!({"kind": "setUiVariant", "variant": "legacy"}))
        .await
        .expect("accepted");
    let after = desk.snap();
    assert_eq!(after.ui_variant, UiVariant::V2);
    assert_eq!(v2_ids(&after), v2_ids(&snap));
}

#[tokio::test]
async fn every_destination_the_legacy_shell_reached_is_a_v2_destination() {
    let mut state = PersistedState::default();
    state.onboarding = OnboardingState::Done;
    let mut desk = Desk::from_state(state);
    for feature in [
        "home",
        "inbox",
        "harness",
        "academy",
        "library",
        "apiClient",
        "administration",
        "runtime",
        "gateway",
        "redTeam",
        "sessions",
        "import",
        "settings",
    ] {
        desk.send(json!({"kind": "navigate", "feature": feature}))
            .await
            .unwrap_or_else(|r| panic!("{feature}: {r:?}"));
        let snap = desk.snap();
        assert_eq!(
            serde_json::to_value(snap.active_feature).expect("json"),
            json!(feature)
        );
    }
}

#[tokio::test]
async fn a_fresh_store_starts_in_the_first_run_wizard_of_the_only_presentation() {
    let desk = Desk::from_state(PersistedState::default());
    let snap = desk.snap();
    assert_eq!(snap.ui_variant, UiVariant::V2);
    assert_eq!(snap.onboarding, OnboardingState::Needed);
    assert!(snap.tenants.is_empty());
}
