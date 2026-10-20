//! C15 API Client execution through the production authority interface,
//! against independent loopback fixture servers (plain HTTP and TLS):
//! the fresh snapshot sent exactly as typed with v1's auth and bodies,
//! cookies and redirects under their origin rules (T04, T14); native
//! admission of tenant credentials before any secret is read, unknown
//! managed operations denied, managed redirects never followed and no
//! resend after a 401 (T05, ADR 0004); known tenant changes only through
//! an exact, consumed, natively confirmed review (T02); lost responses,
//! cancels and the four-request bound with nothing ever resent (T03, T07);
//! large responses streamed to sealed storage with a bounded preview and
//! saved exactly (T14); history across restarts with interrupted runs never
//! replayed and offline sends failing plainly (T23); canary secrets absent
//! from every record and private file (T19). Nothing reaches a product
//! endpoint: fixture builds send to literal loopback addresses only.

mod server;
#[path = "../context_credentials/support.rs"]
#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use desktop_core::fixtures::{self, Handles, VaultCall};
use desktop_core::management::ManagementEndpoints;
use desktop_core::runtime::OperationId;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal, ScanState,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};
use server::{Answer, Http};
use support::{Reply, TestCa, TlsServer};

const CANARY: &str = "SYNTHETIC-CANARY-c15-7f3a9e";
const CLIENT_ID: &str = "c15@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "SYNTHETIC-CLIENT-SECRET-c15";
const SCAN_KEY: &str = "SYNTHETIC-SCAN-KEY-c15-0001";
const TSG: &str = "1234567890";
const TOKEN: &str = "synthetic-token-c15";

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
        .join(format!("airs-v2-c15-{name}-{}", hex::encode(n)));
    std::fs::create_dir_all(&p).expect("fixture");
    p
}

/// A collection inside a Git repository with `variables` and requests.
fn collection(variables: &str, requests: &[(&str, &str)]) -> PathBuf {
    let repo = temp("repo");
    std::fs::create_dir(repo.join(".git")).expect("fixture");
    let root = repo.join("api");
    std::fs::create_dir_all(&root).expect("fixture");
    std::fs::write(
        root.join("collection.toml"),
        format!("format_version = 1\nid = \"c15\"\nname = \"Exec\"\n\n[variables]\n{variables}"),
    )
    .expect("fixture");
    for (relative, text) in requests {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("fixture")).expect("fixture");
        std::fs::write(path, text).expect("fixture");
    }
    root
}

