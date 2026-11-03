//! C14 API Client projects and documents through the production authority
//! interface: v1 collections opened from a folder, fresh-snapshot Save with
//! revisions (stale, invalid, unchanged, typing during a save), conflicts
//! with archived versions, interrupted saves found after a restart, sealed
//! drafts/tabs/private environments, environments and workbench secrets in
//! a masked preview, cURL and Insomnia imports that leave sources alone,
//! the built-in Strata Cloud Manager collection, the read-only Git view,
//! reset, the close guard and no network use (T23).

use std::path::{Path, PathBuf};

use desktop_core::fixtures::{self, Handles};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

const SECRET: &str = "SYNTHETIC-WORKBENCH-SECRET-c14";

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
}

fn temp(name: &str) -> PathBuf {
    let mut n = [0u8; 8];
    getrandom::fill(&mut n).expect("fixture");
    let p = std::env::temp_dir()
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-v2-c14-{name}-{}", hex::encode(n)));
    std::fs::create_dir_all(&p).expect("fixture");
    p
}

/// A v1 collection inside a Git repository, with one request.
fn collection() -> PathBuf {
    let repo = temp("repo");
    std::fs::create_dir(repo.join(".git")).expect("fixture");
    let root = repo.join("api");
    std::fs::create_dir_all(root.join("items")).expect("fixture");
    std::fs::write(
        root.join("collection.toml"),
        "format_version = 1\nid = \"c14\"\nname = \"Items API\"\n\n[variables]\nhost = \"collection.example\"\n",
    )
    .expect("fixture");
    std::fs::write(
        root.join("items/list.request.toml"),
        "# Lists items (kept)\nformat_version = 1\nid = \"list\"\nmethod = \"GET\"\nurl = \"https://{{host}}/items\" # trailing\nbody = \"items/list.body.txt\"\nx-extension = true\n\n[[headers]]\nname = \"Accept\"\nvalue = \"application/json\"\n",
    )
    .expect("fixture");
    std::fs::write(root.join("items/list.body.txt"), "").expect("fixture");
    root
}

async fn start(previous: Option<&Handles>, config: CoreConfig) -> Desk {
    let (mut adapters, mut handles) = fixtures::adapters();
    if let Some(old) = previous {
        adapters.keychain = old.vault.clone();
        handles.vault = old.vault.clone();
        adapters.workbench_root = Some(old.workbench_root.clone());
        handles.workbench_root = old.workbench_root.clone();
    }
    let core = Core::new(config, adapters);
    core.start();
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
    }
}

async fn desk() -> Desk {
    start(None, CoreConfig::default()).await
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

    /// One API Client request and its single result.
    async fn wb(&mut self, request: Value) -> Value {
        let Receipt::Admitted { operation, .. } = self
            .send(json!({"kind": "workbench", "request": request}))
            .await
            .expect("admitted");
        let Observation::Workbench(handle) = self
            .handle
            .observe(ViewScope::Workbench(operation.expect("operation")))
            .expect("observe")
        else {
            unreachable!("workbench scope")
        };
        serde_json::to_value(handle.result().await).expect("json")
    }

    fn snap(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).expect("observe") {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell scope"),
        }
    }

    fn workbench(&self) -> Value {
        serde_json::to_value(self.snap().workbench).expect("json")
    }

    async fn open(&mut self, root: &Path) -> Value {
        self.adapters.folders.reply(Some(root.to_path_buf()));
        self.wb(json!({"kind": "openFolder"})).await
    }

    async fn request(&mut self, relative: &str) -> Value {
        let r = self
            .wb(json!({"kind": "openRequest", "relative": relative}))
            .await;
        assert_eq!(r["kind"], "document", "{r}");
        r["document"].clone()
    }
}

fn fields(doc: &Value) -> Value {
    doc["fields"].clone()
}

fn with(doc: &Value, key: &str, value: &str) -> Value {
    let mut f = fields(doc);
    f[key] = json!(value);
    f
}

#[tokio::test(flavor = "multi_thread")]
async fn saves_a_fresh_snapshot_keeping_the_files_layout_and_revisions() {
    let root = collection();
    let mut desk = desk().await;
    assert_eq!(
        desk.open(&root).await,
        json!({"kind": "opened", "name": "Items API"})
    );
    let wb = desk.workbench();
    assert_eq!(
        wb["collection"]["requests"][0]["relative"],
        "items/list.request.toml"
    );
    assert_eq!(wb["collection"]["complete"], true);
    let doc = desk.request("items/list.request.toml").await;
    assert_eq!(doc["fields"]["headers"], "Accept: application/json");
    assert_eq!(doc["fields"], doc["saved"]);
    let mut snapshot = with(&doc, "method", "POST");
    snapshot["headers"] = json!("X-Trace: 1\nAccept: application/json");
    snapshot["body"] = json!("{\"q\": 1}");
    snapshot["bodyKind"] = json!("json");
    let saved = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": snapshot}))
        .await;
    assert_eq!(saved["kind"], "saved", "{saved}");
    assert_ne!(saved["document"]["revision"], doc["revision"]);
    assert_eq!(saved["document"]["saved"], snapshot);
    let text = std::fs::read_to_string(root.join("items/list.request.toml")).expect("fixture");
    assert!(text.starts_with("# Lists items (kept)\n"));
    assert!(text.contains("method = \"POST\""));
    assert!(text.contains("# trailing"));
    assert!(text.contains("x-extension = true"));
    assert!(text.find("X-Trace").expect("fixture") < text.find("Accept").expect("fixture"));
    assert_eq!(
        std::fs::read_to_string(root.join("items/list.body.txt")).expect("fixture"),
        "{\"q\": 1}"
    );
    // The same snapshot again changes nothing.
    let again = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": saved["document"]["revision"], "fields": snapshot}))
        .await;
    assert_eq!(again["kind"], "unchanged");
    assert_eq!(desk.workbench()["collection"]["tabs"][0]["draft"], false);
}

