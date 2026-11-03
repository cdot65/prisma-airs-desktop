//! C29 (D058): tenant setup through the production authority interface.
//! The wizard works on one core-owned draft with its own context: credentials
//! attach to it, verification is decided in the core, completion commits the
//! draft as a tenant in one step, and a discard or relaunch leaves nothing
//! behind but a cleanup of its credentials.

use desktop_core::features::{OnboardingState, ProductId, VerifyLine, VerifyState};
use desktop_core::fixtures::{self, Handles, ScriptedStore};
use desktop_core::store::PersistedState;
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal,
    ShellSnapshot, ViewScope,
};
use serde_json::json;

const KEY: &str = "synthetic-scan-key-aaaa";

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

    fn setup(&self) -> desktop_core::features::SetupView {
        self.snap().setup.expect("a setup in progress")
    }

    async fn begin(&mut self, mode: &str, label: &str) -> String {
        self.send(
            json!({"kind": "beginTenantSetup", "mode": mode, "profile": {
                "label": label, "tsgId": "1234567890",
                "destination": {"kind": "region", "region": "europe"}, "environment": "staging"
            }}),
        )
        .await
        .expect("begun");
        self.setup().context.as_str().to_owned()
    }

    /// The draft's current context: it changes whenever its inputs change.
    fn context(&self) -> String {
        self.setup().context.as_str().to_owned()
    }

    async fn settled(&self, ready: bool) {
        for _ in 0..200 {
            let s = self.setup();
            let state = s.tenant.scan.state;
            let done = if ready {
                state == desktop_core::ScanState::Ready
            } else {
                state != desktop_core::ScanState::Storing
            };
            if done {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("the scan key write did not settle");
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
async fn a_draft_is_not_a_tenant_and_first_run_needs_the_scan_key_to_verify() {
    let mut desk = Desk::new();
    desk.send(json!({"kind": "setUiVariant", "variant": "v2"}))
        .await
        .expect("v2");
    let context = desk.begin("firstRun", "Northwind Health").await;
    let snap = desk.snap();
    assert!(snap.tenants.is_empty(), "the draft is not listed");
    assert!(snap.active.is_none());
    assert_eq!(snap.onboarding, OnboardingState::Needed);
    let setup = snap.setup.expect("setup");
    assert_eq!(setup.tenant.label, "Northwind Health");
    assert_eq!(
        setup.tenant.environment,
        desktop_core::features::TenantEnvironment::Staging
    );
    assert!(setup.scan_required);
    assert_eq!(setup.products, vec![ProductId::RuntimeSecurity]);
    assert!(setup.verify.is_none() && !setup.completable);
    // A second draft is refused while one is open.
    assert_eq!(
        desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {
            "label": "Other", "tsgId": null, "destination": {"kind": "region", "region": "europe"}}}))
            .await,
        Err(Refusal::Busy)
    );
    // Without a scan key the verify run fails on first run, so completion is refused.
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify started");
    let v = desk.verified().await;
    let scan = v
        .steps
        .iter()
        .find(|s| s.line == VerifyLine::ScanKey)
        .expect("scan line");
    assert_eq!(scan.state, VerifyState::Fail);
    assert!(v.failed && !desk.setup().completable);
    assert_eq!(
        desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
            .await,
        Err(Refusal::InvalidInput)
    );
}

#[tokio::test]
async fn credentials_attach_to_the_draft_and_a_stored_key_is_never_a_pass() {
    let mut desk = Desk::new();
    let context = desk.begin("firstRun", "Northwind Health").await;
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "session", "key": KEY}))
        .await
        .expect("key accepted for the draft");
    desk.settled(true).await;
    let context = desk.context();
    assert!(desk.snap().tenants.is_empty(), "still no tenant");
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    let by_line = |line: VerifyLine| v.steps.iter().find(|s| s.line == line).cloned();
    let scan = by_line(VerifyLine::ScanKey).expect("scan");
    assert_eq!(
        scan.state,
        VerifyState::Stored,
        "explicitly unverified, not Pass"
    );
    assert!(scan.reason.contains("first scan proves it"));
    let management = by_line(VerifyLine::ManagementClient).expect("management");
    assert_eq!(management.state, VerifyState::Skipped);
    assert!(
        by_line(VerifyLine::Gateway).is_none(),
        "unselected products have no line"
    );
    assert!(v.done && !v.failed && !v.stale && v.warnings.is_empty());
    assert!(desk.setup().completable);
}

