//! C19 native Harness lifecycle through the production authority interface.
//!
//! The real pinned engine (extracted from the sealed archive and verified
//! against the compiled pin) runs conversations against a synthetic TLS
//! Gateway that streams Responses. A scripted stand-in
//! (`harness-fixture-sidecar`) produces the failures the engine cannot be
//! made to: malformed, oversized and duplicate frames, a refused or silent
//! bootstrap, a crash mid-answer, ignoring the end of its input, a
//! descendant, an approval request and a flood. Every conversation's
//! process is checked gone and its runtime home removed.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use desktop_core::fixtures::sidecar::ProcessLauncher;
use desktop_core::fixtures::{self, Handles, VaultCall};
use desktop_core::harness::engine::{self, Engine, Pin};
use desktop_core::harness::session::ENV_NAMES;
use desktop_core::harness::sidecar::{FixtureEngine, HarnessAdapters};
use desktop_core::management::ManagementEndpoints;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal, ScanState,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::support::{Reply, Seen, TestCa, TlsServer};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c19";
const KEY: &str = "gw-CANARY-harness-key-c19";
const OTHER_KEY: &str = "gw-CANARY-harness-replacement";
const TSG: &str = "1234567890";
const DATA: &str = "/ai_gw/v2";
const RUNTIME: &str = "/rt/v1";
const WS_A: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";
const WS_B: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8298";
const PROMPT: &str = "Say hello to the C19 canary.";
const ANSWER: &str = "Hello from the synthetic Gateway.";

// ---------------------------------------------------------------- engine

/// The real engine, extracted once from the sealed archive into this test
/// target's temporary directory and verified against the compiled pin.
fn real_engine() -> Engine {
    static EXTRACTED: OnceLock<PathBuf> = OnceLock::new();
    let path = EXTRACTED.get_or_init(|| {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("fixture");
        let record: Value = serde_json::from_slice(
            &std::fs::read(repo.join("resources/harness/pin.json")).expect("fixture"),
        )
        .expect("fixture");
        let archive = repo.join(record["archive"].as_str().expect("fixture"));
        let bytes = std::fs::read(&archive).expect("fixture");
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            record["archiveSha256"].as_str().expect("fixture"),
            "the sealed archive is its pinned bytes (fetch Git LFS objects)"
        );
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .canonicalize()
            .expect("fixture");
        let dest = base.join(format!(
            "harness-{}",
            &record["archiveSha256"].as_str().expect("fixture")[..16]
        ));
        let exe = dest.join("harness").join("airs-harness");
        if !exe.is_file() {
            let staging = base.join(format!("harness-staging-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&staging);
            std::fs::create_dir_all(&staging).expect("fixture");
            let status = std::process::Command::new("/usr/bin/tar")
                .arg("-xzf")
                .arg(&archive)
                .arg("-C")
                .arg(&staging)
                .env_clear()
                .status()
                .expect("fixture");
            assert!(status.success());
            let _ = std::fs::remove_dir_all(&dest);
            std::fs::rename(&staging, &dest).expect("fixture");
        }
        exe.canonicalize().expect("fixture")
    });
    let engine = Engine {
        executable: path.clone(),
    };
    engine::verify(&engine, &engine::pin()).expect("the extracted engine is the pinned one");
    engine
}

/// The scripted stand-in, copied as `airs-harness`, with its own pin.
fn fixture_engine() -> (Engine, Pin) {
    static COPIED: OnceLock<PathBuf> = OnceLock::new();
    let path = COPIED.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .canonicalize()
            .expect("fixture")
            .join(format!("harness-fixture-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("fixture");
        let exe = dir.join("airs-harness");
        std::fs::copy(env!("CARGO_BIN_EXE_harness-fixture-sidecar"), &exe).expect("fixture");
        exe
    });
    let mut pin = engine::pin();
    pin.binary_sha256 = hex::encode(Sha256::digest(std::fs::read(path).expect("fixture")));
    (
        Engine {
            executable: path.clone(),
        },
        pin,
    )
}

/// Whether `pid` names a process (a zombie included).
fn exists(pid: u32) -> bool {
    std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string()])
        .env_clear()
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(d) = pending.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let meta = entry.metadata();
            if meta.as_ref().is_ok_and(|m| m.is_dir()) {
                pending.push(entry.path());
            } else if meta.is_ok_and(|m| m.is_file()) {
                out.push(entry.path());
            }
        }
    }
    out
}

// --------------------------------------------------------------- gateway

/// How the runtime endpoint answers the engine's next Responses request.
#[derive(Clone)]
enum Answer {
    Short,
    /// One 32-byte delta every 50 ms until the client goes away.
    Paced,
    /// An error status (JSON body), every time.
    Status(u16),
}

fn sse(value: &Value) -> Vec<u8> {
    format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().unwrap_or("message")
    )
    .into_bytes()
}

