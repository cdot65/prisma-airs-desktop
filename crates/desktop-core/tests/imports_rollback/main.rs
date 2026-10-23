//! C23: the reviewed v1 import and its rollback, through the production
//! authority interface (D050).
//!
//! Each test stages a synthetic v1 folder and v1 Keychain items (a scripted
//! read-only source), hashes them before and after, and drives the import
//! with intents. A relaunch reuses the same storage, receipts and v1 source.

mod recovery;
mod v1;

use std::path::PathBuf;
use std::time::Duration;

use desktop_core::fixtures::{self, Handles, ScriptedStore};
use desktop_core::import_v1::CommitPoint;
use desktop_core::vault::VaultError;
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::v1::{CANARY, V1};

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    history_root: PathBuf,
}

fn temp(name: &str) -> PathBuf {
    let mut n = [0u8; 8];
    getrandom::fill(&mut n).expect("fixture");
    std::env::temp_dir()
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-v2-c23-{name}-{}", hex::encode(n)))
}

/// A desk; `previous` relaunches with the same data, Keychain, receipts and
/// v1 source.
fn start(previous: Option<Desk>) -> Desk {
    start_as(previous, None)
}

/// As [`start`], with `learner` saved as the signed-in Academy account (its
/// offline work opens without Academy).
fn start_as(previous: Option<Desk>, learner: Option<&str>) -> Desk {
    let mut state = previous
        .as_ref()
        .and_then(|old| old.adapters.store.saved())
        .unwrap_or_default();
    if let Some(account) = learner {
        state.learner = Some(desktop_core::store::StoredLearner {
            account_id: account.to_owned(),
            display_name: "Learner".to_owned(),
            record: desktop_core::vault::RecordId::parse("cr-00000000000c2301").expect("record"),
            issuer: "https://idp.example".to_owned(),
            client_id: "desktop_public_client".to_owned(),
        });
    }
    let store = ScriptedStore::with_state(state);
    let (mut adapters, mut handles) = fixtures::adapters_with(store);
    let history_root = previous
        .as_ref()
        .map_or_else(|| temp("history"), |d| d.history_root.clone());
    adapters.harness.history_root = Some(history_root.clone());
    // Rollbacks are confirmed unless a test says otherwise.
    handles.confirmer.auto(Some(true));
    if let Some(old) = previous {
        drop(old.core);
        let o = old.adapters;
        adapters.keychain = o.vault.clone();
        handles.vault = o.vault.clone();
        adapters.learner_keychain = o.learner_vault.clone();
        handles.learner_vault = o.learner_vault.clone();
        adapters.library_root = Some(o.library_root.clone());
        handles.library_root = o.library_root.clone();
        adapters.sessions_root = Some(o.sessions_root.clone());
        handles.sessions_root = o.sessions_root.clone();
        adapters.offline_root = Some(o.offline_root.clone());
        handles.offline_root = o.offline_root.clone();
        adapters.workbench_root = Some(o.workbench_root.clone());
        handles.workbench_root = o.workbench_root.clone();
        adapters.import_root = Some(o.import_root.clone());
        handles.import_root = o.import_root.clone();
        adapters.v1_source = Some(desktop_core::import_v1::V1Source {
            root: o.v1_root.clone(),
            keychain: o.v1_keychain.clone(),
        });
        handles.v1_root = o.v1_root.clone();
        handles.v1_keychain = o.v1_keychain.clone();
    }
    let core = Core::new(CoreConfig::default(), adapters);
    core.start();
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        history_root,
    }
}

