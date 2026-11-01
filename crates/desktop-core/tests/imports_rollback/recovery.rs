//! C23 review attempt-01 (F1–F5, G1, G2): an import is whole or nothing.
//!
//! Interrupted at every commit point of every category, nothing of it is
//! shown and the next launch undoes it before anything else writes; work
//! added by anyone else is never claimed, even with identical content; a
//! receipt that is not durably recorded authorizes no write; and drafts
//! are only ever written, read back or removed for the account they were
//! planned for.

use std::path::PathBuf;
use std::time::Duration;

use desktop_core::Refusal;
use desktop_core::import_v1::CommitPoint;
use serde_json::{Value, json};

use crate::v1::{self, V1};
use crate::{Desk, drafts_status, item, new_targets, start, start_as, temp, v1_tenant_ids};

/// Every category, all v1 tenants new, signed in as v1's Academy account.
const EVERYTHING: &[&str] = &[
    "tenants",
    "endpoints",
    "settings",
    "cases",
    "reports",
    "conversations",
    "academyDrafts",
    "apiClient",
];

/// v1 with work in every category (and an API Client collection).
fn stage_everything(desk: &Desk) -> (V1, PathBuf) {
    let v1 = V1::stage(&desk.adapters);
    v1.stage_academy(v1::ACCOUNT);
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
    (v1, collection)
}

