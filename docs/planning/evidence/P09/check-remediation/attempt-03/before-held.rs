//! C23 review attempt-02 (F1–F3, G1): what an unsettled import names stays
//! held until it settles.
//!
//! A launch rollback that cannot finish keeps what the import writes
//! waiting until a retry settles it; a paused import grants no authority
//! over the tenants and conversations it staged, through any request; and a
//! receipt is read from its second copy when its first is damaged or its
//! folder cannot be read, while one readable in neither copy holds what
//! imports write until the operator releases it.

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use desktop_core::Refusal;
use desktop_core::import_v1::{CommitPoint, receipt};
use desktop_core::store::{AppStore, FileStore, GatewayPreference, PersistedState};
use desktop_core::tenants::TenantId;
use desktop_core::vault::VaultError;
use serde_json::{Value, json};

use crate::recovery::{open_library, save_case};
use crate::v1::{self, V1};
use crate::{Desk, new_targets, start, start_edited, start_on_disk, temp, v1_tenant_ids};

/// Creates the operator's own tenant, sharing v1 Prod's TSG ID.
async fn ours(desk: &mut Desk) -> String {
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Ours", "tsgId": v1::PROD_TSG,
        "destination": {"kind": "region", "region": "unitedStates"}}}),
    )
    .await
    .expect("created");
    desk.tenant_named("Ours").expect("ours")
}

/// How many items of the current plan are ready.
fn ready(desk: &Desk) -> u32 {
    let count = desk.import()["plan"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter(|i| i["status"]["kind"] == "ready")
        .count();
    u32::try_from(count).expect("count")
}

/// Confirms every ready item.
async fn confirm(desk: &mut Desk) {
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
}

/// The one receipt's ID, from its folder.
fn receipt_id(desk: &Desk) -> String {
    let names: Vec<String> = std::fs::read_dir(&desk.adapters.import_root)
        .expect("receipts")
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".json")
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    names[0].clone()
}

/// v1's tenants imported new, stopped once the first (a tenant) is written
/// and noted applied: the receipt has not committed.
async fn stopped_after_a_tenant(desk: &mut Desk) {
    let view = desk.inspect().await;
    desk.plan(&["tenants"], new_targets(&view)).await;
    desk.core.interrupt_v1_import_at(CommitPoint::Applied, 0);
    confirm(desk).await;
    until(desk, |d| v1_tenant_ids(d).len() == 1).await;
    // Paused there: nothing more is written.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(v1_tenant_ids(desk).len(), 1, "one tenant written");
}