fn request(id: &str, method: &str, url: &str, extra: &str) -> String {
    format!("format_version = 1\nid = \"{id}\"\nmethod = \"{method}\"\nurl = \"{url}\"\n{extra}")
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

    async fn wb(&mut self, request: Value) -> Value {
        let Receipt::Admitted { operation, .. } = self
            .send(json!({"kind": "workbench", "request": request}))
            .await
            .expect("admitted");
        self.wait(operation.expect("operation")).await
    }

    /// Dispatches without waiting; the result comes from `wait`.
    async fn start_wb(&mut self, request: Value) -> OperationId {
        let Receipt::Admitted { operation, .. } = self
            .send(json!({"kind": "workbench", "request": request}))
            .await
            .expect("admitted");
        operation.expect("operation")
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

    fn workbench(&self) -> Value {
        serde_json::to_value(self.snap().workbench).expect("json")
    }

    fn context(&self) -> Option<String> {
        self.snap().active.map(|a| a.context.as_str().to_owned())
    }

    async fn open(&mut self, root: &Path) {
        self.adapters.folders.reply(Some(root.to_path_buf()));
        let r = self.wb(json!({"kind": "openFolder"})).await;
        assert_eq!(r["kind"], "opened", "{r}");
    }

    async fn doc(&mut self, relative: &str) -> Value {
        let r = self
            .wb(json!({"kind": "openRequest", "relative": relative}))
            .await;
        assert_eq!(r["kind"], "document", "{r}");
        r["document"].clone()
    }

    async fn secret(&mut self, reference: &str, value: &str) {
        let r = self
            .wb(json!({"kind": "setSecret", "reference": reference, "value": value}))
            .await;
        assert_eq!(r["kind"], "done", "{r}");
    }

    async fn send_fields(&mut self, doc: &Value, fields: &Value) -> Value {
        let context = self.context();
        self.wb(json!({"kind": "send", "id": doc["id"], "revision": doc["revision"], "fields": fields, "context": context}))
            .await
    }

    async fn send_doc(&mut self, relative: &str) -> Value {
        let doc = self.doc(relative).await;
        let fields = doc["fields"].clone();
        self.send_fields(&doc, &fields).await
    }

    /// Gets of records other than the API Client profile key.
    fn secret_reads(&self) -> usize {
        let key = desktop_core::workbench::profile::key_record();
        self.adapters
            .vault
            .calls()
            .iter()
            .filter(|c| matches!(c, VaultCall::Get(r) if *r != key))
            .count()
    }
}

fn with(doc: &Value, key: &str, value: &str) -> Value {
    let mut f = doc["fields"].clone();
    f[key] = json!(value);
    f
}

/// Every file under `dir`, recursively.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(files(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// ------------------------------------------------------------ plain HTTP

#[tokio::test(flavor = "multi_thread")]
async fn sends_the_fresh_snapshot_with_v1_auth_bodies_and_cookies() {
    let http = Http::start(Arc::new(|seen| {
        if seen.request_line.starts_with("GET /login") {
            Answer::status(
                200,
                &[
                    ("set-cookie", "sid=abc; Path=/; Max-Age=3600"),
                    ("set-cookie", "tmp=1"),
                ],
                "{}",
            )
        } else {
            Answer::ok("{\"ok\":true}")
        }
    }))
    .await;
    let root = collection(
        &format!(
            "base = \"{}\"\ntoken = {{ secret = \"dev-token\" }}\n",
            http.origin()
        ),
        &[
            (
                "items/create.request.toml",
                &request("create", "POST", "{{base}}/items", "body_kind = \"json\"\n"),
            ),
            (
                "login.request.toml",
                &request("login", "GET", "{{base}}/login", ""),
            ),
            (
                "next.request.toml",
                &request("next", "GET", "{{base}}/next", ""),
            ),
            (
                "upload.request.toml",
                &request(
                    "upload",
                    "POST",
                    "{{base}}/upload",
                    "body_kind = \"multipart\"\n",
                ),
            ),
            (
                "form.request.toml",
                &request("form", "POST", "{{base}}/form", "body_kind = \"form\"\n"),
            ),
            ("data/blob.bin", "BLOB\u{0}DATA"),
        ],
    );
    let mut desk = desk().await;
    desk.open(&root).await;
    desk.secret("dev-token", "SYNTHETIC-DEV-TOKEN-c15").await;
    // Typed, never saved: Send uses exactly what the editor holds.
    let doc = desk.doc("items/create.request.toml").await;
    let mut fields = doc["fields"].clone();
    fields["headers"] = json!("X-Trace: t-1\n# X-Off: no");
    fields["query"] = json!("q=a b\nlimit=5");
    fields["body"] = json!("{\"name\": \"{{base}}\"}");
    fields["settings"] = json!("[auth]\nkind = \"bearer\"\nsecret = \"dev-token\"\n");
    let r = desk.send_fields(&doc, &fields).await;
    assert_eq!(r["kind"], "ran", "{r}");
    assert_eq!(r["run"]["record"]["status"], 200);
    assert_eq!(r["run"]["preview"], "{\"ok\":true}");
    let seen = &http.seen()[0];
    assert_eq!(seen.request_line, "POST /items?q=a+b&limit=5 HTTP/1.1");
    assert_eq!(seen.header("x-trace"), Some("t-1"));
    assert_eq!(seen.header("x-off"), None);
    assert_eq!(
        seen.header("authorization"),
        Some("Bearer SYNTHETIC-DEV-TOKEN-c15")
    );
    assert_eq!(seen.header("content-type"), Some("application/json"));
    assert_eq!(
        seen.body,
        format!("{{\"name\": \"{}\"}}", http.origin()).as_bytes()
    );
    assert!(
        seen.header("accept-encoding").is_none(),
        "no headers of the client's own"
    );
    // A snapshot from an older revision is refused unsent.
    let before = http.seen().len();
    let stale = desk
        .wb(json!({"kind": "send", "id": "create", "revision": "0".repeat(32), "fields": fields, "context": null}))
        .await;
    assert_eq!(
        stale,
        json!({"kind": "refused", "problem": {"kind": "stale"}})
    );
    assert_eq!(http.seen().len(), before);
    // The file on disk was not changed by sending.
    assert!(
        std::fs::read_to_string(root.join("items/create.request.toml"))
            .expect("fixture")
            .contains("format_version")
    );
    // Cookies: set by one response, sent with the next request.
    assert_eq!(desk.send_doc("login.request.toml").await["kind"], "ran");
    assert_eq!(desk.send_doc("next.request.toml").await["kind"], "ran");
    let next = http
        .seen()
        .into_iter()
        .find(|s| s.request_line.starts_with("GET /next"))
        .expect("next");
    assert_eq!(next.header("cookie"), Some("sid=abc; tmp=1"));
    // Multipart with a collection file, and a form body.
    let doc = desk.doc("upload.request.toml").await;
    let fields = with(
        &doc,
        "body",
        "[[parts]]\nname = \"note\"\nvalue = \"hello\"\n[[parts]]\nname = \"blob\"\nfile = \"data/blob.bin\"\nmime = \"application/x-test\"\n",
    );
    assert_eq!(desk.send_fields(&doc, &fields).await["kind"], "ran");
    let up = http
        .seen()
        .into_iter()
        .find(|s| s.request_line.starts_with("POST /upload"))
        .expect("upload");
    let boundary = up
        .header("content-type")
        .expect("type")
        .split("boundary=")
        .nth(1)
        .expect("boundary")
        .to_owned();
    let body = String::from_utf8_lossy(&up.body).into_owned();
    assert_eq!(
        body,
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"note\"\r\n\r\nhello\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"blob\"; filename=\"blob.bin\"\r\nContent-Type: application/x-test\r\n\r\nBLOB\u{0}DATA\r\n--{boundary}--\r\n"
        )
    );
    let doc = desk.doc("form.request.toml").await;
    assert_eq!(
        desk.send_fields(&doc, &with(&doc, "body", "a=1 2\nb={{base}}"))
            .await["kind"],
        "ran"
    );
    let form = http
        .seen()
        .into_iter()
        .find(|s| s.request_line.starts_with("POST /form"))
        .expect("form");
    assert_eq!(
        form.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        String::from_utf8_lossy(&form.body),
        format!(
            "a=1+2&b={}",
            url::form_urlencoded::byte_serialize(http.origin().as_bytes()).collect::<String>()
        )
    );
    // A body file outside the collection or over 64 MiB is refused unsent.
    let before = http.seen().len();
    let outside = with(
        &doc,
        "body",
        "[[parts]]\nname = \"x\"\nfile = \"../outside.bin\"\n",
    );
    let mut outside_fields = outside.clone();
    outside_fields["bodyKind"] = json!("multipart");
    let r = desk.send_fields(&doc, &outside_fields).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "render", "problem": {"kind": "field", "problem": {"field": "body", "line": null}}}}),
        "a parent path is not a valid snapshot"
    );
    let big = std::fs::File::create(root.join("data/big.bin")).expect("fixture");
    big.set_len(64 * 1024 * 1024 + 1).expect("fixture");
    let mut big_fields = with(
        &doc,
        "body",
        "[[parts]]\nname = \"x\"\nfile = \"data/big.bin\"\n",
    );
    big_fields["bodyKind"] = json!("multipart");
    let r = desk.send_fields(&doc, &big_fields).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "body", "problem": "tooLarge"}})
    );
    assert_eq!(http.seen().len(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn redirects_follow_origin_rules_and_credentials_never_cross() {
    let other = Http::start(Arc::new(|_| Answer::ok("{}"))).await;
    let target = format!("{}/elsewhere", other.origin());
    let http = Http::start(Arc::new(move |seen| {
        if seen.request_line.starts_with("POST /moved") {
            Answer::status(303, &[("location", "/landed")], "")
        } else if seen.request_line.contains("/away") {
            Answer::status(302, &[("location", target.as_str())], "")
        } else {
            Answer::ok("{}")
        }
    }))
    .await;
    let root = collection(
        &format!(
            "base = \"{}\"\ntoken = {{ secret = \"dev-token\" }}\n",
            http.origin()
        ),
        &[
            (
                "moved.request.toml",
                &request("moved", "POST", "{{base}}/moved", ""),
            ),
            (
                "away.request.toml",
                &request("away", "GET", "{{base}}/away", ""),
            ),
            (
                "away-auth.request.toml",
                &request(
                    "awayauth",
                    "GET",
                    "{{base}}/away-auth",
                    "[auth]\nkind = \"bearer\"\nsecret = \"dev-token\"\n",
                ),
            ),
        ],
    );
    let mut desk = desk().await;
    desk.open(&root).await;
    desk.secret("dev-token", "SYNTHETIC-DEV-TOKEN-c15").await;
    // Anonymous: followed (303 becomes GET without the body).
    let doc = desk.doc("moved.request.toml").await;
    let r = desk.send_fields(&doc, &with(&doc, "body", "payload")).await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    let landed = http
        .seen()
        .into_iter()
        .find(|s| s.request_line.starts_with("GET /landed"))
        .expect("followed");
    assert!(landed.body.is_empty());
    // Anonymous across origins: followed too.
    assert_eq!(
        desk.send_doc("away.request.toml").await["run"]["record"]["status"],
        200
    );
    assert_eq!(other.seen().len(), 1);
    // With a workbench secret: a cross-origin redirect is the result, and
    // the other origin sees nothing.
    let r = desk.send_doc("away-auth.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 302, "{r}");
    assert_eq!(
        r["run"]["record"]["redirect"],
        format!("{}/elsewhere", other.origin())
    );
    assert_eq!(other.seen().len(), 1);
    // A destination this build does not send to is refused unsent.
    let doc = desk.doc("away.request.toml").await;
    let r = desk
        .send_fields(&doc, &with(&doc, "url", "https://example.invalid/x"))
        .await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "network"}})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lost_responses_cancels_and_the_bound_never_resend() {
    let http = Http::start(Arc::new(|seen| {
        if seen.request_line.contains("/lost") {
            Answer::Close
        } else {
            Answer::Hang
        }
    }))
    .await;
    let root = collection(
        &format!("base = \"{}\"\n", http.origin()),
        &[
            (
                "lost.request.toml",
                &request("lost", "POST", "{{base}}/lost", ""),
            ),
            (
                "hang.request.toml",
                &request("hang", "POST", "{{base}}/hang", ""),
            ),
            (
                "offline.request.toml",
                &request("offline", "POST", "http://127.0.0.1:9/offline", ""),
            ),
        ],
    );
    let mut desk = desk().await;
    desk.open(&root).await;
    let r = desk.send_doc("lost.request.toml").await;
    assert_eq!(r["run"]["record"]["state"], "failed", "{r}");
    assert_eq!(
        r["run"]["record"]["uncertain"], true,
        "a change may have arrived: {r}"
    );
    assert_eq!(http.seen().len(), 1, "never resent");
    // Offline: nothing was sent, so nothing is uncertain.
    let r = desk.send_doc("offline.request.toml").await;
    assert_eq!(r["run"]["record"]["failure"], "connect", "{r}");
    assert_eq!(r["run"]["record"]["uncertain"], false);
    // Four at once; the fifth waits for none of them and is refused.
    let doc = desk.doc("hang.request.toml").await;
    let mut operations = Vec::new();
    for _ in 0..4 {
        operations.push(
            desk.start_wb(json!({"kind": "send", "id": "hang", "revision": doc["revision"], "fields": doc["fields"], "context": null}))
                .await,
        );
    }
    assert!(
        support::eventually(|| desk.workbench()["running"]
            .as_array()
            .is_some_and(|r| r.len() == 4))
        .await
    );
    assert!(support::eventually(|| http.seen().len() == 5).await);
    let r = desk.send_fields(&doc, &doc["fields"].clone()).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "busy"}})
    );
    assert_eq!(
        desk.workbench()["busy"],
        false,
        "transfers leave the lane free"
    );
    // Editing continues while they run.
    let p = desk.wb(json!({"kind": "protect", "edit": 1, "id": "hang", "revision": doc["revision"], "fields": with(&doc, "url", "{{base}}/edited")})).await;
    assert_eq!(p["kind"], "protected");
    for run in desk.workbench()["running"]
        .as_array()
        .expect("running")
        .clone()
    {
        assert_eq!(
            desk.wb(json!({"kind": "cancel", "run": run["run"]})).await["kind"],
            "done"
        );
    }
    for op in operations {
        let r = desk.wait(op).await;
        assert_eq!(r["run"]["record"]["state"], "cancelled", "{r}");
        assert_eq!(r["run"]["record"]["uncertain"], true);
    }
    assert_eq!(http.seen().len(), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn large_responses_stream_to_sealed_storage_and_save_exactly() {
    const LEN: u64 = 20 * 1024 * 1024 + 7;
    let http = Http::start(Arc::new(|_| Answer::Stream { len: LEN })).await;
    let root = collection(
        &format!("base = \"{}\"\n", http.origin()),
        &[(
            "big.request.toml",
            &request("big", "GET", "{{base}}/big", ""),
        )],
    );
    let mut desk = desk().await;
    desk.open(&root).await;
    let r = desk.send_doc("big.request.toml").await;
    let record = &r["run"]["record"];
    assert_eq!(record["bytes"], LEN, "{record}");
    assert_eq!(record["stored"], true);
    assert_eq!(r["run"]["previewBytes"], 256 * 1024);
    assert!(
        r["run"]["preview"].is_null(),
        "binary bodies are not shown as text"
    );
    let run = record["id"].as_str().expect("id").to_owned();
    // Stored sealed: the pattern never appears on disk in the clear.
    let sample: Vec<u8> = (0..4096u64).map(server::pattern).collect();
    for f in files(&desk.adapters.workbench_root) {
        assert!(
            !contains(&std::fs::read(&f).expect("read"), &sample),
            "{}",
            f.display()
        );
    }
    // Saved exactly; never into the repository's .git or over a folder.
    let out = temp("save").join("big.bin");
    desk.adapters.folders.reply_save(Some(out.clone()));
    let saved = desk.wb(json!({"kind": "saveResponse", "run": run})).await;
    assert_eq!(
        saved,
        json!({"kind": "responseSaved", "bytes": LEN, "name": "big.bin"})
    );
    let bytes = std::fs::read(&out).expect("saved");
    assert_eq!(bytes.len() as u64, LEN);
    assert!(
        bytes
            .iter()
            .enumerate()
            .all(|(i, b)| *b == server::pattern(i as u64))
    );
    assert_eq!(desk.adapters.folders.suggested(), ["big-response.txt"]);
    desk.adapters
        .folders
        .reply_save(Some(root.parent().expect("repo").join(".git/big.bin")));
    assert_eq!(
        desk.wb(json!({"kind": "saveResponse", "run": run})).await,
        json!({"kind": "saveResponseFailed", "failure": "destination"})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn history_survives_restarts_and_interrupted_runs_are_never_replayed() {
    let http = Http::start(Arc::new(|seen| {
        if seen.request_line.contains("/hang") {
            Answer::Hang
        } else {
            Answer::ok("{\"kept\":1}")
        }
    }))
    .await;
    let root = collection(
        &format!("base = \"{}\"\n", http.origin()),
        &[
            ("ok.request.toml", &request("ok", "GET", "{{base}}/ok", "")),
            (
                "hang.request.toml",
                &request("hang", "DELETE", "{{base}}/hang", ""),
            ),
        ],
    );
    let mut desk = desk().await;
    desk.open(&root).await;
    let done = desk.send_doc("ok.request.toml").await;
    let doc = desk.doc("hang.request.toml").await;
    let _hanging = desk
        .start_wb(json!({"kind": "send", "id": "hang", "revision": doc["revision"], "fields": doc["fields"], "context": null}))
        .await;
    assert!(support::eventually(|| http.seen().len() == 2).await);
    // The desktop stops while the DELETE is in flight.
    let handles = desk.adapters;
    drop(desk.core);
    let mut again = start(Some(&handles), CoreConfig::default()).await;
    again.open(&root).await;
    let page = again.wb(json!({"kind": "history", "before": null})).await;
    let runs = page["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 2, "{page}");
    assert_eq!(runs[0]["state"], "interrupted");
    assert_eq!(runs[0]["uncertain"], true);
    assert_eq!(runs[1]["id"], done["run"]["record"]["id"]);
    let shown = again
        .wb(json!({"kind": "showRun", "run": runs[1]["id"]}))
        .await;
    assert_eq!(shown["run"]["preview"], "{\"kept\":1}");
    // Nothing was sent again by the restart or by reading history.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(http.seen().len(), 2);
    assert_eq!(
        again.wb(json!({"kind": "clearHistory"})).await["kind"],
        "done"
    );
    assert_eq!(
        again.wb(json!({"kind": "history", "before": null})).await["runs"],
        json!([])
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn canary_secrets_never_reach_records_or_private_files() {
    let http = Http::start(Arc::new(|_| {
        Answer::status(200, &[("x-echo", "SYNTHETIC-CANARY-c15-7f3a9e")], "{}")
    }))
    .await;
    let root = collection(
        &format!(
            "base = \"{}\"\ncanary = {{ secret = \"canary\" }}\n",
            http.origin()
        ),
        &[(
            "c.request.toml",
            &request(
                "c",
                "GET",
                "{{base}}/c?k={{canary}}",
                "[[headers]]\nname = \"X-Canary\"\nvalue = \"{{canary}}\"\n",
            ),
        )],
    );
    let mut desk = desk().await;
    desk.open(&root).await;
    desk.secret("canary", CANARY).await;
    let r = desk.send_doc("c.request.toml").await;
    assert_eq!(http.seen()[0].header("x-canary"), Some(CANARY));
    let page = desk.wb(json!({"kind": "history", "before": null})).await;
    for text in [
        r.to_string(),
        page.to_string(),
        desk.workbench().to_string(),
    ] {
        assert!(!text.contains(CANARY), "{text}");
    }
    assert_eq!(
        r["run"]["record"]["headers"]
            .as_array()
            .expect("headers")
            .iter()
            .find(|h| h[0] == "x-echo")
            .expect("echo")[1],
        "[REDACTED]"
    );
    for f in files(&desk.adapters.workbench_root) {
        assert!(
            !contains(&std::fs::read(&f).expect("read"), CANARY.as_bytes()),
            "{}",
            f.display()
        );
    }
}

// ------------------------------------------------------- tenant credentials

struct Tenant {
    desk: Desk,
    server: TlsServer,
    _ca: TestCa,
    origin: String,
}

/// A desk whose active tenant has management (Keychain) and scan
/// credentials bound to a loopback TLS fixture.
async fn tenant() -> Tenant {
    tenant_with(0).await
}

/// [`tenant`] whose token endpoint holds each grant `token_delay` ms.
async fn tenant_with(token_delay: u64) -> Tenant {
    let ca = TestCa::new("airs-v2 c15 ca");
    let server = TlsServer::start(
        ca.leaf(&["127.0.0.1"]),
        Reply::Route(Arc::new(move |seen| {
            let ok = |status: u16, body: &str| Reply::Raw {
                head: format!(
                    "HTTP/1.1 {status} Fixture\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
                    body.len()
                ),
                body: body.as_bytes().to_vec(),
            };
            let line = seen.request_line.as_str();
            if line.starts_with("POST /oauth2/access_token") {
                // RFC 6749 2.3.1: the client ID is form-urlencoded first.
                let id = CLIENT_ID.replace('@', "%40");
                let basic = format!("Basic {}", b64(&format!("{id}:{CLIENT_SECRET}")));
                if seen.header("authorization") == Some(basic.as_str())
                    && String::from_utf8_lossy(&seen.body).contains(&format!("scope=tsg_id%3A{TSG}"))
                {
                    let minted = ok(200, &format!("{{\"access_token\":\"{TOKEN}\",\"expires_in\":600}}"));
                    return if token_delay > 0 {
                        Reply::Delayed { millis: token_delay, then: Box::new(minted) }
                    } else {
                        minted
                    };
                }
                return ok(401, "{}");
            }
            if line.starts_with("GET /iam/v1/access_policies?echo") {
                // Echoes the credential it received (a misbehaving service).
                let echoed = seen.header("authorization").unwrap_or_default().to_owned();
                return Reply::Raw {
                    head: format!(
                        "HTTP/1.1 200 Fixture\r\nx-echo: {echoed}\r\nx-echo-token: {TOKEN}\r\ncontent-length: 2\r\nconnection: close"
                    ),
                    body: b"{}".to_vec(),
                };
            }
            if let Some(rest) = line.strip_prefix("GET /iam/v1/access_policies?login=") {
                // A known read that signs in with a session cookie.
                let sid = rest.split(' ').next().unwrap_or_default();
                return Reply::Raw {
                    head: format!(
                        "HTTP/1.1 200 Fixture\r\nset-cookie: sid={sid}; Path=/; Max-Age=3600\r\ncontent-length: 2\r\nconnection: close"
                    ),
                    body: b"{}".to_vec(),
                };
            }
            if line.starts_with("GET /iam/v1/roles") {
                return ok(401, "{}");
            }
            if let Some(rest) = line.strip_prefix("GET /iam/v1/permissions") {
                // Redirects to `?to=` (an unregistered origin), or within
                // the tenant's own.
                let to = rest
                    .split(' ')
                    .next()
                    .and_then(|q| q.strip_prefix("?to="))
                    .unwrap_or("/iam/v1/access_policies");
                return Reply::Raw {
                    head: format!("HTTP/1.1 302 Found\r\nlocation: {to}\r\ncontent-length: 0\r\nconnection: close"),
                    body: Vec::new(),
                };
            }
            if seen.header("authorization") == Some(format!("Bearer {TOKEN}").as_str())
                || seen.header("x-pan-token") == Some(SCAN_KEY)
            {
                return ok(200, "{\"ok\":true}");
            }
            ok(403, "{}")
        })),
    )
    .await;
    let origin = format!("https://127.0.0.1:{}", server.port);
    let (adapters, handles) = fixtures::adapters();
    let config = CoreConfig {
        management: ManagementEndpoints::at_origin(&origin),
        workbench_api_origins: Vec::new(),
        ..CoreConfig::default()
    };
    let core = Core::new(config, adapters);
    core.start();
    core.set_transport_policy(TransportPolicy {
        revision: 2,
        proxy: ProxyMode::Direct,
        extra_roots: vec![ca.pem.as_bytes().to_vec()],
    })
    .expect("policy");
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    let mut desk = Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
    };
    for label in ["Tenant A", "Tenant B"] {
        desk.send(
            json!({"kind": "createTenant", "profile": {"label": label, "tsgId": TSG,
            "destination": {"kind": "custom", "origin": origin}}}),
        )
        .await
        .expect("tenant");
    }
    select(&mut desk, "Tenant A").await;
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("management");
    let context = desk.context();
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "keychain", "key": SCAN_KEY}))
        .await
        .expect("scan");
    assert!(
        support::eventually(|| desk
            .snap()
            .tenants
            .iter()
            .any(|t| t.management.state == ScanState::Ready && t.scan.state == ScanState::Ready))
        .await
    );
    Tenant {
        desk,
        server,
        _ca: ca,
        origin,
    }
}

async fn select(desk: &mut Desk, label: &str) {
    let id = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == label)
        .expect("tenant")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": id}))
        .await
        .expect("selected");
}

fn b64(text: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i <= c.len() {
                char::from(T[((n >> (18 - 6 * i)) & 63) as usize])
            } else {
                '='
            });
        }
    }
    out
}