/// Confirms every ready item without waiting; how many there are.
async fn confirm_ready(desk: &mut Desk) -> usize {
    let plan = desk.import()["plan"].clone();
    let items: Vec<u64> = plan["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|i| i["status"]["kind"] == "ready")
        .map(|i| i["index"].as_u64().expect("index"))
        .collect();
    desk.send(json!({"kind": "confirmV1Import", "review": plan["review"], "items": items}))
        .await
        .expect("confirm");
    items.len()
}

/// The v2 tenants the newest receipt on disk writes into.
fn receipt_targets(desk: &Desk) -> Vec<String> {
    let mut targets = Vec::new();
    for entry in std::fs::read_dir(&desk.adapters.import_root)
        .into_iter()
        .flatten()
        .flatten()
    {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(r) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        for i in r["items"].as_array().into_iter().flatten() {
            if let Some(t) = i["target"].as_str()
                && !targets.iter().any(|x| x == t)
            {
                targets.push(t.to_owned());
            }
        }
    }
    targets
}

/// No view shows anything of v1's work.
fn nothing_shown(desk: &Desk, why: &str) {
    let snap = desk.json();
    assert!(
        snap["tenants"].as_array().is_some_and(Vec::is_empty),
        "{why}: {}",
        snap["tenants"]
    );
    assert_eq!(snap["appearance"], "system", "{why}");
    assert!(
        snap["workbench"]["recent"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{why}: {}",
        snap["workbench"]["recent"]
    );
    assert!(
        snap["academyOffline"]["localDrafts"]
            .as_array()
            .is_none_or(Vec::is_empty),
        "{why}: {}",
        snap["academyOffline"]
    );
}

/// v2 keeps nothing of v1's work, on disk.
async fn nothing_kept(desk: &mut Desk, targets: &[String], why: &str) {
    nothing_shown(desk, why);
    assert!(v1_tenant_ids(desk).is_empty(), "{why}");
    assert_eq!(
        desk.adapters.store.saved().map(|s| s.appearance),
        Some(desktop_core::wire::Theme::System),
        "{why}"
    );
    for t in targets {
        assert!(desk.cases(t).is_empty(), "{why}: cases of {t}");
        assert!(desk.reports(t).is_empty(), "{why}: reports of {t}");
        assert!(
            desk.conversations(t, v1::WORKSPACE).is_empty(),
            "{why}: conversations of {t}"
        );
    }
    assert_eq!(
        drafts_status(desk).await,
        json!({"kind": "ready"}),
        "{why}: no draft kept"
    );
    let _ = desk.send(json!({"kind": "dismissV1Import"})).await;
}

/// Interrupted at every commit point of every item (each category, and the
/// writes of each multi-entry item, noted together before any is made):
/// nothing is shown before the relaunch, what the import writes waits, and
/// the launch undoes it before anything else writes. Importing it again
/// then writes everything once, and its rollback removes it all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_import_interrupted_anywhere_shows_nothing_and_is_undone_at_the_next_launch() {
    // How many items write.
    let ready = {
        let mut desk = start_as(None, Some(v1::ACCOUNT));
        let _staged = stage_everything(&desk);
        let view = desk.inspect().await;
        desk.plan(EVERYTHING, new_targets(&view)).await;
        let plan = desk.import()["plan"].clone();
        plan["items"]
            .as_array()
            .expect("items")
            .iter()
            .filter(|i| i["status"]["kind"] == "ready")
            .count()
    };
    assert!(ready >= 8, "every category has an item: {ready}");
    let mut points = vec![(CommitPoint::Created, 0)];
    for n in 0..ready {
        let n = u32::try_from(n).expect("count");
        points.extend([
            (CommitPoint::Writing, n),
            (CommitPoint::Written, n),
            (CommitPoint::Applied, n),
        ]);
    }
    for (point, skip) in points {
        let why = format!("{point:?} #{skip}");
        let mut desk = start_as(None, Some(v1::ACCOUNT));
        let (v1, _collection) = stage_everything(&desk);
        let before = v1.fingerprint();
        let view = desk.inspect().await;
        desk.plan(EVERYTHING, new_targets(&view)).await;
        desk.core.interrupt_v1_import_at(point, skip);
        confirm_ready(&mut desk).await;
        desk.interrupted().await;
        // Nothing of an import that has not committed is shown.
        nothing_shown(&desk, &why);
        // What it writes waits meanwhile.
        assert_eq!(
            desk.send(
                json!({"kind": "createTenant", "profile": {"label": "Later", "tsgId": null,
                "destination": {"kind": "region", "region": "unitedStates"}}})
            )
            .await
            .err(),
            Some(Refusal::Busy),
            "{why}"
        );
        let targets = receipt_targets(&desk);
        // The app "ends" here and is opened again: the import is undone
        // before anything else writes.
        let mut desk = start_as(Some(desk), Some(v1::ACCOUNT));
        let done = desk.settled().await;
        assert!(done["problem"].is_null(), "{why}: {done}");
        let receipt = desk.receipt();
        assert_eq!(receipt["state"], "rolledBack", "{why}: {receipt}");
        assert_eq!(receipt["interrupted"], true, "{why}");
        assert_eq!(receipt["canResume"], true, "{why}");
        nothing_kept(&mut desk, &targets, &why).await;
        // Imported again: v1 is read again and every entry written once.
        desk.send(json!({"kind": "resumeV1Import", "receipt": receipt["id"]}))
            .await
            .expect("resume");
        let done = desk.settled().await;
        assert!(done["problem"].is_null(), "{why}: {done}");
        assert_eq!(desk.receipt()["state"], "applied", "{why}");
        let prod = desk.tenant_named("Prod").expect("tenant");
        assert_eq!(
            desk.tenants()
                .iter()
                .filter(|t| t["label"] == "Prod")
                .count(),
            1,
            "{why}: once"
        );
        assert_eq!(desk.cases(&prod).len(), 2, "{why}");
        assert_eq!(desk.reports(&prod).len(), 1, "{why}");
        assert_eq!(desk.conversations(&prod, v1::WORKSPACE).len(), 1, "{why}");
        assert_eq!(desk.json()["appearance"], "dark", "{why}");
        assert_eq!(
            desk.json()["workbench"]["recent"].as_array().map(Vec::len),
            Some(1),
            "{why}"
        );
        assert_eq!(
            desk.json()["academyOffline"]["localDrafts"]
                .as_array()
                .map(Vec::len),
            Some(1),
            "{why}"
        );
        // Every entry is noted once: a rollback removes all of them.
        let after = desk.roll_back(&desk.receipt()["id"]).await;
        assert!(after["problem"].is_null(), "{why}: {after}");
        let r = desk.receipt();
        assert!(
            r["items"]
                .as_array()
                .is_some_and(|i| i.iter().all(|x| x["state"] == "reverted")),
            "{why}: {r}"
        );
        let targets = receipt_targets(&desk);
        nothing_kept(&mut desk, &targets, &why).await;
        v1.assert_unchanged(&before);
    }
}

/// F4: interrupted before its case write, relaunched, and the operator then
/// adds the identical case: importing again and rolling back never claims
/// or removes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_identical_case_added_after_an_interruption_is_never_claimed() {
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Ours", "tsgId": v1::PROD_TSG,
        "destination": {"kind": "region", "region": "unitedStates"}}}),
    )
    .await
    .expect("created");
    let ours = desk.tenant_named("Ours").expect("ours");
    let _ = desk.inspect().await;
    let targets = json!([{"source": 0, "target": {"kind": "existing", "tenant": ours}},
        {"source": 1, "target": {"kind": "skip"}}]);
    desk.plan(&["cases"], targets.clone()).await;
    // Stops after the receipt names both cases, before the library write.
    desk.core.interrupt_v1_import(CommitPoint::Writing);
    confirm_ready(&mut desk).await;
    desk.interrupted().await;
    assert!(desk.cases(&ours).is_empty(), "not written");
    let mut desk = start(Some(desk));
    desk.settled().await;
    let first = desk.receipt();
    assert_eq!(first["state"], "rolledBack", "{first}");
    // The operator adds the very same case (same identity and content).
    v1::plant_case(&desk.adapters, &ours, v1::CASE_A, "Injection");
    let _ = desk.inspect().await;
    let plan = desk.plan(&["cases"], targets).await;
    assert_eq!(
        item(&plan["plan"], "cases", "Prod")["counts"],
        json!({"new": 1, "present": 1, "conflict": 0, "skipped": 0}),
        "identical: {plan}"
    );
    let _ = desk.send(json!({"kind": "dismissV1Import"})).await;
    desk.send(json!({"kind": "resumeV1Import", "receipt": first["id"]}))
        .await
        .expect("resume");
    let done = desk.settled().await;
    assert!(done["problem"].is_null(), "{done}");
    let receipt = desk.receipt();
    assert_eq!(receipt["state"], "applied");
    let cases = receipt["items"]
        .as_array()
        .and_then(|i| i.iter().find(|x| x["category"] == "cases").cloned())
        .expect("cases item");
    assert_eq!(cases["written"], 1, "only the case it added: {receipt}");
    assert_eq!(desk.cases(&ours).len(), 2);
    let after = desk.roll_back(&receipt["id"]).await;
    assert!(after["problem"].is_null(), "{after}");
    let held = desk.cases(&ours);
    assert_eq!(held.len(), 1, "{held:?}");
    assert_eq!(held[0]["id"], v1::CASE_A, "the operator's case stays");
    drop(v1);
}

