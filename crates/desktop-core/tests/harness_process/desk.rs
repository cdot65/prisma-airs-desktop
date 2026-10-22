//! Shared Harness test desk: the authority with a synthetic TLS Gateway
//! (management, workspaces and Responses), the real or stand-in engine,
//! and helpers to drive and observe conversations (C19, reused by C20).

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

use super::support::{Reply, Seen, TestCa, TlsServer};

pub const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
pub const CLIENT_SECRET: &str = "synthetic-client-secret-c19";
pub const KEY: &str = "gw-CANARY-harness-key-c19";
pub const OTHER_KEY: &str = "gw-CANARY-harness-replacement";
pub const TSG: &str = "1234567890";
pub const DATA: &str = "/ai_gw/v2";
pub const RUNTIME: &str = "/rt/v1";
pub const WS_A: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";
pub const WS_B: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8298";
pub const PROMPT: &str = "Say hello to the C19 canary.";
pub const ANSWER: &str = "Hello from the synthetic Gateway.";

// ---------------------------------------------------------------- engine

/// The real engine, extracted once from the sealed archive into this test
/// target's temporary directory and verified against the compiled pin.
pub fn real_engine() -> Engine {
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
pub fn fixture_engine() -> (Engine, Pin) {
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
pub fn exists(pid: u32) -> bool {
    std::process::Command::new("/bin/ps")
        .args(["-p", &pid.to_string()])
        .env_clear()
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Every file under `dir`, recursively.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
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
pub enum Answer {
    Short,
    /// A call of one tool (C21): the first request of a turn gets it; the
    /// request carrying its output gets the short answer.
    Tool {
        namespace: Option<String>,
        name: String,
        arguments: String,
    },
    /// One 32-byte delta every 50 ms until the client goes away.
    Paced,
    /// An error status (JSON body), every time.
    Status(u16),
}

pub fn sse(value: &Value) -> Vec<u8> {
    format!(
        "event: {}\ndata: {value}\n\n",
        value["type"].as_str().unwrap_or("message")
    )
    .into_bytes()
}

/// A Responses stream holding one function call (C21).
pub fn tool_call(namespace: Option<&str>, name: &str, arguments: &str) -> Reply {
    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close"
        .to_owned();
    let mut item = json!({"type": "function_call", "call_id": "call-c21", "name": name, "arguments": arguments});
    if let Some(ns) = namespace {
        item["namespace"] = json!(ns);
    }
    let parts = vec![
        (
            0,
            sse(&json!({"type": "response.created", "response": {"id": "resp_tool"}})),
        ),
        (
            0,
            sse(&json!({"type": "response.output_item.done", "item": item})),
        ),
        (
            0,
            sse(
                &json!({"type": "response.completed", "response": {"id": "resp_tool",
                "usage": {"input_tokens": 1, "input_tokens_details": null, "output_tokens": 1,
                    "output_tokens_details": null, "total_tokens": 2}}}),
            ),
        ),
    ];
    Reply::Stream {
        head,
        parts,
        hang: false,
    }
}

pub fn responses(answer: &Answer) -> Reply {
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
        Answer::Tool { .. } => responses(&Answer::Short),
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

pub struct Gateway {
    pub answer: Answer,
    /// Other routes on the same server (C20's identity provider).
    pub hook: Option<Hook>,
}

pub type Hook = Arc<dyn Fn(&Seen) -> Option<Reply> + Send + Sync>;

pub fn workspace(id: &str, name: &str, slug: &str) -> Value {
    json!({"id": id, "slug": slug, "name": name, "icon": null, "description": null,
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "is_default": 0, "status": "active", "scope_name": format!("scope-{slug}"),
           "object": "workspace", "defaults": null, "usage_limits": null, "rate_limits": null})
}

pub fn json_reply(status: u16, body: &Value) -> Reply {
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
    pub fn route(&mut self, seen: &Seen) -> Reply {
        if let Some(reply) = self.hook.as_ref().and_then(|hook| hook(seen)) {
            return reply;
        }
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
            // A tool's output came back: the turn is answered (C21).
            // The turn's tool call was answered when the request's last
            // input item is its output (earlier turns' outputs are history).
            let carries_output = serde_json::from_slice::<Value>(&seen.body)
                .ok()
                .and_then(|b| b["input"].as_array().and_then(|i| i.last().cloned()))
                .is_some_and(|last| last["type"] == "function_call_output");
            if let (
                Answer::Tool {
                    namespace,
                    name,
                    arguments,
                },
                false,
            ) = (&self.answer, carries_output)
            {
                return tool_call(namespace.as_deref(), name, arguments);
            }
            if carries_output {
                return responses(&Answer::Short);
            }
            return responses(&self.answer);
        }
        json_reply(404, &json!({"error": {"message": "not here"}}))
    }
}

// ------------------------------------------------------------------ desk

pub struct Desk {
    pub core: Core,
    pub handle: AuthorityHandle,
    pub session: String,
    pub sequence: u64,
    pub adapters: Handles,
    pub gateway: Arc<Mutex<Gateway>>,
    pub server: TlsServer,
    pub _ca: TestCa,
    pub launcher: Arc<ProcessLauncher>,
    pub runtime_root: PathBuf,
    pub history_root: PathBuf,
}

/// Waits up to `secs` for `condition`.
pub async fn settle_for(secs: u64, mut condition: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < until {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
}

pub async fn settle(condition: impl FnMut() -> bool) -> bool {
    settle_for(10, condition).await
}

/// A desk whose Harness runs `engine` (with `pin` when it is a stand-in).
pub async fn desk_with(engine: Option<Engine>, fixture: Option<Pin>) -> Desk {
    let ca = TestCa::new("airs-v2 c19 ca");
    let gateway = Arc::new(Mutex::new(Gateway {
        answer: Answer::Short,
        hook: None,
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
        // The desk's own data folder (the app's data root is its parent).
        .join(format!("airs-c19-desk-{}", hex::encode(rand_bytes())))
        .join("harness-runtime");
    let history_root = runtime_root.with_file_name("harness-history");
    adapters.harness = HarnessAdapters {
        launcher: launcher.clone(),
        engine,
        runtime_root: Some(runtime_root.clone()),
        history_root: Some(history_root.clone()),
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
        history_root,
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

pub fn rand_bytes() -> [u8; 8] {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("fixture");
    b
}

impl Desk {
    pub async fn send(&mut self, intent: Value) -> Result<Receipt, Refusal> {
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

    pub fn snap(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).expect("observe") {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell scope"),
        }
    }

    pub fn json(&self) -> Value {
        serde_json::to_value(self.snap()).expect("json")
    }

    pub fn conversation(&self) -> Value {
        self.json()["harness"]["conversation"].clone()
    }

    pub fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    pub fn endpoint(&self, mode: Option<&str>) -> String {
        match mode {
            Some(mode) => format!("https://localhost:{}/{mode}/v1", self.server.port),
            None => format!("https://localhost:{}{RUNTIME}", self.server.port),
        }
    }

    pub async fn read_workspaces(&mut self) {
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

    pub async fn choose(&mut self, workspace: &str) -> String {
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

    pub async fn store_key(&mut self, reference: &str, key: &str, mode: Option<&str>) {
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

    pub async fn start(&mut self, reference: &str) -> Result<String, Refusal> {
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
    pub async fn ready(&mut self, mode: Option<&str>) -> String {
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

    pub async fn say(&mut self, id: &str, prompt: &str) -> Result<Receipt, Refusal> {
        self.send(json!({"kind": "sendHarness", "conversation": id, "prompt": prompt}))
            .await
    }

    pub async fn answered(&mut self) -> Value {
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
    pub fn last_pid(&self) -> Option<u32> {
        self.core.harness_pid_for_tests()
    }

    pub fn requests(&self, path: &str) -> Vec<Seen> {
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

    pub fn run_dirs(&self) -> Vec<PathBuf> {
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

    pub async fn end(&mut self, id: &str) {
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
pub async fn gone(pid: u32) -> bool {
    settle_for(10, || !exists(pid)).await
}