/// Waits (up to 10 s) for a paused import to reach its stop, or a relaunch
/// to show what it read.
async fn until(desk: &Desk, reached: impl Fn(&Desk) -> bool) {
    for _ in 0..400 {
        if reached(desk) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!(
        "the import did not reach the state awaited: {}",
        desk.import()
    );
}

fn create_later() -> Value {
    json!({"kind": "createTenant", "profile": {"label": "Later", "tsgId": null,
        "destination": {"kind": "region", "region": "unitedStates"}}})
}

/// F1: the launch rollback cannot read the sealed library (the Keychain is
/// locked), so the receipt stays unsettled: what it names stays hidden and
/// what it writes stays waiting, also once the Keychain is back, until a
/// retried rollback settles it. The operator's identical case is then
/// theirs alone.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rollback_that_cannot_finish_keeps_what_the_import_writes_waiting() {
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    let ours = ours(&mut desk).await;
    // Ours has a sealed library already.
    v1::plant_case(
        &desk.adapters,
        &ours,
        "66666666-7777-4888-8999-aaaaaaaaaaaa",
        "Existing",
    );
    let _ = desk.inspect().await;
    let targets = json!([{"source": 0, "target": {"kind": "existing", "tenant": ours}},
        {"source": 1, "target": {"kind": "skip"}}]);
    desk.plan(&["cases"], targets).await;
    // Stops with the cases named, before the library write.
    desk.core.interrupt_v1_import(CommitPoint::Writing);
    confirm(&mut desk).await;
    until(&desk, |d| {
        std::fs::read_dir(&d.adapters.import_root)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
            .any(|r| {
                r["items"]
                    .as_array()
                    .is_some_and(|i| i.iter().any(|x| x["state"] == "writing"))
            })
    })
    .await;
    desk.adapters.vault.fail_with(Some(VaultError::Locked));
    let mut desk = start(Some(desk));
    let done = desk.settled().await;
    assert!(
        !done["problem"].is_null(),
        "the rollback could not finish: {done}"
    );
    let held = desk.receipt();
    assert_eq!(held["state"], "rollingBack", "{held}");
    // The Keychain is back; the receipt has still not settled.
    desk.adapters.vault.fail_with(None);
    open_library(&mut desk).await;
    let context = desk.json()["active"]["context"].clone();
    assert_eq!(
        desk.send(
            json!({"kind": "saveCase", "context": context, "draft": {"product": "runtime-security",
            "name": "Mine", "profile": "prod", "prompt": "a prompt", "response": "",
            "appName": "Desktop", "expected": "", "payload": ""}})
        )
        .await
        .err(),
        Some(Refusal::Busy),
        "a case write waits"
    );
    assert_eq!(
        desk.send(create_later()).await.err(),
        Some(Refusal::Busy),
        "a tenant write waits"
    );
    // Retried, the rollback settles.
    let after = desk.roll_back(&held["id"]).await;
    assert!(after["problem"].is_null(), "{after}");
    assert_eq!(desk.receipt()["state"], "rolledBack");
    // Now the operator's own work goes ahead through the ordinary case
    // save, a case named like v1's included, and nothing can claim it.
    save_case(&mut desk, "Injection").await;
    until(&desk, |d| d.cases(&ours).len() == 2).await;
    let mut names: Vec<String> = desk
        .cases(&ours)
        .iter()
        .map(|c| c["name"].as_str().unwrap_or_default().to_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["Existing".to_owned(), "Injection".to_owned()]);
    assert!(desk.send(create_later()).await.is_ok());
    assert!(
        desk.send(json!({"kind": "rollBackV1Import", "receipt": held["id"]}))
            .await
            .is_err(),
        "a settled rollback is not run again"
    );
}

