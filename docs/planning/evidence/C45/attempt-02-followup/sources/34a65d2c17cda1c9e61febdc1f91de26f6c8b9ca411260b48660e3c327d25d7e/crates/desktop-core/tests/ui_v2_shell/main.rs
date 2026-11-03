//! C28 (D058): the v2 shell's Rust-owned presentation state through the
//! feature catalogue the menu/sidebar/palette share, `navigate`, onboarding
//! transitions and the "show the calls" preference, all persisted or
//! session-scoped exactly as state-ownership.md assigns them.

use desktop_core::features::{self, FeatureId, OnboardingState};
use desktop_core::fixtures::{self, Handles, ScriptedStore};
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
    adapters: Handles,
}

impl Desk {
    fn new() -> Self {
        Self::from_state(PersistedState::default())
    }

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

#[tokio::test]
async fn a_fresh_store_is_the_one_presentation_with_onboarding_needed_and_calls_shown() {
    // C44: there is one presentation. A fresh store is v2 with Home first (⌘1).
    let desk = Desk::new();
    let snap = desk.snap();

    assert_eq!(snap.onboarding, OnboardingState::Needed);
    assert!(snap.show_calls);
    assert_eq!(snap.active_feature, FeatureId::Home);
    assert_eq!(snap.features[0].id, FeatureId::Home);
    assert_eq!(snap.features[0].shortcut, Some(1));
    assert!(snap.features.iter().any(|f| f.id == FeatureId::Inbox));
}

#[tokio::test]
async fn the_one_catalogue_survives_a_restart() {
    let desk = Desk::new();

    let snap = desk.snap();

    assert_eq!(
        snap.active_feature,
        FeatureId::Home,
        "falls back to the first sidebar entry"
    );
    let sidebar: Vec<FeatureId> = snap
        .features
        .iter()
        .filter(|f| f.sidebar)
        .map(|f| f.id)
        .collect();
    assert_eq!(
        sidebar,
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
    let shortcuts: Vec<Option<u8>> = snap.features.iter().map(|f| f.shortcut).collect();
    assert_eq!(
        &shortcuts[..7],
        &[
            Some(1),
            Some(2),
            Some(3),
            Some(4),
            Some(5),
            Some(6),
            Some(7)
        ]
    );
    assert!(shortcuts[7..].iter().all(Option::is_none));
    // The one catalogue exposes every supported destination.
    for id in features::catalogue().iter().map(|f| f.id) {
        assert!(snap.features.iter().any(|f| f.id == id), "{id:?} dropped");
    }
    // Nothing about the presentation is persisted by the switch; a restart is v2 on Home.
    let desk = desk.restart();

    assert_eq!(desk.snap().active_feature, FeatureId::Home);
}

#[tokio::test]
async fn navigate_accepts_only_catalogued_destinations_and_is_session_scoped() {
    let mut desk = Desk::new();
    // Home is the v2 catalogue's first entry; Harness is listed too.
    desk.send(json!({"kind": "navigate", "feature": "home"}))
        .await
        .expect("home");
    desk.send(json!({"kind": "navigate", "feature": "harness"}))
        .await
        .expect("harness");
    assert_eq!(desk.snap().active_feature, FeatureId::Harness);
    // A variant intent (a no-op since C44) leaves navigation where it is.

    assert_eq!(desk.snap().active_feature, FeatureId::Harness);
    // The Inbox arrived in C30: listed and available (an unlisted destination
    // stays refused, never a blank page).
    desk.send(json!({"kind": "navigate", "feature": "inbox"}))
        .await
        .expect("inbox");
    assert_eq!(desk.snap().active_feature, FeatureId::Inbox);
    desk.send(json!({"kind": "navigate", "feature": "administration"}))
        .await
        .expect("administration");
    assert_eq!(desk.snap().active_feature, FeatureId::Administration);
    // Navigation is not persisted: a restart lands on the first sidebar entry.
    let desk = desk.restart();
    assert_eq!(desk.snap().active_feature, FeatureId::Home);
    // An unknown feature name is not even a well-formed intent.
    assert!(
        serde_json::from_value::<Intent>(json!({"kind": "navigate", "feature": "nope"})).is_err()
    );
}

#[tokio::test]
async fn onboarding_only_moves_forward_and_show_calls_persists() {
    let mut desk = Desk::new();
    desk.send(json!({"kind": "setOnboarding", "state": "inProgress"}))
        .await
        .expect("in progress");
    assert_eq!(
        desk.send(json!({"kind": "setOnboarding", "state": "needed"}))
            .await,
        Err(Refusal::InvalidInput),
        "never back to needed"
    );
    desk.send(json!({"kind": "setOnboarding", "state": "done"}))
        .await
        .expect("done");
    assert_eq!(
        desk.send(json!({"kind": "setOnboarding", "state": "inProgress"}))
            .await,
        Err(Refusal::InvalidInput)
    );
    desk.send(json!({"kind": "setShowCalls", "on": false}))
        .await
        .expect("off");
    let saved = desk.adapters.store.saved().expect("saved");
    assert_eq!(saved.onboarding, OnboardingState::Done);
    assert!(!saved.show_calls);
    let desk = desk.restart();
    let snap = desk.snap();
    assert_eq!(snap.onboarding, OnboardingState::Done);
    assert!(!snap.show_calls);
}

#[tokio::test]
async fn a_pre_c28_state_document_loads_into_the_one_presentation() {
    // A store written before C28 has none of the new fields; since C44 it opens in v2 like every other.
    let mut doc = serde_json::to_value(PersistedState::default()).expect("json");
    let object = doc.as_object_mut().expect("object");
    object.remove("uiVariant");
    object.remove("onboarding");
    object.remove("showCalls");
    let state: PersistedState = serde_json::from_value(doc).expect("older document loads");

    assert_eq!(state.onboarding, OnboardingState::Needed);
    assert!(state.show_calls);
    let desk = Desk::from_state(state);
    assert_eq!(desk.snap().active_feature, FeatureId::Home);
}