fn responses(answer: &Answer) -> Reply {
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close"
        .to_owned();
    let created = json!({"type": "response.created", "response": {"id": "resp_c19", "object": "response",
        "status": "in_progress", "model": "fixture", "output": []}});
    let added = json!({"type": "response.output_item.added", "output_index": 0,
        "item": {"id": "msg_1", "type": "message", "role": "assistant", "status": "in_progress", "content": []}});
    let delta = |t: &str| {
        sse(
            &json!({"type": "response.output_text.delta", "item_id": "msg_1", "output_index": 0,
            "content_index": 0, "delta": t}),
        )
    };
    match answer {
        Answer::Short => {
            let item = json!({"id": "msg_1", "type": "message", "role": "assistant", "status": "completed",
                "content": [{"type": "output_text", "text": ANSWER, "annotations": []}]});
            let parts = vec![
                (0, sse(&created)),
                (0, sse(&added)),
                (0, delta("Hello from ")),
                (0, delta("the synthetic Gateway.")),
                (
                    0,
                    sse(
                        &json!({"type": "response.output_item.done", "output_index": 0, "item": item}),
                    ),
                ),
                (
                    0,
                    sse(
                        &json!({"type": "response.completed", "response": {"id": "resp_c19", "object": "response",
                    "status": "completed", "model": "fixture", "output": [item],
                    "usage": {"input_tokens": 10, "output_tokens": 6, "total_tokens": 16,
                        "input_tokens_details": {"cached_tokens": 0},
                        "output_tokens_details": {"reasoning_tokens": 0}}}}),
                    ),
                ),
            ];
            Reply::Stream {
                head,
                parts,
                hang: false,
            }
        }
        Answer::Status(status) => {
            json_reply(*status, &json!({"error": {"message": "synthetic failure"}}))
        }
        Answer::Paced => {
            let mut parts = vec![(0, sse(&created)), (0, sse(&added))];
            parts.extend((0..400).map(|_| (50, delta("thirty-two bytes of paced text."))));
            Reply::Stream {
                head,
                parts,
                hang: true,
            }
        }
    }
}

struct Gateway {
    answer: Answer,
}

fn workspace(id: &str, name: &str, slug: &str) -> Value {
    json!({"id": id, "slug": slug, "name": name, "icon": null, "description": null,
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "is_default": 0, "status": "active", "scope_name": format!("scope-{slug}"),
           "object": "workspace", "defaults": null, "usage_limits": null, "rate_limits": null})
}

fn json_reply(status: u16, body: &Value) -> Reply {
    let body = serde_json::to_vec(body).expect("json");
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}

impl Gateway {
    fn route(&mut self, seen: &Seen) -> Reply {
        let mut parts = seen.request_line.split(' ');
        let method = parts.next().unwrap_or("").to_owned();
        let target = parts.next().unwrap_or("").to_owned();
        let path = target
            .split_once('?')
            .map_or(target.as_str(), |(p, _)| p)
            .to_owned();
        if path == "/oauth2/access_token" {
            return json_reply(
                200,
                &json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        if method == "GET" && path == format!("{DATA}/workspaces") {
            let rows = vec![
                workspace(WS_A, "Team A", "team-a"),
                workspace(WS_B, "Team B", "team-b"),
            ];
            return json_reply(
                200,
                &json!({"object": "list", "total": 2, "success": true, "has_more": false, "data": rows}),
            );
        }
        if method == "POST" && path == format!("{RUNTIME}/responses") {
            return responses(&self.answer);
        }
        json_reply(404, &json!({"error": {"message": "not here"}}))
    }
}

// ------------------------------------------------------------------ desk

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    gateway: Arc<Mutex<Gateway>>,
    server: TlsServer,
    _ca: TestCa,
    launcher: Arc<ProcessLauncher>,
    runtime_root: PathBuf,
}

/// Waits up to `secs` for `condition`.
async fn settle_for(secs: u64, mut condition: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}

async fn settle(condition: impl FnMut() -> bool) -> bool {
    settle_for(10, condition).await
}

/// A desk whose Harness runs `engine` (with `pin` when it is a stand-in).
async fn desk_with(engine: Option<Engine>, fixture: Option<Pin>) -> Desk {
    let ca = TestCa::new("airs-v2 c19 ca");
    let gateway = Arc::new(Mutex::new(Gateway {
        answer: Answer::Short,
    }));
    let routed = Arc::clone(&gateway);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("gateway").route(seen)
        })),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    let (mut adapters, handles) = fixtures::adapters();
    let launcher = ProcessLauncher::new();
    let runtime_root = std::env::temp_dir()
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-c19-runtime-{}", hex::encode(rand_bytes())));
    adapters.harness = HarnessAdapters {
        launcher: launcher.clone(),
        engine,
        runtime_root: Some(runtime_root.clone()),
        fixture: fixture.map(|pin| FixtureEngine {
            pin,
            bootstrap_timeout: Duration::from_secs(1),
        }),
    };
    let core = Core::new(
        CoreConfig {
            management: ManagementEndpoints::at_origin(&origin),
            ..CoreConfig::default()
        },
        adapters,
    );
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
        gateway,
        server,
        _ca: ca,
        launcher,
        runtime_root,
    };
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("management key");
    assert!(settle(|| desk.snap().tenants[0].management.state == ScanState::Ready).await);
    desk
}

