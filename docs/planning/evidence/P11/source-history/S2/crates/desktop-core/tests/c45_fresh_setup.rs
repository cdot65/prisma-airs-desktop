//! C45 fresh setup against real FileStore and production authority, synthetic roots/vaults only.
use desktop_core::fixtures::{self, ScriptedVault};
use desktop_core::store::FileStore;
use desktop_core::vault::{RecordId, Vault};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, ShellSnapshot,
    ViewScope,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;
struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let mut n = [0u8; 8];
        getrandom::fill(&mut n).expect("fixture");
        let p = std::env::temp_dir().join(format!("airs-desktop-c45-{}", hex::encode(n)));
        std::fs::create_dir_all(&p).expect("root");
        Self(p)
    }
}
impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
}
impl Desk {
    async fn open(root: &Path, vault: Arc<ScriptedVault>) -> Self {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let store = loop {
            match FileStore::open(root) {
                Ok(s) => break s,
                Err(e) => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "new store lease did not release: {e:?}"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
        };
        let (mut adapters, _) = fixtures::adapters();
        adapters.store = Arc::new(store);
        adapters.keychain = vault;
        adapters.offline_root = Some(root.join("academy-offline"));
        adapters.sessions_root = Some(root.join("sessions"));
        adapters.library_root = Some(root.join("library"));
        adapters.workbench_root = Some(root.join("api-client"));
        adapters.logs_root = Some(root.join("logs"));
        let core = Core::new(
            CoreConfig {
                management: desktop_core::management::ManagementEndpoints::at_origin(
                    "https://localhost:9",
                ),
                workbench_network: desktop_core::workbench::admission::Network::LoopbackOnly,
                ..CoreConfig::default()
            },
            adapters,
        );
        core.start();
        let handle = core.operator();
        let session = handle.register_delivery_session().expect("delivery").nonce;
        Self {
            core,
            handle,
            session,
            sequence: 0,
        }
    }
    fn snapshot(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).expect("snapshot") {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell"),
        }
    }
    fn json(&self) -> Value {
        serde_json::to_value(self.snapshot()).expect("json")
    }
    async fn send(&mut self, value: Value) -> Receipt {
        self.sequence += 1;
        self.handle
            .dispatch(
                Delivery {
                    session: self.session.clone(),
                    sequence: self.sequence,
                },
                serde_json::from_value(value).expect("typed intent"),
            )
            .await
            .expect("admitted")
    }
}
async fn eventually(mut f: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    while !f() {
        assert!(std::time::Instant::now() < deadline, "settlement missing");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}
#[tokio::test]
async fn fresh_bootstrap_restart_and_reset_never_adopt_or_purge_prior_roots_or_credentials() {
    let sandbox = Sandbox::new();
    let root = sandbox.0.join("io.cdot.airs-desktop.fixture");
    let mut old_files = Vec::new();
    for namespace in [
        "io.cdot.prisma-airs-desktop",
        "io.cdot.prisma-airs-desktop.v2",
    ] {
        for name in [
            "state.json",
            "sessions/history.canary",
            "api-client/profile.canary",
            "academy-offline/approval.canary",
        ] {
            let p = sandbox.0.join(namespace).join(name);
            std::fs::create_dir_all(p.parent().expect("parent")).expect("old fixture directory");
            let bytes=serde_json::to_vec(&json!({"schema":"prisma-airs-desktop-v2/state/1","tenants":[{"label":"SYNTHETIC-OLD-TENANT"}],"credentials":["SYNTHETIC-OLD-CREDENTIAL"],"pendingCleanup":["cr-0123456789abcdef"],"activeTenant":"SYNTHETIC-OLD-ACTIVE","approvals":["SYNTHETIC-OLD-APPROVAL"],"sentinel":name})).expect("fixture");
            std::fs::write(&p, &bytes).expect("seed synthetic old root");
            old_files.push((p, bytes));
        }
    }
    let old_vault = Arc::new(ScriptedVault::default());
    let old_id = RecordId::parse("cr-0123456789abcdef").expect("id");
    old_vault
        .put(&old_id, b"SYNTHETIC-OLD-KEY")
        .expect("old fixture key");
    let old_calls = old_vault.calls();
    let new_vault = Arc::new(ScriptedVault::default());
    let mut desk = Desk::open(&root, new_vault.clone()).await;
    let fresh = desk.json();
    assert_eq!(fresh["storage"], "ready");
    assert_eq!(fresh["onboarding"], "needed");
    assert_eq!(fresh["activeFeature"], "home");
    assert!(fresh["tenants"].as_array().expect("tenants").is_empty());
    assert!(fresh["active"].is_null());
    for key in ["uiVariant", "import", "v1Import"] {
        assert!(fresh.get(key).is_none(), "removed contract {key}");
    }
    assert_eq!(
        old_vault.calls(),
        old_calls,
        "bootstrap makes no old-vault calls"
    );
    assert!(new_vault.records().is_empty());
    desk.send(json!({"kind":"createTenant","profile":{"label":"New current tenant","destination":{"kind":"custom","origin":"https://localhost:9"}}})).await;
    let context = desk.json()["active"]["context"].clone();
    desk.send(json!({"kind":"configureScanCredential","context":context,"mode":"keychain","key":"SYNTHETIC-NEW-C45-KEY"})).await;
    eventually(|| desk.json()["tenants"][0]["scan"]["state"] == "ready").await;
    let document: Value =
        serde_json::from_slice(&std::fs::read(root.join("state.json")).expect("new state"))
            .expect("document");
    assert_eq!(document["schema"], "prisma-airs-desktop/state/1");
    assert!(!document.to_string().contains("SYNTHETIC-NEW-C45-KEY"));
    desk.core.shutdown();
    drop(desk);
    let mut restarted = Desk::open(&root, new_vault.clone()).await;
    assert_eq!(
        restarted.json()["tenants"][0]["label"],
        "New current tenant"
    );
    assert_eq!(
        restarted.json()["tenants"]
            .as_array()
            .expect("tenants")
            .len(),
        1
    );
    restarted.send(json!({"kind":"resetLocalData"})).await;
    eventually(|| {
        restarted.json()["storage"] == "ready"
            && restarted.json()["tenants"]
                .as_array()
                .is_some_and(|v| v.is_empty())
    })
    .await;
    assert!(new_vault.records().is_empty());
    assert_eq!(
        old_vault.calls(),
        old_calls,
        "restart/reset make no old-vault calls"
    );
    assert_eq!(
        old_vault.secret(&old_id).as_deref(),
        Some(b"SYNTHETIC-OLD-KEY".as_slice())
    );
    for (path, bytes) in old_files {
        assert_eq!(
            std::fs::read(path).expect("old synthetic sentinel"),
            bytes,
            "old root is byte-identical"
        );
    }
    restarted.core.shutdown();
}
#[test]
fn retired_variant_profile_and_alpha_adoption_actions_are_not_native_intents() {
    for value in [
        json!({"kind":"setUiVariant","variant":"legacy"}),
        json!({"kind":"chooseProfileImport"}),
        json!({"kind":"inspectV1Import"}),
        json!({"kind":"workbench","request":{"kind":"readAdoption"}}),
    ] {
        assert!(
            serde_json::from_value::<Intent>(value).is_err(),
            "retired request cannot cross authority"
        );
    }
}