/// Every file under `root`, by path, with its bytes.
fn project_bytes(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("fixture") {
            let path = entry.expect("fixture").path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).expect("fixture");
                files.push((path, bytes));
            }
        }
    }
    files.sort();
    files
}

#[tokio::test(flavor = "multi_thread")]
async fn saving_the_saved_snapshot_drops_an_older_protected_draft() {
    let root = collection();
    let before = project_bytes(&root);
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    // B is protected, then the editor reverts to the saved A and saves it
    // before the next Protect would run.
    desk.wb(json!({"kind": "editing", "edit": 1, "id": "list"}))
        .await;
    let b = with(&doc, "url", "https://a.example/obsolete");
    let p = desk
        .wb(json!({"kind": "protect", "edit": 1, "id": "list", "revision": doc["revision"], "fields": b}))
        .await;
    assert_eq!(p, json!({"kind": "protected", "id": "list"}));
    assert_eq!(desk.workbench()["collection"]["tabs"][0]["draft"], true);
    desk.wb(json!({"kind": "editing", "edit": 2, "id": "list"}))
        .await;
    let saved = desk
        .wb(json!({"kind": "save", "edit": 2, "id": "list", "revision": doc["revision"], "fields": fields(&doc)}))
        .await;
    assert_eq!(saved["kind"], "unchanged", "{saved}");
    assert_eq!(saved["document"]["fields"], doc["saved"]);
    assert_eq!(saved["document"]["recovered"], false);
    assert_eq!(desk.workbench()["collection"]["tabs"][0]["draft"], false);
    assert_eq!(desk.workbench()["unprotected"], false);
    assert_eq!(project_bytes(&root), before);
    // The acknowledgement was durable: a restart recovers nothing.
    let handles = desk.adapters;
    drop(desk.core);
    let mut again = start(Some(&handles), CoreConfig::default()).await;
    again.open(&root).await;
    let tab = &again.workbench()["collection"]["tabs"];
    assert!(
        tab.as_array().is_none_or(|t| t
            .iter()
            .all(|t| t["draft"] == false && t["recovered"] == false)),
        "{tab}"
    );
    let reopened = again.request("items/list.request.toml").await;
    assert_eq!(reopened["fields"], doc["saved"]);
    assert_eq!(reopened["recovered"], false);
    assert_eq!(project_bytes(&root), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn stale_and_invalid_snapshots_never_write_and_typing_during_save_stays_protected() {
    let root = collection();
    let before = std::fs::read(root.join("items/list.request.toml")).expect("fixture");
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    // Invalid input is refused with the field and row, never rewritten.
    let invalid = with(&doc, "headers", "Accept: application/json\nnot a header");
    let r = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": invalid}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "saveRefused", "refusal": {"kind": "invalid", "problem": {"field": "headers", "line": 2}}})
    );
    let r = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": with(&doc, "settings", "[auth]\ntoken = \"inline\"\n")}))
        .await;
    assert_eq!(r["refusal"]["problem"]["field"], "settings");
    assert_eq!(
        std::fs::read(root.join("items/list.request.toml")).expect("fixture"),
        before
    );
    // A revision the document no longer holds is refused (reload first).
    let first = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": with(&doc, "url", "https://a.example/one")}))
        .await;
    assert_eq!(first["kind"], "saved");
    let stale = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": with(&doc, "url", "https://a.example/two")}))
        .await;
    assert_eq!(
        stale,
        json!({"kind": "refused", "problem": {"kind": "stale"}})
    );
    // Typing after the Save captured its snapshot: the newer text is
    // protected against the new revision and survives a restart.
    desk.wb(json!({"kind": "editing", "edit": 1, "id": "list"}))
        .await;
    assert_eq!(desk.workbench()["unprotected"], true);
    let typed = with(&first["document"], "url", "https://a.example/one-and-more");
    let p = desk
        .wb(json!({"kind": "protect", "edit": 1, "id": "list", "revision": first["document"]["revision"], "fields": typed}))
        .await;
    assert_eq!(p, json!({"kind": "protected", "id": "list"}));
    assert_eq!(desk.workbench()["unprotected"], false);
    // Invalid text is protected as typed too.
    let broken = with(&first["document"], "headers", "broken row");
    desk.wb(json!({"kind": "protect", "edit": 1, "id": "list", "revision": first["document"]["revision"], "fields": broken}))
        .await;
    let handles = desk.adapters;
    drop(desk.core);
    let mut again = start(Some(&handles), CoreConfig::default()).await;
    again.open(&root).await;
    let tab = &again.workbench()["collection"]["tabs"][0];
    assert_eq!(
        (tab["draft"].clone(), tab["recovered"].clone()),
        (json!(true), json!(true))
    );
    let recovered = again.request("items/list.request.toml").await;
    assert_eq!(recovered["fields"], broken);
    assert_eq!(recovered["recovered"], true);
    assert_eq!(recovered["revision"], first["document"]["revision"]);
    // The draft file is sealed: no editor text in plain bytes.
    for entry in std::fs::read_dir(&handles.workbench_root).expect("fixture") {
        let p = entry.expect("fixture").path();
        if p.is_file() {
            let bytes = String::from_utf8_lossy(&std::fs::read(&p).expect("fixture")).into_owned();
            assert!(!bytes.contains("broken row") && !bytes.contains("one-and-more"));
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn external_changes_become_conflicts_and_both_versions_are_archived() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    let mine = with(&doc, "url", "https://mine.example/");
    desk.wb(json!({"kind": "protect", "edit": 1, "id": "list", "revision": doc["revision"], "fields": mine}))
        .await;
    std::fs::write(
        root.join("items/list.request.toml"),
        "format_version = 1\nid = \"list\"\nmethod = \"PUT\"\nurl = \"https://theirs.example/\"\nbody = \"items/list.body.txt\"\n",
    )
    .expect("fixture");
    let r = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": mine}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "saveRefused", "refusal": {"kind": "conflict"}})
    );
    assert_eq!(
        desk.workbench()["collection"]["tabs"][0]["conflict"],
        "changed"
    );
    assert!(
        std::fs::read_to_string(root.join("items/list.request.toml"))
            .expect("fixture")
            .contains("theirs")
    );
    // Keep mine: the disk version becomes the baseline, local edits stay.
    let kept = desk
        .wb(json!({"kind": "resolve", "id": "list", "revision": doc["revision"], "fields": mine, "choice": "keepMine"}))
        .await;
    assert_eq!(kept["kind"], "document");
    assert_eq!(kept["document"]["saved"]["method"], "PUT");
    assert_eq!(kept["document"]["fields"]["url"], "https://mine.example/");
    assert_eq!(desk.workbench()["collection"]["archives"], 1);
    let saved = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": kept["document"]["revision"], "fields": mine}))
        .await;
    assert_eq!(saved["kind"], "saved");
    assert!(
        std::fs::read_to_string(root.join("items/list.request.toml"))
            .expect("fixture")
            .contains("mine.example")
    );
    // Deleted on disk; Use disk closes the request.
    std::fs::remove_file(root.join("items/list.request.toml")).expect("fixture");
    desk.wb(json!({"kind": "refresh"})).await;
    assert_eq!(
        desk.workbench()["collection"]["tabs"][0]["conflict"],
        "deleted"
    );
    let gone = desk
        .wb(json!({"kind": "resolve", "id": "list", "revision": saved["document"]["revision"], "fields": mine, "choice": "useDisk"}))
        .await;
    assert_eq!(gone, json!({"kind": "closed", "id": "list"}));
    assert_eq!(desk.workbench()["collection"]["archives"], 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_interrupted_save_is_found_after_a_restart_and_nothing_replays() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    let mut edit = with(&doc, "url", "https://half.example/");
    edit["body"] = json!("new body");
    desk.core.interrupt_next_save(1);
    let r = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": edit}))
        .await;
    assert_eq!(r, json!({"kind": "saveRefused", "refusal": {"kind": "io"}}));
    // The body was written, the metadata not.
    assert_eq!(
        std::fs::read_to_string(root.join("items/list.body.txt")).expect("fixture"),
        "new body"
    );
    let metadata = std::fs::read(root.join("items/list.request.toml")).expect("fixture");
    let handles = desk.adapters;
    drop(desk.core);
    let mut again = start(Some(&handles), CoreConfig::default()).await;
    again.open(&root).await;
    // Nothing was written again at launch.
    assert_eq!(
        std::fs::read(root.join("items/list.request.toml")).expect("fixture"),
        metadata
    );
    assert_eq!(
        again.workbench()["collection"]["tabs"][0]["interrupted"],
        true
    );
    let held = again.request("items/list.request.toml").await;
    assert_eq!(held["fields"], edit);
    let r = again
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": held["revision"], "fields": edit}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "saveRefused", "refusal": {"kind": "interrupted"}})
    );
    let kept = again
        .wb(json!({"kind": "resolve", "id": "list", "revision": held["revision"], "fields": edit, "choice": "keepMine"}))
        .await;
    let saved = again
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": kept["document"]["revision"], "fields": edit}))
        .await;
    assert_eq!(saved["kind"], "saved", "{saved}");
    assert!(
        std::fs::read_to_string(root.join("items/list.request.toml"))
            .expect("fixture")
            .contains("half.example")
    );
    assert_eq!(
        again.workbench()["collection"]["tabs"][0]["interrupted"],
        false
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn environments_and_workbench_secrets_resolve_masked_in_the_preview() {
    let root = collection();
    std::fs::create_dir(root.join("environments")).expect("fixture");
    std::fs::write(
        root.join("environments/dev.toml"),
        "format_version = 1\n\n[variables]\nhost = \"dev.example\"\ntoken = { secret = \"dev-token\" }\n",
    )
    .expect("fixture");
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    let local = "format_version = 1\n\n[variables]\nhost = \"local.example\"\n";
    let r = desk
        .wb(json!({"kind": "saveEnvironment", "name": "dev", "private": true, "revision": null, "text": local}))
        .await;
    assert_eq!(r["kind"], "environment");
    assert_eq!(
        desk.wb(json!({"kind": "selectEnvironment", "name": "dev"}))
            .await["kind"],
        "done"
    );
    assert_eq!(
        desk.workbench()["collection"]["environments"],
        json!([{"name": "dev", "origin": "both"}])
    );
    let mut f = with(&doc, "headers", "Authorization: Bearer {{token}}");
    f["url"] = json!("https://{{host}}/{{missing}}");
    let p = desk
        .wb(json!({"kind": "preview", "id": "list", "fields": f}))
        .await;
    assert_eq!(p["preview"]["url"], "https://local.example/{{missing}}");
    assert_eq!(p["preview"]["headers"][0][1], "Bearer ••••••");
    assert_eq!(p["preview"]["unresolved"], json!(["missing"]));
    assert_eq!(p["preview"]["missingSecrets"], json!(["dev-token"]));
    let set = desk
        .wb(json!({"kind": "setSecret", "reference": "dev-token", "value": SECRET}))
        .await;
    assert_eq!(set["kind"], "done");
    let p = desk
        .wb(json!({"kind": "preview", "id": "list", "fields": f}))
        .await;
    assert_eq!(p["preview"]["missingSecrets"], json!([]));
    let secrets = desk.workbench()["collection"]["secrets"].clone();
    assert_eq!(
        secrets,
        json!([{"reference": "dev-token", "stored": true, "managed": false}])
    );
    // Tenant references belong to native tenant authority.
    let managed = desk
        .wb(json!({"kind": "setSecret", "reference": "tenant.select", "value": SECRET}))
        .await;
    assert_eq!(
        managed,
        json!({"kind": "refused", "problem": {"kind": "managed"}})
    );
    // The value never appears in a snapshot, result or private file.
    let shown = serde_json::to_string(&desk.snap()).expect("fixture");
    assert!(!shown.contains(SECRET) && !p.to_string().contains(SECRET));
    for entry in std::fs::read_dir(&desk.adapters.workbench_root).expect("fixture") {
        let path = entry.expect("fixture").path();
        if path.is_file() {
            assert!(
                !String::from_utf8_lossy(&std::fs::read(&path).expect("fixture")).contains(SECRET)
            );
        }
    }
    let vault = desk.adapters.vault.clone();
    let holds = move || {
        vault
            .records()
            .iter()
            .any(|r| vault.secret(r).as_deref() == Some(SECRET.as_bytes()))
    };
    assert!(holds());
    assert_eq!(
        desk.wb(json!({"kind": "removeSecret", "reference": "dev-token"}))
            .await["kind"],
        "done"
    );
    assert!(!holds());
}

#[tokio::test(flavor = "multi_thread")]
async fn imports_create_drafts_and_never_replace_or_change_sources() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let r = desk
        .wb(json!({"kind": "importCurl", "relative": "imported/create",
            "text": "curl -X POST https://a.example/items -H 'Authorization: Bearer abc' --json '{\"n\":1}'"}))
        .await;
    assert_eq!(r["kind"], "document", "{r}");
    assert_eq!(r["document"]["new"], true);
    assert_eq!(r["document"]["relative"], "imported/create.request.toml");
    assert!(!r.to_string().contains("abc"));
    assert!(
        !root.join("imported").exists(),
        "an import is a draft until Save"
    );
    let refused = desk
        .wb(json!({"kind": "importCurl", "relative": "x", "text": "curl https://a.example/ ; rm -rf /"}))
        .await;
    assert_eq!(
        refused,
        json!({"kind": "refused", "problem": {"kind": "curl", "detail": "shell"}})
    );
    for bad in ["../escape", "/abs", ".hidden/x", "items/list"] {
        let r = desk
            .wb(json!({"kind": "newRequest", "relative": bad}))
            .await;
        assert_eq!(r["kind"], "refused", "{bad}: {r}");
    }
    // Insomnia: preview, then create the selection; an existing file stays.
    std::fs::create_dir_all(root.join("environments")).expect("fixture");
    std::fs::write(
        root.join("environments/insomnia-env-1-base.toml"),
        "format_version = 1\n# mine\n",
    )
    .expect("fixture");
    let export = "type: collection.insomnia.rest/5.0\nschema_version: \"5.1\"\nname: Sample\ncollection:\n  - name: List\n    method: GET\n    url: \"{{ _.base }}/items\"\n  - name: Script\n    method: GET\n    url: https://a.example/\n    scripts:\n      preRequest: x\nenvironments:\n  name: Base\n  data:\n    base: https://a.example\n";
    desk.adapters
        .workbench_files
        .reply(Some(Ok(("export.yaml".into(), export.as_bytes().to_vec()))));
    let preview = desk.wb(json!({"kind": "chooseInsomnia"})).await;
    assert_eq!(preview["kind"], "import");
    assert_eq!(preview["resources"][1]["problem"], "fields");
    let created = desk
        .wb(json!({"kind": "createImported", "selected": [0, 1, 2]}))
        .await;
    assert_eq!(
        created,
        json!({"kind": "created", "requests": 1, "environments": 0, "skipped": 2})
    );
    assert_eq!(
        std::fs::read_to_string(root.join("environments/insomnia-env-1-base.toml"))
            .expect("fixture"),
        "format_version = 1\n# mine\n"
    );
    desk.adapters
        .workbench_files
        .reply(Some(Ok(("big.yaml".into(), vec![b' '; 1024 * 1024 + 1]))));
    assert_eq!(
        desk.wb(json!({"kind": "chooseInsomnia"})).await,
        json!({"kind": "refused", "problem": {"kind": "export", "detail": "tooLarge"}})
    );
    let tabs = desk.workbench()["collection"]["tabs"]
        .as_array()
        .expect("fixture")
        .len();
    assert_eq!(tabs, 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_built_in_collection_is_private_generic_and_keeps_edits() {
    let mut desk = desk().await;
    let r = desk.wb(json!({"kind": "openBuiltIn"})).await;
    assert_eq!(r, json!({"kind": "opened", "name": "Strata Cloud Manager"}));
    let wb = desk.workbench();
    assert_eq!(wb["collection"]["builtIn"], true);
    assert_eq!(
        wb["collection"]["requests"]
            .as_array()
            .expect("fixture")
            .len(),
        193
    );
    let folder = desk.adapters.workbench_root.join("scm");
    assert!(folder.join("collection.toml").exists());
    let first = wb["collection"]["requests"][0]["relative"]
        .as_str()
        .expect("fixture")
        .to_owned();
    std::fs::write(
        folder.join(&first),
        "format_version = 1\nid = \"edited\"\nmethod = \"GET\"\nurl = \"https://x/\"\n",
    )
    .expect("fixture");
    desk.wb(json!({"kind": "openBuiltIn"})).await;
    assert!(
        std::fs::read_to_string(folder.join(&first))
            .expect("fixture")
            .contains("edited")
    );
    // Tenant references are shown as managed and never resolved here.
    let entry = desk.workbench()["collection"]["requests"]
        .as_array()
        .expect("fixture")
        .iter()
        .find(|r| r["method"] == "GET" && r["relative"] != first.as_str())
        .expect("fixture")["relative"]
        .clone();
    let doc = desk.request(entry.as_str().expect("fixture")).await;
    let p = desk
        .wb(json!({"kind": "preview", "id": doc["id"], "fields": doc["fields"]}))
        .await;
    assert_eq!(p["preview"]["missingSecrets"], json!([]));
    assert_eq!(p["preview"]["auth"], "oauth2");
    let secrets = desk.workbench()["collection"]["secrets"].clone();
    assert!(
        secrets
            .as_array()
            .expect("fixture")
            .iter()
            .all(|s| s["managed"] == true && s["stored"] == false)
    );
    // A catalogue request with a JSON body sidecar saves its own payload.
    let (relative, sidecar) = project_bytes(&folder)
        .into_iter()
        .find_map(|(p, bytes)| {
            let text = String::from_utf8(bytes).ok()?;
            let body = text
                .lines()
                .find_map(|l| {
                    l.strip_prefix("body = \"")?
                        .strip_suffix('"')
                        .map(str::to_owned)
                })
                .filter(|b| b.ends_with(".json"))?;
            let relative = p.strip_prefix(&folder).ok()?.to_str()?.to_owned();
            relative
                .ends_with(".request.toml")
                .then_some((relative, body))
        })
        .expect("a catalogue request with a JSON body");
    let metadata = std::fs::read(folder.join(&relative)).expect("fixture");
    let doc = desk.request(&relative).await;
    let saved = desk
        .wb(json!({"kind": "save", "edit": 1, "id": doc["id"], "revision": doc["revision"], "fields": with(&doc, "body", "{\"edited\": true}")}))
        .await;
    assert_eq!(saved["kind"], "saved", "{saved}");
    assert_eq!(
        std::fs::read_to_string(folder.join(&sidecar)).expect("fixture"),
        "{\"edited\": true}"
    );
    assert_eq!(
        std::fs::read(folder.join(&relative)).expect("fixture"),
        metadata
    );
    let reopened = desk.request(&relative).await;
    assert_eq!(reopened["fields"]["body"], "{\"edited\": true}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_git_view_is_read_only_and_bounded() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let git = desk.wb(json!({"kind": "readGit"})).await;
    // The fixture repository is a bare `.git` folder: not a working tree
    // Git recognizes (or Git is absent on this host).
    assert!(
        ["notRepository", "unavailable", "failed"]
            .contains(&git["git"]["state"].as_str().expect("fixture")),
        "{git}"
    );
    let none = start(
        None,
        CoreConfig {
            git: None,
            ..CoreConfig::default()
        },
    )
    .await;
    let mut none = none;
    none.open(&root).await;
    assert_eq!(
        none.wb(json!({"kind": "readGit"})).await["git"]["state"],
        "unavailable"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn empty_folders_become_collections_and_git_is_optional() {
    let mut desk = desk().await;
    let not_collection =
        json!({"kind": "refused", "problem": {"kind": "project", "detail": "notCollection"}});
    // Open collection… on an empty folder (Finder's .DS_Store aside)
    // outside any repository initialises it in place (D057).
    let empty = temp("empty");
    std::fs::write(empty.join(".DS_Store"), b"finder").expect("fixture");
    let name = empty
        .file_name()
        .and_then(|n| n.to_str())
        .expect("fixture")
        .to_owned();
    assert_eq!(
        desk.open(&empty).await,
        json!({"kind": "opened", "name": name})
    );
    assert!(empty.join("collection.toml").exists());
    assert_eq!(desk.workbench()["collection"]["name"], name.as_str());
    // A folder with other files and no collection.toml stays refused and
    // untouched.
    let full = temp("full");
    std::fs::write(full.join("notes.txt"), "kept").expect("fixture");
    assert_eq!(desk.open(&full).await, not_collection);
    assert!(!full.join("collection.toml").exists());
    // New collection… outside a repository creates the folder.
    let parent = temp("plain");
    desk.adapters.folders.reply_new(Some(parent.join("Fresh")));
    assert_eq!(
        desk.wb(json!({"kind": "createCollection"})).await,
        json!({"kind": "opened", "name": "Fresh"})
    );
    // …on an existing empty folder initialises it in place…
    std::fs::create_dir(parent.join("Waiting")).expect("fixture");
    desk.adapters
        .folders
        .reply_new(Some(parent.join("Waiting")));
    assert_eq!(
        desk.wb(json!({"kind": "createCollection"})).await,
        json!({"kind": "opened", "name": "Waiting"})
    );
    // …and anything else already there is refused.
    desk.adapters.folders.reply_new(Some(full.clone()));
    assert_eq!(
        desk.wb(json!({"kind": "createCollection"})).await,
        json!({"kind": "refused", "problem": {"kind": "project", "detail": "exists"}})
    );
    assert_eq!(
        std::fs::read_dir(&full).expect("fixture").count(),
        1,
        "nothing was written"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_removes_the_private_folder_and_keeps_the_operators_collections() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    desk.wb(json!({"kind": "protect", "edit": 1, "id": "list", "revision": doc["revision"], "fields": with(&doc, "url", "https://draft/")}))
        .await;
    desk.wb(json!({"kind": "openBuiltIn"})).await;
    assert!(desk.adapters.workbench_root.exists());
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    let root_gone = desk.adapters.workbench_root.clone();
    assert!(support_eventually(|| !root_gone.exists()).await);
    assert!(root.join("items/list.request.toml").exists());
    assert_eq!(desk.workbench()["collection"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn requests_wait_their_turn_and_the_close_guard_sees_unprotected_edits() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    let held = desk.core.hold_workbench().await;
    let Receipt::Admitted { operation, .. } = desk
        .send(json!({"kind": "workbench", "request": {"kind": "editing", "edit": 1, "id": "list"}}))
        .await
        .expect("admitted");
    assert_eq!(desk.workbench()["busy"], true);
    assert!(!desk.core.workbench_unprotected());
    drop(held);
    let Observation::Workbench(handle) = desk
        .handle
        .observe(ViewScope::Workbench(operation.expect("fixture")))
        .expect("fixture")
    else {
        unreachable!()
    };
    assert_eq!(
        serde_json::to_value(handle.result().await).expect("fixture")["kind"],
        "done"
    );
    assert!(desk.core.workbench_unprotected());
    desk.wb(json!({"kind": "protect", "edit": 1, "id": "list", "revision": doc["revision"], "fields": doc["fields"]}))
        .await;
    assert!(!desk.core.workbench_unprotected());
    assert_eq!(desk.workbench()["busy"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_save_stopped_between_writes_restores_every_submitted_field() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    // Saved before any protection pause: only Save itself carries it.
    let mut edit = with(&doc, "url", "https://stopped.example/");
    edit["body"] = json!("stopped body");
    edit["headers"] = json!("Accept: text/plain\nX-Stop: 1");
    desk.core.crash_next_save(1);
    let r = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": edit}))
        .await;
    assert_eq!(r, json!({"kind": "saveRefused", "refusal": {"kind": "io"}}));
    assert_eq!(
        std::fs::read_to_string(root.join("items/list.body.txt")).expect("fixture"),
        "stopped body"
    );
    let metadata = std::fs::read(root.join("items/list.request.toml")).expect("fixture");
    let handles = desk.adapters;
    drop(desk.core);
    let mut again = start(Some(&handles), CoreConfig::default()).await;
    again.open(&root).await;
    assert_eq!(
        std::fs::read(root.join("items/list.request.toml")).expect("fixture"),
        metadata
    );
    assert_eq!(
        again.workbench()["collection"]["tabs"][0]["interrupted"],
        true
    );
    let held = again.request("items/list.request.toml").await;
    assert_eq!(held["fields"], edit, "every submitted field is restored");
    assert_eq!(held["interrupted"], true);
    let r = again
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": held["revision"], "fields": edit}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "saveRefused", "refusal": {"kind": "interrupted"}})
    );
    assert_eq!(
        std::fs::read(root.join("items/list.request.toml")).expect("fixture"),
        metadata,
        "nothing is written without an explicit choice"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_older_acknowledgement_never_clears_newer_unprotected_edits() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    let first = with(&doc, "url", "https://one.example/");
    // Edit, Save, edit again: the second edit is guarded again.
    desk.wb(json!({"kind": "editing", "edit": 1, "id": "list"}))
        .await;
    let saved = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "list", "revision": doc["revision"], "fields": first}))
        .await;
    assert_eq!(saved["kind"], "saved", "{saved}");
    assert!(!desk.core.workbench_unprotected());
    let revision = saved["document"]["revision"].clone();
    desk.wb(json!({"kind": "editing", "edit": 2, "id": "list"}))
        .await;
    assert!(desk.core.workbench_unprotected());
    // Typing (edit 3) reported while a Protect of edit 2 is outstanding:
    // that older acknowledgement leaves the newer edit guarded.
    desk.wb(json!({"kind": "editing", "edit": 3, "id": "list"}))
        .await;
    let second = with(&saved["document"], "url", "https://two.example/");
    let p = desk
        .wb(json!({"kind": "protect", "edit": 2, "id": "list", "revision": revision, "fields": second}))
        .await;
    assert_eq!(p["kind"], "protected");
    assert!(desk.core.workbench_unprotected());
    let third = with(&saved["document"], "url", "https://three.example/");
    desk.wb(
        json!({"kind": "protect", "edit": 3, "id": "list", "revision": revision, "fields": third}),
    )
    .await;
    assert!(!desk.core.workbench_unprotected());
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_with_unprotected_edits_waits_for_protection_or_an_explicit_discard() {
    let root = collection();
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/list.request.toml").await;
    desk.wb(json!({"kind": "editing", "edit": 1, "id": "list"}))
        .await;
    for close in [
        json!({"kind": "closeTab", "id": "list", "discard": false}),
        json!({"kind": "closeCollection", "discard": false}),
    ] {
        assert_eq!(
            desk.wb(close).await,
            json!({"kind": "refused", "problem": {"kind": "unprotected"}})
        );
    }
    // The editor protects its latest (even invalid) text first; then the
    // tab closes and the text survives a restart.
    let mut latest = with(&doc, "url", "https://latest.example/{{");
    latest["method"] = json!("NOT A METHOD");
    desk.wb(json!({"kind": "protect", "edit": 1, "id": "list", "revision": doc["revision"], "fields": latest}))
        .await;
    assert_eq!(
        desk.wb(json!({"kind": "closeTab", "id": "list", "discard": false}))
            .await["kind"],
        "closed"
    );
    let handles = desk.adapters;
    drop(desk.core);
    let mut again = start(Some(&handles), CoreConfig::default()).await;
    again.open(&root).await;
    let held = again.request("items/list.request.toml").await;
    assert_eq!(held["fields"], latest);
    // An explicit discard closes the collection with unprotected edits.
    again
        .wb(json!({"kind": "editing", "edit": 7, "id": "list"}))
        .await;
    assert_eq!(
        again
            .wb(json!({"kind": "closeCollection", "discard": true}))
            .await["kind"],
        "done"
    );
    assert!(!again.core.workbench_unprotected());
}

#[tokio::test(flavor = "multi_thread")]
async fn documents_open_from_their_own_metadata_not_the_inventory() {
    let root = collection();
    let long = format!("https://long.example/{}", "a".repeat(3000));
    std::fs::write(
        root.join("items/long.request.toml"),
        format!("format_version = 1\nid = \"long\"\nmethod = \"GET\"\nurl = \"{long}\"\n"),
    )
    .expect("fixture");
    let mut desk = desk().await;
    desk.open(&root).await;
    let doc = desk.request("items/long.request.toml").await;
    assert_eq!(doc["fields"]["url"], long);
    let saved = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "long", "revision": doc["revision"], "fields": with(&doc, "headers", "X-Only: header")}))
        .await;
    assert_eq!(saved["kind"], "saved", "{saved}");
    let text = std::fs::read_to_string(root.join("items/long.request.toml")).expect("fixture");
    assert!(text.contains(&long), "the long URL is kept");
    // Changed on disk after discovery, before opening: opened faithfully.
    std::fs::write(
        root.join("items/list.request.toml"),
        "format_version = 1\nid = \"list\"\nmethod = \"POST\"\nurl = \"https://changed.example/\"\nbody = \"items/list.body.txt\"\n",
    )
    .expect("fixture");
    let changed = desk.request("items/list.request.toml").await;
    assert_eq!(changed["fields"]["method"], "POST");
    assert_eq!(changed["fields"]["url"], "https://changed.example/");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_duplicate_identity_pair_still_owns_its_body() {
    let root = collection();
    for name in ["a", "b"] {
        std::fs::write(
            root.join(format!("items/{name}.request.toml")),
            "format_version = 1\nid = \"dup\"\nmethod = \"GET\"\nurl = \"https://dup.example/\"\nbody = \"items/shared.body.txt\"\n",
        )
        .expect("fixture");
    }
    std::fs::write(
        root.join("items/unique.request.toml"),
        "format_version = 1\nid = \"unique\"\nmethod = \"POST\"\nurl = \"https://unique.example/\"\nbody = \"items/shared.body.txt\"\n",
    )
    .expect("fixture");
    std::fs::write(root.join("items/shared.body.txt"), "shared").expect("fixture");
    let before: Vec<Vec<u8>> = [
        "a.request.toml",
        "b.request.toml",
        "unique.request.toml",
        "shared.body.txt",
    ]
    .iter()
    .map(|f| std::fs::read(root.join("items").join(f)).expect("fixture"))
    .collect();
    let mut desk = desk().await;
    let opened = desk.open(&root).await;
    assert_eq!(opened["kind"], "opened", "{opened}");
    let view = desk.workbench();
    let duplicates = view["collection"]["issues"]
        .as_array()
        .expect("fixture")
        .iter()
        .filter(|i| i["kind"] == "duplicate")
        .count();
    assert_eq!(duplicates, 2, "the pair's diagnostics are exposed");
    let doc = desk.request("items/unique.request.toml").await;
    let r = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "unique", "revision": doc["revision"], "fields": with(&doc, "body", "overwritten")}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "saveRefused", "refusal": {"kind": "sharedBody"}})
    );
    let after: Vec<Vec<u8>> = [
        "a.request.toml",
        "b.request.toml",
        "unique.request.toml",
        "shared.body.txt",
    ]
    .iter()
    .map(|f| std::fs::read(root.join("items").join(f)).expect("fixture"))
    .collect();
    assert_eq!(before, after, "no project file changed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_body_is_owned_by_the_file_it_reaches_not_its_spelling() {
    let root = collection();
    // macOS volumes fold case by default; the rule holds either way.
    let folds = {
        std::fs::write(root.join("items/probe-case"), "").expect("fixture");
        root.join("items/PROBE-CASE").exists()
    };
    std::fs::remove_file(root.join("items/probe-case")).expect("fixture");
    let request = |name: &str, body: &str| {
        std::fs::write(
            root.join(format!("items/{name}.request.toml")),
            format!("format_version = 1\nid = \"{name}\"\nmethod = \"POST\"\nurl = \"https://{name}.example/\"\nbody = \"{body}\"\n"),
        )
        .expect("fixture");
    };
    request("lower", "items/shared.body.txt");
    request("upper", "items/SHARED.body.txt");
    request("meta", "items/lower.request.toml");
    request("config", "collection.toml");
    request("own", "items/own.body.txt");
    std::fs::write(root.join("items/shared.body.txt"), "shared").expect("fixture");
    if !folds {
        std::fs::write(root.join("items/SHARED.body.txt"), "shared").expect("fixture");
    }
    std::fs::write(root.join("items/own.body.txt"), "own").expect("fixture");
    // APFS also ignores Unicode normalization: a decomposed spelling reaches
    // the composed file, and only the file's identity shows it.
    std::fs::write(root.join("items/caf\u{e9}.body.txt"), "nfc").expect("fixture");
    let normalizes = root.join("items/cafe\u{301}.body.txt").exists();
    if normalizes {
        request("composed", "items/caf\u{e9}.body.txt");
        request("decomposed", "items/cafe\u{301}.body.txt");
    }
    let snapshot = || -> Vec<(String, Vec<u8>)> {
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(root.join("items"))
            .expect("fixture")
            .map(|e| e.expect("fixture").path())
            .map(|p| (p.display().to_string(), std::fs::read(&p).expect("fixture")))
            .collect();
        files.push((
            "collection.toml".into(),
            std::fs::read(root.join("collection.toml")).expect("fixture"),
        ));
        files.sort();
        files
    };
    let before = snapshot();
    let mut desk = desk().await;
    let opened = desk.open(&root).await;
    assert_eq!(opened["kind"], "opened", "{opened}");
    // Another spelling of another request's body, another request's file and
    // the collection file are all owned elsewhere.
    let aliases: &[&str] = if normalizes {
        &["upper", "meta", "config", "decomposed"]
    } else {
        &["upper", "meta", "config"]
    };
    for id in aliases {
        let doc = desk.request(&format!("items/{id}.request.toml")).await;
        let r = desk
            .wb(
                json!({"kind": "save", "edit": 1, "id": id, "revision": doc["revision"],
                       "fields": with(&doc, "body", "overwritten")}),
            )
            .await;
        assert_eq!(
            r,
            json!({"kind": "saveRefused", "refusal": {"kind": "sharedBody"}}),
            "{id}"
        );
        assert_eq!(before, snapshot(), "no project byte changed ({id})");
    }
    // An independent body still saves.
    let doc = desk.request("items/own.request.toml").await;
    let saved = desk
        .wb(
            json!({"kind": "save", "edit": 1, "id": "own", "revision": doc["revision"],
                   "fields": with(&doc, "body", "changed")}),
        )
        .await;
    assert_eq!(saved["kind"], "saved", "{saved}");
    assert_eq!(
        std::fs::read_to_string(root.join("items/own.body.txt")).expect("fixture"),
        "changed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_json_body_saves_as_its_requests_while_an_associated_schema_stays_owned() {
    let root = collection();
    let write = |name: &str, text: &str| std::fs::write(root.join(name), text).expect("fixture");
    write(
        "items/post.request.toml",
        "format_version = 1\nid = \"post\"\nmethod = \"POST\"\nurl = \"https://a.example/\"\nbody = \"items/post.body.json\"\n",
    );
    write("items/post.body.json", "{\"q\": 1}");
    write(
        "items/linked.request.toml",
        "format_version = 1\nid = \"linked\"\nmethod = \"POST\"\nurl = \"https://b.example/\"\nbody = \"items/linked.yaml\"\n",
    );
    write(
        "items/user.request.toml",
        "format_version = 1\nid = \"user\"\nmethod = \"GET\"\nurl = \"https://c.example/\"\nschema = \"items/linked.yaml\"\n",
    );
    write("items/linked.yaml", "openapi: 3.1.0\n");
    let others = |root: &Path| -> Vec<(PathBuf, Vec<u8>)> {
        project_bytes(root)
            .into_iter()
            .filter(|(p, _)| !p.ends_with("items/post.body.json"))
            .collect()
    };
    let before = others(&root);
    let mut desk = desk().await;
    desk.open(&root).await;
    // The schema another request associates is never a body's to rewrite.
    let doc = desk.request("items/linked.request.toml").await;
    let r = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "linked", "revision": doc["revision"], "fields": with(&doc, "body", "overwritten")}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "saveRefused", "refusal": {"kind": "sharedBody"}})
    );
    // A JSON body named only by its own request saves and reopens.
    let doc = desk.request("items/post.request.toml").await;
    assert_eq!(doc["fields"]["body"], "{\"q\": 1}");
    let saved = desk
        .wb(json!({"kind": "save", "edit": 1, "id": "post", "revision": doc["revision"], "fields": with(&doc, "body", "{\"q\": 2}")}))
        .await;
    assert_eq!(saved["kind"], "saved", "{saved}");
    assert_eq!(
        std::fs::read_to_string(root.join("items/post.body.json")).expect("fixture"),
        "{\"q\": 2}"
    );
    assert_eq!(others(&root), before, "metadata and every other file kept");
    let handles = desk.adapters;
    drop(desk.core);
    let mut again = start(Some(&handles), CoreConfig::default()).await;
    again.open(&root).await;
    let reopened = again.request("items/post.request.toml").await;
    assert_eq!(reopened["fields"]["body"], "{\"q\": 2}");
    assert_eq!(reopened["recovered"], false);
}

async fn support_eventually(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..300 {
        if condition() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    condition()
}