fn rand_bytes() -> [u8; 8] {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("fixture");
    b
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

    fn conversation(&self) -> Value {
        self.json()["harness"]["conversation"].clone()
    }

    fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    fn endpoint(&self, mode: Option<&str>) -> String {
        match mode {
            Some(mode) => format!("https://localhost:{}/{mode}/v1", self.server.port),
            None => format!("https://localhost:{}{RUNTIME}", self.server.port),
        }
    }

    async fn read_workspaces(&mut self) {
        let context = self.context();
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = self
            .send(
                json!({"kind": "management", "context": context, "request": {"kind": "gateway",
                "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}}}),
            )
            .await
            .expect("read admitted")
        else {
            panic!("no operation");
        };
        let Ok(Observation::Management(handle)) =
            self.handle.observe(ViewScope::Management(operation))
        else {
            panic!("management observation");
        };
        tokio::time::timeout(Duration::from_secs(20), handle.result())
            .await
            .expect("read settles");
        assert!(settle(|| self.snap().gateway.workspaces.is_some()).await);
    }

    async fn choose(&mut self, workspace: &str) -> String {
        if self.snap().gateway.workspaces.is_none() {
            self.read_workspaces().await;
        }
        let context = self.context();
        self.send(
            json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": workspace}),
        )
        .await
        .expect("selected");
        self.json()["gateway"]["selected"]["reference"]
            .as_str()
            .expect("reference")
            .to_owned()
    }

    async fn store_key(&mut self, reference: &str, key: &str, mode: Option<&str>) {
        let context = self.context();
        let endpoint = self.endpoint(mode);
        self.send(
            json!({"kind": "configureInferenceCredential", "context": context,
            "mode": "keychain", "workspace": reference, "endpoint": endpoint, "secret": key}),
        )
        .await
        .expect("key stored");
        assert!(settle(|| self.json()["tenants"][0]["inference"]["state"] == "ready").await);
    }

    async fn start(&mut self, reference: &str) -> Result<String, Refusal> {
        let context = self.context();
        match self
            .send(json!({"kind": "startHarness", "context": context, "workspace": reference}))
            .await?
        {
            Receipt::Admitted {
                operation: Some(operation),
                ..
            } => Ok(operation.as_str().to_owned()),
            other => panic!("unexpected receipt {other:?}"),
        }
    }

    /// A ready conversation in WS_A with `KEY` (and the stand-in `mode`).
    async fn ready(&mut self, mode: Option<&str>) -> String {
        let reference = self.choose(WS_A).await;
        self.store_key(&reference, KEY, mode).await;
        let id = self.start(&reference).await.expect("started");
        assert!(
            settle_for(30, || self.conversation()["phase"] == "ready").await,
            "ready: {}",
            self.conversation()
        );
        id
    }

    async fn say(&mut self, id: &str, prompt: &str) -> Result<Receipt, Refusal> {
        self.send(json!({"kind": "sendHarness", "conversation": id, "prompt": prompt}))
            .await
    }

    async fn answered(&mut self) -> Value {
        assert!(
            settle_for(30, || {
                let c = self.conversation();
                c["phase"] != "answering"
                    && c["phase"] != "interrupting"
                    && !c["lastTurn"].is_null()
                    || c["phase"] == "failed"
            })
            .await,
            "answered: {}",
            self.conversation()
        );
        self.conversation()
    }

    /// The pid of the engine the last launch started.
    fn last_pid(&self) -> Option<u32> {
        self.core.harness_pid_for_tests()
    }

    fn requests(&self, path: &str) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| {
                s.request_line
                    .split(' ')
                    .nth(1)
                    .is_some_and(|t| t.starts_with(path))
            })
            .collect()
    }

    fn run_dirs(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.runtime_root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("run-"))
            })
            .collect()
    }

    async fn end(&mut self, id: &str) {
        self.send(json!({"kind": "endHarness", "conversation": id}))
            .await
            .expect("end admitted");
        assert!(
            settle_for(10, || matches!(
                self.conversation()["phase"].as_str(),
                Some("ended" | "failed")
            ))
            .await,
            "ended: {}",
            self.conversation()
        );
    }
}

/// The pid recorded while the engine runs, checked gone afterwards.
async fn gone(pid: u32) -> bool {
    settle_for(10, || !exists(pid)).await
}

