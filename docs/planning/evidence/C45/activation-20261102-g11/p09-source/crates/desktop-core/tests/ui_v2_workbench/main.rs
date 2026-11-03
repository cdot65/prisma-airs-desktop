//! C33: API Client and Test Cases in the v2 shell, and "Show the calls",
//! through the production authority interface against loopback fixture
//! servers (screens/api-client-test-cases.md; invariant I5): an API
//! response kept in history never changes after a later configuration
//! change, while a new send reflects it (V2-R05, V2-T05); starter packs are
//! named and not run until a Playground scan opened from the case records
//! its verdict, only while the case still holds what was scanned; and the
//! calls made on the tenant's behalf are shown only while chosen, without
//! any secret, query or body.

#[path = "../workbench_execution/server.rs"]
#[allow(dead_code)]
mod server;
#[path = "../context_credentials/support.rs"]
#[allow(dead_code)]
mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::ManagementEndpoints;
use desktop_core::runtime::OperationId;
use desktop_core::store::PersistedState;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, LibraryState, Observation, Receipt,
    Refusal, ScanState, ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};
use server::{Answer, Http};
use sha2::{Digest, Sha256};
use support::{Reply, Seen, TestCa, TlsServer, eventually};

const SCAN_KEY: &str = "SYNTHETIC-SCAN-KEY-c33-0001";
const CLIENT_ID: &str = "c33@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "SYNTHETIC-CLIENT-SECRET-c33";
const TSG: &str = "1234567890";

struct Desk {
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

    fn json(&self) -> Value {
        serde_json::to_value(self.snap()).expect("json")
    }

    fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    async fn open(&mut self, root: &Path) {
        self.adapters.folders.reply(Some(root.to_path_buf()));
        let r = self.wb(json!({"kind": "openFolder"})).await;
        assert_eq!(r["kind"], "opened", "{r}");
    }

    async fn send_doc(&mut self, relative: &str) -> Value {
        let r = self
            .wb(json!({"kind": "openRequest", "relative": relative}))
            .await;
        assert_eq!(r["kind"], "document", "{r}");
        let doc = r["document"].clone();
        self.wb(json!({"kind": "send", "id": doc["id"], "revision": doc["revision"], "fields": doc["fields"], "context": null}))
            .await
    }

    async fn show(&mut self, run: &Value) -> Value {
        self.wb(json!({"kind": "showRun", "run": run})).await
    }

    async fn open_library(&mut self) {
        let context = self.context();
        self.send(json!({"kind": "openLibrary", "context": context}))
            .await
            .expect("open");
        assert!(eventually(|| self.snap().library.state == LibraryState::Ready).await);
    }