/// F5: an import into an existing tenant whose library is open, stopped
/// after its cases and before its reports: the library shows the previous
/// generation until the import commits, and work the operator authors after
/// the relaunch survives importing again and rolling back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_open_library_shows_the_previous_generation_until_the_import_commits() {
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Ours", "tsgId": v1::PROD_TSG,
        "destination": {"kind": "region", "region": "unitedStates"}}}),
    )
    .await
    .expect("created");
    let ours = desk.tenant_named("Ours").expect("ours");
    let names = |desk: &Desk| -> Vec<String> {
        desk.json()["library"]["cases"]
            .as_array()
            .map(|c| {
                c.iter()
                    .map(|x| x["name"].as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    };
    open_library(&mut desk).await;
    save_case(&mut desk, "Mine").await;
    assert_eq!(names(&desk), vec!["Mine".to_owned()]);
    let _ = desk.inspect().await;
    let targets = json!([{"source": 0, "target": {"kind": "existing", "tenant": ours}},
        {"source": 1, "target": {"kind": "skip"}}]);
    desk.plan(&["cases", "reports"], targets).await;
    // The cases item is applied; the reports item never starts.
    desk.core.interrupt_v1_import(CommitPoint::Applied);
    confirm_ready(&mut desk).await;
    desk.interrupted().await;
    assert_eq!(desk.cases(&ours).len(), 3, "written to the library");
    assert_eq!(
        names(&desk),
        vec!["Mine".to_owned()],
        "shown only once committed"
    );
    let mut desk = start(Some(desk));
    desk.settled().await;
    let first = desk.receipt();
    assert_eq!(first["state"], "rolledBack", "{first}");
    assert_eq!(desk.cases(&ours).len(), 1, "undone at launch");
    // The operator's work after the relaunch.
    open_library(&mut desk).await;
    save_case(&mut desk, "Later").await;
    desk.send(json!({"kind": "resumeV1Import", "receipt": first["id"]}))
        .await
        .expect("resume");
    let done = desk.settled().await;
    assert!(done["problem"].is_null(), "{done}");
    let mut shown = names(&desk);
    shown.sort();
    assert_eq!(shown, ["Injection", "Jailbreak", "Later", "Mine"]);
    assert_eq!(desk.reports(&ours).len(), 1);
    desk.roll_back(&desk.receipt()["id"]).await;
    let mut shown = names(&desk);
    shown.sort();
    assert_eq!(shown, ["Later", "Mine"]);
    assert!(desk.reports(&ours).is_empty());
}

pub(crate) async fn open_library(desk: &mut Desk) {
    let context = desk.json()["active"]["context"].clone();
    desk.send(json!({"kind": "openLibrary", "context": context}))
        .await
        .expect("open");
    for _ in 0..200 {
        if desk.json()["library"]["state"] == "ready" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("library did not open: {}", desk.json()["library"]);
}

pub(crate) async fn save_case(desk: &mut Desk, name: &str) {
    let context = desk.json()["active"]["context"].clone();
    let count = desk.json()["library"]["cases"]
        .as_array()
        .map_or(0, Vec::len);
    desk.send(
        json!({"kind": "saveCase", "context": context, "draft": {"product": "runtime-security",
        "name": name, "profile": "prod", "prompt": "a prompt", "response": "",
        "appName": "Desktop", "expected": "", "payload": ""}}),
    )
    .await
    .expect("saved");
    for _ in 0..200 {
        if desk.json()["library"]["cases"]
            .as_array()
            .is_some_and(|c| c.len() > count)
        {
            // Its write has settled too.
            tokio::time::sleep(Duration::from_millis(100)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("case not saved: {}", desk.json()["library"]);
}

/// F3: a receipt whose directory sync fails authorizes no owner write: at
/// creation, before an item's write, after earlier items, and before a
/// rollback's removals. Each relaunch shows a truthful receipt.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_receipt_that_is_not_durably_recorded_authorizes_no_write() {
    use desktop_core::import_v1::receipt::fail_sync;
    // At creation, and at the first item's writing note: nothing written.
    for after in [0, 1] {
        let mut desk = start(None);
        let v1 = V1::stage(&desk.adapters);
        let view = desk.inspect().await;
        desk.plan(&["tenants", "cases", "reports"], new_targets(&view))
            .await;
        fail_sync(&desk.adapters.import_root, Some(after));
        let done = desk.import_ready().await;
        assert_eq!(done["problem"], "notRecorded", "#{after}: {done}");
        assert!(v1_tenant_ids(&desk).is_empty(), "#{after}");
        let targets = receipt_targets(&desk);
        fail_sync(&desk.adapters.import_root, None);
        let desk = start(Some(desk));
        desk.settled().await;
        for r in desk.import()["receipts"].as_array().into_iter().flatten() {
            assert_eq!(r["state"], "rolledBack", "#{after}: {r}");
        }
        for t in &targets {
            assert!(desk.cases(t).is_empty() && desk.reports(t).is_empty());
        }
        assert!(desk.tenants().is_empty());
        drop(v1);
    }
    // After two items: the rollback cannot be noted either, so nothing is
    // removed yet and nothing is shown; the next launch undoes it.
    let mut desk = start(None);
    let v1 = V1::stage(&desk.adapters);
    let view = desk.inspect().await;
    desk.plan(&["tenants", "cases", "reports"], new_targets(&view))
        .await;
    // Created, then two items written and applied.
    fail_sync(&desk.adapters.import_root, Some(5));
    let done = desk.import_ready().await;
    assert_eq!(done["problem"], "notRecorded", "{done}");
    let prod_on_disk = v1_tenant_ids(&desk);
    assert!(!prod_on_disk.is_empty(), "written before the failure");
    assert!(desk.tenants().is_empty(), "never shown");
    let receipt = desk.receipt();
    assert_ne!(receipt["state"], "applied", "{receipt}");
    assert_eq!(receipt["canRollBack"], true);
    let targets = receipt_targets(&desk);
    fail_sync(&desk.adapters.import_root, None);
    let mut desk = start(Some(desk));
    let done = desk.settled().await;
    assert!(done["problem"].is_null(), "{done}");
    let receipt = desk.receipt();
    assert_eq!(receipt["state"], "rolledBack", "{receipt}");
    assert_eq!(receipt["interrupted"], true);
    assert!(v1_tenant_ids(&desk).is_empty());
    for t in &targets {
        assert!(desk.cases(t).is_empty(), "{t}");
    }
    // A committed import whose rollback cannot be noted stays as it was.
    let _ = desk.inspect().await;
    let view = desk.inspect().await;
    desk.plan(&["tenants", "cases"], new_targets(&view)).await;
    desk.import_ready().await;
    let committed = desk.receipt();
    assert_eq!(committed["state"], "applied");
    let shown = desk.tenants().len();
    assert_eq!(shown, 2);
    fail_sync(&desk.adapters.import_root, Some(0));
    let after = desk.roll_back(&committed["id"]).await;
    assert_eq!(after["problem"], "notRecorded", "{after}");
    assert_eq!(desk.receipt()["state"], "applied");
    assert_eq!(desk.tenants().len(), shown, "nothing removed");
    fail_sync(&desk.adapters.import_root, None);
    let after = desk.roll_back(&committed["id"]).await;
    assert!(after["problem"].is_null(), "{after}");
    assert!(desk.tenants().is_empty());
    drop(v1);
}

/// F1, F2, G1: drafts are written, shown, read and removed only for the
/// account they were planned for; one saved through Academy meanwhile is
/// never replaced or removed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn drafts_belong_to_the_planned_account_through_import_and_rollback() {
    // Signed out between the review and the write: refused, nothing kept.
    let mut desk = start_as(None, Some(v1::ACCOUNT));
    let v1 = V1::stage(&desk.adapters);
    v1.stage_academy(v1::ACCOUNT);
    let before = v1.fingerprint();
    desk.inspect().await;
    desk.plan(&["academyDrafts"], json!([])).await;
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("signed out");
    let done = desk.import_ready().await;
    assert_eq!(done["problem"], "signInRequired", "{done}");
    assert_eq!(desk.receipt()["state"], "rolledBack");
    // Imported for the account.
    let mut desk = start_as(Some(desk), Some(v1::ACCOUNT));
    assert_eq!(drafts_status(&mut desk).await, json!({"kind": "ready"}));
    desk.import_ready().await;
    let imported = desk.receipt();
    assert_eq!(imported["state"], "applied");
    // Another account signs in and saves its own draft for the same
    // attempt, with the very same content.
    let mut desk = start_as(Some(desk), Some("someone_else"));
    save_draft(
        &mut desk,
        "a-draft",
        r#"{"steps":{"s1":"my answer"},"commands":[]}"#,
        4,
    )
    .await;
    // Rolling back the first account's import never touches it: the
    // import's draft cannot be removed without its account.
    let after = desk.roll_back(&imported["id"]).await;
    assert_eq!(after["problem"], "accessDenied", "{after}");
    assert_eq!(receipt_of(&desk, &imported["id"])["state"], "rollingBack");
    assert_eq!(
        local_drafts(&desk),
        vec!["a-draft".to_owned()],
        "the other account's draft is intact, and shown to it"
    );
    assert_eq!(
        loaded_draft(&mut desk, "a-draft").await["baseVersion"],
        4,
        "and read back"
    );
    // The planned account signs in again: its launch finishes the rollback.
    let desk = start_as(Some(desk), Some(v1::ACCOUNT));
    let done = desk.settled().await;
    assert!(done["problem"].is_null(), "{done}");
    assert_eq!(receipt_of(&desk, &imported["id"])["state"], "rolledBack");
    assert!(local_drafts(&desk).is_empty());
    // Edited through Academy after the review: the import does not
    // replace it, and an edit after the import is kept by the rollback.
    let mut desk = start_as(Some(desk), Some(v1::ACCOUNT));
    desk.inspect().await;
    desk.plan(&["academyDrafts"], json!([])).await;
    save_draft(&mut desk, "a-draft", r#"{"steps":{"s1":"newer"}}"#, 5).await;
    let done = desk.import_ready().await;
    assert!(done["problem"].is_null(), "{done}");
    let receipt = desk.receipt();
    assert_eq!(receipt["items"][0]["written"], 0, "{receipt}");
    assert_eq!(loaded_draft(&mut desk, "a-draft").await["baseVersion"], 5);
    desk.roll_back(&receipt["id"]).await;
    assert_eq!(loaded_draft(&mut desk, "a-draft").await["baseVersion"], 5);
    // A second unsaved v1 draft: imported, then edited through Academy;
    // the rollback keeps the edit.
    v1.assert_unchanged(&before);
    v1.attempt(v1::ACCOUNT, "b-draft", true);
    let before = v1.fingerprint();
    let _ = desk.send(json!({"kind": "dismissV1Import"})).await;
    desk.inspect().await;
    desk.plan(&["academyDrafts"], json!([])).await;
    desk.import_ready().await;
    let receipt = desk.receipt();
    assert_eq!(receipt["items"][0]["written"], 1, "{receipt}");
    save_draft(&mut desk, "b-draft", r#"{"steps":{"s1":"edited"}}"#, 6).await;
    let after = desk.roll_back(&receipt["id"]).await;
    assert!(after["problem"].is_null(), "{after}");
    let r = desk.receipt();
    assert_eq!(r["items"][0]["state"], "keptChanged", "{r}");
    assert_eq!(loaded_draft(&mut desk, "b-draft").await["baseVersion"], 6);
    assert_eq!(loaded_draft(&mut desk, "a-draft").await["baseVersion"], 5);
    v1.assert_unchanged(&before);
}

/// A receipt by its ID (receipts made in the same second may list in
/// either order after a relaunch).
fn receipt_of(desk: &Desk, id: &Value) -> Value {
    desk.import()["receipts"]
        .as_array()
        .and_then(|r| r.iter().find(|x| &x["id"] == id).cloned())
        .expect("the receipt")
}

fn local_drafts(desk: &Desk) -> Vec<String> {
    desk.json()["academyOffline"]["localDrafts"]
        .as_array()
        .map(|d| {
            d.iter()
                .map(|x| x["attemptId"].as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// An Academy offline call's reply, observed as the presentation does.
async fn call(desk: &mut Desk, intent: Value) -> Value {
    let receipt = desk.send(intent).await.expect("admitted");
    let desktop_core::Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!("no operation: {receipt:?}")
    };
    let Ok(desktop_core::Observation::AcademyCall(handle)) = desk
        .handle
        .observe(desktop_core::ViewScope::AcademyCall(operation))
    else {
        panic!("call observation");
    };
    let result = tokio::time::timeout(Duration::from_secs(20), handle.result())
        .await
        .expect("result in time");
    serde_json::to_value(result).expect("json")
}

async fn offline_ready(desk: &Desk) {
    for _ in 0..200 {
        // Opened, or nothing kept yet (the first save makes the store).
        let state = desk.json()["academyOffline"]["state"].clone();
        if state == "ready" || state == "empty" {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "offline work did not open: {}",
        desk.json()["academyOffline"]
    );
}

async fn save_draft(desk: &mut Desk, attempt: &str, draft: &str, base: u32) {
    offline_ready(desk).await;
    let learner = desk.json()["learner"].clone();
    let reply = call(
        desk,
        json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": attempt,
        "baseVersion": base, "draft": draft}),
    )
    .await;
    assert_eq!(reply["kind"], "data", "{reply}");
}

async fn loaded_draft(desk: &mut Desk, attempt: &str) -> Value {
    let learner = desk.json()["learner"].clone();
    let reply = call(
        desk,
        json!({"kind": "loadLocalDraft", "learner": learner, "attemptId": attempt}),
    )
    .await;
    let data: Value = serde_json::from_str(reply["data"].as_str().unwrap_or("null")).expect("json");
    data["json"].clone()
}
