//! C43: the wizard as the R2Onboard board draws it, through the production
//! authority interface (docs/planning/chunks/C43.md). What the re-ported
//! presentation draws from, unchanged from C29: the setup draft's profile
//! (the rail's context line and the Tenant card), which credential is
//! required (the Required/Optional tags), the products chosen (the product
//! cards), the verify run's lines including the unavailable products (the
//! Checks card), the packs offered with their counts (the starter-case rows)
//! and completability (the footer's primary). Nothing here commits a tenant.

use desktop_core::features::{ProductId, VerifyLine, VerifyState};
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
    #[allow(dead_code)]
    adapters: Handles,
}

impl Desk {
    fn new() -> Self {
        let (adapters, handles) =
            fixtures::adapters_with(ScriptedStore::with_state(PersistedState::default()));
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

    fn setup(&self) -> desktop_core::features::SetupView {
        self.snap().setup.expect("a setup in progress")
    }

    async fn verified(&self) -> desktop_core::features::VerifyView {
        for _ in 0..200 {
            if let Some(v) = self.setup().verify
                && v.done
            {
                return v;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("verification did not finish");
    }
}

#[tokio::test]
async fn the_board_draws_the_draft_the_tags_the_cards_and_the_checks_from_the_setup_view() {
    let mut desk = Desk::new();

    desk.send(
        json!({"kind": "beginTenantSetup", "mode": "firstRun", "profile": {
        "label": "Northwind Health", "tsgId": "1762204418",
        "destination": {"kind": "region", "region": "europe"}, "environment": "staging"}}),
    )
    .await
    .expect("begun");
    let s = serde_json::to_value(desk.setup()).expect("json");
    // The rail's context line and the Tenant card.
    assert_eq!(s["tenant"]["label"], "Northwind Health");
    assert_eq!(s["tenant"]["tsgId"], "1762204418");
    assert_eq!(s["tenant"]["environment"], "staging");
    assert_eq!(s["tenant"]["destination"]["region"], "europe");
    // Every setup requires OAuth authentication; the Scan Key card is optional.
    assert_eq!(s["scanRequired"], false);
    assert_eq!(s["managementRequired"], true);
    assert_eq!(s["tenant"]["scan"]["state"], "missing");
    assert_eq!(s["tenant"]["management"]["state"], "missing");
    // The product cards: Runtime always on; the packs rows with their counts.
    assert_eq!(s["products"], json!(["runtimeSecurity"]));
    let packs = s["packs"].as_array().expect("packs");
    assert!(
        packs
            .iter()
            .any(|p| p["product"] == "runtimeSecurity" && p["count"].as_u64().unwrap_or(0) > 0),
        "{packs:?}"
    );
    // Before any run the footer's primary on Verify is closed.
    assert!(s["verify"].is_null());
    assert_eq!(s["completable"], false);
    // Choosing a product is recorded in the core as the card toggles.
    let context = desk.setup().context.as_str().to_owned();
    desk.send(json!({"kind": "chooseSetupProducts", "context": context, "products": ["runtimeSecurity", "gateway"]}))
        .await
        .expect("products");
    assert_eq!(
        desk.setup().products,
        vec![ProductId::RuntimeSecurity, ProductId::Gateway]
    );
    // The Checks card: one line per credential and chosen product, and the two unavailable products.
    let context = desk.setup().context.as_str().to_owned();
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    let lines: Vec<VerifyLine> = v.steps.iter().map(|s| s.line).collect();
    for line in [
        VerifyLine::ScanKey,
        VerifyLine::ManagementClient,
        VerifyLine::Gateway,
        VerifyLine::Ais,
        VerifyLine::Aes,
    ] {
        assert!(lines.contains(&line), "{lines:?}");
    }
    assert!(
        v.steps
            .iter()
            .filter(|s| matches!(s.line, VerifyLine::Ais | VerifyLine::Aes))
            .all(|s| s.state == VerifyState::Unavailable)
    );
    // Missing management authentication blocks the footer; the Scan Key is not required.
    assert!(v.failed && v.blocking);
    assert!(!desk.setup().completable);
}

#[tokio::test]
async fn add_mode_has_no_required_scan_key_and_the_sheet_discards_its_draft_on_cancel() {
    let mut desk = Desk::new();

    desk.send(json!({"kind": "setOnboarding", "state": "done"}))
        .await
        .expect("done");
    desk.send(
        json!({"kind": "beginTenantSetup", "mode": "add", "profile": {
        "label": "Coastal Bank", "tsgId": "1001464285",
        "destination": {"kind": "region", "region": "unitedStates"}, "environment": "production"}}),
    )
    .await
    .expect("begun");
    let s = serde_json::to_value(desk.setup()).expect("json");
    assert_eq!(s["mode"], "add");
    // The add sheet's scan card is Optional.
    assert_eq!(s["scanRequired"], false);
    assert_eq!(s["managementRequired"], true);
    assert!(desk.snap().tenants.is_empty(), "a draft is not a tenant");
    // Cancel discards the draft in the core; the shell is untouched.
    let context = desk.setup().context.as_str().to_owned();
    desk.send(json!({"kind": "discardTenantSetup", "context": context}))
        .await
        .expect("discarded");
    assert!(desk.snap().setup.is_none());
    assert!(desk.snap().tenants.is_empty());
}