    fn cases(&self) -> Vec<Value> {
        self.json()["library"]["cases"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }
}

async fn start(adapters: (desktop_core::Adapters, Handles), config: CoreConfig) -> Desk {
    let (adapters, handles) = adapters;
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

fn temp(name: &str) -> PathBuf {
    let mut n = [0u8; 8];
    getrandom::fill(&mut n).expect("fixture");
    let p = std::env::temp_dir()
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-v2-c33-{name}-{}", hex::encode(n)));
    std::fs::create_dir_all(&p).expect("fixture");
    p
}

/// A collection inside a Git repository with one request.
fn collection(origin: &str) -> PathBuf {
    let repo = temp("repo");
    std::fs::create_dir(repo.join(".git")).expect("fixture");
    let root = repo.join("api");
    std::fs::create_dir_all(&root).expect("fixture");
    std::fs::write(
        root.join("collection.toml"),
        format!("format_version = 1\nid = \"c33\"\nname = \"Topics\"\n\n[variables]\nbase = \"{origin}\"\n"),
    )
    .expect("fixture");
    std::fs::write(
        root.join("topics.request.toml"),
        "format_version = 1\nid = \"topics\"\nmethod = \"GET\"\nurl = \"{{base}}/topics\"\n",
    )
    .expect("fixture");
    root
}

fn sha(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

#[tokio::test(flavor = "multi_thread")]
async fn an_api_response_in_history_never_changes_while_a_new_send_reflects_the_change() {
    // V2-R05, V2-T05 (I5): a topic applied after the first send.
    let topics = Arc::new(Mutex::new(vec!["Finance"]));
    let served = Arc::clone(&topics);
    let http = Http::start(Arc::new(move |_| {
        let names = served.lock().expect("topics").clone();
        Answer::ok(&json!({"custom_topics": names}).to_string())
    }))
    .await;
    let root = collection(&http.origin());
    let mut desk = start(fixtures::adapters(), CoreConfig::default()).await;
    desk.open(&root).await;
    let first = desk.send_doc("topics.request.toml").await;
    let first_run = first["run"]["record"]["id"].clone();
    let before = desk.show(&first_run).await;
    let kept = before["run"]["preview"]
        .as_str()
        .expect("preview")
        .to_owned();
    assert_eq!(kept, r#"{"custom_topics":["Finance"]}"#);
    // The topic is applied: the service now answers with it.
    topics.lock().expect("topics").push("Travel");
    let second = desk.send_doc("topics.request.toml").await;
    let second_run = second["run"]["record"]["id"].clone();
    assert_ne!(second_run, first_run);
    let after = desk.show(&second_run).await;
    assert_eq!(
        after["run"]["preview"],
        r#"{"custom_topics":["Finance","Travel"]}"#
    );
    // The earlier response is exactly as it was, record and body.
    let again = desk.show(&first_run).await;
    assert_eq!(
        sha(again["run"]["preview"].as_str().expect("preview")),
        sha(&kept)
    );
    assert_eq!(again["run"]["record"], before["run"]["record"]);
    let page = desk.wb(json!({"kind": "history", "before": null})).await;
    let runs = page["runs"].as_array().expect("runs");
    assert_eq!(runs.len(), 2, "{page}");
    // A restart reads the same snapshot again; nothing is resent.
    let handles = desk.adapters;
    drop(desk.core);
    let (mut adapters, mut kept_handles) = fixtures::adapters();
    adapters.keychain = handles.vault.clone();
    kept_handles.vault = handles.vault.clone();
    adapters.workbench_root = Some(handles.workbench_root.clone());
    kept_handles.workbench_root = handles.workbench_root.clone();
    let mut again = start((adapters, kept_handles), CoreConfig::default()).await;
    again.open(&root).await;
    let reread = again.show(&first_run).await;
    assert_eq!(reread["run"]["preview"].as_str().map(sha), Some(sha(&kept)));
    assert_eq!(http.seen().len(), 2);
}

/// What the synthetic Runtime and management endpoints answer.
struct Script {
    action: &'static str,
    scan_id: String,
}

/// A long response header name the service may send: one past the call
/// record's bound, and one at it (attempt-01 R2).
fn long_header(n: usize) -> String {
    format!("x-{}", "l".repeat(n - 2))
}

fn raw_with(status: u16, body: &Value, extra: &str) -> Reply {
    let body = serde_json::to_vec(body).expect("json");
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\n{extra}connection: close",
            body.len()
        ),
        body,
    }
}

fn raw(status: u16, body: &Value) -> Reply {
    let body = serde_json::to_vec(body).expect("json");
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}

impl Script {
    fn route(&self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        match path.as_str() {
            "/v1/scan/sync/request" => raw_with(
                200,
                &json!({"report_id": "R-c33", "scan_id": self.scan_id, "tr_id": "T-1",
                    "profile_name": "Production guardrails", "category": if self.action == "block" { "malicious" } else { "benign" },
                    "action": self.action, "timeout": false, "error": false, "errors": [],
                    "prompt_detected": {"injection": self.action == "block"}, "response_detected": {}}),
                &format!("{}: a\r\n{}: b\r\n", long_header(65), long_header(64)),
            ),
            "/oauth2/access_token" => raw(
                200,
                &json!({"access_token": "SYNTHETIC-ACCESS-c33", "token_type": "Bearer", "expires_in": 3600}),
            ),
            p if p == format!("/aisec/v1/mgmt/apikeys/tsg/{TSG}") => {
                raw(200, &json!({"api_keys": []}))
            }
            _ => raw(404, &json!({"error": "not found"})),
        }
    }
}

struct Tenant {
    desk: Desk,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
    ca: TestCa,
}

async fn tenant_with(
    previous: Option<(Handles, PersistedState)>,
    ca: TestCa,
    server: TlsServer,
    script: Arc<Mutex<Script>>,
) -> Tenant {
    let origin = format!("https://localhost:{}", server.port);
    let adapters = match previous {
        None => fixtures::adapters(),
        Some((old, state)) => {
            let (mut adapters, mut handles) =
                fixtures::adapters_with(fixtures::ScriptedStore::with_state(state));
            adapters.keychain = old.vault.clone();
            handles.vault = old.vault;
            adapters.library_root = Some(old.library_root.clone());
            handles.library_root = old.library_root;
            (adapters, handles)
        }
    };
    let fresh = adapters.1.store.saved().is_none();
    let config = CoreConfig {
        management: ManagementEndpoints::at_origin(&origin),
        ..CoreConfig::default()
    };
    let desk = start(adapters, config).await;
    desk.core
        .set_transport_policy(TransportPolicy {
            revision: 2,
            proxy: ProxyMode::Direct,
            extra_roots: vec![ca.pem.as_bytes().to_vec()],
        })
        .expect("policy");
    let mut t = Tenant {
        desk,
        script,
        server,
        ca,
    };
    if fresh {
        t.desk
            .send(
                json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
                "destination": {"kind": "custom", "origin": origin}}}),
            )
            .await
            .expect("tenant");
        let context = t.desk.context();
        t.desk
            .send(json!({"kind": "configureScanCredential", "context": context, "mode": "keychain", "key": SCAN_KEY}))
            .await
            .expect("scan key");
        assert!(eventually(|| t.desk.snap().tenants[0].scan.state == ScanState::Ready).await);
        let context = t.desk.context();
        t.desk
            .send(json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
                "clientId": CLIENT_ID, "secret": CLIENT_SECRET}))
            .await
            .expect("management key");
        assert!(eventually(|| t.desk.snap().tenants[0].management.state == ScanState::Ready).await);
    }
    t
}

