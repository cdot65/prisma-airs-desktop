//! C41: the API Client as the R2App board draws it, through the production
//! authority interface against a loopback service (screens/api-client-
//! test-cases.md, docs/planning/chunks/C41.md). What the board draws from:
//! the collection view's requests, folders, tabs and selection as requests
//! open and drafts are created; the Params rows the table edits are the
//! file's `name=value` lines, read back by Preview and sent as the query;
//! History lists the runs newest first and a shown run is the record as it
//! was (I5). Nothing here sends by itself: every request is a workbench
//! intent the operator's view would send.

#[path = "../workbench_execution/server.rs"]
#[allow(dead_code)]
mod server;
#[path = "../context_credentials/support.rs"]
#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use desktop_core::fixtures::{self, Handles};
use desktop_core::runtime::OperationId;
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};
use server::{Answer, Http};

struct Desk {
    #[allow(dead_code)]
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
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

    async fn wb(&mut self, request: Value) -> Value {
        let Receipt::Admitted { operation, .. } = self
            .send(json!({"kind": "workbench", "request": request}))
            .await
            .expect("admitted");
        self.wait(operation.expect("operation")).await
    }

    async fn wait(&self, operation: OperationId) -> Value {
        let Observation::Workbench(handle) = self
            .handle
            .observe(ViewScope::Workbench(operation))
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

    fn collection(&self) -> Value {
        serde_json::to_value(self.snap()).expect("json")["workbench"]["collection"].clone()
    }

    async fn open(&mut self, root: &Path) {
        self.adapters.folders.reply(Some(root.to_path_buf()));
        let r = self.wb(json!({"kind": "openFolder"})).await;
        assert_eq!(r["kind"], "opened", "{r}");
    }

    async fn document(&mut self, relative: &str) -> Value {
        let r = self
            .wb(json!({"kind": "openRequest", "relative": relative}))
            .await;
        assert_eq!(r["kind"], "document", "{r}");
        r["document"].clone()
    }

    async fn send_fields(&mut self, doc: &Value, fields: &Value) -> Value {
        self.wb(json!({"kind": "send", "id": doc["id"], "revision": doc["revision"], "fields": fields, "context": null}))
            .await
    }
}

async fn start() -> Desk {
    let (adapters, handles) = fixtures::adapters();
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
    }
}

fn temp(name: &str) -> PathBuf {
    let mut n = [0u8; 8];
    getrandom::fill(&mut n).expect("fixture");
    let p = std::env::temp_dir()
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-v2-c41-{name}-{}", hex::encode(n)));
    std::fs::create_dir_all(&p).expect("fixture");
    p
}

/// A collection inside a Git repository: a folder with one request, and a
/// root request (what the tree lists).
fn collection(origin: &str) -> PathBuf {
    let repo = temp("repo");
    std::fs::create_dir(repo.join(".git")).expect("fixture");
    let root = repo.join("api");
    std::fs::create_dir_all(root.join("topics")).expect("fixture");
    std::fs::write(
        root.join("collection.toml"),
        format!("format_version = 1\nid = \"c41\"\nname = \"Topics API\"\n\n[variables]\nbase = \"{origin}\"\n"),
    )
    .expect("fixture");
    std::fs::write(
        root.join("topics/list.request.toml"),
        "format_version = 1\nid = \"list\"\nmethod = \"GET\"\nurl = \"{{base}}/topics\"\n",
    )
    .expect("fixture");
    std::fs::write(
        root.join("health.request.toml"),
        "format_version = 1\nid = \"health\"\nmethod = \"GET\"\nurl = \"{{base}}/health\"\n",
    )
    .expect("fixture");
    root
}