const OAUTH: &str = "[auth]\nkind = \"oauth2\"\ntoken_url = \"{{token_url}}\"\nclient_id = \"{{client_id}}\"\nclient_secret = \"{{client_secret}}\"\nscope = \"tsg_id:{{tsg_id}}\"\n";

fn managed_collection(origin: &str, requests: &[(&str, &str, &str)]) -> PathBuf {
    let files: Vec<(String, String)> = requests
        .iter()
        .map(|(id, method, path)| {
            (
                format!("{id}.request.toml"),
                request(id, method, &format!("{{{{scm_base}}}}{path}"), OAUTH),
            )
        })
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    // An imported copy of the built-in collection's identity changes nothing.
    let root = collection(
        &format!(
            "scm_base = \"{origin}\"\ntoken_url = \"{origin}/oauth2/access_token\"\ntsg_id = {{ secret = \"tenant.tsg_id\" }}\nclient_id = {{ secret = \"tenant.client_id\" }}\nclient_secret = {{ secret = \"tenant.client_secret\" }}\nkey = {{ secret = \"tenant.api_key\" }}\n"
        ),
        &refs,
    );
    let text = std::fs::read_to_string(root.join("collection.toml")).expect("fixture");
    std::fs::write(
        root.join("collection.toml"),
        text.replace("id = \"c15\"", "id = \"prisma-airs-scm\""),
    )
    .expect("fixture");
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn tenant_credentials_are_admitted_before_any_secret_is_read() {
    let Tenant {
        mut desk,
        server,
        origin,
        ..
    } = tenant().await;
    // An unregistered loopback service a managed redirect could point to.
    let elsewhere = Http::start(Arc::new(|_| Answer::status(200, &[], "{}"))).await;
    let away = format!("/iam/v1/permissions?to={}/stolen", elsewhere.origin());
    let root = managed_collection(
        &origin,
        &[
            ("read", "GET", "/iam/v1/access_policies"),
            ("unknown", "GET", "/iam/v1/not-in-the-catalogue"),
            ("roles", "GET", "/iam/v1/roles"),
            ("perm", "GET", &away),
        ],
    );
    desk.open(&root).await;
    let reads = desk.secret_reads();
    // Edited token URL, unknown operation, a leaked placement, no tenant:
    // all refused before any credential is read or anything is sent.
    let doc = desk.doc("read.request.toml").await;
    let cases = [
        (
            with(
                &doc,
                "settings",
                &OAUTH.replace("{{token_url}}", &format!("{origin}/elsewhere")),
            ),
            json!({"kind": "tokenDestination"}),
        ),
        (
            with(&doc, "headers", "X-Copy: {{client_secret}}"),
            json!({"kind": "managedPlacement"}),
        ),
        (
            with(&doc, "headers", "Authorization: Bearer mine"),
            json!({"kind": "ambiguousCredential"}),
        ),
        (
            with(&doc, "url", "https://127.0.0.2:1/iam/v1/access_policies"),
            json!({"kind": "requestDestination"}),
        ),
        // Another virtual host or an extra implicit credential.
        (
            with(&doc, "headers", "host: evil.example"),
            json!({"kind": "ambiguousCredential"}),
        ),
        (
            with(
                &doc,
                "url",
                &format!(
                    "{}/iam/v1/access_policies",
                    origin.replace("https://", "https://u:p@")
                ),
            ),
            json!({"kind": "ambiguousCredential"}),
        ),
    ];
    for (fields, expected) in cases {
        let r = desk.send_fields(&doc, &fields).await;
        assert_eq!(r, json!({"kind": "sendRefused", "refusal": expected}));
    }
    assert_eq!(
        desk.send_doc("unknown.request.toml").await,
        json!({"kind": "sendRefused", "refusal": {"kind": "unknownEndpoint"}})
    );
    let r = desk
        .wb(json!({"kind": "send", "id": "read", "revision": doc["revision"], "fields": doc["fields"], "context": null}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "noTenant"}})
    );
    assert_eq!(desk.secret_reads(), reads, "no credential was read");
    assert!(server.seen().is_empty(), "nothing was sent");
    // A known read: the token is minted from the tenant's credential, then
    // the request goes with it.
    let r = desk.send_doc("read.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    assert_eq!(r["run"]["record"]["class"], "managed");
    assert!(
        r["run"]["record"]["operation"]
            .as_str()
            .is_some_and(|o| !o.is_empty()),
        "{r}"
    );
    let lines: Vec<String> = server
        .seen()
        .iter()
        .map(|s| s.request_line.clone())
        .collect();
    assert_eq!(
        lines,
        [
            "POST /oauth2/access_token HTTP/1.1",
            "GET /iam/v1/access_policies HTTP/1.1"
        ]
    );
    // A managed redirect is shown, never followed.
    let r = desk.send_doc("perm.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 302, "{r}");
    assert_eq!(
        server.seen().len(),
        3,
        "the token was reused; the redirect not followed"
    );
    assert!(
        elsewhere.seen().is_empty(),
        "nothing followed the redirect to an unregistered origin"
    );
    // A 401 drops the token and nothing is resent.
    let r = desk.send_doc("roles.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 401);
    assert_eq!(r["run"]["tokenDropped"], true);
    assert_eq!(server.seen().len(), 4);
    // The next Send mints a new token first.
    desk.send_doc("read.request.toml").await;
    let lines: Vec<String> = server
        .seen()
        .iter()
        .skip(4)
        .map(|s| s.request_line.clone())
        .collect();
    assert_eq!(
        lines,
        [
            "POST /oauth2/access_token HTTP/1.1",
            "GET /iam/v1/access_policies HTTP/1.1"
        ]
    );
    // The scan key goes only as x-pan-token to the scan origin.
    let doc = desk.doc("read.request.toml").await;
    let mut scan = with(&doc, "url", &format!("{origin}/v1/scan/results"));
    scan["settings"] = json!("");
    scan["headers"] = json!("x-pan-token: {{key}}");
    let r = desk.send_fields(&doc, &scan).await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
}

#[tokio::test(flavor = "multi_thread")]
async fn known_tenant_changes_need_an_exact_confirmed_review_and_are_sent_once() {
    let Tenant {
        mut desk,
        server,
        origin,
        ..
    } = tenant().await;
    let root = managed_collection(
        &origin,
        &[
            ("del", "DELETE", "/iam/v1/access_policies/abc"),
            ("create", "POST", "/iam/v1/access_policies"),
        ],
    );
    std::fs::write(root.join("policy.json"), "{\"name\":\"p\"}").expect("fixture");
    desk.open(&root).await;
    let doc = desk.doc("del.request.toml").await;
    let r = desk.send_doc("del.request.toml").await;
    assert_eq!(r["kind"], "reviewRequired", "{r}");
    let review = r["review"].clone();
    assert_eq!(review["method"], "DELETE");
    assert_eq!(review["destination"], origin);
    assert!(server.seen().is_empty(), "a review sends nothing");
    assert_eq!(desk.workbench()["review"], review);
    let context = desk.context();
    let apply = |review: &Value, fields: &Value| json!({"kind": "apply", "review": review, "id": "del", "revision": doc["revision"], "fields": fields, "context": context});
    // A forged review, then a changed snapshot: refused before any sheet
    // (were one shown, it would be declined).
    desk.adapters.confirmer.auto(Some(false));
    let r = desk
        .wb(apply(&json!("rv-0000000000000000"), &doc["fields"]))
        .await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "reviewMissing"}})
    );
    let changed = with(&doc, "url", "{{scm_base}}/iam/v1/access_policies/other");
    let r = desk.wb(apply(&review["review"], &changed)).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "reviewStale"}})
    );
    // The operator declines natively: nothing is sent, the review stays.
    desk.adapters.confirmer.auto(Some(false));
    let r = desk.wb(apply(&review["review"], &doc["fields"])).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "declined"}})
    );
    assert!(server.seen().is_empty());
    // Confirmed: sent exactly once, then the review is consumed.
    desk.adapters.confirmer.auto(Some(true));
    let r = desk.wb(apply(&review["review"], &doc["fields"])).await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    let deletes = |server: &TlsServer| {
        server
            .seen()
            .iter()
            .filter(|s| s.request_line.starts_with("DELETE"))
            .count()
    };
    assert_eq!(deletes(&server), 1);
    let (_, confirmation) = desk
        .adapters
        .confirmer
        .requests()
        .last()
        .cloned()
        .expect("confirmed");
    assert!(
        confirmation.destructive && confirmation.message.contains("/iam/v1/access_policies/abc")
    );
    let r = desk.wb(apply(&review["review"], &doc["fields"])).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "reviewConsumed"}})
    );
    assert_eq!(deletes(&server), 1);
    // A body file changed after review: stale.
    let doc = desk.doc("create.request.toml").await;
    let mut fields = doc["fields"].clone();
    fields["bodyKind"] = json!("multipart");
    fields["body"] = json!("[[parts]]\nname = \"p\"\nfile = \"policy.json\"\n");
    let r = desk.send_fields(&doc, &fields).await;
    assert_eq!(r["kind"], "reviewRequired", "{r}");
    std::fs::write(root.join("policy.json"), "{\"name\":\"changed\"}").expect("fixture");
    let context = desk.context();
    let r = desk
        .wb(json!({"kind": "apply", "review": r["review"]["review"], "id": "create", "revision": doc["revision"], "fields": fields, "context": context}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "reviewStale"}})
    );
    // Another tenant: the review's context is gone.
    let r = desk.send_fields(&doc, &fields).await;
    let stale_context = desk.context();
    select(&mut desk, "Tenant B").await;
    let r = desk
        .wb(json!({"kind": "apply", "review": r["review"]["review"], "id": "create", "revision": doc["revision"], "fields": fields, "context": stale_context}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "noTenant"}})
    );
    assert_eq!(
        server
            .seen()
            .iter()
            .filter(|s| s.request_line.starts_with("POST /iam"))
            .count(),
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn trust_and_proxies_follow_the_native_transport_policy() {
    // A certificate from an authority the policy does not trust.
    let untrusted = TestCa::new("airs-v2 c15 untrusted");
    let tls = TlsServer::start(
        untrusted.leaf(&["127.0.0.1"]),
        Reply::Raw {
            head: "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close".into(),
            body: b"{}".to_vec(),
        },
    )
    .await;
    let root = collection(
        &format!("tls = \"https://127.0.0.1:{}\"\n", tls.port),
        &[("t.request.toml", &request("t", "GET", "{{tls}}/x", ""))],
    );
    let mut desk = desk().await;
    desk.open(&root).await;
    let r = desk.send_doc("t.request.toml").await;
    assert_eq!(r["run"]["record"]["failure"], "tls", "{r}");
    assert!(tls.seen().is_empty());
    // Trusted once the reviewed root is in the policy; an explicit proxy
    // carries the request when the build may reach beyond loopback.
    let proxy = support::Proxy::start(tls.port).await;
    let config = CoreConfig {
        workbench_network: desktop_core::workbench::admission::Network::Live,
        ..CoreConfig::default()
    };
    let mut live = start(None, config).await;
    live.core
        .set_transport_policy(TransportPolicy {
            revision: 2,
            proxy: ProxyMode::Explicit { url: proxy.url() },
            extra_roots: vec![untrusted.pem.as_bytes().to_vec()],
        })
        .expect("policy");
    live.open(&root).await;
    let r = live.send_doc("t.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    assert_eq!(proxy.tunnels(), [format!("127.0.0.1:{}", tls.port)]);
    // Fixture builds (loopback only) never use a proxy.
    desk.core
        .set_transport_policy(TransportPolicy {
            revision: 2,
            proxy: ProxyMode::Explicit { url: proxy.url() },
            extra_roots: vec![untrusted.pem.as_bytes().to_vec()],
        })
        .expect("policy");
    let r = desk.send_doc("t.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    assert_eq!(proxy.tunnels().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn registered_prisma_origins_are_classified_whatever_the_credential() {
    let Tenant {
        mut desk,
        server,
        origin,
        ..
    } = tenant().await;
    // A generic service that redirects into the registered origin.
    let target = format!("{origin}/iam/v1/access_policies/abc");
    let redirector = Http::start(Arc::new(move |_| {
        Answer::status(307, &[("location", target.as_str())], "")
    }))
    .await;
    let root = collection(
        &format!(
            "scm_base = \"{origin}\"\nelsewhere = \"{}\"\nmine = {{ secret = \"dev-token\" }}\n",
            redirector.origin()
        ),
        &[
            (
                "manual.request.toml",
                &request(
                    "manual",
                    "DELETE",
                    "{{scm_base}}/iam/v1/access_policies/abc",
                    &format!("[[headers]]\nname = \"Authorization\"\nvalue = \"Bearer {TOKEN}\"\n"),
                ),
            ),
            (
                "keyed.request.toml",
                &request(
                    "keyed",
                    "DELETE",
                    "{{scm_base}}/iam/v1/access_policies/abc",
                    "[auth]\nkind = \"bearer\"\nsecret = \"dev-token\"\n",
                ),
            ),
            (
                "bare.request.toml",
                &request(
                    "bare",
                    "DELETE",
                    "{{scm_base}}/iam/v1/access_policies/abc",
                    "",
                ),
            ),
            (
                "unknown.request.toml",
                &request(
                    "unknown",
                    "GET",
                    "{{scm_base}}/iam/v1/not-in-the-catalogue",
                    "",
                ),
            ),
            (
                "bounce.request.toml",
                &request("bounce", "DELETE", "{{elsewhere}}/bounce", ""),
            ),
        ],
    );
    desk.open(&root).await;
    desk.secret("dev-token", TOKEN).await;
    // With or without a context, a manual header, a collection secret or
    // no credential at all: a known mutation stops at an exact review.
    for relative in ["manual", "keyed", "bare"] {
        let doc = desk.doc(&format!("{relative}.request.toml")).await;
        for context in [None, desk.context()] {
            let r = desk
                .wb(json!({"kind": "send", "id": doc["id"], "revision": doc["revision"], "fields": doc["fields"], "context": context}))
                .await;
            assert_eq!(r["kind"], "reviewRequired", "{relative}: {r}");
            assert_eq!(r["review"]["method"], "DELETE");
        }
    }
    let unknown = desk.doc("unknown.request.toml").await;
    let r = desk
        .wb(json!({"kind": "send", "id": "unknown", "revision": unknown["revision"], "fields": unknown["fields"], "context": null}))
        .await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "unknownEndpoint"}})
    );
    // Plain HTTP to the registered host and port is the same origin.
    let plain = with(
        &unknown,
        "url",
        &format!(
            "{}/iam/v1/not-in-the-catalogue",
            origin.replace("https://", "http://")
        ),
    );
    let r = desk.send_fields(&unknown, &plain).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "unknownEndpoint"}})
    );
    assert!(server.seen().is_empty(), "nothing reached the tenant");
    // The manual-credential review, confirmed, is sent once as written.
    let doc = desk.doc("manual.request.toml").await;
    let r = desk
        .wb(json!({"kind": "send", "id": "manual", "revision": doc["revision"], "fields": doc["fields"], "context": null}))
        .await;
    desk.adapters.confirmer.auto(Some(true));
    let r = desk
        .wb(json!({"kind": "apply", "review": r["review"]["review"], "id": "manual", "revision": doc["revision"], "fields": doc["fields"], "context": null}))
        .await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    assert_eq!(r["run"]["record"]["class"], "anonymous");
    assert!(
        r["run"]["record"]["operation"]
            .as_str()
            .is_some_and(|o| !o.is_empty())
    );
    assert_eq!(server.seen().len(), 1);
    // A generic redirect never enters the registered origin.
    let r = desk.send_doc("bounce.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 307, "{r}");
    assert!(
        r["run"]["record"]["redirect"]
            .as_str()
            .is_some_and(|l| l.contains("/iam/v1/access_policies/abc"))
    );
    assert_eq!(redirector.seen().len(), 1);
    assert_eq!(server.seen().len(), 1, "the redirect was not followed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_tenant_change_retires_reviews_and_unsent_requests() {
    let Tenant {
        mut desk,
        server,
        origin,
        ..
    } = tenant_with(3000).await;
    let root = managed_collection(
        &origin,
        &[
            ("read", "GET", "/iam/v1/access_policies"),
            ("del", "DELETE", "/iam/v1/access_policies/abc"),
        ],
    );
    desk.open(&root).await;
    let api = |server: &TlsServer| {
        server
            .seen()
            .iter()
            .filter(|s| s.request_line.contains("/iam/"))
            .count()
    };
    // Held while its token is minted; the tenant changes: it never goes.
    let doc = desk.doc("read.request.toml").await;
    let context = desk.context();
    let op = desk
        .start_wb(json!({"kind": "send", "id": "read", "revision": doc["revision"], "fields": doc["fields"], "context": context}))
        .await;
    assert!(
        support::eventually(|| !server.seen().is_empty()).await,
        "token requested"
    );
    select(&mut desk, "Tenant B").await;
    let r = desk.wait(op).await;
    assert_eq!(r["run"]["record"]["failure"], "retired", "{r}");
    assert_eq!(r["run"]["record"]["uncertain"], false);
    assert_eq!(api(&server), 0, "the customer request was never sent");
    // Held at the native confirmation (after the review was planned): a
    // switch away and back leaves nothing to send and nothing to revive.
    select(&mut desk, "Tenant A").await;
    let doc = desk.doc("del.request.toml").await;
    let context = desk.context();
    let review = desk.send_doc("del.request.toml").await;
    assert_eq!(review["kind"], "reviewRequired", "{review}");
    let apply = |context: &Option<String>| json!({"kind": "apply", "review": review["review"]["review"], "id": "del", "revision": doc["revision"], "fields": doc["fields"], "context": context});
    desk.adapters.confirmer.auto(None);
    let op = desk.start_wb(apply(&context)).await;
    assert!(support::eventually(|| desk.adapters.confirmer.held() == 1).await);
    select(&mut desk, "Tenant B").await;
    assert_eq!(desk.workbench()["review"], Value::Null, "retired at once");
    assert!(desk.adapters.confirmer.complete(true));
    let r = desk.wait(op).await;
    assert_eq!(r["kind"], "sendRefused", "{r}");
    select(&mut desk, "Tenant A").await;
    desk.adapters.confirmer.auto(Some(true));
    let r = desk.wb(apply(&desk.context())).await;
    assert_eq!(
        r,
        json!({"kind": "sendRefused", "refusal": {"kind": "reviewConsumed"}})
    );
    // Replacing the tenant's credential retires a review the same way.
    let review = desk.send_doc("del.request.toml").await;
    assert_eq!(
        desk.workbench()["review"]["review"],
        review["review"]["review"]
    );
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("replaced");
    assert!(support::eventually(|| desk.workbench()["review"].is_null()).await);
    let r = desk
        .wb(json!({"kind": "apply", "review": review["review"]["review"], "id": "del", "revision": doc["revision"], "fields": doc["fields"], "context": desk.context()}))
        .await;
    assert_eq!(r["kind"], "sendRefused", "{r}");
    assert_eq!(
        server
            .seen()
            .iter()
            .filter(|s| s.request_line.starts_with("DELETE"))
            .count(),
        0
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn wire_credentials_are_redacted_wherever_runs_are_kept() {
    let Tenant {
        mut desk,
        server,
        origin,
        ..
    } = tenant().await;
    let root = managed_collection(&origin, &[("read", "GET", "/iam/v1/access_policies")]);
    desk.open(&root).await;
    // A minted bearer echoed back by the service.
    let doc = desk.doc("read.request.toml").await;
    let mut fields = doc["fields"].clone();
    fields["query"] = json!("echo=1");
    let r = desk.send_fields(&doc, &fields).await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    assert!(
        server
            .seen()
            .iter()
            .any(|s| s.header("authorization") == Some(format!("Bearer {TOKEN}").as_str()))
    );
    // A collection Keychain Basic credential echoed back.
    let echo = Http::start(Arc::new(|seen| {
        let auth = seen.header("authorization").unwrap_or_default().to_owned();
        Answer::status(200, &[("x-echo", auth.as_str())], "{}")
    }))
    .await;
    let plain = collection(
        &format!("base = \"{}\"\n", echo.origin()),
        &[(
            "basic.request.toml",
            &request(
                "basic",
                "GET",
                "{{base}}/echo",
                "[auth]\nkind = \"basic\"\nusername = \"operator\"\nsecret = \"pw\"\n",
            ),
        )],
    );
    let basic = b64("operator:SYNTHETIC-BASIC-PASSWORD-c15");
    let mut desk2 = start(None, CoreConfig::default()).await;
    desk2.open(&plain).await;
    desk2.secret("pw", "SYNTHETIC-BASIC-PASSWORD-c15").await;
    let r2 = desk2.send_doc("basic.request.toml").await;
    assert_eq!(
        echo.seen()[0].header("authorization"),
        Some(format!("Basic {basic}").as_str())
    );
    let leaks = |text: &str| text.contains(TOKEN) || text.contains(&basic);
    for (d, r) in [(&mut desk, r), (&mut desk2, r2)] {
        let run = r["run"]["record"]["id"].clone();
        let page = d.wb(json!({"kind": "history", "before": null})).await;
        let shown = d.wb(json!({"kind": "showRun", "run": run})).await;
        for text in [r.to_string(), page.to_string(), shown.to_string()] {
            assert!(!leaks(&text), "{text}");
        }
        assert!(
            r["run"]["record"]["headers"]
                .to_string()
                .contains("[REDACTED]"),
            "{r}"
        );
        for f in files(&d.adapters.workbench_root) {
            let bytes = std::fs::read(&f).expect("read");
            assert!(
                !contains(&bytes, TOKEN.as_bytes()) && !contains(&bytes, basic.as_bytes()),
                "{}",
                f.display()
            );
        }
    }
    // After a restart the kept record is still redacted.
    let mut again = start(Some(&desk2.adapters), CoreConfig::default()).await;
    again.open(&plain).await;
    let page = again.wb(json!({"kind": "history", "before": null})).await;
    assert_eq!(page["runs"].as_array().map(Vec::len), Some(1), "{page}");
    assert!(!leaks(&page.to_string()), "{page}");
}

#[tokio::test(flavor = "multi_thread")]
async fn offline_then_reconnected_sends_nothing_by_itself() {
    // A registered origin on a port that goes offline and comes back.
    let first = Http::start(Arc::new(|seen| {
        if seen.request_line.starts_with("DELETE") {
            Answer::Close
        } else {
            Answer::ok("{}")
        }
    }))
    .await;
    let port = first.port;
    let base = first.origin();
    let config = CoreConfig {
        workbench_api_origins: vec![base.clone()],
        ..CoreConfig::default()
    };
    let mut desk = start(None, config).await;
    let root = collection(
        &format!("base = \"{base}\"\n"),
        &[
            (
                "del.request.toml",
                &request("del", "DELETE", "{{base}}/iam/v1/access_policies/abc", ""),
            ),
            (
                "read.request.toml",
                &request("read", "GET", "{{base}}/iam/v1/access_policies", ""),
            ),
        ],
    );
    desk.open(&root).await;
    let doc = desk.doc("del.request.toml").await;
    let apply = |review: &Value| json!({"kind": "apply", "review": review, "id": "del", "revision": doc["revision"], "fields": doc["fields"], "context": null});
    desk.adapters.confirmer.auto(Some(true));
    // The change goes once and its response is lost: uncertain.
    let first_review = desk.send_doc("del.request.toml").await["review"]["review"].clone();
    let lost = desk.wb(apply(&first_review)).await;
    assert_eq!(lost["run"]["record"]["uncertain"], true, "{lost}");
    assert_eq!(first.seen().len(), 1);
    first.stop().await;
    // Offline: a read fails plainly; a review can still be prepared.
    let r = desk.send_doc("read.request.toml").await;
    assert_eq!(r["run"]["record"]["failure"], "connect", "{r}");
    assert_eq!(r["run"]["record"]["uncertain"], false);
    let second_review = desk.send_doc("del.request.toml").await["review"]["review"].clone();
    // Reconnected: nothing is sent by itself.
    let back = Http::start_on(port, Arc::new(|_| Answer::ok("{}"))).await;
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(back.seen().is_empty(), "no replay on reconnect");
    let page = desk.wb(json!({"kind": "history", "before": null})).await;
    let kept = page["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .find(|r| r["id"] == lost["run"]["record"]["id"])
        .cloned()
        .expect("kept");
    assert_eq!(kept["uncertain"], true, "the uncertain outcome stays");
    // The spent review stays spent; a fresh, explicit Apply and Send work.
    assert_eq!(
        desk.wb(apply(&first_review)).await,
        json!({"kind": "sendRefused", "refusal": {"kind": "reviewConsumed"}})
    );
    assert!(back.seen().is_empty());
    let r = desk.wb(apply(&second_review)).await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    let r = desk.send_doc("read.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    let lines: Vec<String> = back.seen().iter().map(|s| s.request_line.clone()).collect();
    assert_eq!(
        lines,
        [
            "DELETE /iam/v1/access_policies/abc HTTP/1.1",
            "GET /iam/v1/access_policies HTTP/1.1"
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn generated_overrides_and_extra_credentials_never_reach_a_registered_origin() {
    let Tenant {
        mut desk,
        server,
        origin,
        ..
    } = tenant().await;
    let ordinary = Http::start(Arc::new(|_| Answer::status(200, &[], "{}"))).await;
    let key = |name: &str| {
        format!("[auth]\nkind = \"api-key\"\nname = \"{name}\"\nsecret = \"dev-token\"\n")
    };
    let read = "{{scm_base}}/iam/v1/access_policies";
    let root = collection(
        &format!(
            "scm_base = \"{origin}\"\nelsewhere = \"{}\"\nmine = {{ secret = \"dev-token\" }}\n",
            ordinary.origin()
        ),
        &[
            (
                "host.request.toml",
                &request("host", "GET", read, &key("Host")),
            ),
            (
                "method.request.toml",
                &request("method", "GET", read, &key("X-HTTP-Method-Override")),
            ),
            (
                "override.request.toml",
                &request(
                    "override",
                    "GET",
                    read,
                    "[[headers]]\nname = \"X-HTTP-Method-Override\"\nvalue = \"DELETE\"\n",
                ),
            ),
            (
                "double.request.toml",
                &request(
                    "double",
                    "GET",
                    read,
                    "[[headers]]\nname = \"Authorization\"\nvalue = \"Bearer manual\"\n\n[auth]\nkind = \"bearer\"\nsecret = \"dev-token\"\n",
                ),
            ),
            (
                "single.request.toml",
                &request(
                    "single",
                    "GET",
                    read,
                    "[[headers]]\nname = \"Authorization\"\nvalue = \"Bearer manual\"\n",
                ),
            ),
            (
                "generic.request.toml",
                &request(
                    "generic",
                    "GET",
                    "{{elsewhere}}/x",
                    "[[headers]]\nname = \"X-HTTP-Method-Override\"\nvalue = \"DELETE\"\n\n[auth]\nkind = \"api-key\"\nname = \"Host\"\nsecret = \"dev-token\"\n",
                ),
            ),
        ],
    );
    desk.open(&root).await;
    desk.secret("dev-token", TOKEN).await;
    let reads = desk.secret_reads();
    for id in ["host", "method", "override", "double"] {
        let r = desk.send_doc(&format!("{id}.request.toml")).await;
        assert_eq!(
            r,
            json!({"kind": "sendRefused", "refusal": {"kind": "ambiguousCredential"}}),
            "{id}"
        );
    }
    assert_eq!(
        desk.secret_reads(),
        reads,
        "refused before any secret is read"
    );
    assert!(server.seen().is_empty(), "nothing reached the tenant");
    // One manual Authorization still goes, as written.
    let r = desk.send_doc("single.request.toml").await;
    assert_eq!(r["run"]["record"]["state"], "completed", "{r}");
    assert_eq!(server.seen().len(), 1);
    assert_eq!(
        server.seen()[0].header("authorization"),
        Some("Bearer manual")
    );
    // Ordinary HTTP elsewhere is the operator's own.
    let r = desk.send_doc("generic.request.toml").await;
    assert_eq!(r["run"]["record"]["status"], 200, "{r}");
    assert_eq!(ordinary.seen().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reviews_are_bound_to_the_credentials_and_cookies_actually_sent() {
    let Tenant {
        mut desk,
        server,
        origin,
        ..
    } = tenant().await;
    let target = "{{scm_base}}/iam/v1/access_policies/abc";
    let basic = |user: &str| {
        format!("[auth]\nkind = \"basic\"\nusername = \"{user}\"\nsecret = \"dev-token\"\n")
    };
    let bearer = |secret: &str| format!("[auth]\nkind = \"bearer\"\nsecret = \"{secret}\"\n");
    let root = collection(
        &format!("scm_base = \"{origin}\"\n"),
        &[
            (
                "basic.request.toml",
                &request("basic", "DELETE", target, &basic("alice")),
            ),
            (
                "bearer.request.toml",
                &request("bearer", "DELETE", target, &bearer("dev-token")),
            ),
            ("bare.request.toml", &request("bare", "DELETE", target, "")),
            (
                "one.request.toml",
                &request(
                    "one",
                    "GET",
                    "{{scm_base}}/iam/v1/access_policies?login=one",
                    "",
                ),
            ),
            (
                "two.request.toml",
                &request(
                    "two",
                    "GET",
                    "{{scm_base}}/iam/v1/access_policies?login=two",
                    "",
                ),
            ),
        ],
    );
    desk.open(&root).await;
    desk.secret("dev-token", "first-value").await;
    desk.secret("other-token", "other-value").await;
    let deletes = |server: &TlsServer| {
        server
            .seen()
            .iter()
            .filter(|s| s.request_line.starts_with("DELETE"))
            .cloned()
            .collect::<Vec<_>>()
    };
    let stale = json!({"kind": "sendRefused", "refusal": {"kind": "reviewStale"}});
    desk.adapters.confirmer.auto(Some(true));
    // Reviewed, then Applied with another username or secret reference.
    for (id, changed) in [("basic", basic("bob")), ("bearer", bearer("other-token"))] {
        let doc = desk.doc(&format!("{id}.request.toml")).await;
        let review = desk.send_doc(&format!("{id}.request.toml")).await;
        assert_eq!(review["kind"], "reviewRequired", "{review}");
        let r = desk
            .wb(
                json!({"kind": "apply", "review": review["review"]["review"], "id": id,
                       "revision": doc["revision"], "fields": with(&doc, "settings", &changed),
                       "context": desk.context()}),
            )
            .await;
        assert_eq!(r, stale, "{id}");
    }
    // The referenced secret replaced after review, or while the native
    // confirmation is held.
    let doc = desk.doc("bearer.request.toml").await;
    let apply = |review: &Value, context: Option<String>| {
        json!({"kind": "apply", "review": review["review"]["review"], "id": "bearer",
               "revision": doc["revision"], "fields": doc["fields"], "context": context})
    };
    let review = desk.send_doc("bearer.request.toml").await;
    desk.secret("dev-token", "second-value").await;
    let r = desk.wb(apply(&review, desk.context())).await;
    assert_eq!(r, stale, "replaced after review");
    let review = desk.send_doc("bearer.request.toml").await;
    desk.adapters.confirmer.auto(None);
    let op = desk.start_wb(apply(&review, desk.context())).await;
    assert!(support::eventually(|| desk.adapters.confirmer.held() == 1).await);
    desk.secret("dev-token", "third-value").await;
    assert!(desk.adapters.confirmer.complete(true));
    assert_eq!(desk.wait(op).await, stale, "replaced while confirming");
    desk.adapters.confirmer.auto(Some(true));
    // Cookies: a new session cookie or a cleared jar after review.
    assert_eq!(
        desk.send_doc("one.request.toml").await["run"]["record"]["status"],
        200
    );
    let bare = desk.doc("bare.request.toml").await;
    let apply_bare = |review: &Value, context: Option<String>| {
        json!({"kind": "apply", "review": review["review"]["review"], "id": "bare",
               "revision": bare["revision"], "fields": bare["fields"], "context": context})
    };
    let review = desk.send_doc("bare.request.toml").await;
    assert_eq!(
        desk.send_doc("two.request.toml").await["run"]["record"]["status"],
        200
    );
    assert_eq!(
        desk.wb(apply_bare(&review, desk.context())).await,
        stale,
        "new cookie"
    );
    let review = desk.send_doc("bare.request.toml").await;
    assert_eq!(
        desk.wb(json!({"kind": "clearCookies"})).await["kind"],
        "done"
    );
    assert_eq!(
        desk.wb(apply_bare(&review, desk.context())).await,
        stale,
        "cleared"
    );
    assert!(
        deletes(&server).is_empty(),
        "nothing went under an old approval"
    );
    // Unchanged, each goes once with exactly what was reviewed.
    assert_eq!(
        desk.send_doc("one.request.toml").await["run"]["record"]["status"],
        200
    );
    let review = desk.send_doc("bare.request.toml").await;
    let r = desk.wb(apply_bare(&review, desk.context())).await;
    assert_eq!(r["kind"], "ran", "{r}");
    let review = desk.send_doc("basic.request.toml").await;
    let doc = desk.doc("basic.request.toml").await;
    let r = desk
        .wb(json!({"kind": "apply", "review": review["review"]["review"], "id": "basic",
                   "revision": doc["revision"], "fields": doc["fields"], "context": desk.context()}))
        .await;
    assert_eq!(r["kind"], "ran", "{r}");
    let sent = deletes(&server);
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[0].header("cookie"), Some("sid=one"));
    assert_eq!(
        sent[1].header("authorization"),
        Some(format!("Basic {}", b64("alice:third-value")).as_str())
    );
}