async fn tenant() -> Tenant {
    let ca = TestCa::new("airs-v2 c33 ca");
    let script = Arc::new(Mutex::new(Script {
        action: "block",
        scan_id: "S-c33-1".into(),
    }));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("script").route(seen)
        })),
    )
    .await;
    tenant_with(None, ca, server, script).await
}

/// Scan requests the synthetic Runtime endpoint has received.
fn scan_requests(t: &Tenant) -> usize {
    t.server
        .seen()
        .iter()
        .filter(|s| s.request_line.contains("/v1/scan/sync/request"))
        .count()
}

impl Tenant {
    async fn scan(&mut self, prompt: &str, response: &str, case: Option<&str>) -> Value {
        let context = self.desk.context();
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = self
            .desk
            .send(
                json!({"kind": "scan", "context": context, "case": case, "draft": {
                "profile": {"kind": "name", "name": "Production guardrails"},
                "prompt": prompt, "response": response, "appName": "Desktop examples"}}),
            )
            .await
            .expect("admitted")
        else {
            panic!("no operation");
        };
        let id = operation.as_str().to_owned();
        assert!(
            eventually(|| self.desk.json()["scans"].as_array().is_some_and(|s| s
                .iter()
                .any(|r| r["operation"] == id && r["progress"] == "settled")))
            .await
        );
        self.desk.json()["scans"]
            .as_array()
            .and_then(|s| s.iter().find(|r| r["operation"] == id).cloned())
            .expect("scan")
    }