// ----------------------------------------------------------- real engine

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_answer_is_named_and_only_server_failures_are_retried_by_the_engine() {
    // The pinned engine's provider retries a 5xx or transport failure four
    // more times within one attempt, then reconnects its stream (up to five
    // times, each with those retries again: 30 requests). The host stops the
    // turn at the engine's first reconnect notice (D046), so a failing
    // Gateway sees at most the five requests of one attempt; never a 4xx.
    let mut desk = desk_with(Some(real_engine()), None).await;
    let id = desk.ready(None).await;
    desk.gateway.lock().expect("fixture").answer = Answer::Status(401);
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "failed", "{c}");
    assert_eq!(c["phase"], "ready");
    assert_eq!(
        desk.requests(RUNTIME).len(),
        1,
        "a refusal is never retried"
    );
    desk.gateway.lock().expect("fixture").answer = Answer::Status(503);
    desk.say(&id, "Again.").await.expect("sent");
    assert!(
        settle_for(60, || desk.conversation()["lastTurn"] == "failed"
            && desk.conversation()["phase"] == "ready")
        .await
    );
    assert_eq!(
        desk.requests(RUNTIME).len(),
        1 + 5,
        "one attempt, retried four times inside it, and no reconnect"
    );
    // No answer text arrived, so the operator's two messages are all there is.
    let roles: Vec<Value> = desk.conversation()["entries"]
        .as_array()
        .expect("fixture")
        .iter()
        .map(|e| e["role"].clone())
        .collect();
    assert_eq!(roles, [json!("operator"), json!("operator")]);
    // The conversation goes on.
    desk.gateway.lock().expect("fixture").answer = Answer::Short;
    desk.say(&id, "Once more.").await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_conversation_streams_through_the_pinned_engine_with_only_the_workspace_key() {
    let engine = real_engine();
    let mut desk = desk_with(Some(engine.clone()), None).await;
    let id = desk.ready(None).await;
    let pid = desk.last_pid().expect("engine running");
    assert!(exists(pid));

    // Launched exactly as the engine, with nothing but its own environment.
    let launches = desk.launcher.launches();
    assert_eq!(launches.len(), 1);
    let spec = &launches[0];
    assert_eq!(spec.executable, engine.executable);
    assert_eq!(spec.args, vec![std::ffi::OsString::from("desktop-server")]);
    let names: Vec<String> = spec
        .env
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| ENV_NAMES.contains(&n.as_str())),
        "{names:?}"
    );
    for required in [
        "PATH",
        "LANG",
        "TMPDIR",
        "AIRS_HARNESS_HOME",
        "CODEX_CA_CERTIFICATE",
        "NO_PROXY",
    ] {
        assert!(names.iter().any(|n| n == required), "{required}");
    }
    assert!(
        spec.env
            .iter()
            .all(|(_, v)| !v.to_string_lossy().contains(KEY))
    );
    let path = spec
        .env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.to_string_lossy().into_owned())
        .expect("fixture");
    assert_eq!(path, "/usr/bin:/bin:/usr/sbin:/sbin");
    // No Node or Bun on that path: the engine needs neither.
    for dir in path.split(':') {
        for runtime in ["node", "bun", "npm", "npx"] {
            assert!(!Path::new(dir).join(runtime).exists(), "{dir}/{runtime}");
        }
    }
    let home = PathBuf::from(
        spec.env
            .iter()
            .find(|(k, _)| k == "AIRS_HARNESS_HOME")
            .expect("fixture")
            .1
            .clone(),
    );
    assert_eq!(
        home.parent(),
        Some(desk.runtime_root.canonicalize().expect("fixture").as_path())
    );
    assert_eq!(spec.cwd, home);

    // The process's own environment and arguments carry no credential.
    let ps = std::process::Command::new("/bin/ps")
        .args(["eww", "-o", "command=", "-p", &pid.to_string()])
        .output()
        .expect("fixture");
    let ps = String::from_utf8_lossy(&ps.stdout);
    assert!(ps.contains("desktop-server"));
    assert!(
        !ps.contains(KEY),
        "the key is never in the engine's environment or arguments"
    );

    // One answer, streamed, sent once with the workspace key.
    assert!(matches!(
        desk.say(&id, PROMPT).await,
        Ok(Receipt::Admitted { .. })
    ));
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert_eq!(
        c["entries"][0],
        json!({"id": "u-1", "role": "operator", "text": PROMPT, "state": "complete"})
    );
    assert_eq!(c["entries"][1]["role"], "assistant");
    assert_eq!(c["entries"][1]["text"], ANSWER);
    assert_eq!(c["entries"][1]["state"], "complete");
    assert_eq!(c["usage"]["total"], 16);
    assert_eq!(c["runtimeVersion"], "0.1.7-desktop.2");
    assert_eq!(c["workspace"]["id"], WS_A);
    assert_eq!(c["endpoint"], desk.endpoint(None));
    let sent = desk.requests(RUNTIME);
    assert_eq!(sent.len(), 1, "one Responses request");
    assert!(
        sent[0]
            .request_line
            .starts_with(&format!("POST {RUNTIME}/responses "))
    );
    assert_eq!(sent[0].header("x-portkey-api-key"), Some(KEY));
    assert!(String::from_utf8_lossy(&sent[0].body).contains(PROMPT));
    // Nothing else was fetched: no download, no other route.
    assert!(desk.server.seen().iter().all(|s| {
        let t = s.request_line.split(' ').nth(1).unwrap_or("");
        t.starts_with(&format!("{RUNTIME}/responses"))
            || t.starts_with(DATA)
            || t.starts_with("/oauth2/")
    }));

    // Nothing of the conversation or its key is on disk.
    for file in files_under(&desk.runtime_root) {
        let bytes = std::fs::read(&file).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(KEY) && !text.contains(PROMPT) && !text.contains(ANSWER),
            "{}",
            file.display()
        );
    }

    // A second turn in the same conversation.
    desk.say(&id, "And again.").await.expect("second turn");
    let c = desk.answered().await;
    assert_eq!(c["entries"].as_array().expect("fixture").len(), 4);
    assert_eq!(desk.requests(RUNTIME).len(), 2);

    // Ending stops and reaps the engine and removes its home.
    desk.end(&id).await;
    let c = desk.conversation();
    assert_eq!(c["phase"], "ended");
    assert_eq!(c["stop"], "ended");
    assert_eq!(c["cleanup"], "graceful");
    assert!(c["failure"].is_null());
    assert!(gone(pid).await);
    assert!(desk.run_dirs().is_empty());
    assert!(!home.exists());
    // An ended conversation takes nothing more.
    assert_eq!(
        desk.say(&id, PROMPT).await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.send(json!({"kind": "endHarness", "conversation": id}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );

    // A new conversation starts a new engine with a new home; the old one's
    // transcript is not carried over.
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    assert!(desk.conversation().is_null());
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let next = desk.start(&reference).await.expect("started again");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    assert_ne!(next, id);
    assert!(
        desk.conversation()["entries"]
            .as_array()
            .expect("fixture")
            .is_empty()
    );
    assert_eq!(desk.launcher.launches().len(), 2);
    let second = desk.last_pid().expect("fixture");
    assert_ne!(second, pid);
    desk.end(&next).await;
    assert!(gone(second).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_an_answer_interrupts_only_that_turn() {
    let mut desk = desk_with(Some(real_engine()), None).await;
    let id = desk.ready(None).await;
    desk.gateway.lock().expect("fixture").answer = Answer::Paced;
    desk.say(&id, PROMPT).await.expect("sent");
    assert!(
        settle_for(20, || desk.conversation()["entries"][1]["text"]
            .as_str()
            .is_some_and(|t| t.len() > 60))
        .await,
        "{}",
        desk.conversation()
    );
    // Only while answering may another prompt wait: it is refused.
    assert_eq!(desk.say(&id, "too soon").await.err(), Some(Refusal::Busy));
    desk.send(json!({"kind": "interruptHarness", "conversation": id}))
        .await
        .expect("interrupt");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "interrupted", "{c}");
    assert_eq!(c["phase"], "ready");
    assert_eq!(c["entries"][1]["state"], "interrupted");
    // The stream was closed by the engine, not left running.
    assert!(settle(|| desk.server.closed_by_client() >= 1).await);
    // The conversation goes on.
    desk.gateway.lock().expect("fixture").answer = Answer::Short;
    desk.say(&id, "Short this time.").await.expect("next turn");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    assert_eq!(c["entries"][3]["text"], ANSWER);
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_changed_key_workspace_or_context_retires_the_conversation_at_once() {
    let mut desk = desk_with(Some(real_engine()), None).await;
    // The key replaced.
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    desk.store_key(&reference, OTHER_KEY, None).await;
    assert!(
        desk.conversation().is_null(),
        "withdrawn in the same snapshot"
    );
    assert!(gone(pid).await);
    assert!(settle(|| desk.run_dirs().is_empty()).await);

    // The key removed.
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    let pid = desk.last_pid().expect("fixture");
    let context = desk.context();
    desk.send(json!({"kind": "removeInferenceCredential", "context": context}))
        .await
        .expect("removed");
    assert!(desk.conversation().is_null());
    assert!(gone(pid).await);
    // Refused either way: stale while its task settles, unknown once cleared.
    assert!(matches!(
        desk.say(&id, PROMPT).await.err(),
        Some(Refusal::StaleContext | Refusal::InvalidInput)
    ));

    // Another workspace chosen.
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    let _ = desk.choose(WS_B).await;
    assert!(desk.conversation().is_null());
    assert!(gone(pid).await);

    // The tenant context reissued (a new transport policy).
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    desk.core
        .set_transport_policy(TransportPolicy {
            revision: 3,
            proxy: ProxyMode::Direct,
            extra_roots: vec![desk._ca.pem.as_bytes().to_vec()],
        })
        .expect("policy");
    assert!(desk.conversation().is_null());
    assert!(gone(pid).await);
    assert!(settle(|| desk.run_dirs().is_empty()).await);
    // Nothing was sent by any retired conversation.
    assert!(desk.requests(RUNTIME).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sleep_and_quit_end_the_conversation_and_reap_the_engine() {
    let mut desk = desk_with(Some(real_engine()), None).await;
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    desk.core.system_will_sleep();
    assert!(settle(|| desk.conversation()["phase"] == "ended").await);
    assert_eq!(desk.conversation()["stop"], "sleep");
    assert!(gone(pid).await);

    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let id = desk.json()["harness"]["conversation"]["id"]
        .as_str()
        .expect("fixture")
        .to_owned();
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    let pid = desk.last_pid().expect("fixture");
    let core = desk.core.clone();
    let reaped =
        tokio::task::spawn_blocking(move || core.end_harness_for_exit(Duration::from_secs(5)))
            .await
            .expect("fixture");
    assert!(reaped);
    assert!(!exists(pid));
    assert_eq!(desk.conversation()["stop"], "quit");
    assert!(desk.run_dirs().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_changed_or_missing_engine_is_never_started_and_no_key_is_read() {
    let real = real_engine();
    let dir = std::env::temp_dir()
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-c19-corrupt-{}", hex::encode(rand_bytes())));
    std::fs::create_dir_all(&dir).expect("fixture");
    let corrupt = dir.join("airs-harness");
    std::fs::copy(&real.executable, &corrupt).expect("fixture");
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&corrupt)
            .expect("fixture");
        f.seek(SeekFrom::Start(4096)).expect("fixture");
        f.write_all(&[0x5a]).expect("fixture");
    }
    let mut desk = desk_with(
        Some(Engine {
            executable: corrupt,
        }),
        None,
    )
    .await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    let reads_before = desk
        .adapters
        .vault
        .calls()
        .iter()
        .filter(|c| matches!(c, VaultCall::Get(_)))
        .count();
    desk.start(&reference).await.expect("admitted");
    assert!(settle_for(20, || desk.conversation()["phase"] == "failed").await);
    assert_eq!(desk.conversation()["failure"], "engineUnavailable");
    assert!(desk.launcher.launches().is_empty(), "nothing launched");
    let reads_after = desk
        .adapters
        .vault
        .calls()
        .iter()
        .filter(|c| matches!(c, VaultCall::Get(_)))
        .count();
    assert_eq!(reads_before, reads_after, "the key was never read");
    assert!(settle(|| desk.run_dirs().is_empty()).await);
    std::fs::remove_dir_all(&dir).expect("fixture");

    // No engine bundled: no Harness at all.
    let mut desk = desk_with(None, None).await;
    assert_eq!(desk.json()["harness"]["engine"]["available"], false);
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    assert_eq!(
        desk.start(&reference).await.err(),
        Some(Refusal::CapabilityMissing)
    );
}

#[test]
fn the_sealed_engine_ships_without_node_bun_or_a_managed_cli() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let engine = real_engine();
    let root = engine.executable.parent().expect("fixture");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("MANIFEST.json")).expect("fixture"))
            .expect("fixture");
    let files = manifest["files"].as_object().expect("fixture");
    for name in files.keys() {
        let lower = name.to_ascii_lowercase();
        for banned in [
            "node",
            "bun",
            "npm",
            "package.json",
            "airs-cli",
            "managed-cli",
            ".js",
            ".mjs",
        ] {
            let base = lower.rsplit('/').next().expect("fixture");
            assert!(
                !(base == banned
                    || base.ends_with(banned) && banned.starts_with('.')
                    || lower.contains("node_modules")),
                "{name} looks like a JavaScript runtime or package"
            );
        }
    }
    // Every extracted file is in the inventory with its hash, and the pin
    // names the inventory.
    let mut extracted = 0;
    for file in files_under(root) {
        let rel = file
            .strip_prefix(root)
            .expect("fixture")
            .to_string_lossy()
            .into_owned();
        if rel == "MANIFEST.json" {
            continue;
        }
        extracted += 1;
        assert_eq!(
            files[&rel],
            hex::encode(Sha256::digest(std::fs::read(&file).expect("fixture"))),
            "{rel}"
        );
    }
    assert_eq!(extracted, files.len());
    let digest = hex::encode(Sha256::digest(
        files
            .iter()
            .map(|(n, h)| format!("{n}\0{}\n", h.as_str().expect("fixture")))
            .collect::<String>()
            .as_bytes(),
    ));
    let pin = engine::pin();
    assert_eq!(manifest["pin"]["resourcesSha256"], pin.resources_sha256);
    assert_eq!(digest, pin.resources_sha256);
    assert_eq!(files["airs-harness"], pin.binary_sha256);
    let record: Value = serde_json::from_slice(
        &std::fs::read(repo.join("resources/harness/pin.json")).expect("fixture"),
    )
    .expect("fixture");
    assert_eq!(
        record["engineCommit"],
        "9331a5839bc6a763d94dee7584746f58bbbe1181"
    );
    assert_eq!(
        record["source"]["v1Commit"],
        "74754c02b19d87187b6dca81f64550a669b0f841"
    );
}

// ------------------------------------------------------------- stand-in

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_engine_is_given_nothing_but_its_bootstrap() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("normal")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    let text = c["entries"][1]["text"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let key_hash = hex::encode(&Sha256::digest(KEY.as_bytes())[..8]);
    assert!(text.contains(&format!("key:{key_hash}")), "{text}");
    assert!(text.contains("args:desktop-server "), "{text}");
    assert!(text.contains(&format!("prompt:{}", PROMPT.len())), "{text}");
    let env = text
        .split("env:")
        .nth(1)
        .expect("fixture")
        .split(' ')
        .next()
        .expect("fixture");
    let mut seen: Vec<&str> = env.split(',').collect();
    seen.sort();
    // macOS may add __CF_USER_TEXT_ENCODING for any process; nothing else.
    seen.retain(|n| *n != "__CF_USER_TEXT_ENCODING");
    assert_eq!(
        seen,
        [
            "AIRS_HARNESS_HOME",
            "CODEX_CA_CERTIFICATE",
            "LANG",
            "NO_PROXY",
            "PATH",
            "TMPDIR"
        ]
    );
    // The key never reaches what is shown.
    assert!(!desk.json().to_string().contains(KEY));
    desk.end(&id).await;
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");

    // Engine text that holds the key shows it replaced, even split across
    // deltas, at every snapshot while it streams.
    let id = desk.ready(Some("echo-key")).await;
    let Observation::Shell { mut updates, .. } =
        desk.handle.observe(ViewScope::Shell).expect("observe")
    else {
        unreachable!("shell scope")
    };
    while updates.try_next().is_some() {}
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["entries"][1]["text"], "the key is [redacted].");
    let mut published = 0;
    while let Some(update) = updates.try_next() {
        if let desktop_core::Update::Shell(snapshot) = update {
            published += 1;
            assert!(
                !serde_json::to_string(&*snapshot)
                    .expect("fixture")
                    .contains(KEY),
                "a published snapshot held the key"
            );
        }
    }
    assert!(published > 0);
    desk.end(&id).await;
}

/// Starts a stand-in conversation in `mode` and, for turn modes, sends a
/// prompt; returns the conversation once it has failed and its pid.
async fn failing(desk: &mut Desk, mode: &str, turn: bool) -> (Value, Option<u32>) {
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, Some(mode)).await;
    let id = desk.start(&reference).await.expect("started");
    let mut pid = None;
    if turn {
        assert!(
            settle_for(10, || desk.conversation()["phase"] == "ready").await,
            "{mode}: {}",
            desk.conversation()
        );
        pid = desk.last_pid();
        desk.say(&id, PROMPT).await.expect("sent");
    } else {
        let _ = settle_for(2, || desk.last_pid().is_some()).await;
        pid = pid.or(desk.last_pid());
    }
    assert!(
        settle_for(10, || desk.conversation()["phase"] == "failed").await,
        "{mode}: {}",
        desk.conversation()
    );
    let c = desk.conversation();
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    (c, pid)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_oversized_duplicate_or_foreign_frames_fail_the_conversation_and_reap_it() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    for (mode, turn, failure) in [
        ("reject", false, "bootstrapRejected"),
        ("wrong-receipt", false, "bootstrapRejected"),
        ("receipt-big", false, "bounds"),
        ("silent", false, "timeout"),
        ("exit", false, "exited"),
        ("duplicate", false, "protocol"),
        ("malformed", true, "protocol"),
        ("long-id", true, "protocol"),
        ("foreign", true, "protocol"),
        ("oversized", true, "bounds"),
        ("crash", true, "exited"),
    ] {
        let (c, pid) = failing(&mut desk, mode, turn).await;
        assert_eq!(c["failure"], failure, "{mode}: {c}");
        assert!(!c["cleanup"].is_null(), "{mode}: {c}");
        // Nothing past the contract's bounds was published (entry ids <= 300).
        assert!(
            c["entries"].as_array().is_some_and(|entries| entries
                .iter()
                .all(|e| e["id"].as_str().is_some_and(|i| i.len() <= 300))),
            "{mode}: {c}"
        );
        if let Some(pid) = pid {
            assert!(gone(pid).await, "{mode}: process left");
        }
        assert!(
            settle(|| desk.run_dirs().is_empty()).await,
            "{mode}: home left"
        );
        if mode == "crash" {
            // What arrived is kept, marked as not finished.
            assert_eq!(c["entries"][1]["text"], "partial");
            assert_eq!(c["entries"][1]["state"], "interrupted");
        }
    }
    assert!(!desk.json().to_string().contains(KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_engine_that_ignores_its_input_closing_is_killed_with_its_descendants() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("hang-on-eof")).await;
    let pid = desk.last_pid().expect("fixture");
    let started = Instant::now();
    desk.end(&id).await;
    let took = started.elapsed();
    let c = desk.conversation();
    assert_eq!(c["cleanup"], "forced", "{c}");
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(5),
        "{took:?}"
    );
    assert!(!exists(pid));
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");

    // A descendant goes with the group.
    let id = desk.ready(Some("descendant")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    let text = c["entries"][1]["text"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let child: u32 = text
        .split("child:")
        .nth(1)
        .expect("fixture")
        .trim()
        .parse()
        .expect("fixture");
    assert!(exists(child));
    desk.end(&id).await;
    assert_eq!(desk.conversation()["cleanup"], "graceful");
    assert!(
        gone(child).await,
        "the descendant was killed with the group"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn approvals_are_declined_and_an_answer_past_its_bound_is_truncated() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("approval")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["entries"][1]["text"], "approval:decline");
    desk.end(&id).await;
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");

    let id = desk.ready(Some("flood")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "truncated", "{}", c["lastTurn"]);
    assert_eq!(c["entries"][1]["state"], "truncated");
    let kept = c["entries"][1]["text"].as_str().expect("fixture").len();
    assert!(kept <= desktop_core::harness::MAX_ANSWER_BYTES, "{kept}");
    assert!(kept > 0);
    // Still usable.
    assert_eq!(c["phase"], "ready");
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_answers_and_token_counts_are_held_to_their_bounds() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let bound = desktop_core::harness::MAX_ANSWER_BYTES;
    let clear = |id: String| json!({"kind": "clearHarness", "conversation": id});

    // A completion past the bound after a short delta: cut at the bound and
    // never shown as complete.
    let id = desk.ready(Some("big-complete")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "truncated", "{}", c["lastTurn"]);
    assert_eq!(c["entries"][1]["state"], "truncated");
    assert_eq!(c["entries"][1]["text"].as_str().map(str::len), Some(bound));
    assert_eq!(c["phase"], "ready");
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // Two items of one turn, together past the bound: the second is cut so
    // the turn keeps exactly the bound.
    let id = desk.ready(Some("two-items")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "truncated", "{c}");
    let first = c["entries"][1]["text"].as_str().map_or(0, str::len);
    let second = c["entries"][2]["text"].as_str().map_or(0, str::len);
    assert_eq!(
        (first, c["entries"][1]["state"].as_str()),
        (200 * 1024, Some("complete"))
    );
    assert_eq!(c["entries"][2]["state"], "truncated");
    assert_eq!(first + second, bound);
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // Whole answers with no delta until the transcript has no room: the
    // fourth is cut to what is left, and the transcript stays bounded.
    let id = desk.ready(Some("fill")).await;
    for turn in 0..4 {
        desk.say(&id, PROMPT).await.expect("sent");
        let c = desk.answered().await;
        let want = if turn < 3 { "completed" } else { "truncated" };
        assert_eq!(c["lastTurn"], want, "turn {turn}");
    }
    let c = desk.conversation();
    let total: usize = c["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["text"].as_str().map_or(0, str::len))
        .sum();
    assert_eq!(total, desktop_core::harness::MAX_TRANSCRIPT_BYTES);
    assert_eq!(c["entries"][7]["state"], "truncated");
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // A whole answer with no start and no delta is kept, complete.
    let id = desk.ready(Some("complete-only")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    // (The stand-in starts its usual item first; this one it never started.)
    let entry = c["entries"]
        .as_array()
        .and_then(|e| e.iter().find(|e| e["id"] == "u-1/unstarted"))
        .expect("the unstarted item's entry");
    assert_eq!(entry["text"], "Only completed.");
    assert_eq!(entry["state"], "complete");
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // Token counts past JavaScript's safe integers are never published: the
    // answer is shown and every number in the snapshot stays exact there.
    let id = desk.ready(Some("huge-usage")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    assert_eq!(c["entries"][1]["text"], "small");
    assert!(c["usage"].is_null(), "{}", c["usage"]);
    fn safe(v: &Value) -> bool {
        match v {
            Value::Number(n) => n.as_u64().is_none_or(|n| n < (1 << 53)),
            Value::Array(items) => items.iter().all(safe),
            Value::Object(map) => map.values().all(safe),
            _ => true,
        }
    }
    assert!(safe(&desk.json()));
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_one_conversation_and_only_bounded_prompts_are_admitted() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("normal")).await;
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    assert_eq!(desk.start(&reference).await.err(), Some(Refusal::Busy));
    assert_eq!(
        desk.say(&id, "   ").await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.say(
            &id,
            &"x".repeat(desktop_core::harness::protocol::MAX_PROMPT_BYTES + 1)
        )
        .await
        .err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.say("op-999999", PROMPT).await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.send(json!({"kind": "clearHarness", "conversation": id}))
            .await
            .err(),
        Some(Refusal::InvalidInput),
        "a running conversation is not cleared"
    );
    // A stale context is refused.
    let context = desk.context();
    desk.core
        .set_transport_policy(TransportPolicy {
            revision: 4,
            proxy: ProxyMode::Direct,
            extra_roots: vec![desk._ca.pem.as_bytes().to_vec()],
        })
        .expect("policy");
    assert_eq!(
        desk.send(json!({"kind": "startHarness", "context": context, "workspace": reference}))
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
}