#[tokio::test]
async fn changing_inputs_makes_results_stale_and_products_needing_a_client_warn() {
    let mut desk = Desk::new();
    let context = desk.begin("firstRun", "Northwind Health").await;
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "session", "key": KEY}))
        .await
        .expect("key");
    desk.settled(true).await;
    let context = desk.context();
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let first = desk.verified().await;
    assert_eq!(first.generation, 1);
    // Choosing Gateway afterwards invalidates the run; completion is refused.
    desk.send(json!({"kind": "chooseSetupProducts", "context": context, "products": ["gateway", "gateway", "redTeam"]}))
        .await
        .expect("products");
    let setup = desk.setup();
    assert_eq!(
        setup.products,
        vec![
            ProductId::RuntimeSecurity,
            ProductId::Gateway,
            ProductId::RedTeam
        ]
    );
    assert!(setup.verify.expect("kept").stale && !setup.completable);
    assert_eq!(
        desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
            .await,
        Err(Refusal::InvalidInput)
    );
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify again");
    let second = desk.verified().await;
    assert_eq!(second.generation, 2);
    assert!(!second.stale);
    let lines: Vec<(VerifyLine, VerifyState)> =
        second.steps.iter().map(|s| (s.line, s.state)).collect();
    assert_eq!(
        lines,
        vec![
            (VerifyLine::ScanKey, VerifyState::Stored),
            (VerifyLine::ManagementClient, VerifyState::Warning),
            (VerifyLine::Gateway, VerifyState::Warning),
            (VerifyLine::RedTeam, VerifyState::Warning),
            (VerifyLine::Ais, VerifyState::Unavailable),
            (VerifyLine::Aes, VerifyState::Unavailable),
        ]
    );
    assert_eq!(second.warnings.len(), 3);
    assert_eq!(second.warnings[1].product, ProductId::Gateway);
    assert!(desk.setup().completable, "warnings do not block completion");
    // A profile change (new destination) retires the key and goes stale.
    desk.send(
        json!({"kind": "updateTenantSetup", "context": context, "profile": {
        "label": "Northwind Health", "tsgId": "1234567890",
        "destination": {"kind": "region", "region": "unitedStates"}, "environment": "production"}}),
    )
    .await
    .expect("updated");
    let setup = desk.setup();
    assert_eq!(setup.tenant.scan.state, desktop_core::ScanState::Missing);
    assert_eq!(
        setup.tenant.environment,
        desktop_core::features::TenantEnvironment::Production
    );
    assert!(
        setup.verify.is_none() && !setup.completable,
        "the run went with the old context"
    );
    assert_ne!(
        setup.context.as_str(),
        context,
        "a profile change retires the draft's context"
    );
    assert_eq!(
        desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
            .await,
        Err(Refusal::StaleContext)
    );
}

#[tokio::test]
async fn completion_commits_the_draft_once_with_products_warnings_and_onboarding() {
    let mut desk = Desk::new();
    desk.send(json!({"kind": "setUiVariant", "variant": "v2"}))
        .await
        .expect("v2");
    desk.send(json!({"kind": "setOnboarding", "state": "inProgress"}))
        .await
        .expect("in progress");
    let context = desk.begin("firstRun", "Northwind Health").await;
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "session", "key": KEY}))
        .await
        .expect("key");
    desk.settled(true).await;
    let context = desk.context();
    desk.send(json!({"kind": "chooseSetupProducts", "context": context, "products": ["gateway"]}))
        .await
        .expect("products");
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    desk.verified().await;
    // A pack for an unchosen product is refused.
    assert_eq!(
        desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": ["redTeam"]}))
            .await,
        Err(Refusal::InvalidInput)
    );
    desk.send(
        json!({"kind": "completeTenantSetup", "context": context, "packs": ["runtimeSecurity"]}),
    )
    .await
    .expect("completed");
    let snap = desk.snap();
    assert!(snap.setup.is_none());
    assert_eq!(snap.tenants.len(), 1);
    let tenant = &snap.tenants[0];
    assert_eq!(tenant.label, "Northwind Health");
    assert_eq!(
        tenant.products,
        vec![ProductId::RuntimeSecurity, ProductId::Gateway]
    );
    assert_eq!(
        tenant.scan.state,
        desktop_core::ScanState::Ready,
        "the key moved with it"
    );
    assert_eq!(
        tenant.setup_warnings.len(),
        2,
        "management + gateway warnings"
    );
    assert_eq!(tenant.setup_warnings[0].id, "vw-1");
    assert_eq!(tenant.setup_warnings[1].product, ProductId::Gateway);
    assert!(tenant.setup_warnings[0].recorded_secs > 1_700_000_000);
    assert_eq!(
        snap.active.as_ref().map(|a| &a.tenant),
        Some(&tenant.id),
        "active once"
    );
    assert_ne!(
        snap.active.as_ref().map(|a| a.context.as_str()),
        Some(context.as_str()),
        "a fresh context, not the draft's"
    );
    assert_eq!(snap.onboarding, OnboardingState::Done);
    assert_eq!(snap.active_feature, desktop_core::features::FeatureId::Home);
    // The draft's context is retired with it.
    assert_eq!(
        desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
            .await,
        Err(Refusal::StaleContext)
    );
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(saved.tenant_draft.is_none());
    assert_eq!(saved.tenant_warnings.len(), 1);
    let desk = desk.restart();
    let snap = desk.snap();
    assert_eq!(snap.onboarding, OnboardingState::Done);
    assert_eq!(snap.tenants[0].setup_warnings.len(), 2, "restored");
}