    async fn restart(self) -> Tenant {
        let state = self.desk.adapters.store.saved().expect("saved");
        let Tenant {
            desk,
            script,
            server,
            ca,
        } = self;
        let handles = desk.adapters;
        desk.core.shutdown();
        drop(desk.core);
        tenant_with(Some((handles, state)), ca, server, script).await
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn starter_packs_are_named_and_a_playground_scan_records_a_case_result_only_while_it_holds() {
    let mut t = tenant().await;
    t.desk.open_library().await;
    let context = t.desk.context();
    t.desk
        .send(json!({"kind": "importExamples", "context": context, "source": "bundled"}))
        .await
        .expect("packs");
    assert!(eventually(|| !t.desk.cases().is_empty()).await);
    let cases = t.desk.cases();
    // Every starter case carries its product's pack name and no result: "Not run".
    for c in &cases {
        let pack = c["pack"].as_str().expect("pack");
        assert!(pack.ends_with("starter pack"), "{c}");
        assert!(c.get("result").is_none(), "{c}");
    }
    let runtime = cases
        .iter()
        .find(|c| c["product"] == "runtime-security")
        .expect("runtime case")
        .clone();
    assert_eq!(runtime["pack"], "Runtime Security starter pack");
    let id = runtime["id"].as_str().expect("id").to_owned();
    let prompt = runtime["prompt"].as_str().expect("prompt").to_owned();
    let response = runtime["response"].as_str().expect("response").to_owned();
    // A scan opened from the case, of what the case holds: its verdict is recorded.
    let scan = t.scan(&prompt, &response, Some(&id)).await;
    assert_eq!(scan["result"]["verdict"], "block", "{scan}");
    assert!(
        eventually(|| t
            .desk
            .cases()
            .iter()
            .any(|c| c["id"] == id.as_str() && c["result"]["verdict"] == "block"))
        .await
    );
    let recorded = t
        .desk
        .cases()
        .into_iter()
        .find(|c| c["id"] == id.as_str())
        .expect("case");
    assert_eq!(recorded["result"]["scanId"], "S-c33-1");
    // A scan of something else (the prompt was changed in the Playground)
    // records nothing for the case, whatever its verdict.
    t.script.lock().expect("script").action = "allow";
    let other = t
        .scan(&format!("{prompt} (edited)"), &response, Some(&id))
        .await;
    assert_eq!(other["result"]["verdict"], "allow");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let still = t
        .desk
        .cases()
        .into_iter()
        .find(|c| c["id"] == id.as_str())
        .expect("case");
    assert_eq!(still["result"]["verdict"], "block");
    // Never a result for a case of another product, and a malformed case is refused.
    let spec = cases
        .iter()
        .find(|c| c["product"] != "runtime-security")
        .expect("spec case")
        .clone();
    // A specification named with its own exact input: refused before anything
    // is sent (attempt-01 R1), as is a case this library does not hold.
    let requests = scan_requests(&t);
    let context = t.desk.context();
    for (case, prompt, response) in [
        (
            spec["id"].as_str().expect("id").to_owned(),
            spec["prompt"].as_str().expect("prompt").to_owned(),
            spec["response"].as_str().expect("response").to_owned(),
        ),
        (
            "00000000-0000-4000-8000-00000000dead".to_owned(),
            prompt.clone(),
            response.clone(),
        ),
    ] {
        assert_eq!(
            t.desk
                .send(
                    json!({"kind": "scan", "context": context, "case": case, "draft": {
                    "profile": {"kind": "name", "name": "Production guardrails"},
                    "prompt": prompt, "response": response, "appName": "Desktop examples"}})
                )
                .await
                .err(),
            Some(Refusal::InvalidInput),
            "{case}"
        );
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(scan_requests(&t), requests, "nothing was sent");
    assert!(
        t.desk
            .cases()
            .iter()
            .find(|c| c["id"] == spec["id"])
            .is_some_and(|c| c.get("result").is_none())
    );
    let context = t.desk.context();
    assert!(
        t.desk
            .send(
                json!({"kind": "scan", "context": context, "case": "not-a-case", "draft": {
                "profile": {"kind": "name", "name": "Production guardrails"},
                "prompt": "x", "response": "", "appName": "Desktop examples"}})
            )
            .await
            .is_err()
    );
    // The result and pack survive a restart; an export carries neither.
    let mut t = t.restart().await;
    t.desk.open_library().await;
    let kept = t
        .desk
        .cases()
        .into_iter()
        .find(|c| c["id"] == id.as_str())
        .expect("case");
    assert_eq!(kept["result"]["verdict"], "block");
    assert_eq!(kept["pack"], "Runtime Security starter pack");
    assert!(
        t.desk
            .cases()
            .iter()
            .filter(|c| c["product"] != "runtime-security")
            .all(|c| c.get("result").is_none())
    );
    let case = desktop_core::library::TestCase {
        id: id.clone(),
        tenant: "t".into(),
        product: desktop_core::library::CaseProduct::RuntimeSecurity,
        name: "n".into(),
        profile: "p".into(),
        prompt: "x".into(),
        response: String::new(),
        app_name: "a".into(),
        expected: String::new(),
        payload: Value::Null,
        pack: Some("Runtime Security starter pack".into()),
        result: Some(desktop_core::library::CaseResult {
            verdict: desktop_core::scanning::Verdict::Block,
            at: 1,
            scan_id: Some("S".into()),
        }),
    };
    let file = desktop_core::library::export("t", &[case]).expect("export");
    let text = String::from_utf8(file).expect("utf8");
    assert!(!text.contains("pack") && !text.contains("result"), "{text}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_calls_are_shown_only_while_chosen_and_never_carry_a_secret_query_or_body() {
    let mut t = tenant().await;
    // A tenant exists, so onboarding is done: not chosen, the calls are hidden.
    assert!(!t.desk.snap().show_calls);
    assert!(t.desk.json().get("calls").is_none());
    t.desk
        .send(json!({"kind": "setShowCalls", "on": true}))
        .await
        .expect("on");
    assert_eq!(t.desk.json()["calls"], json!([]));
    let prompt = "SYNTHETIC-PROMPT-c33-never-shown";
    let _ = t.scan(prompt, "", None).await;
    let calls = t.desk.json()["calls"].clone();
    let scan = calls
        .as_array()
        .and_then(|c| c.iter().find(|c| c["purpose"] == "scan"))
        .expect("scan call")
        .clone();
    assert_eq!(scan["method"], "POST");
    assert_eq!(scan["path"], "/v1/scan/sync/request");
    assert_eq!(scan["status"], 200);
    assert_eq!(scan["credential"], "scanKey");
    assert_eq!(scan["query"], false);
    assert_eq!(scan["contentType"], "application/json");
    assert!(
        scan["requestHeaders"]
            .as_array()
            .is_some_and(|h| h.iter().any(|n| n == "x-pan-token")),
        "{scan}"
    );
    assert!(scan["requestBytes"].as_u64().is_some_and(|b| b > 0));
    // A header name past the bound is left out; one at it is kept; none longer.
    let names: Vec<&str> = scan["responseHeaders"]
        .as_array()
        .expect("names")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(names.contains(&long_header(64).as_str()), "{names:?}");
    assert!(!names.contains(&long_header(65).as_str()), "{names:?}");
    assert!(names.iter().all(|n| n.len() <= 64));
    // A management read: its token request and the read, both by the client credential.
    let context = t.desk.context();
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = t
        .desk
        .send(json!({"kind": "management", "context": context, "request": {"kind": "page", "resource": "apiKeys", "offset": 0, "limit": 5}}))
        .await
        .expect("admitted")
    else {
        panic!("no operation");
    };
    let Ok(Observation::Management(handle)) =
        t.desk.handle.observe(ViewScope::Management(operation))
    else {
        panic!("observation");
    };
    tokio::time::timeout(Duration::from_secs(20), handle.result())
        .await
        .expect("result");
    assert!(
        eventually(|| t.desk.json()["calls"].as_array().is_some_and(|c| c
            .iter()
            .any(|c| c["path"] == format!("/aisec/v1/mgmt/apikeys/tsg/{TSG}"))))
        .await
    );
    let calls = t.desk.json()["calls"].clone();
    let read = calls
        .as_array()
        .and_then(|c| {
            c.iter()
                .find(|c| c["path"] == format!("/aisec/v1/mgmt/apikeys/tsg/{TSG}"))
        })
        .expect("read")
        .clone();
    assert_eq!(read["purpose"], "management");
    assert_eq!(read["credential"], "managementClient");
    assert_eq!(read["query"], true, "its query is withheld, never shown");
    assert!(
        calls
            .as_array()
            .is_some_and(|c| c.iter().any(|c| c["path"] == "/oauth2/access_token"))
    );
    // Nothing secret, no query, no body, anywhere in the snapshot.
    let text = t.desk.json().to_string();
    for secret in [
        SCAN_KEY,
        CLIENT_SECRET,
        "SYNTHETIC-ACCESS-c33",
        prompt,
        "offset=",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    let sequences: Vec<u64> = calls
        .as_array()
        .expect("calls")
        .iter()
        .filter_map(|c| c["sequence"].as_u64())
        .collect();
    assert!(sequences.windows(2).all(|w| w[0] < w[1]), "newest last");
    // Off: no calls in the snapshot; the choice outlives a restart.
    t.desk
        .send(json!({"kind": "setShowCalls", "on": false}))
        .await
        .expect("off");
    assert!(t.desk.json().get("calls").is_none());
    let t = t.restart().await;
    assert!(!t.desk.snap().show_calls);
    let mut t = t;
    t.desk
        .send(json!({"kind": "setShowCalls", "on": true}))
        .await
        .expect("on");
    let t = t.restart().await;
    assert!(t.desk.snap().show_calls);
    // A fresh run's log is empty: calls are never stored.
    assert_eq!(t.desk.json()["calls"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn before_onboarding_the_calls_are_shown_by_default() {
    let desk = start(fixtures::adapters(), CoreConfig::default()).await;
    // No tenant yet: first run, the calls are on until onboarding completes.
    assert!(desk.snap().show_calls);
    // No active tenant: nothing to show.
    assert!(desk.json().get("calls").is_none());
}

/// The tenant's sealed library files (not a write's temporary file).
fn library_files(t: &Tenant) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(&t.desk.adapters.library_root)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn unsaved_packs_and_results_stay_shown_and_a_retry_keeps_them_across_a_restart() {
    // attempt-01 R4: a failed protected write of a pack or a case result is
    // shown as unsaved, the work stays visible, and Save again keeps it.
    let mut t = tenant().await;
    t.desk.open_library().await;
    let context = t.desk.context();
    t.desk
        .send(
            json!({"kind": "saveCase", "context": context, "draft": {"product": "runtime-security",
            "name": "Kept first", "profile": "prod", "prompt": "First input", "response": "",
            "appName": "App", "expected": "", "payload": ""}}),
        )
        .await
        .expect("saved");
    assert!(eventually(|| library_files(&t).len() == 1).await);
    // A directory stands where the sealed file goes: every write fails.
    let file = library_files(&t)[0].clone();
    std::fs::remove_file(&file).expect("remove");
    std::fs::create_dir_all(file.join("blocker")).expect("blocker");
    t.desk
        .send(json!({"kind": "importExamples", "context": context, "source": "bundled"}))
        .await
        .expect("packs");
    assert!(eventually(|| t.desk.json()["library"]["unsaved"].is_string()).await);
    let runtime = t
        .desk
        .cases()
        .into_iter()
        .find(|c| c["pack"] == "Runtime Security starter pack")
        .expect("pack shown");
    let id = runtime["id"].as_str().expect("id").to_owned();
    let _ = t
        .scan(
            runtime["prompt"].as_str().expect("prompt"),
            runtime["response"].as_str().expect("response"),
            Some(&id),
        )
        .await;
    assert!(
        eventually(|| t
            .desk
            .cases()
            .iter()
            .any(|c| c["id"] == id.as_str() && c["result"]["verdict"] == "block"))
        .await,
        "the result is shown though not saved"
    );
    assert!(t.desk.json()["library"]["unsaved"].is_string());
    // Unblocked, an explicit retry saves everything shown.
    std::fs::remove_dir_all(&file).expect("unblock");
    t.desk
        .send(json!({"kind": "saveLibraryAgain", "context": context}))
        .await
        .expect("retried");
    assert!(eventually(|| t.desk.json()["library"]["unsaved"].is_null()).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut t = t.restart().await;
    t.desk.open_library().await;
    let cases = t.desk.cases();
    assert!(cases.iter().any(|c| c["name"] == "Kept first"));
    assert!(
        cases
            .iter()
            .any(|c| c["pack"] == "AI Red Teaming starter pack")
    );
    assert!(
        cases
            .iter()
            .any(|c| c["id"] == id.as_str() && c["result"]["verdict"] == "block")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scan_without_an_identifier_is_recorded_without_one_and_stays_readable() {
    // attempt-02 R1: the service's empty (or control-only) scan identifier
    // is never invented: the result is recorded without one.
    let mut t = tenant().await;
    t.desk.open_library().await;
    let context = t.desk.context();
    t.desk
        .send(json!({"kind": "importExamples", "context": context, "source": "bundled"}))
        .await
        .expect("packs");
    assert!(eventually(|| !t.desk.cases().is_empty()).await);
    let runtime: Vec<Value> = t
        .desk
        .cases()
        .into_iter()
        .filter(|c| c["product"] == "runtime-security")
        .take(2)
        .collect();
    for (case, id) in runtime.iter().zip(["", "\u{1}\u{2}"]) {
        t.script.lock().expect("script").scan_id = id.into();
        let case_id = case["id"].as_str().expect("id").to_owned();
        let _ = t
            .scan(
                case["prompt"].as_str().expect("prompt"),
                case["response"].as_str().expect("response"),
                Some(&case_id),
            )
            .await;
        assert!(
            eventually(|| t
                .desk
                .cases()
                .iter()
                .any(|c| c["id"] == case_id.as_str() && c["result"]["verdict"] == "block"))
            .await
        );
        let shown = t
            .desk
            .cases()
            .into_iter()
            .find(|c| c["id"] == case_id.as_str())
            .expect("case");
        assert!(shown["result"].get("scanId").is_none(), "{shown}");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut t = t.restart().await;
    t.desk.open_library().await;
    let kept: Vec<Value> = t
        .desk
        .cases()
        .into_iter()
        .filter(|c| runtime.iter().any(|r| r["id"] == c["id"]))
        .collect();
    assert_eq!(kept.len(), 2);
    assert!(
        kept.iter()
            .all(|c| c["result"]["verdict"] == "block" && c["result"].get("scanId").is_none())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_result_with_no_room_in_a_full_library_is_said_and_every_case_survives() {
    // attempt-02 R2: near 256 KiB, a case result that would not fit is not
    // recorded; the library says so and stays readable across a restart.
    let mut t = tenant().await;
    t.desk.open_library().await;
    let context = t.desk.context();
    let save = |name: &str, prompt: String| {
        json!({"kind": "saveCase", "context": context, "draft": {
        "product": "runtime-security", "name": name, "profile": "prod", "prompt": prompt,
        "response": "", "appName": "App", "expected": "", "payload": ""}})
    };
    for (n, name) in ["Filler one", "Filler two"].iter().enumerate() {
        t.desk
            .send(save(name, "f".repeat(90_000)))
            .await
            .expect("saved");
        assert!(eventually(|| t.desk.cases().len() == n + 1).await);
    }
    assert!(eventually(|| library_files(&t).len() == 1).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    // The exact bytes the loader will measure, read with the library's own reader.
    let tenant_id = t.desk.snap().tenants[0].id.as_str().to_owned();
    let stored = desktop_core::library::read(
        &t.desk.adapters.library_root,
        &*t.desk.adapters.vault,
        &tenant_id,
    )
    .expect("read");
    let size = serde_json::to_vec(&stored.cases).expect("json").len();
    let probe = |prompt: &str| {
        serde_json::to_vec(&desktop_core::library::TestCase {
            id: "00000000-0000-4000-8000-000000000000".into(),
            tenant: tenant_id.clone(),
            product: desktop_core::library::CaseProduct::RuntimeSecurity,
            name: "Edge".into(),
            profile: "prod".into(),
            prompt: prompt.into(),
            response: String::new(),
            app_name: "App".into(),
            expected: String::new(),
            payload: Value::Null,
            pack: None,
            result: None,
        })
        .expect("json")
        .len()
    };
    let overhead = probe("");
    // 20 bytes short of the limit: no room for a result.
    let length = desktop_core::library::LIBRARY_BYTES - size - 1 - overhead - 20;
    let prompt = "e".repeat(length);
    t.desk
        .send(save("Edge", prompt.clone()))
        .await
        .expect("saved");
    assert!(eventually(|| t.desk.cases().len() == 3).await);
    let edge = t
        .desk
        .cases()
        .into_iter()
        .find(|c| c["name"] == "Edge")
        .expect("edge");
    let id = edge["id"].as_str().expect("id").to_owned();
    let _ = t.scan(&prompt, "", Some(&id)).await;
    assert!(eventually(|| t.desk.json()["library"]["problem"] == "tooLarge").await);
    assert!(
        t.desk.cases().iter().all(|c| c.get("result").is_none()),
        "nothing recorded"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut t = t.restart().await;
    t.desk.open_library().await;
    let names: Vec<String> = t
        .desk
        .cases()
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(names, ["Filler one", "Filler two", "Edge"]);
    assert_eq!(t.desk.json()["library"]["state"], "ready");
}