fn topics_service() -> impl std::future::Future<Output = Http> {
    Http::start(Arc::new(|_| Answer::ok(r#"{"custom_topics":["Finance"]}"#)))
}

fn names(v: &Value, key: &str) -> Vec<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x[key].as_str().expect(key).to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn cloned_collection_roots_have_distinct_view_scopes_even_with_the_same_id_and_basename() {
    let a = collection("http://127.0.0.1:8081");
    let b = collection("http://127.0.0.1:8082");
    let mut desk = start().await;
    // Persist the same request selection in each copy, then replace A directly with B.
    desk.open(&b).await;
    desk.document("topics/list.request.toml").await;
    desk.open(&a).await;
    desk.document("topics/list.request.toml").await;
    let first = desk.collection();
    desk.open(&b).await;
    let second = desk.collection();
    assert_eq!(first["id"], second["id"]);
    assert_eq!(first["folder"], "api");
    assert_eq!(first["folder"], second["folder"]);
    assert_eq!(first["selected"], "list");
    assert_eq!(first["selected"], second["selected"]);
    assert_ne!(
        first["scope"], second["scope"],
        "the renderer must distinguish these roots"
    );
    for view in [&first, &second] {
        let scope = view["scope"].as_str().expect("opaque collection scope");
        assert_eq!(scope.len(), 32);
        assert!(scope.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    desk.open(&a).await;
    assert_eq!(
        desk.collection()["scope"],
        first["scope"],
        "scope is stable for the same root and id"
    );
    drop(desk);
    std::fs::remove_dir_all(a.parent().expect("fixture parent")).expect("cleanup");
    std::fs::remove_dir_all(b.parent().expect("fixture parent")).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tree_tabs_and_selection_the_board_draws_from() {
    let http = topics_service().await;
    let root = collection(&http.origin());
    let mut desk = start().await;
    desk.open(&root).await;
    // The tree: the folder and both requests; nothing open yet.
    let c = desk.collection();
    assert_eq!(
        names(&c["requests"], "relative"),
        vec!["health.request.toml", "topics/list.request.toml"],
        "{c}"
    );
    assert!(
        c["folders"]
            .as_array()
            .expect("folders")
            .iter()
            .any(|f| f == "topics"),
        "{c}"
    );
    assert_eq!(c["tabs"].as_array().expect("tabs").len(), 0);
    assert!(c["selected"].is_null());
    // Opening a request: one tab, selected, not a draft.
    let list = desk.document("topics/list.request.toml").await;
    assert_eq!(list["fields"]["method"], "GET");
    assert_eq!(list["new"], false);
    let c = desk.collection();
    assert_eq!(
        names(&c["tabs"], "relative"),
        vec!["topics/list.request.toml"]
    );
    assert_eq!(c["selected"], list["id"]);
    assert_eq!(c["tabs"][0]["draft"], false);
    // A new folder and a new request draft from the tree's +: the draft is a
    // second, selected tab marked new, and the folder is listed.
    let r = desk
        .wb(json!({"kind": "newFolder", "relative": "admin"}))
        .await;
    assert_eq!(r["kind"], "done", "{r}");
    let r = desk
        .wb(json!({"kind": "newRequest", "relative": "admin/create"}))
        .await;
    assert_eq!(r["kind"], "document", "{r}");
    assert_eq!(r["document"]["new"], true);
    let c = desk.collection();
    assert!(
        c["folders"]
            .as_array()
            .expect("folders")
            .iter()
            .any(|f| f == "admin"),
        "{c}"
    );
    assert_eq!(
        names(&c["tabs"], "relative"),
        vec!["topics/list.request.toml", "admin/create.request.toml"]
    );
    assert_eq!(c["selected"], r["document"]["id"]);
    assert_eq!(c["tabs"][1]["new"], true);
    // Selecting the first tab again, then closing the draft, discarding it.
    let r = desk
        .wb(json!({"kind": "selectTab", "id": list["id"]}))
        .await;
    assert_eq!(r["kind"], "done", "{r}");
    assert_eq!(desk.collection()["selected"], list["id"]);
    let r = desk
        .wb(json!({"kind": "closeTab", "id": r#""#.to_owned() + c["tabs"][1]["id"].as_str().expect("id"), "discard": true}))
        .await;
    assert_eq!(r["kind"], "closed", "{r}");
    assert_eq!(
        names(&desk.collection()["tabs"], "relative"),
        vec!["topics/list.request.toml"]
    );
    assert!(http.seen().is_empty(), "nothing was sent");
}

#[tokio::test(flavor = "multi_thread")]
async fn params_rows_are_the_files_lines_read_by_preview_and_sent_as_the_query() {
    let http = topics_service().await;
    let root = collection(&http.origin());
    let mut desk = start().await;
    desk.open(&root).await;
    let list = desk.document("topics/list.request.toml").await;
    // The table's rows: an enabled row and a disabled one, as the editor writes them.
    let mut fields = list["fields"].clone();
    fields["query"] = json!("page=1\n# limit=5");
    let r = desk
        .wb(json!({"kind": "save", "id": list["id"], "revision": list["revision"], "fields": fields, "edit": 1}))
        .await;
    assert_eq!(r["kind"], "saved", "{r}");
    let saved = r["document"].clone();
    assert_eq!(saved["fields"]["query"], "page=1\n# limit=5");
    // The file holds the rows as `[[query]]` tables (name, value, enabled), the form Rust always read.
    let file = std::fs::read_to_string(root.join("topics/list.request.toml")).expect("file");
    assert!(
        file.contains("[[query]]")
            && file.contains("name = \"page\"")
            && file.contains("value = \"1\""),
        "{file}"
    );
    assert!(
        file.contains("name = \"limit\"") && file.contains("enabled = false"),
        "{file}"
    );
    // Preview reads the enabled row only.
    let r = desk
        .wb(json!({"kind": "preview", "id": saved["id"], "fields": saved["fields"]}))
        .await;
    assert_eq!(r["kind"], "preview", "{r}");
    assert_eq!(r["preview"]["query"], json!([["page", "1"]]));
    assert!(
        r["preview"]["url"]
            .as_str()
            .expect("url")
            .ends_with("/topics")
    );
    assert!(http.seen().is_empty(), "preview sends nothing");
    // Send appends it to the URL.
    let r = desk.send_fields(&saved, &saved["fields"]).await;
    assert_eq!(r["kind"], "ran", "{r}");
    assert_eq!(r["run"]["record"]["status"], 200);
    let seen = http.seen();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0].request_line.starts_with("GET /topics?page=1 "),
        "{}",
        seen[0].request_line
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn history_lists_runs_newest_first_and_a_shown_run_is_the_record_as_it_was() {
    let http = topics_service().await;
    let root = collection(&http.origin());
    let mut desk = start().await;
    desk.open(&root).await;
    let list = desk.document("topics/list.request.toml").await;
    let first = desk.send_fields(&list, &list["fields"]).await;
    assert_eq!(first["kind"], "ran", "{first}");
    let health = desk.document("health.request.toml").await;
    let second = desk.send_fields(&health, &health["fields"]).await;
    assert_eq!(second["kind"], "ran", "{second}");
    let page = desk.wb(json!({"kind": "history", "before": null})).await;
    assert_eq!(page["kind"], "history", "{page}");
    let runs = page["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0]["id"], second["run"]["record"]["id"], "newest first");
    assert_eq!(runs[0]["relative"], "health.request.toml");
    assert_eq!(runs[1]["relative"], "topics/list.request.toml");
    // Showing the older run is the record, with its body; nothing is sent again.
    let shown = desk
        .wb(json!({"kind": "showRun", "run": runs[1]["id"]}))
        .await;
    assert_eq!(shown["kind"], "ran", "{shown}");
    assert_eq!(shown["run"]["record"], first["run"]["record"]);
    assert_eq!(shown["run"]["preview"], r#"{"custom_topics":["Finance"]}"#);
    assert_eq!(http.seen().len(), 2);
}