/// F2: an import paused before it commits grants no authority over what it
/// staged, through normal delivery: its new tenant cannot be selected (nor
/// is it listed, matched or shown active) and its conversation cannot be
/// opened or resumed by its ID; relaunched, it is undone and the operator's
/// own tenant is still the active one.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_paused_import_grants_no_authority_over_what_it_staged() {
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    let ours = ours(&mut desk).await;
    // Ours works in v1's Gateway workspace, where v1's conversation goes.
    let tenant = TenantId::parse(&ours).expect("id");
    let mut desk = start_edited(Some(desk), None, |s| {
        s.gateway_workspaces.push(GatewayPreference {
            tenant,
            workspace: v1::WORKSPACE.to_owned(),
        });
    });
    let _ = desk.inspect().await;
    let targets = json!([{"source": 0, "target": {"kind": "existing", "tenant": ours}},
        {"source": 1, "target": {"kind": "new"}}]);
    desk.plan(&["tenants", "conversations"], targets).await;
    // Every item written and noted applied; the receipt never commits.
    let last = ready(&desk) - 1;
    desk.core.interrupt_v1_import_at(CommitPoint::Applied, last);
    confirm(&mut desk).await;
    until(&desk, |d| {
        v1_tenant_ids(d).len() == 2 && d.conversations(&ours, v1::WORKSPACE).len() == 1
    })
    .await;
    let staged: Vec<String> = v1_tenant_ids(&desk)
        .into_iter()
        .filter(|t| *t != ours)
        .collect();
    assert_eq!(staged.len(), 1, "Scratch was written: {staged:?}");
    let conversations = desk.conversations(&ours, v1::WORKSPACE);
    assert_eq!(conversations.len(), 1, "the conversation was saved");
    let conversation = conversations[0].id.clone();
    // The staged tenant: not listed, not selectable, never active.
    assert!(desk.tenants().iter().all(|t| t["id"] != staged[0]));
    assert_eq!(
        desk.send(json!({"kind": "selectTenant", "tenant": staged[0]}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(desk.json()["active"]["tenant"], ours);
    // The staged conversation: not opened, so never resumed.
    let context = desk.json()["active"]["context"].clone();
    assert_eq!(
        desk.send(json!({"kind": "openHarnessHistory", "context": context,
            "workspace": null, "id": conversation}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    assert!(
        desk.json()["harness"]["history"]["opened"].is_null(),
        "{}",
        desk.json()["harness"]["history"]
    );
    // Relaunched: undone before anything else, and nothing staged is active.
    let desk = start(Some(desk));
    let done = desk.settled().await;
    assert!(done["problem"].is_null(), "{done}");
    assert_eq!(desk.receipt()["state"], "rolledBack");
    assert_eq!(v1_tenant_ids(&desk), vec![ours.clone()]);
    assert!(desk.conversations(&ours, v1::WORKSPACE).is_empty());
    assert_eq!(desk.json()["active"]["tenant"], ours);
}

/// The staged tenant (on disk, hidden): never listed, never selectable.
async fn staged_tenant_withheld(desk: &mut Desk, why: &str) {
    let staged = v1_tenant_ids(desk);
    assert_eq!(staged.len(), 1, "{why}: written and still on disk");
    assert!(desk.tenants().is_empty(), "{why}: not listed");
    assert!(desk.json()["active"].is_null(), "{why}: not active");
    assert_eq!(
        desk.send(json!({"kind": "selectTenant", "tenant": staged[0]}))
            .await
            .err(),
        Some(Refusal::InvalidInput),
        "{why}: not selectable"
    );
    assert_eq!(
        desk.send(create_later()).await.err(),
        Some(Refusal::Busy),
        "{why}: writes wait"
    );
}

/// The receipt's two files (`first`, `second`).
fn receipt_files(desk: &Desk, id: &str) -> [std::path::PathBuf; 2] {
    let root = desk.adapters.import_root.clone();
    [
        root.join(format!("{id}.json")),
        receipt::copies(&root).join(format!("{id}.json")),
    ]
}

/// v1's tenants imported new and committed.
async fn committed(desk: &mut Desk) {
    let view = desk.inspect().await;
    desk.plan(&["tenants"], new_targets(&view)).await;
    let done = desk.import_ready().await;
    assert!(done["problem"].is_null(), "{done}");
    assert_eq!(desk.receipt()["state"], "applied");
}

/// F3 (attempt-02) and R3-F1/F2 (attempt-03): what an unsettled import
/// wrote is never published, whatever happened to its receipt files. A
/// receipt is read from its second file when the first is damaged or its
/// folder cannot be listed; when both files are lost, or neither folder can
/// be listed, the saved document's copy (its commit binding) still names
/// what it wrote, so that stays hidden and unselectable until the rollback
/// settles it. A settled receipt has no document copy: it is read from its
/// second file when the first is damaged. One lost in both files, or folders that cannot
/// be listed with nothing standing in, hold what imports write and say so
/// until access returns or the operator releases the record.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn what_an_unsettled_import_wrote_is_never_published_whatever_its_receipt_files() {
    // The first file damaged: read from the second and undone at launch.
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    stopped_after_a_tenant(&mut desk).await;
    let id = receipt_id(&desk);
    std::fs::write(&receipt_files(&desk, &id)[0], b"{").expect("damage");
    let mut desk = start(Some(desk));
    let done = desk.settled().await;
    assert!(done["problem"].is_null(), "{done}");
    assert_eq!(desk.receipt()["state"], "rolledBack");
    assert!(
        v1_tenant_ids(&desk).is_empty(),
        "undone from the second file"
    );
    assert!(desk.send(create_later()).await.is_ok());

    // Both files damaged, and the launch rollback cannot record its notes:
    // the document's copy keeps the tenant hidden and unselectable, and
    // what imports write waiting, until a retried rollback settles it.
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    stopped_after_a_tenant(&mut desk).await;
    let id = receipt_id(&desk);
    for file in receipt_files(&desk, &id) {
        std::fs::write(file, b"{").expect("damage");
    }
    let root = desk.adapters.import_root.clone();
    receipt::fail_sync(&root, Some(0));
    let mut desk = start(Some(desk));
    let done = desk.settled().await;
    assert!(
        !done["problem"].is_null(),
        "its rollback is not recorded: {done}"
    );
    assert_eq!(desk.receipt()["id"], id.as_str(), "read from the document");
    assert!(
        desk.import()["damagedReceipts"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    staged_tenant_withheld(&mut desk, "both files damaged").await;
    receipt::fail_sync(&root, None);
    let after = desk.roll_back(&json!(id)).await;
    assert!(after["problem"].is_null(), "{after}");
    assert_eq!(desk.receipt()["state"], "rolledBack");
    assert!(v1_tenant_ids(&desk).is_empty());
    assert!(desk.send(create_later()).await.is_ok());

    // Neither folder can be listed, and the rollback cannot record: the
    // same, from the document's copy.
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    stopped_after_a_tenant(&mut desk).await;
    let root = desk.adapters.import_root.clone();
    for dir in [root.clone(), receipt::copies(&root)] {
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).expect("deny");
    }
    receipt::fail_sync(&root, Some(0));
    let mut desk = start(Some(desk));
    let done = desk.settled().await;
    assert!(!done["problem"].is_null(), "{done}");
    staged_tenant_withheld(&mut desk, "neither folder listed").await;
    receipt::fail_sync(&root, None);
    let id = desk.receipt()["id"].clone();
    let after = desk.roll_back(&id).await;
    assert!(after["problem"].is_null(), "{after}");
    assert!(v1_tenant_ids(&desk).is_empty());
    assert!(desk.send(create_later()).await.is_ok());

    // The first folder cannot be listed and the second file was never
    // there: recovered from the document and undone.
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    stopped_after_a_tenant(&mut desk).await;
    let root = desk.adapters.import_root.clone();
    std::fs::remove_dir_all(receipt::copies(&root)).expect("remove");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).expect("deny");
    let mut desk = start(Some(desk));
    let done = desk.settled().await;
    assert!(done["problem"].is_null(), "{done}");
    assert_eq!(desk.receipt()["state"], "rolledBack");
    assert!(v1_tenant_ids(&desk).is_empty());
    assert!(desk.send(create_later()).await.is_ok());

    // A committed import's first file damaged: settled, its receipt has no
    // document copy, so it is read from the second file; nothing is held.
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    committed(&mut desk).await;
    let id = receipt_id(&desk);
    std::fs::write(&receipt_files(&desk, &id)[0], b"{").expect("damage");
    let mut desk = start(Some(desk));
    until(&desk, |d| {
        d.import()["receipts"]
            .as_array()
            .is_some_and(|r| r.len() == 1)
    })
    .await;
    assert_eq!(
        desk.receipt()["id"],
        id.as_str(),
        "read from the second file"
    );
    assert_eq!(desk.receipt()["state"], "applied");
    assert_eq!(desk.import()["damagedReceipts"], json!([]));
    assert!(desk.send(create_later()).await.is_ok());

    // A committed import's receipt lost in both files: what it wrote is
    // committed and shown, but what imports write is held and the record
    // named until the operator releases it.
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    committed(&mut desk).await;
    let id = receipt_id(&desk);
    for file in receipt_files(&desk, &id) {
        std::fs::write(file, b"{").expect("damage");
    }
    let mut desk = start(Some(desk));
    until(&desk, |d| d.import()["damagedReceipts"] == json!([id])).await;
    assert_eq!(desk.tenants().len(), 2, "committed: shown");
    assert_eq!(desk.send(create_later()).await.err(), Some(Refusal::Busy));
    let view = desk.inspect().await;
    desk.plan(&["tenants"], new_targets(&view)).await;
    let plan = desk.import()["plan"].clone();
    assert_eq!(
        desk.send(json!({"kind": "confirmV1Import", "review": plan["review"], "items": [0]}))
            .await
            .err(),
        Some(Refusal::Busy),
        "no import while a record is lost"
    );
    let _ = desk.send(json!({"kind": "dismissV1Import"})).await;
    desk.send(json!({"kind": "forgetV1ImportReceipt", "receipt": id}))
        .await
        .expect("released");
    assert!(
        desk.import()["damagedReceipts"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert!(desk.send(create_later()).await.is_ok());
    for file in receipt_files(&desk, &id) {
        assert!(!file.exists());
    }

    // A committed import, its first folder unlistable and its second file
    // gone: the receipts are unknown, so writes wait and the view says so,
    // until access returns.
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    committed(&mut desk).await;
    let root = desk.adapters.import_root.clone();
    std::fs::remove_dir_all(receipt::copies(&root)).expect("remove");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o000)).expect("deny");
    let mut desk = start(Some(desk));
    until(&desk, |d| d.import()["receiptsUnreadable"] == true).await;
    assert_eq!(desk.send(create_later()).await.err(), Some(Refusal::Busy));
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).expect("allow");
    let mut desk = start(Some(desk));
    until(&desk, |d| d.import()["receiptsUnreadable"] == false).await;
    assert_eq!(desk.receipt()["state"], "applied", "its record is back");
    assert!(desk.send(create_later()).await.is_ok());
}

/// R3-F1: a note is recorded only once the saved document's copy is
/// confirmed durable. While the document's saves are unconfirmed, an import
/// records nothing and so writes nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_import_whose_document_copy_is_not_confirmed_writes_nothing() {
    let mut desk = start(None);
    let _v1 = V1::stage(&desk.adapters);
    let view = desk.inspect().await;
    desk.plan(&["tenants"], new_targets(&view)).await;
    desk.adapters.store.unconfirmed_saves(true);
    let done = desk.import_ready().await;
    desk.adapters.store.unconfirmed_saves(false);
    assert_eq!(done["problem"], "notRecorded", "{done}");
    assert!(v1_tenant_ids(&desk).is_empty(), "nothing written");
    assert!(desk.tenants().is_empty());
}

/// The saved document in `dir`, as written.
fn document(dir: &std::path::Path) -> (Vec<u8>, PersistedState) {
    let bytes = std::fs::read(dir.join("state.json")).expect("document");
    let state = serde_json::from_slice(&bytes).expect("parsed");
    (bytes, state)
}

/// R4-F1: the saved document's copy of an unsettled receipt is checked as
/// its files are, by the production store at launch. With both receipt files
/// damaged, a current copy still drives the rollback; one of another schema,
/// or one that claims to have settled, makes the document fail to load:
/// storage is reported damaged, nothing is shown or written, no rollback
/// runs from it, and the document and receipt files stay as they were.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_document_copy_of_a_receipt_is_checked_before_it_drives_anything() {
    type Edit = fn(&mut receipt::Receipt);
    let cases: [(&str, Option<Edit>); 3] = [
        ("current", None),
        (
            "another schema",
            Some(|r| r.schema = "prisma-airs-desktop-v2/import-receipt/2".to_owned()),
        ),
        (
            "claims it settled",
            Some(|r| r.state = receipt::ReceiptState::Applied),
        ),
    ];
    for (case, edit) in cases {
        let mut desk = start(None);
        let _v1 = V1::stage(&desk.adapters);
        stopped_after_a_tenant(&mut desk).await;
        let id = receipt_id(&desk);
        let files = receipt_files(&desk, &id);
        for file in &files {
            std::fs::write(file, b"{").expect("damage");
        }
        let mut state = desk.adapters.store.saved().expect("saved");
        assert_eq!(state.open_imports.len(), 1, "{case}: bound");
        if let Some(edit) = edit {
            edit(&mut state.open_imports[0]);
        }
        let dir = temp("document");
        FileStore::open(&dir)
            .expect("file store")
            .save(&state)
            .expect("written");
        let (written, _) = document(&dir);
        let mut desk = start_on_disk(desk, &dir);
        let done = desk.settled().await;
        if edit.is_none() {
            // A current copy: read from the document and undone.
            assert_eq!(desk.json()["storage"], "ready", "{case}");
            assert!(done["problem"].is_null(), "{case}: {done}");
            assert_eq!(desk.receipt()["id"], id.as_str(), "{case}");
            assert_eq!(desk.receipt()["state"], "rolledBack", "{case}");
            let (_, after) = document(&dir);
            assert!(after.tenants.is_empty(), "{case}: undone");
            assert!(after.open_imports.is_empty(), "{case}: settled");
            assert!(desk.send(create_later()).await.is_ok(), "{case}");
            continue;
        }
        // Another copy: the document is damaged and drives nothing.
        assert_eq!(desk.json()["storage"], "corrupt", "{case}");
        assert!(desk.tenants().is_empty(), "{case}: nothing shown");
        assert_eq!(done["receipts"], json!([]), "{case}: not adopted");
        assert_eq!(done["damagedReceipts"], json!([id]), "{case}: named");
        assert_eq!(
            desk.send(create_later()).await.err(),
            Some(Refusal::Busy),
            "{case}: held by the lost record, before the damaged store"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
        let (after, kept) = document(&dir);
        assert_eq!(after, written, "{case}: the document is untouched");
        assert_eq!(kept.tenants.len(), 1, "{case}: what it wrote is kept");
        for file in &files {
            assert_eq!(
                std::fs::read(file).expect("kept"),
                b"{",
                "{case}: files untouched"
            );
        }
    }
}