#[tokio::test]
async fn add_mode_keeps_the_shell_untouched_until_completion_and_discard_cleans_up() {
    let mut desk = Desk::new();
    desk.send(json!({"kind": "createTenant", "profile": {
        "label": "First", "tsgId": "1111111111", "destination": {"kind": "region", "region": "europe"}}}))
        .await
        .expect("first tenant");
    let before = desk.snap();
    let first = before.active.clone().expect("active");
    assert_eq!(
        before.onboarding,
        OnboardingState::Done,
        "an existing tenant means no wizard"
    );
    assert_eq!(
        desk.send(
            json!({"kind": "beginTenantSetup", "mode": "firstRun", "profile": {
            "label": "X", "tsgId": null, "destination": {"kind": "region", "region": "europe"}}})
        )
        .await,
        Err(Refusal::InvalidInput),
        "first run is impossible once a tenant exists"
    );
    let context = desk.begin("add", "Second").await;
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "keychain", "key": KEY}))
        .await
        .expect("key in the Keychain");
    desk.settled(true).await;
    let context = desk.context();
    let records = desk.adapters.vault.records();
    assert_eq!(records.len(), 1, "one Keychain record for the draft");
    let during = desk.snap();
    assert_eq!(during.tenants.len(), 1, "the shell still lists one tenant");
    assert_eq!(
        during.active,
        Some(first.clone()),
        "the active tenant and context are untouched"
    );
    assert!(
        !during.setup.as_ref().expect("setup").scan_required,
        "add mode: the key is optional"
    );
    // Discard: nothing changed, and the draft's record is queued for cleanup.
    desk.send(json!({"kind": "discardTenantSetup", "context": context}))
        .await
        .expect("discarded");
    let after = desk.snap();
    assert!(after.setup.is_none());
    assert_eq!(after.tenants.len(), 1);
    assert_eq!(after.active, Some(first));
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(saved.tenant_draft.is_none());
    assert!(
        saved
            .credentials
            .iter()
            .all(|c| c.tenant == before.tenants[0].id)
    );
    assert!(
        saved.pending_cleanup.contains(&records[0])
            || !desk.adapters.vault.records().contains(&records[0]),
        "the draft's Keychain record is being removed"
    );
    assert_eq!(
        desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
            .await,
        Err(Refusal::StaleContext)
    );
}

#[tokio::test]
async fn add_mode_without_a_key_verifies_with_a_warning_and_completes_switching_once() {
    let mut desk = Desk::new();
    desk.send(json!({"kind": "createTenant", "profile": {
        "label": "First", "tsgId": "1111111111", "destination": {"kind": "region", "region": "europe"}}}))
        .await
        .expect("first tenant");
    let context = desk.begin("add", "Second").await;
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    assert_eq!(
        v.steps[0].state,
        VerifyState::Warning,
        "no key is a warning in add mode"
    );
    assert!(desk.setup().completable);
    desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
        .await
        .expect("completed");
    let snap = desk.snap();
    assert_eq!(snap.tenants.len(), 2);
    let second = snap
        .tenants
        .iter()
        .find(|t| t.label == "Second")
        .expect("second");
    assert_eq!(snap.active.as_ref().map(|a| &a.tenant), Some(&second.id));
    assert_eq!(second.setup_warnings.len(), 1);
    assert_eq!(snap.onboarding, OnboardingState::Done);
}