impl Desk {
    async fn send(&mut self, intent: Value) -> Result<Receipt, Refusal> {
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

    fn json(&self) -> Value {
        serde_json::to_value(self.snap()).expect("json")
    }

    fn import(&self) -> Value {
        self.json()["v1Import"].clone()
    }

    /// Waits until no import step is under way.
    async fn settled(&self) -> Value {
        for _ in 0..400 {
            let v = self.import();
            if v["activity"].is_null() {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("import did not settle: {}", self.import());
    }

    async fn inspect(&mut self) -> Value {
        self.send(json!({"kind": "inspectV1Import"}))
            .await
            .expect("inspect");
        self.settled().await
    }

    async fn plan(&mut self, categories: &[&str], targets: Value) -> Value {
        self.send(json!({"kind": "planV1Import", "categories": categories, "targets": targets}))
            .await
            .expect("plan");
        self.settled().await
    }

    /// Imports every ready item of the current review.
    async fn import_ready(&mut self) -> Value {
        let plan = self.import()["plan"].clone();
        let items: Vec<u64> = plan["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter(|i| i["status"]["kind"] == "ready")
            .map(|i| i["index"].as_u64().expect("index"))
            .collect();
        self.send(json!({"kind": "confirmV1Import", "review": plan["review"], "items": items}))
            .await
            .expect("confirm");
        self.settled().await
    }

    fn receipt(&self) -> Value {
        self.import()["receipts"]
            .as_array()
            .and_then(|r| r.last().cloned())
            .expect("a receipt")
    }

    async fn roll_back(&mut self, id: &Value) -> Value {
        self.send(json!({"kind": "rollBackV1Import", "receipt": id}))
            .await
            .expect("roll back");
        self.settled().await
    }

    fn tenants(&self) -> Vec<Value> {
        self.json()["tenants"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    fn tenant_named(&self, label: &str) -> Option<String> {
        self.tenants()
            .into_iter()
            .find(|t| t["label"] == label)
            .and_then(|t| t["id"].as_str().map(str::to_owned))
    }

    /// Every case of a tenant, read from its sealed library.
    fn cases(&self, tenant: &str) -> Vec<Value> {
        desktop_core::library::read(
            &self.adapters.library_root,
            self.adapters.vault.as_ref(),
            tenant,
        )
        .map(|l| {
            l.cases
                .iter()
                .map(|c| serde_json::to_value(c).expect("json"))
                .collect()
        })
        .unwrap_or_default()
    }

    fn reports(&self, tenant: &str) -> Vec<Value> {
        let path = desktop_core::sessions::reports_path(&self.adapters.sessions_root, tenant);
        desktop_core::sessions::load_reports(&path)
            .map(|b| {
                b.reports
                    .iter()
                    .map(|r| serde_json::to_value(r).expect("json"))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn conversations(
        &self,
        tenant: &str,
        workspace: &str,
    ) -> Vec<desktop_core::harness::history::Record> {
        let scope =
            desktop_core::harness::history::Scope::new(&self.history_root, tenant, workspace);
        desktop_core::harness::history::list(self.adapters.vault.as_ref(), &scope)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|l| match l {
                desktop_core::harness::history::Listed::Saved { id, .. } => {
                    desktop_core::harness::history::read(self.adapters.vault.as_ref(), &scope, &id)
                        .ok()
                }
                _ => None,
            })
            .collect()
    }
}

/// Every new tenant, all categories that need no Academy account.
const ALL: &[&str] = &[
    "tenants",
    "endpoints",
    "settings",
    "cases",
    "reports",
    "conversations",
];

fn new_targets(view: &Value) -> Value {
    Value::Array(
        view["tenants"]
            .as_array()
            .expect("tenants")
            .iter()
            .map(|t| json!({"source": t["index"], "target": {"kind": "new"}}))
            .collect(),
    )
}

fn item<'a>(plan: &'a Value, category: &str, label: &str) -> &'a Value {
    plan["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|i| i["category"] == category && i["label"] == label)
        .unwrap_or_else(|| panic!("no {category} item for {label}: {plan}"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn inspection_reads_files_only_and_the_keychain_only_for_selected_categories() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    let before = v1.fingerprint();
    let view = desk.inspect().await;
    assert_eq!(view["source"], "ready", "{view}");
    assert_eq!(view["tenants"].as_array().map(Vec::len), Some(2), "{view}");
    assert_eq!(view["tenants"][1]["sessionOnly"], true);
    assert_eq!(view["notMapped"], json!(["navigation"]));
    let category = |c: &str| {
        view["categories"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["category"] == c).cloned())
            .expect("category")
    };
    assert_eq!(category("tenants")["found"], 2);
    assert_eq!(category("reports")["found"], 2);
    // What only a Keychain read could tell is not counted.
    assert!(category("cases")["found"].is_null());
    assert!(category("conversations")["found"].is_null());
    assert!(
        desk.adapters.v1_keychain.reads().is_empty(),
        "no Keychain read"
    );
    // Planning reads only the selected protected items of targeted tenants.
    let targets = json!([{"source": 0, "target": {"kind": "new"}}, {"source": 1, "target": {"kind": "skip"}}]);
    let plan = desk.plan(&["tenants", "cases"], targets).await;
    assert!(plan["plan"].is_object(), "{plan}");
    assert_eq!(
        desk.adapters.v1_keychain.reads(),
        vec![format!("cases-{}", v1::PROD)],
        "only the selected tenant's cases"
    );
    // Nothing of v2 changed by inspecting or reviewing.
    assert!(desk.tenants().is_empty());
    v1.assert_unchanged(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_import_brings_work_without_credentials_and_a_rollback_removes_only_it() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    let before = v1.fingerprint();
    let view = desk.inspect().await;
    let plan = desk.plan(ALL, new_targets(&view)).await;
    let p = &plan["plan"];
    assert_eq!(item(p, "tenants", "Prod")["status"]["kind"], "ready", "{p}");
    assert_eq!(item(p, "cases", "Prod")["counts"]["new"], 2, "{p}");
    assert_eq!(item(p, "reports", "Prod")["counts"]["new"], 1, "{p}");
    // The session-only tenant kept nothing; it is said, not skipped.
    assert_eq!(
        item(p, "cases", "Scratch")["status"],
        json!({"kind": "blocked", "problem": "noStoredWork"})
    );
    // A report beyond v2's sub-request range is counted, not imported.
    assert_eq!(item(p, "reports", "Prod")["counts"]["skipped"], 1);
    let done = desk.import_ready().await;
    assert!(done["problem"].is_null(), "{done}");
    let receipt = desk.receipt();
    assert_eq!(receipt["state"], "applied", "{receipt}");
    // Tenants: names and TSG IDs, the mapped endpoint, and no credential.
    let prod = desk.tenant_named("Prod").expect("imported");
    let t = desk
        .tenants()
        .into_iter()
        .find(|t| t["id"] == prod.as_str())
        .expect("tenant");
    assert_eq!(t["tsgId"], v1::PROD_TSG);
    assert_eq!(
        t["destination"],
        json!({"kind": "region", "region": "europe"})
    );
    assert_eq!(t["scan"]["state"], "missing", "{t}");
    let scratch = desk.tenant_named("Scratch").expect("imported");
    // The secret parts of v1's credential appear nowhere in v2.
    let text = desk.json().to_string();
    assert!(!text.contains(CANARY));
    let saved = serde_json::to_string(&desk.adapters.store.saved()).expect("json");
    assert!(!saved.contains(CANARY));
    let receipt_text = std::fs::read_dir(&desk.adapters.import_root)
        .expect("receipts")
        .flatten()
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect::<String>();
    assert!(!receipt_text.contains(CANARY));
    // Cases rebound to the new tenant, reports, appearance, conversations.
    let cases = desk.cases(&prod);
    assert_eq!(cases.len(), 2);
    assert!(cases.iter().all(|c| c["tenant"] == prod.as_str()));
    assert_eq!(desk.reports(&prod).len(), 1);
    assert_eq!(desk.json()["appearance"], "dark");
    let conversations = desk.conversations(&prod, v1::WORKSPACE);
    assert_eq!(conversations.len(), 1);
    assert!(conversations[0].replay.is_null() && conversations[0].context.is_null());
    assert!(
        conversations[0]
            .entries
            .iter()
            .all(|e| !e.text.contains(CANARY))
    );
    // A preexisting tenant and its work are never touched by the rollback;
    // an imported case edited since is kept.
    let mine = {
        desk.send(
            json!({"kind": "createTenant", "profile": {"label": "Mine", "tsgId": null,
            "destination": {"kind": "region", "region": "unitedStates"}}}),
        )
        .await
        .expect("created");
        desk.tenant_named("Mine").expect("mine")
    };
    v1::edit_case(&desk.adapters, &prod, v1::CASE_B);
    let id = receipt["id"].clone();
    let after = desk.roll_back(&id).await;
    assert!(after["problem"].is_null(), "{after}");
    let receipt = desk.receipt();
    assert_eq!(receipt["state"], "rolledBack", "{receipt}");
    let cases = desk.cases(&prod);
    assert_eq!(cases.len(), 1, "the edited case is kept");
    assert_eq!(cases[0]["id"], v1::CASE_B);
    assert!(desk.reports(&prod).is_empty());
    assert!(desk.conversations(&prod, v1::WORKSPACE).is_empty());
    assert_eq!(desk.json()["appearance"], "system");
    // Prod still holds the edited case, so it stays; Scratch goes.
    assert!(desk.tenant_named("Prod").is_some());
    assert!(desk.tenant_named("Scratch").is_none(), "{scratch}");
    assert_eq!(desk.tenant_named("Mine"), Some(mine));
    let cases_item = receipt["items"]
        .as_array()
        .and_then(|i| {
            i.iter()
                .find(|x| x["category"] == "cases" && x["label"] == "Prod")
        })
        .expect("cases item");
    assert_eq!(cases_item["state"], "keptChanged");
    assert_eq!(cases_item["kept"], 1);
    // v1 is byte for byte as it was, and its Keychain was only read.
    v1.assert_unchanged(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn importing_twice_finds_everything_already_present_and_owns_nothing() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    let view = desk.inspect().await;
    desk.plan(&["tenants", "cases", "reports"], new_targets(&view))
        .await;
    desk.import_ready().await;
    let first = desk.receipt();
    let prod = desk.tenant_named("Prod").expect("imported");
    // Into the same tenant again: everything is already there.
    let view = desk.inspect().await;
    let targets = json!([{"source": 0, "target": {"kind": "existing", "tenant": prod}},
        {"source": 1, "target": {"kind": "skip"}}]);
    assert_eq!(view["tenants"][0]["matches"], json!([prod]));
    let plan = desk.plan(&["cases", "reports"], targets).await;
    let p = &plan["plan"];
    assert_eq!(
        item(p, "cases", "Prod")["status"]["kind"],
        "alreadyPresent",
        "{p}"
    );
    assert_eq!(item(p, "cases", "Prod")["counts"]["present"], 2);
    assert_eq!(
        item(p, "reports", "Prod")["status"]["kind"],
        "alreadyPresent"
    );
    // Nothing ready: nothing to confirm.
    assert_eq!(
        desk.send(json!({"kind": "confirmV1Import", "review": p["review"], "items": [0]}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    // Rolling back the first import still removes exactly its work.
    desk.roll_back(&first["id"]).await;
    assert!(desk.cases(&prod).is_empty());
    drop(v1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conflicts_are_reported_and_never_overwritten() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    // v1 also reported a scan v2 does not hold, so the reports item is ready
    // and its conflicting report is in the write.
    v1.add_report("scan-3", "a new one");
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Ours", "tsgId": v1::PROD_TSG,
        "destination": {"kind": "region", "region": "unitedStates"}}}),
    )
    .await
    .expect("created");
    let ours = desk.tenant_named("Ours").expect("ours");
    // v2 already holds a different case and report under v1's identities.
    v1::plant_case(&desk.adapters, &ours, v1::CASE_A, "Edited in v2");
    v1::plant_report(&desk.adapters, &ours, "scan-1", "edited in v2");
    let view = desk.inspect().await;
    let targets = json!([{"source": 0, "target": {"kind": "existing", "tenant": ours}},
        {"source": 1, "target": {"kind": "skip"}}]);
    let plan = desk.plan(&["cases", "reports"], targets).await;
    let p = &plan["plan"];
    let cases = item(p, "cases", "Prod");
    assert_eq!(cases["status"]["kind"], "ready", "{p}");
    assert_eq!(
        cases["counts"],
        json!({"new": 1, "present": 0, "conflict": 1, "skipped": 0})
    );
    let reports = item(p, "reports", "Prod");
    assert_eq!(reports["status"]["kind"], "ready", "{p}");
    assert_eq!(
        reports["counts"],
        json!({"new": 1, "present": 0, "conflict": 1, "skipped": 1})
    );
    desk.import_ready().await;
    let held = desk.cases(&ours);
    assert_eq!(held.len(), 2);
    assert_eq!(
        held.iter().find(|c| c["id"] == v1::CASE_A).expect("kept")["name"],
        "Edited in v2"
    );
    let held = desk.reports(&ours);
    assert_eq!(held.len(), 2, "{held:?}");
    assert_eq!(
        held.iter().find(|r| r["scanId"] == "scan-1").expect("kept")["reason"],
        "edited in v2"
    );
    // Another TSG's tenant is never a target.
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Other", "tsgId": "999",
        "destination": {"kind": "region", "region": "unitedStates"}}}),
    )
    .await
    .expect("created");
    let other = desk.tenant_named("Other").expect("other");
    let _ = desk.inspect().await;
    let targets = json!([{"source": 0, "target": {"kind": "existing", "tenant": other}}]);
    assert_eq!(
        desk.send(json!({"kind": "planV1Import", "categories": ["cases"], "targets": targets}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    let _ = view;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn damaged_unsupported_linked_and_oversized_sources_block_only_their_category() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    // A newer reports file, an unreadable case item, a linked appearance
    // file and an oversized history item.
    std::fs::write(
        v1.root.join("fpfn-reports.json"),
        br#"{"version":9,"tenants":[]}"#,
    )
    .expect("w");
    desk.adapters
        .v1_keychain
        .plant(&format!("cases-{}", v1::PROD), b"not json");
    std::fs::remove_file(v1.root.join("appearance.json")).expect("rm");
    std::os::unix::fs::symlink("/etc/hosts", v1.root.join("appearance.json")).expect("link");
    desk.adapters.v1_keychain.plant(
        &format!("harness-history-{}", v1::PROD_REF),
        &vec![b' '; 8 * 1024 * 1024 + 1],
    );
    let before = v1.fingerprint();
    let view = desk.inspect().await;
    let plan = desk.plan(ALL, new_targets(&view)).await;
    let p = &plan["plan"];
    let blocked = |c: &str, l: &str| item(p, c, l)["status"].clone();
    assert_eq!(
        blocked("reports", "FP/FN reports"),
        json!({"kind": "blocked", "problem": "unsupportedVersion"})
    );
    assert_eq!(
        blocked("cases", "Prod"),
        json!({"kind": "blocked", "problem": "corrupt"})
    );
    assert_eq!(
        blocked("settings", "Appearance"),
        json!({"kind": "blocked", "problem": "pathRejected"})
    );
    assert_eq!(
        blocked("conversations", "Prod"),
        json!({"kind": "blocked", "problem": "tooLarge"})
    );
    // The rest is still ready.
    assert_eq!(item(p, "tenants", "Prod")["status"]["kind"], "ready");
    v1.assert_unchanged(&before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_running_v1_or_a_locked_keychain_blocks_without_fallback() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    // v1 running: it holds its instance lock exclusively.
    let running = std::fs::File::open(v1.root.join(".desktop-instance.lock")).expect("lock");
    running.try_lock().expect("exclusive");
    let view = desk.inspect().await;
    assert_eq!(view["source"], "locked", "{view}");
    drop(running);
    let view = desk.inspect().await;
    assert_eq!(view["source"], "ready");
    // A locked Keychain: protected categories are blocked by name.
    desk.adapters
        .v1_keychain
        .fail_with(Some(VaultError::Locked));
    let plan = desk.plan(ALL, new_targets(&view)).await;
    let p = &plan["plan"];
    assert_eq!(
        item(p, "cases", "Prod")["status"],
        json!({"kind": "blocked", "problem": "keychainLocked"})
    );
    // The tenant itself needs no Keychain read; only its endpoint is not
    // imported, and the item says why.
    let tenant = item(p, "tenants", "Prod");
    assert_eq!(tenant["status"]["kind"], "ready");
    assert_eq!(tenant["partial"], "keychainLocked");
    desk.adapters
        .v1_keychain
        .fail_with(Some(VaultError::Denied));
    let _ = desk.send(json!({"kind": "dismissV1Import"})).await;
    let view = desk.inspect().await;
    let plan = desk.plan(&["cases"], new_targets(&view)).await;
    assert_eq!(
        item(&plan["plan"], "cases", "Prod")["status"],
        json!({"kind": "blocked", "problem": "accessDenied"})
    );
    // v1 starting while a review is open is refused (the lock is shared).
    let probe = std::fs::File::open(v1.root.join(".desktop-instance.lock")).expect("lock");
    assert!(probe.try_lock().is_err(), "v1 cannot start during a review");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_source_changed_after_review_is_not_imported() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    let view = desk.inspect().await;
    desk.plan(&["tenants", "reports", "settings"], new_targets(&view))
        .await;
    // Changed behind the review (not by v1, which cannot run meanwhile).
    std::fs::write(
        v1.root.join("appearance.json"),
        br#"{"appearance":"light"}"#,
    )
    .expect("w");
    let done = desk.import_ready().await;
    assert_eq!(done["problem"], "sourceChanged", "{done}");
    assert_eq!(
        desk.json()["appearance"],
        "system",
        "nothing of it was written"
    );
    // The import is whole or nothing: what it wrote before is undone.
    assert!(desk.tenants().is_empty(), "{}", desk.json()["tenants"]);
    assert!(
        desk.adapters
            .store
            .saved()
            .is_none_or(|s| s.tenants.is_empty())
    );
    let receipt = desk.receipt();
    assert_eq!(receipt["state"], "rolledBack", "{receipt}");
    assert_eq!(receipt["interrupted"], true);
    assert_eq!(receipt["canResume"], true);
    let settings = receipt["items"]
        .as_array()
        .and_then(|i| i.iter().find(|x| x["category"] == "settings").cloned())
        .expect("settings item");
    assert_eq!(settings["state"], "failed");
    // A review is used once.
    let plan = desk.import();
    assert!(plan["plan"].is_null());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_interrupted_rollback_finishes_after_relaunch() {
    for point in [CommitPoint::Reverting, CommitPoint::Reverted] {
        let mut desk = start(None);
        let _v1 = V1::stage(&desk.adapters);
        let view = desk.inspect().await;
        desk.plan(&["tenants", "cases"], new_targets(&view)).await;
        desk.import_ready().await;
        let id = desk.receipt()["id"].clone();
        desk.core.interrupt_v1_import(point);
        desk.send(json!({"kind": "rollBackV1Import", "receipt": id}))
            .await
            .expect("roll back");
        tokio::time::sleep(Duration::from_millis(500)).await;
        // Hidden while it was being removed.
        assert!(desk.tenants().is_empty(), "{point:?}");
        // The next launch finishes it before anything else writes.
        let desk = start(Some(desk));
        let after = desk.settled().await;
        assert!(after["problem"].is_null(), "{point:?}: {after}");
        let receipt = desk.receipt();
        assert_eq!(receipt["state"], "rolledBack", "{point:?}: {receipt}");
        assert_eq!(receipt["canResume"], false, "it had committed");
        assert!(desk.tenants().is_empty(), "{point:?}");
        let prod = v1_tenant_ids(&desk);
        assert!(prod.is_empty(), "{point:?}: {prod:?}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_declined_rollback_changes_nothing() {
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    let view = desk.inspect().await;
    desk.plan(&["tenants"], new_targets(&view)).await;
    desk.import_ready().await;
    let id = desk.receipt()["id"].clone();
    desk.adapters.confirmer.auto(Some(false));
    desk.roll_back(&id).await;
    assert_eq!(desk.receipt()["state"], "applied");
    assert_eq!(desk.tenants().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn imported_conversations_are_inspect_only() {
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    let view = desk.inspect().await;
    desk.plan(&["tenants", "conversations"], new_targets(&view))
        .await;
    desk.import_ready().await;
    let prod = desk.tenant_named("Prod").expect("tenant");
    let records = desk.conversations(&prod, v1::WORKSPACE);
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(
        r.origin,
        Some(desktop_core::harness::history::Origin::ImportedV1)
    );
    // v1's approval is shown declined and never pending; its notice dropped.
    assert!(
        r.entries
            .iter()
            .any(|e| e.text == "Approval from v1 (not transferred)")
    );
    assert_eq!(r.entries.len(), 3, "{:?}", r.entries);
    // Its binding can never be a live one.
    assert!(r.binding.endpoint.is_empty() && r.binding.credential_epoch == 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reset_waits_for_an_import_and_removes_its_receipts() {
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    let view = desk.inspect().await;
    desk.plan(&["tenants"], new_targets(&view)).await;
    desk.import_ready().await;
    assert!(
        std::fs::read_dir(&desk.adapters.import_root)
            .expect("dir")
            .count()
            > 0
    );
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    for _ in 0..200 {
        if desk.json()["storage"] == "ready" && desk.tenants().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(desk.tenants().is_empty());
    assert!(
        !desk.adapters.import_root.exists(),
        "receipts went with the data"
    );
    assert!(
        desk.import()["receipts"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn academy_drafts_go_only_to_the_same_signed_in_account() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    v1.stage_academy(v1::ACCOUNT);
    let before = v1.fingerprint();
    // Signed out: blocked, and the v1 key is never read.
    desk.inspect().await;
    let plan = desk.plan(&["academyDrafts"], json!([])).await;
    assert_eq!(
        item(&plan["plan"], "academyDrafts", "Academy lab drafts")["status"],
        json!({"kind": "blocked", "problem": "signInRequired"})
    );
    assert!(desk.adapters.v1_keychain.reads().is_empty());
    // Another account: v1's drafts are not its own.
    let mut desk = start_as(Some(desk), Some("someone_else"));
    desk.inspect().await;
    let plan = desk.plan(&["academyDrafts"], json!([])).await;
    assert_eq!(
        item(&plan["plan"], "academyDrafts", "Academy lab drafts")["status"],
        json!({"kind": "blocked", "problem": "accountMismatch"})
    );
    // The same account: the unsaved draft is imported; the saved attempt
    // is not a draft.
    let mut desk = start_as(Some(desk), Some(v1::ACCOUNT));
    desk.inspect().await;
    let plan = desk.plan(&["academyDrafts"], json!([])).await;
    let drafts = item(&plan["plan"], "academyDrafts", "Academy lab drafts");
    assert_eq!(drafts["status"]["kind"], "ready", "{plan}");
    assert_eq!(
        drafts["counts"],
        json!({"new": 1, "present": 0, "conflict": 0, "skipped": 1})
    );
    desk.import_ready().await;
    assert_eq!(desk.receipt()["state"], "applied");
    // v2's offline store now holds exactly v1's draft (same attempt, base
    // version and work), across a relaunch.
    let mut desk = start_as(Some(desk), Some(v1::ACCOUNT));
    assert_eq!(
        drafts_status(&mut desk).await,
        json!({"kind": "alreadyPresent"})
    );
    let id = desk.receipt()["id"].clone();
    desk.roll_back(&id).await;
    assert_eq!(desk.receipt()["state"], "rolledBack");
    assert_eq!(drafts_status(&mut desk).await, json!({"kind": "ready"}));
    v1.assert_unchanged(&before);
}

/// How a fresh review sees v1's drafts against v2's offline store.
async fn drafts_status(desk: &mut Desk) -> Value {
    let _ = desk.send(json!({"kind": "dismissV1Import"})).await;
    desk.inspect().await;
    let plan = desk.plan(&["academyDrafts"], json!([])).await;
    item(&plan["plan"], "academyDrafts", "Academy lab drafts")["status"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn api_client_collections_join_the_recent_list_and_leave_it_on_rollback() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    // A collection of the operator's (it stays where it is) that v1 listed.
    let collection = temp("collection");
    std::fs::create_dir_all(&collection).expect("fixture");
    std::fs::write(
        collection.join("collection.toml"),
        "format_version = 1\nid = \"c23\"\nname = \"Orders API\"\n",
    )
    .expect("fixture");
    let collection = collection.canonicalize().expect("fixture");
    let profile = v1.root.join("api-client");
    std::fs::create_dir_all(&profile).expect("fixture");
    std::fs::write(
        profile.join("recent-collections.json"),
        serde_json::to_vec(&json!([collection])).expect("json"),
    )
    .expect("fixture");
    let before = v1.fingerprint();
    let view = desk.inspect().await;
    let found = view["categories"]
        .as_array()
        .and_then(|c| c.iter().find(|x| x["category"] == "apiClient").cloned())
        .expect("category");
    assert_eq!(found["found"], 1, "{view}");
    let plan = desk.plan(&["apiClient"], json!([])).await;
    let item = item(&plan["plan"], "apiClient", "API Client");
    assert_eq!(item["counts"]["new"], 1, "{plan}");
    desk.import_ready().await;
    assert_eq!(desk.receipt()["state"], "applied");
    let recent = || desk.json()["workbench"]["recent"].clone();
    assert_eq!(recent()[0]["name"], "Orders API", "{}", recent());
    // Reviewed again, it is already there.
    let _ = desk.inspect().await;
    let plan = desk.plan(&["apiClient"], json!([])).await;
    assert_eq!(
        item_status(&plan, "apiClient", "API Client"),
        json!({"kind": "alreadyPresent"})
    );
    let _ = desk.send(json!({"kind": "dismissV1Import"})).await;
    let id = desk.receipt()["id"].clone();
    desk.roll_back(&id).await;
    assert_eq!(desk.receipt()["state"], "rolledBack");
    assert!(
        desk.json()["workbench"]["recent"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    // The collection itself is never touched.
    assert!(collection.join("collection.toml").is_file());
    v1.assert_unchanged(&before);
}

/// Tenants the saved document holds (shown or not).
fn v1_tenant_ids(desk: &Desk) -> Vec<String> {
    desk.adapters
        .store
        .saved()
        .map(|s| s.tenants.iter().map(|t| t.id.as_str().to_owned()).collect())
        .unwrap_or_default()
}

fn item_status(plan: &Value, category: &str, label: &str) -> Value {
    item(&plan["plan"], category, label)["status"].clone()
}