#[tokio::test]
async fn a_relaunch_drops_the_draft_and_queues_its_credentials_for_cleanup() {
    let mut desk = Desk::new();
    let context = desk.begin("firstRun", "Northwind Health").await;
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "keychain", "key": KEY}))
        .await
        .expect("key");
    desk.settled(true).await;
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(
        saved.tenant_draft.is_some(),
        "the draft is written so its records are known"
    );
    assert_eq!(saved.credentials.len(), 1);
    assert!(saved.tenants.is_empty());
    let record = saved.credentials[0].record.clone();
    let mut desk = desk.restart();
    let snap = desk.snap();
    assert!(snap.setup.is_none() && snap.tenants.is_empty());
    assert_eq!(snap.onboarding, OnboardingState::Needed);
    // The next write carries no draft; its record was queued for cleanup
    // (and removed by the start-up cleanup, which the fresh vault records).
    desk.send(json!({"kind": "setShowCalls", "on": false}))
        .await
        .expect("saved again");
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(saved.tenant_draft.is_none());
    assert!(saved.credentials.is_empty());
    let deleted = desk
        .adapters
        .vault
        .calls()
        .into_iter()
        .any(|c| matches!(c, fixtures::VaultCall::Delete(r) if r == record));
    assert!(
        deleted || saved.pending_cleanup.contains(&record),
        "cleanup reached the vault"
    );
}

#[tokio::test]
async fn a_desktop_with_a_tenant_is_never_sent_through_the_wizard() {
    let mut desk = Desk::new();
    assert_eq!(desk.snap().onboarding, OnboardingState::Needed);
    desk.send(json!({"kind": "createTenant", "profile": {
        "label": "Northwind Health", "tsgId": "1234567890",
        "destination": {"kind": "region", "region": "europe"}}}))
        .await
        .expect("created");
    assert_eq!(desk.snap().onboarding, OnboardingState::Done);
    assert_eq!(
        desk.adapters.store.saved().expect("saved").onboarding,
        OnboardingState::Needed,
        "the stored state is untouched"
    );
    desk.send(json!({"kind": "setUiVariant", "variant": "v2"}))
        .await
        .expect("v2");
    assert_eq!(desk.snap().onboarding, OnboardingState::Done);
}

#[tokio::test]
async fn a_draft_id_is_never_selectable_or_deletable_as_a_tenant() {
    let mut desk = Desk::new();
    desk.send(json!({"kind": "createTenant", "profile": {
        "label": "First", "tsgId": "1111111111", "destination": {"kind": "region", "region": "europe"}}}))
        .await
        .expect("first tenant");
    let before = desk.snap();
    desk.begin("add", "Second").await;
    let draft_id = desk.setup().tenant.id.as_str().to_owned();
    assert_eq!(
        desk.send(json!({"kind": "selectTenant", "tenant": draft_id}))
            .await,
        Err(Refusal::InvalidInput),
        "a draft is not a tenant"
    );
    assert_eq!(
        desk.send(json!({"kind": "deleteTenant", "tenant": draft_id}))
            .await,
        Err(Refusal::InvalidInput)
    );
    let after = desk.snap();
    assert_eq!(after.active, before.active);
    assert_eq!(after.tenants, before.tenants);
    let saved = desk.adapters.store.saved().expect("saved");
    assert_eq!(saved.active_tenant, before.active.map(|a| a.tenant));
    assert!(
        saved.tenant_draft.is_some(),
        "the draft itself is still there"
    );
}

#[tokio::test]
async fn a_tenant_created_outside_the_wizard_ends_a_first_run_draft_and_the_shell_takes_over() {
    // The v1 import during first run commits tenants through the same path.
    let mut desk = Desk::new();
    desk.send(json!({"kind": "setUiVariant", "variant": "v2"}))
        .await
        .expect("v2");
    desk.send(json!({"kind": "setOnboarding", "state": "inProgress"}))
        .await
        .expect("in progress");
    let context = desk.begin("firstRun", "Drafted").await;
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "keychain", "key": KEY}))
        .await
        .expect("key");
    desk.settled(true).await;
    let record = desk.adapters.vault.records()[0].clone();
    desk.send(json!({"kind": "createTenant", "profile": {
        "label": "Imported", "tsgId": "2222222222", "destination": {"kind": "region", "region": "europe"}}}))
        .await
        .expect("imported tenant");
    let snap = desk.snap();
    assert!(snap.setup.is_none(), "the first-run draft is over");
    assert_eq!(
        snap.onboarding,
        OnboardingState::Done,
        "a tenant exists: the shell takes over"
    );
    assert_eq!(snap.tenants.len(), 1);
    assert_eq!(snap.tenants[0].label, "Imported");
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(saved.tenant_draft.is_none());
    assert!(
        saved.credentials.is_empty(),
        "the draft's record left the ledger"
    );
    let deleted = desk
        .adapters
        .vault
        .calls()
        .into_iter()
        .any(|c| matches!(c, fixtures::VaultCall::Delete(r) if r == record));
    assert!(
        deleted || saved.pending_cleanup.contains(&record),
        "its Keychain record is cleaned up"
    );
    let desk = desk.restart();
    assert_eq!(
        desk.snap().onboarding,
        OnboardingState::Done,
        "and stays so after a relaunch"
    );
}
