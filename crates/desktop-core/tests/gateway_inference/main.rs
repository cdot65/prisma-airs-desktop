//! C18 Gateway inference and observation through the production authority
//! interface against a synthetic Gateway: the management planes at the
//! management origin (workspaces, telemetry, logs and limit policies) and
//! a runtime endpoint that streams chat completions. The inference key is
//! bound to its workspace and endpoint, a run streams once and is never
//! retried, its output is bounded, stopping it never claims nothing was
//! charged, and every observation read keeps each source's data or
//! failure for the chosen workspace only.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::ManagementEndpoints;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal, ScanState,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c18";
const INFERENCE_KEY: &str = "gw-CANARY-inference-key";
const OTHER_KEY: &str = "gw-CANARY-replacement-key";
const TSG: &str = "1234567890";
const DATA: &str = "/ai_gw/v2";
const RUNTIME: &str = "/rt/v1";
const WS_A: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";
const WS_B: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8298";
const ROUTE: &str = "cfg-default";

/// How the runtime endpoint answers the next chat request.
#[derive(Clone)]
enum Chat {
    /// These text deltas, then usage and `[DONE]`.
    Text(Vec<String>),
    /// One delta, then nothing until the client goes away.
    Hang,
    /// One delta every `millis`, `count` times, then `[DONE]`.
    Paced {
        millis: u64,
        count: usize,
    },
    Status(u16),
    /// One valid delta, then an event that is not a chunk.
    Malformed,
    /// The request is read, then the connection closes (a lost handshake).
    Lost,
    /// A successful answer that is not an event stream.
    NotStream,
    /// One delta, then a `length` finish (cut at the token limit).
    Cut,
}

/// How the runtime endpoint answers the next Responses request.
#[derive(Clone)]
enum Responses {
    /// These text deltas, then the completed event with usage.
    Text(Vec<String>),
    /// One delta, then the failed event.
    Failed,
    /// One delta, then the incomplete event.
    Incomplete,
}

fn response(id: &str, status: &str, usage: bool) -> Value {
    let mut r = json!({"id": id, "object": "response", "created_at": 1_760_000_000,
        "status": status, "model": "gpt-4o-2024", "metadata": null, "temperature": null,
        "top_p": null, "instructions": null, "tools": [], "tool_choice": "auto", "error": null,
        "incomplete_details": null, "output": [], "parallel_tool_calls": false});
    if usage {
        r["usage"] = json!({"input_tokens": 2, "input_tokens_details": {"cached_tokens": 0},
            "output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 0},
            "total_tokens": 5});
    }
    if status == "incomplete" {
        r["incomplete_details"] = json!({"reason": "max_output_tokens"});
    }
    r
}

fn event(value: &Value) -> Vec<u8> {
    format!("data: {value}\n\n").into_bytes()
}

fn delta(text: &str) -> Vec<u8> {
    event(
        &json!({"type": "response.output_text.delta", "item_id": "msg_1",
        "output_index": 0, "content_index": 0, "delta": text}),
    )
}

/// How the runtime endpoint answers the next file upload or delete.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Files {
    Ok,
    Status(u16),
    /// The request is read, then the connection closes.
    Lost,
    /// A delete is answered for another file, with this `deleted`.
    Another(bool),
    /// A delete is answered for the file asked, with `deleted: false`.
    Kept,
}

/// One file's metadata as the Gateway lists it.
fn gateway_file(id: &str, name: &str, bytes: usize) -> Value {
    json!({"id": id, "object": "file", "bytes": bytes, "created_at": 1_760_000_000,
        "filename": name, "purpose": "assistants", "status": "processed"})
}

/// A multipart body's parts by name: (file name, bytes).
fn multipart(seen: &Seen) -> BTreeMap<String, (Option<String>, Vec<u8>)> {
    let kind = seen.header("content-type").expect("content type");
    let boundary = kind
        .split("boundary=")
        .nth(1)
        .expect("boundary")
        .trim_matches('"');
    let delimiter = format!("\r\n--{boundary}").into_bytes();
    let mut body = b"\r\n".to_vec();
    body.extend_from_slice(&seen.body);
    let mut parts = BTreeMap::new();
    let mut at = 0;
    let find = |from: usize, needle: &[u8]| {
        body[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|p| p + from)
    };
    while let Some(start) = find(at, &delimiter) {
        let head_start = start + delimiter.len() + 2;
        let Some(head_end) = find(head_start, b"\r\n\r\n") else {
            break;
        };
        let Some(next) = find(head_end, &delimiter) else {
            break;
        };
        let head = String::from_utf8_lossy(&body[head_start..head_end]).into_owned();
        let field = |key: &str| {
            head.split(&format!("{key}=\""))
                .nth(1)
                .and_then(|r| r.split('"').next())
                .map(str::to_owned)
        };
        if let Some(name) = field("name") {
            parts.insert(name, (field("filename"), body[head_end + 4..next].to_vec()));
        }
        at = next;
    }
    parts
}

struct Gateway {
    chat: Chat,
    responses: Responses,
    /// The workspace's files and how the next upload or delete answers.
    files: Vec<Value>,
    file_reply: Files,
    /// Replies by path (telemetry and policies).
    replies: BTreeMap<String, (u16, Value)>,
    /// Delay (ms) before the requests chart answers.
    chart_delay: u64,
    serial: u32,
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

fn chunk(id: &str, text: Option<&str>, finish: Option<&str>, usage: Option<Value>) -> Vec<u8> {
    let mut delta = json!({});
    if let Some(text) = text {
        delta["content"] = json!(text);
    }
    let mut event = json!({"id": id, "object": "chat.completion.chunk", "created": 1_760_000_000,
        "model": "gpt-4o", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]});
    if let Some(usage) = usage {
        event["usage"] = usage;
    }
    format!("data: {event}\n\n").into_bytes()
}

const SSE_HEAD: &str = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncache-control: no-cache\r\nconnection: close";

fn series(total: f64, errors: f64) -> Value {
    json!({"success": true, "data": {"records": [
        {"x": "2026-10-20T00:00:00Z", "y": total / 2.0},
        {"x": "2026-10-20T01:00:00Z", "y": total / 2.0}],
        "total": total, "total_llm_count": total, "total_llm_error_count": errors,
        "isQuotaExceeded": false}})
}

fn log_row(id: &str, slug: &str, trace: &str, status: u16) -> Value {
    json!({"id": id, "workspace_slug": slug, "ai_model": "gpt-4o", "_user": "person@example.com",
        "total_units": 12, "cost": 0.4, "trace_id": trace, "is_proxy_call": 0,
        "created_at": "2026-10-20T00:00:00Z", "is_success": if status < 400 { 1 } else { 0 },
        "cache_status": "MISS", "retry_success_count": 0, "mode": "single",
        "last_used_option_index": 0, "response_status_code": status,
        "request_url": "https://x/v1/chat/completions?secret=CANARY-url", "request_method": "post",
        "ai_org": "openai", "api_key_id": "key-1", "license_id": "l-1",
        "log_store_file_path_format": "v1", "metadataKey": ["k"],
        "metadataValue": ["CANARY-metadata"], "prompt_slug": "", "feedback": []})
}

fn policy(id: &str, ws: &str, usage: bool) -> Value {
    let mut p = json!({"id": id, "name": "Team limit", "status": "active", "workspace_id": ws,
        "organisation_id": TSG, "created_at": "2026-10-01T00:00:00Z",
        "last_updated_at": "2026-10-01T00:00:00Z",
        "conditions": [{"key": "api_key", "value": "CANARY-condition"}],
        "group_by": [{"key": "api_key"}]});
    if usage {
        p["type"] = json!("cost");
        p["credit_limit"] = json!(5000);
        p["alert_threshold"] = json!(80);
        p["periodic_reset"] = json!("monthly");
        p["periodic_reset_days"] = Value::Null;
        p["next_usage_reset_at"] = json!("2026-11-01T00:00:00Z");
        p["last_reset_at"] = Value::Null;
    } else {
        p["type"] = json!("requests");
        p["unit"] = json!("rpm");
        p["value"] = json!(60);
    }
    p
}

impl Gateway {
    fn new() -> Self {
        let mut replies = BTreeMap::new();
        replies.insert(
            format!("{DATA}/logs/charts/requests"),
            (200, series(40.0, 4.0)),
        );
        replies.insert(
            format!("{DATA}/logs/charts/cost"),
            (200, json!({"success": true, "data": {"records": [{"x": "2026-10-20T00:00:00Z", "y": 120}],
                "total": 120, "avg": 3, "isQuotaExceeded": false}})),
        );
        replies.insert(
            format!("{DATA}/logs/charts/tokens"),
            (
                200,
                json!({"success": true, "data": {"records": [{"x": "2026-10-20T00:00:00Z", "y": 900,
                "total_request_units": 300, "total_response_units": 600, "avg": 22}],
                "total": 900, "avg": 22, "total_request_units": 300, "total_response_units": 600,
                "isQuotaExceeded": false}}),
            ),
        );
        replies.insert(
            format!("{DATA}/logs/charts/latency"),
            (200, json!({"success": true, "data": {"records": [{"x": "2026-10-20T00:00:00Z", "y": 350,
                "p50": 300, "p90": 500, "p99": 900}], "total": 350, "p50": 300, "p90": 500, "p99": 900,
                "isQuotaExceeded": false}})),
        );
        replies.insert(
            format!("{DATA}/logs/groups/model"),
            (200, json!({"object": "list", "is_quota_exceeded": false, "total": 2, "data": [
                {"object": "group", "model": "gpt-4o", "ai_org": "openai", "requests": 30, "cost": 100},
                {"object": "group", "model": "claude", "ai_org": "anthropic", "requests": 10}]})),
        );
        replies.insert(
            format!("{DATA}/logs"),
            (
                200,
                json!({"success": true, "data": {"records": [
                log_row("log-1", "team-a", "airs-trace-1", 200),
                log_row("log-2", "team-a", "airs-trace-2", 446)],
                "total": 2, "capturedTotal": 2, "isQuotaExceeded": false}}),
            ),
        );
        replies.insert(
            format!("{DATA}/policies/usage-limits"),
            (
                200,
                json!({"object": "list", "total": 1, "data": [policy("ul-1", WS_A, true)]}),
            ),
        );
        replies.insert(
            format!("{DATA}/policies/rate-limits"),
            (
                200,
                json!({"object": "list", "total": 1, "data": [policy("rl-1", WS_A, false)]}),
            ),
        );
        Self {
            chat: Chat::Text(vec!["Hello".into(), ", world".into()]),
            responses: Responses::Text(vec!["Hi".into(), " there".into()]),
            replies,
            files: vec![gateway_file("file-seed", "seed.txt", 4)],
            file_reply: Files::Ok,
            chart_delay: 0,
            serial: 0,
        }
    }

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
        if method == "POST" && path == format!("{RUNTIME}/chat/completions") {
            self.serial += 1;
            let id = format!("chatcmpl-{}", self.serial);
            return match self.chat.clone() {
                Chat::Text(texts) => {
                    let mut parts: Vec<(u64, Vec<u8>)> = texts
                        .iter()
                        .map(|t| (0, chunk(&id, Some(t), None, None)))
                        .collect();
                    parts.push((
                        0,
                        chunk(
                            &id,
                            None,
                            Some("stop"),
                            Some(json!({"prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8})),
                        ),
                    ));
                    parts.push((0, b"data: [DONE]\n\n".to_vec()));
                    Reply::Stream {
                        head: SSE_HEAD.into(),
                        parts,
                        hang: false,
                    }
                }
                Chat::Hang => Reply::Stream {
                    head: SSE_HEAD.into(),
                    parts: vec![(0, chunk(&id, Some("partial"), None, None))],
                    hang: true,
                },
                Chat::Paced { millis, count } => {
                    let mut parts: Vec<(u64, Vec<u8>)> = (0..count)
                        .map(|i| (millis, chunk(&id, Some(&format!("{i:04} ")), None, None)))
                        .collect();
                    parts.push((0, b"data: [DONE]\n\n".to_vec()));
                    Reply::Stream {
                        head: SSE_HEAD.into(),
                        parts,
                        hang: false,
                    }
                }
                Chat::Status(status) => json_reply(
                    status,
                    &json!({"error": {"message": "synthetic refusal", "type": "blocked"}}),
                ),
                Chat::Lost => Reply::Close,
                Chat::NotStream => json_reply(200, &json!({"id": id, "object": "chat.completion"})),
                Chat::Cut => Reply::Stream {
                    head: SSE_HEAD.into(),
                    parts: vec![
                        (0, chunk(&id, Some("part"), None, None)),
                        (
                            0,
                            chunk(
                                &id,
                                None,
                                Some("length"),
                                Some(
                                    json!({"prompt_tokens": 3, "completion_tokens": 256, "total_tokens": 259}),
                                ),
                            ),
                        ),
                        (0, b"data: [DONE]\n\n".to_vec()),
                    ],
                    hang: false,
                },
                Chat::Malformed => Reply::Stream {
                    head: SSE_HEAD.into(),
                    parts: vec![
                        (0, chunk(&id, Some("ok"), None, None)),
                        (0, b"data: {\"unexpected\": true}\n\n".to_vec()),
                        (0, b"data: [DONE]\n\n".to_vec()),
                    ],
                    hang: false,
                },
            };
        }
        if method == "POST" && path == format!("{RUNTIME}/responses") {
            self.serial += 1;
            let id = format!("resp_{}", self.serial);
            let created = event(
                &json!({"type": "response.created", "response": response(&id, "in_progress", false)}),
            );
            let (texts, last) = match self.responses.clone() {
                Responses::Text(texts) => (
                    texts,
                    json!({"type": "response.completed", "response": response(&id, "completed", true)}),
                ),
                Responses::Failed => (
                    vec!["part".into()],
                    json!({"type": "response.failed", "response": response(&id, "failed", false)}),
                ),
                Responses::Incomplete => (
                    vec!["part".into()],
                    json!({"type": "response.incomplete", "response": response(&id, "incomplete", true)}),
                ),
            };
            let mut parts = vec![(0, created)];
            parts.extend(texts.iter().map(|t| (0, delta(t))));
            parts.push((0, event(&last)));
            return Reply::Stream {
                head: SSE_HEAD.into(),
                parts,
                hang: false,
            };
        }
        if path == format!("{RUNTIME}/files") || path.starts_with(&format!("{RUNTIME}/files/")) {
            if method == "GET" && path == format!("{RUNTIME}/files") {
                return json_reply(200, &json!({"object": "list", "data": self.files}));
            }
            match self.file_reply {
                Files::Status(status) => {
                    return json_reply(status, &json!({"error": {"message": "synthetic"}}));
                }
                Files::Lost => return Reply::Close,
                Files::Another(deleted) if method == "DELETE" => {
                    return json_reply(
                        200,
                        &json!({"id": "file-other", "object": "file", "deleted": deleted}),
                    );
                }
                Files::Kept if method == "DELETE" => {
                    let id = path.rsplit('/').next().unwrap_or_default();
                    return json_reply(200, &json!({"id": id, "object": "file", "deleted": false}));
                }
                _ => {}
            }
            if method == "POST" {
                self.serial += 1;
                let parts = multipart(seen);
                let (name, bytes) = parts.get("file").cloned().expect("file part");
                let file = gateway_file(
                    &format!("file-up{}", self.serial),
                    &name.expect("file name"),
                    bytes.len(),
                );
                self.files.insert(0, file.clone());
                return json_reply(200, &file);
            }
            if method == "DELETE" {
                let id = path.rsplit('/').next().unwrap_or_default().to_owned();
                self.files.retain(|f| f["id"] != id.as_str());
                return json_reply(200, &json!({"id": id, "object": "file", "deleted": true}));
            }
        }
        if method == "GET"
            && let Some((status, reply)) = self.replies.get(&path)
        {
            if self.chart_delay > 0 && path.ends_with("/logs/charts/requests") {
                return Reply::Delayed {
                    millis: self.chart_delay,
                    then: Box::new(json_reply(*status, reply)),
                };
            }
            return json_reply(*status, reply);
        }
        json_reply(404, &json!({"error": "not here"}))
    }
}

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    gateway: Arc<Mutex<Gateway>>,
    server: TlsServer,
    ca: TestCa,
}

async fn desk() -> Desk {
    let ca = TestCa::new("airs-v2 c18 ca");
    let gateway = Arc::new(Mutex::new(Gateway::new()));
    let routed = Arc::clone(&gateway);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("gateway").route(seen)
        })),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = fixtures::adapters();
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
        ca,
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

/// Waits up to 10 s for `condition`.
async fn settle(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..1000 {
        if condition() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    condition()
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

    fn playground(&self) -> Value {
        self.json()["gateway"]["playground"].clone()
    }

    fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    fn endpoint(&self) -> String {
        format!("https://localhost:{}{RUNTIME}", self.server.port)
    }

    fn service(&self) -> std::sync::MutexGuard<'_, Gateway> {
        self.gateway.lock().expect("gateway")
    }

    /// Runs one management request and returns its result as JSON.
    async fn read(&mut self, request: Value) -> Value {
        let context = self.context();
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = self
            .send(json!({"kind": "management", "context": context, "request": request}))
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
        let result = tokio::time::timeout(Duration::from_secs(20), handle.result())
            .await
            .expect("read settles");
        serde_json::to_value(result).expect("json")
    }

    /// Lists the workspaces (natively) and chooses `workspace`.
    async fn choose(&mut self, workspace: &str) -> String {
        self.read(json!({"kind": "gateway", "workspace": null,
            "read": {"kind": "inventory", "resource": "workspaces"}}))
            .await;
        assert!(settle(|| self.snap().gateway.workspaces.is_some()).await);
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

    async fn store_key(&mut self, reference: &str, key: &str) -> Result<Receipt, Refusal> {
        let context = self.context();
        let endpoint = self.endpoint();
        let sent = self
            .send(
                json!({"kind": "configureInferenceCredential", "context": context,
                "mode": "keychain", "workspace": reference, "endpoint": endpoint, "secret": key}),
            )
            .await;
        if sent.is_ok() {
            assert!(settle(|| self.json()["tenants"][0]["inference"]["state"] == "ready").await);
        }
        sent
    }

    async fn run(&mut self, reference: &str, run: Value) -> Result<String, Refusal> {
        let context = self.context();
        match self
            .send(json!({"kind": "runInference", "context": context, "workspace": reference, "run": run}))
            .await?
        {
            Receipt::Admitted {
                operation: Some(operation),
                ..
            } => Ok(operation.as_str().to_owned()),
            other => panic!("unexpected receipt {other:?}"),
        }
    }

    async fn ended(&self) -> Value {
        assert!(
            settle(|| self.playground()["state"]
                .as_str()
                .is_some_and(|s| s != "running"))
            .await,
            "run ends"
        );
        self.playground()
    }

    fn files(&self) -> Value {
        self.json()["gateway"]["files"].clone()
    }

    /// Sends a files intent for the chosen workspace.
    async fn file_intent(
        &mut self,
        kind: &str,
        reference: &str,
        file: Option<&str>,
    ) -> Result<Receipt, Refusal> {
        let context = self.context();
        let mut intent = json!({"kind": kind, "context": context, "workspace": reference});
        if let Some(file) = file {
            intent["file"] = json!(file);
        }
        self.send(intent).await
    }

    /// Reads the workspace's files and waits for the read to end.
    async fn read_files(&mut self, reference: &str) -> Value {
        self.file_intent("readGatewayFiles", reference, None)
            .await
            .expect("read admitted");
        assert!(
            settle(
                || self.files()["reading"] == false && !self.files()["files"].is_null()
                    || !self.files()["readFailure"].is_null()
            )
            .await
        );
        self.files()
    }

    /// Chooses `name` with `bytes` in the (scripted) sheet; returns its token.
    async fn choose_file(&mut self, reference: &str, name: &str, bytes: &[u8]) -> String {
        self.adapters
            .gateway_files
            .reply(Some(Ok((name.to_owned(), bytes.to_vec()))));
        self.file_intent("chooseGatewayFile", reference, None)
            .await
            .expect("sheet");
        assert!(settle(|| self.files()["chosen"]["name"] == name).await);
        self.files()["chosen"]["token"]
            .as_str()
            .expect("token")
            .to_owned()
    }

    /// Waits for the upload or delete going now to settle.
    async fn file_settled(&self) -> Value {
        assert!(settle(|| !self.files()["action"]["outcome"].is_null()).await);
        self.files()["action"].clone()
    }

    fn file_requests(&self, method: &str) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| {
                s.request_line
                    .starts_with(&format!("{method} {RUNTIME}/files"))
            })
            .collect()
    }

    fn chats(&self) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| {
                s.request_line
                    .starts_with(&format!("POST {RUNTIME}/chat/completions"))
            })
            .collect()
    }
}

/// The same Mac relaunched with no network: the saved tenant, Keychain and
/// local files, and a management origin that refuses connections.
struct Offline {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
}

impl Offline {
    fn start(desk: &Desk) -> Self {
        let saved = desk.adapters.store.saved().expect("saved");
        let (mut adapters, _handles) =
            fixtures::adapters_with(fixtures::ScriptedStore::with_state(saved));
        adapters.keychain = desk.adapters.vault.clone();
        adapters.sessions_root = Some(desk.adapters.sessions_root.clone());
        let core = Core::new(
            CoreConfig {
                management: ManagementEndpoints::at_origin("https://localhost:9"),
                ..CoreConfig::default()
            },
            adapters,
        );
        core.start();
        let handle = core.operator();
        let session = handle.register_delivery_session().expect("session").nonce;
        Self {
            _core: core,
            handle,
            session,
            sequence: 0,
        }
    }

    fn json(&self) -> Value {
        match self.handle.observe(ViewScope::Shell).expect("observe") {
            Observation::Shell { snapshot, .. } => serde_json::to_value(snapshot).expect("json"),
            _ => unreachable!("shell"),
        }
    }

    fn context(&self) -> String {
        self.json()["active"]["context"]
            .as_str()
            .expect("context")
            .to_owned()
    }

    async fn send(&mut self, intent: Value) -> Result<Receipt, Refusal> {
        self.sequence += 1;
        self.handle
            .dispatch(
                Delivery {
                    session: self.session.clone(),
                    sequence: self.sequence,
                },
                serde_json::from_value(intent).expect("intent"),
            )
            .await
    }

    /// Opens the local book and returns it.
    async fn local(&mut self) -> Value {
        let context = self.context();
        self.send(json!({"kind": "openGatewayLocal", "context": context}))
            .await
            .expect("opened");
        assert!(settle(|| self.json()["gateway"]["local"]["state"] == "ready").await);
        self.json()["gateway"]["local"].clone()
    }
}

fn prompt(text: &str) -> Value {
    json!({"model": "@openai-prod/gpt-4o", "prompt": text})
}

#[tokio::test(flavor = "multi_thread")]
async fn the_inference_key_is_bound_to_its_workspace_and_endpoint_and_never_leaves_the_core() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let context = desk.context();
    // Only the workspace chosen now, an HTTPS endpoint without credentials
    // or a query, and a header-safe key are accepted.
    for (workspace, endpoint, key) in [
        (
            "gw-000000000000000000000000",
            desk.endpoint(),
            INFERENCE_KEY.to_owned(),
        ),
        (
            reference.as_str(),
            "http://localhost:1/rt/v1".to_owned(),
            INFERENCE_KEY.to_owned(),
        ),
        (
            reference.as_str(),
            format!("{}?key=x", desk.endpoint()),
            INFERENCE_KEY.to_owned(),
        ),
        (reference.as_str(), desk.endpoint(), "two words".to_owned()),
    ] {
        assert!(
            desk.send(
                json!({"kind": "configureInferenceCredential", "context": context,
                "mode": "keychain", "workspace": workspace, "endpoint": endpoint, "secret": key})
            )
            .await
            .is_err(),
            "{workspace} {endpoint}"
        );
    }
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    let snapshot = desk.json();
    assert_eq!(
        snapshot["tenants"][0]["inference"]["target"]["workspace"]["id"],
        WS_A
    );
    assert_eq!(
        snapshot["tenants"][0]["inference"]["target"]["endpoint"],
        desk.endpoint()
    );
    let saved = format!("{:?}", desk.adapters.store.saved().expect("saved"));
    for text in [snapshot.to_string(), saved.clone()] {
        assert!(!text.contains(INFERENCE_KEY) && !text.contains(CLIENT_SECRET));
    }
    assert!(saved.contains(WS_A), "the binding is saved, never the key");
    // Storing the key leaves the tenant context and workspace choice alone.
    assert_eq!(desk.context(), context);
    assert_eq!(
        desk.json()["gateway"]["selected"]["reference"],
        reference.as_str()
    );
    // A run sends the inference key, and only it, to the stored endpoint.
    desk.run(&reference, prompt("hi")).await.expect("run");
    desk.ended().await;
    let chats = desk.chats();
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].header("x-portkey-api-key"), Some(INFERENCE_KEY));
    assert_eq!(
        chats[0].header("authorization"),
        None,
        "the management token never goes there"
    );
    // In another workspace the key is not offered.
    let other = desk.choose(WS_B).await;
    assert_eq!(
        desk.run(&other, prompt("hi")).await,
        Err(Refusal::CapabilityMissing)
    );
    // Removing it deletes the Keychain item.
    let records = desk.adapters.vault.records().len();
    let context = desk.context();
    desk.send(json!({"kind": "removeInferenceCredential", "context": context}))
        .await
        .expect("removed");
    assert!(settle(|| desk.adapters.vault.records().len() < records).await);
    assert_eq!(desk.json()["tenants"][0]["inference"]["state"], "missing");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_streams_its_output_once_with_its_route_and_trace_and_is_never_retried() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    desk.run(
        &reference,
        json!({"model": "@openai-prod/gpt-4o", "route": ROUTE, "system": "be brief", "prompt": "hi"}),
    )
    .await
    .expect("run");
    let view = desk.ended().await;
    assert_eq!(view["state"], "completed");
    assert_eq!(view["outcome"], "completed");
    assert_eq!(view["output"], "Hello, world");
    assert_eq!(view["servedModel"], "gpt-4o");
    assert_eq!(view["finish"], "stop");
    assert_eq!(view["usage"]["total"], 8);
    assert_eq!(view["workspace"]["id"], WS_A);
    let chats = desk.chats();
    assert_eq!(chats.len(), 1, "sent once");
    let trace = view["trace"].as_str().expect("trace");
    assert_eq!(chats[0].header("x-portkey-trace-id"), Some(trace));
    assert_eq!(chats[0].header("x-portkey-config"), Some(ROUTE));
    let body: Value = serde_json::from_slice(&chats[0].body).expect("body");
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(
        body["messages"][0],
        json!({"role": "system", "content": "be brief"})
    );
    assert_eq!(
        body["messages"][1],
        json!({"role": "user", "content": "hi"})
    );
    // Without a route nothing routes it, and a refusal is never retried
    // or sent elsewhere.
    // A definite refusal (4xx) is rejected; a 5xx, a lost handshake or a
    // malformed successful one is the SDK's unknown: the Gateway may have
    // run it.
    for (chat, failure, outcome) in [
        (Chat::Status(446), "blocked", "rejected"),
        (Chat::Status(429), "rateLimited", "rejected"),
        (Chat::Status(401), "unauthorized", "rejected"),
        (Chat::Status(403), "denied", "rejected"),
        (Chat::Status(500), "unavailable", "unknown"),
        (Chat::Status(502), "unavailable", "unknown"),
        (Chat::Status(503), "unavailable", "unknown"),
        (Chat::Status(504), "unavailable", "unknown"),
        (Chat::Lost, "network", "unknown"),
        (Chat::NotStream, "invalidResponse", "unknown"),
    ] {
        let status = match &chat {
            Chat::Status(s) => s.to_string(),
            Chat::Lost => "lost".into(),
            _ => "not a stream".into(),
        };
        let operation = desk.playground()["operation"]
            .as_str()
            .expect("op")
            .to_owned();
        desk.send(json!({"kind": "clearInference", "operation": operation}))
            .await
            .expect("cleared");
        desk.service().chat = chat;
        let before = desk.server.seen().len();
        desk.run(&reference, prompt("again")).await.expect("run");
        let view = desk.ended().await;
        assert_eq!(view["state"], "failed", "{status}");
        assert_eq!(view["failure"], failure, "{status}");
        assert_eq!(view["outcome"], outcome, "{status}");
        let seen = desk.server.seen();
        assert_eq!(
            seen.len(),
            before + 1,
            "{status}: one request, nothing else"
        );
        assert_eq!(seen.last().expect("last").header("x-portkey-config"), None);
    }
    // Only one run at a time.
    let operation = desk.playground()["operation"]
        .as_str()
        .expect("op")
        .to_owned();
    desk.send(json!({"kind": "clearInference", "operation": operation}))
        .await
        .expect("cleared");
    desk.service().chat = Chat::Hang;
    desk.run(&reference, prompt("long")).await.expect("run");
    assert!(settle(|| desk.playground()["output"] == "partial").await);
    assert_eq!(
        desk.run(&reference, prompt("again")).await,
        Err(Refusal::Busy)
    );
    // Invalid runs are refused before anything is sent.
    let before = desk.server.seen().len();
    for run in [
        json!({"model": "", "prompt": "x"}),
        json!({"model": "m", "prompt": "   "}),
        json!({"model": "m", "route": "a b", "prompt": "x"}),
        json!({"model": "m", "prompt": "x".repeat(32 * 1024 + 1)}),
    ] {
        assert!(desk.run(&reference, run).await.is_err());
    }
    assert_eq!(desk.server.seen().len(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn output_is_bounded_and_a_malformed_chunk_ends_the_run() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    // 70 KiB of output: 64 KiB are kept and the stream is closed.
    desk.service().chat = Chat::Text(vec!["x".repeat(1024); 70]);
    desk.run(&reference, prompt("long")).await.expect("run");
    let view = desk.ended().await;
    assert_eq!(view["state"], "truncated");
    assert_eq!(view["output"].as_str().map(str::len), Some(64 * 1024));
    let operation = view["operation"].as_str().expect("op").to_owned();
    desk.send(json!({"kind": "clearInference", "operation": operation}))
        .await
        .expect("cleared");
    desk.service().chat = Chat::Malformed;
    desk.run(&reference, prompt("bad")).await.expect("run");
    let view = desk.ended().await;
    assert_eq!(view["state"], "failed");
    assert_eq!(view["failure"], "invalidResponse");
    assert_eq!(view["output"], "ok", "what arrived before stays");
    assert_eq!(view["outcome"], "completed", "the Gateway answered");
    assert_eq!(desk.chats().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_ends_the_stream_and_never_claims_nothing_was_charged() {
    let mut desk = desk().await;
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    desk.service().chat = Chat::Hang;
    let operation = desk.run(&reference, prompt("long")).await.expect("run");
    assert!(
        settle(|| desk.playground()["output"] == "partial").await,
        "first chunk shown"
    );
    desk.send(json!({"kind": "stopInference", "operation": operation}))
        .await
        .expect("stopped");
    let view = desk.ended().await;
    assert_eq!(view["state"], "stopped");
    assert_eq!(
        view["outcome"], "unknown",
        "stopping proves nothing about charges"
    );
    assert_eq!(view["output"], "partial");
    assert!(
        settle(|| desk.server.log.lock().expect("log").closed_by_client >= 1).await,
        "stream closed"
    );
    // The run is kept on this Mac without its output.
    assert!(
        settle(|| desk.json()["gateway"]["local"]["runs"]
            .as_array()
            .is_some_and(|r| r.len() == 1))
        .await
    );
    let record = desk.json()["gateway"]["local"]["runs"][0].clone();
    assert_eq!(record["trace"], view["trace"]);
    assert_eq!(record["end"], "stopped");
    assert_eq!(record["outputBytes"], 7);
    assert!(!record.to_string().contains("partial"));
    // A stopped run's operation cannot be stopped again.
    assert!(
        desk.send(json!({"kind": "stopInference", "operation": operation}))
            .await
            .is_err()
    );
    assert_eq!(desk.chats().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn another_workspace_tenant_or_key_stops_the_run_and_its_late_output_is_dropped() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    // Another workspace is chosen mid-stream.
    desk.service().chat = Chat::Paced {
        millis: 50,
        count: 200,
    };
    desk.run(&reference, prompt("long")).await.expect("run");
    assert!(
        settle(|| desk.playground()["output"]
            .as_str()
            .is_some_and(|o| !o.is_empty()))
        .await
    );
    desk.choose(WS_B).await;
    assert!(
        desk.playground().is_null(),
        "nothing of it is shown under another workspace"
    );
    assert!(settle(|| desk.server.log.lock().expect("log").closed_by_client >= 1).await);
    // Choosing the first workspace again does not bring it back.
    let reference = desk.choose(WS_A).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(desk.playground().is_null());
    // The key is replaced mid-stream: the run stops, the context and
    // workspace choice stay.
    desk.run(&reference, prompt("long")).await.expect("run");
    assert!(
        settle(|| desk.playground()["output"]
            .as_str()
            .is_some_and(|o| !o.is_empty()))
        .await
    );
    let context = desk.context();
    desk.store_key(&reference, OTHER_KEY)
        .await
        .expect("replaced");
    assert!(settle(|| desk.playground().is_null()).await);
    assert!(settle(|| desk.server.log.lock().expect("log").closed_by_client >= 2).await);
    assert_eq!(desk.context(), context);
    assert_eq!(
        desk.json()["gateway"]["selected"]["reference"],
        reference.as_str()
    );
    // The new key is the one sent next.
    desk.service().chat = Chat::Text(vec!["ok".into()]);
    desk.run(&reference, prompt("again")).await.expect("run");
    desk.ended().await;
    assert_eq!(
        desk.chats()
            .last()
            .expect("chat")
            .header("x-portkey-api-key"),
        Some(OTHER_KEY)
    );
    // A tenant context change (credential replacement) stops it too.
    let operation = desk.playground()["operation"]
        .as_str()
        .expect("op")
        .to_owned();
    desk.send(json!({"kind": "clearInference", "operation": operation}))
        .await
        .expect("cleared");
    desk.service().chat = Chat::Paced {
        millis: 50,
        count: 200,
    };
    desk.run(&reference, prompt("long")).await.expect("run");
    assert!(
        settle(|| desk.playground()["output"]
            .as_str()
            .is_some_and(|o| !o.is_empty()))
        .await
    );
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("replaced");
    assert!(desk.playground().is_null());
    assert!(settle(|| desk.server.log.lock().expect("log").closed_by_client >= 3).await);
    let chats = desk.chats().len();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(desk.chats().len(), chats, "nothing is retried or replayed");
}

#[tokio::test(flavor = "multi_thread")]
async fn usage_logs_and_limits_read_only_the_chosen_workspace_and_keep_each_sources_failure() {
    let mut desk = desk().await;
    // Observation reads are scoped: refused without the current reference.
    let context = desk.context();
    assert!(
        desk.send(
            json!({"kind": "management", "context": context, "request": {"kind": "gateway",
            "workspace": null, "read": {"kind": "observe", "read": {"kind": "usage", "days": 7}}}})
        )
        .await
        .is_err()
    );
    let reference = desk.choose(WS_A).await;
    let observe = |read: Value| json!({"kind": "gateway", "workspace": reference, "read": {"kind": "observe", "read": read}});
    let usage = desk
        .read(observe(json!({"kind": "usage", "days": 7})))
        .await;
    let usage = &usage["gateway"]["observed"]["usage"];
    assert_eq!(usage["workspace"], WS_A);
    assert_eq!(usage["requests"]["total"], 40.0);
    assert_eq!(usage["requests"]["errors"], 4.0);
    assert_eq!(usage["cost"]["total"], 120.0);
    assert_eq!(usage["latency"]["total"], 350.0);
    assert_eq!(usage["models"]["rows"][0]["model"], "gpt-4o");
    let charts: Vec<Seen> = desk
        .server
        .seen()
        .into_iter()
        .filter(|s| s.request_line.contains("/logs/charts/"))
        .collect();
    assert_eq!(charts.len(), 4);
    assert!(
        charts
            .iter()
            .all(|s| s.request_line.contains("workspaceSlug=team-a")
                && s.request_line.contains(&format!("organisationId={TSG}")))
    );
    // One source failing keeps the others.
    desk.service().replies.insert(
        format!("{DATA}/logs/charts/cost"),
        (500, json!({"error": "down"})),
    );
    let usage = desk
        .read(observe(json!({"kind": "usage", "days": 1})))
        .await;
    let usage = &usage["gateway"]["observed"]["usage"];
    assert_eq!(usage["costFailure"], "unavailable");
    assert!(usage["cost"].is_null());
    assert_eq!(usage["requests"]["total"], 40.0);
    // Logs: allow-listed rows of this workspace only.
    let logs = desk
        .read(observe(
            json!({"kind": "logs", "days": 7, "page": 0, "trace": "airs-trace-2"}),
        ))
        .await;
    assert_eq!(
        logs["kind"], "failed",
        "a row without the filtered trace is invalid"
    );
    desk.service().replies.insert(
        format!("{DATA}/logs"),
        (200, json!({"success": true, "data": {"records": [log_row("log-2", "team-a", "airs-trace-2", 446)],
            "total": 1, "capturedTotal": 1, "isQuotaExceeded": false}})),
    );
    let logs = desk
        .read(observe(
            json!({"kind": "logs", "days": 7, "page": 0, "trace": "airs-trace-2"}),
        ))
        .await;
    let page = &logs["gateway"]["observed"]["logs"];
    assert_eq!(page["rows"][0]["status"], 446);
    assert_eq!(page["more"], false);
    let text = logs.to_string();
    assert!(!text.contains("CANARY") && !text.contains("person@example.com"));
    let line = desk
        .server
        .seen()
        .into_iter()
        .rev()
        .find(|s| s.request_line.starts_with(&format!("GET {DATA}/logs?")))
        .expect("logs read")
        .request_line;
    assert!(line.contains("traceId=airs-trace-2") && line.contains("pageSize=25"));
    // The last page read (the 1,000-row cap) with more reported is capped,
    // never "all read".
    let full: Vec<Value> = (0..25)
        .map(|i| log_row(&format!("log-c{i}"), "team-a", "t", 200))
        .collect();
    desk.service().replies.insert(
        format!("{DATA}/logs"),
        (
            200,
            json!({"success": true, "data": {"records": full, "total": 5000,
            "capturedTotal": 5000, "isQuotaExceeded": false}}),
        ),
    );
    let capped = desk
        .read(observe(json!({"kind": "logs", "days": 7, "page": 39})))
        .await;
    let page = &capped["gateway"]["observed"]["logs"];
    assert_eq!(
        (page["more"].clone(), page["capped"].clone()),
        (json!(false), json!(true))
    );
    assert_eq!(page["total"], 5000);
    desk.service().replies.insert(
        format!("{DATA}/logs"),
        (
            200,
            json!({"success": true, "data": {"records": [log_row("log-9", "team-b", "t", 200)],
            "total": 1, "capturedTotal": 1, "isQuotaExceeded": false}}),
        ),
    );
    let foreign = desk
        .read(observe(json!({"kind": "logs", "days": 7, "page": 0})))
        .await;
    assert_eq!(
        foreign["kind"], "failed",
        "another workspace's row is never shown"
    );
    // Limits: the policies that name this workspace; conditions stay here.
    let limits = desk.read(observe(json!({"kind": "limits"}))).await;
    let limits_view = &limits["gateway"]["observed"]["limits"];
    assert_eq!(limits_view["usage"]["policies"][0]["creditLimit"], 5000.0);
    assert_eq!(limits_view["rate"]["policies"][0]["unit"], "rpm");
    assert!(!limits.to_string().contains("CANARY"));
    assert!(desk.server.seen().iter().any(|s| {
        s.request_line
            .starts_with(&format!("GET {DATA}/policies/usage-limits?"))
            && s.request_line.contains(&format!("workspace_id={WS_A}"))
    }));
    desk.service().replies.insert(
        format!("{DATA}/policies/rate-limits"),
        (403, json!({"error": "denied"})),
    );
    let limits = desk.read(observe(json!({"kind": "limits"}))).await;
    assert_eq!(
        limits["gateway"]["observed"]["limits"]["rateFailure"],
        "denied"
    );
    assert_eq!(limits["gateway"]["observed"]["limits"]["usage"]["total"], 1);
    // A read admitted under one choice is withdrawn after another.
    let stale = observe(json!({"kind": "limits"}));
    desk.choose(WS_B).await;
    let context = desk.context();
    assert!(
        desk.send(json!({"kind": "management", "context": context, "request": stale}))
            .await
            .is_err()
    );
    // Nothing here sent inference.
    assert!(desk.chats().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_overview_reports_the_chosen_workspaces_gateway_day_with_its_source_state() {
    let mut desk = desk().await;
    let overview = desk.read(json!({"kind": "overview"})).await;
    assert!(
        overview["overview"]["gateway"].is_null(),
        "no workspace chosen: not read"
    );
    desk.choose(WS_A).await;
    let overview = desk.read(json!({"kind": "overview"})).await;
    let gateway = &overview["overview"]["gateway"];
    assert_eq!(gateway["workspaceName"], "Team A");
    assert_eq!(gateway["requests"], 40.0);
    assert_eq!(gateway["errors"], 4.0);
    assert_eq!(gateway["cost"], 120.0);
    assert_eq!(gateway["policies"], 2);
    let items = overview["overview"]["items"].as_array().expect("items");
    let item = items
        .iter()
        .find(|i| i["destination"] == "gateway")
        .expect("gateway item");
    assert_eq!(item["tone"], "bad", "one error in ten");
    assert_eq!(item["perMille"], 100);
    assert!(
        settle(|| desk.json()["overview"]["attention"]
            .as_array()
            .is_some_and(|a| a.iter().any(|i| i["destination"] == "gateway")))
        .await
    );
    // A failed source is a gap, never "all clear".
    desk.service().replies.insert(
        format!("{DATA}/logs/charts/requests"),
        (500, json!({"error": "down"})),
    );
    let overview = desk.read(json!({"kind": "overview"})).await;
    assert_eq!(
        overview["overview"]["gateway"]["usageFailure"],
        "unavailable"
    );
    assert!(
        overview["overview"]["gaps"]
            .as_array()
            .is_some_and(|g| g.iter().any(|g| g == "Gateway activity unavailable"))
    );
    assert!(desk.chats().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn playground_drafts_and_uncertain_runs_survive_an_offline_restart_and_nothing_is_replayed() {
    let mut desk = desk().await;
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| desk.json()["gateway"]["local"]["state"] == "ready").await);
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    let context = desk.context();
    desk.send(
        json!({"kind": "savePlaygroundDraft", "context": context, "workspace": reference,
        "draft": {"model": "@openai-prod/gpt-4o", "route": ROUTE, "prompt": "draft prompt"}}),
    )
    .await
    .expect("kept");
    // A run that fails on the network after being sent is uncertain.
    desk.service().chat = Chat::Hang;
    let operation = desk
        .run(&reference, prompt("draft prompt"))
        .await
        .expect("run");
    assert!(settle(|| desk.playground()["output"] == "partial").await);
    desk.send(json!({"kind": "stopInference", "operation": operation}))
        .await
        .expect("stopped");
    desk.ended().await;
    assert!(
        settle(|| desk.json()["gateway"]["local"]["runs"]
            .as_array()
            .is_some_and(|r| r.len() == 1))
        .await
    );
    let trace = desk.json()["gateway"]["local"]["runs"][0]["trace"]
        .as_str()
        .expect("trace")
        .to_owned();
    let tenant = desk.snap().active.expect("active").tenant;
    let path = desktop_core::gateway_local::path(&desk.adapters.sessions_root, tenant.as_str());
    assert!(settle(|| std::fs::read_to_string(&path).is_ok_and(|f| f.contains(&trace))).await);
    let file = std::fs::read_to_string(&path).expect("kept");
    assert!(
        file.contains("draft prompt") && !file.contains(INFERENCE_KEY) && !file.contains("partial")
    );
    let chats = desk.chats().len();
    // Offline restart: the draft and the run are there; nothing is sent.
    let mut offline = Offline::start(&desk);
    let context = offline.context();
    let local = offline.local().await;
    assert_eq!(local["playground"][0]["prompt"], "draft prompt");
    assert_eq!(local["playground"][0]["workspace"]["id"], WS_A);
    assert_eq!(local["runs"][0]["outcome"], "unknown");
    assert_eq!(local["runs"][0]["trace"], trace.as_str());
    assert_eq!(
        offline.json()["tenants"][0]["inference"]["state"],
        "ready",
        "the key binding is restored"
    );
    assert!(
        offline.json()["gateway"]["playground"].is_null(),
        "no run is restored"
    );
    assert!(
        offline.json()["gateway"]["workspaces"].is_null(),
        "no workspace listing"
    );
    // The kept draft is edited by its ID, under the workspace it was kept
    // for, with no listing or workspace reference (T23).
    let id = local["playground"][0]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    offline
        .send(json!({"kind": "savePlaygroundDraft", "context": context,
            "draft": {"id": id, "api": "responses", "model": "@openai-prod/gpt-4o-mini",
            "system": "be brief", "prompt": "edited offline"}}))
        .await
        .expect("edited");
    let local = offline.json()["gateway"]["local"].clone();
    assert_eq!(local["playground"].as_array().map(Vec::len), Some(1));
    assert_eq!(local["playground"][0]["id"], id.as_str());
    assert_eq!(local["playground"][0]["prompt"], "edited offline");
    assert_eq!(local["playground"][0]["api"], "responses");
    assert_eq!(local["playground"][0]["workspace"]["id"], WS_A);
    // An unknown draft, or an ID with a workspace reference, is refused.
    for draft in [
        json!({"kind": "savePlaygroundDraft", "context": context,
            "draft": {"id": "gp-000000000000000000000000", "model": "m", "prompt": "x"}}),
        json!({"kind": "savePlaygroundDraft", "context": context, "workspace": reference,
            "draft": {"id": id, "model": "m", "prompt": "x"}}),
        json!({"kind": "savePlaygroundDraft", "context": context,
            "draft": {"model": "m", "prompt": "x"}}),
    ] {
        assert!(offline.send(draft).await.is_err());
    }
    // A run needs a workspace chosen from a fresh listing.
    assert!(
        offline
            .send(json!({"kind": "runInference", "context": context,
                "workspace": reference, "run": prompt("edited offline")}))
            .await
            .is_err()
    );
    // The edit is on disk: a second offline relaunch has it.
    assert!(
        settle(|| std::fs::read_to_string(&path).is_ok_and(|f| f.contains("edited offline"))).await
    );
    drop(offline);
    let mut again = Offline::start(&desk);
    let local = again.local().await;
    assert_eq!(local["playground"][0]["prompt"], "edited offline");
    assert_eq!(local["runs"][0]["trace"], trace.as_str());
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(desk.chats().len(), chats, "nothing replayed");
    let _ = (&desk.core, &desk.ca);
}

#[tokio::test(flavor = "multi_thread")]
async fn uncertain_openings_keep_their_trace_across_a_restart_and_are_never_replayed() {
    let mut desk = desk().await;
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| desk.json()["gateway"]["local"]["state"] == "ready").await);
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    let mut traces = Vec::new();
    for (chat, outcome) in [
        (Chat::Status(500), "unknown"),
        (Chat::Status(502), "unknown"),
        (Chat::Status(503), "unknown"),
        (Chat::Status(504), "unknown"),
        (Chat::Lost, "unknown"),
        (Chat::NotStream, "unknown"),
        (Chat::Status(400), "rejected"),
        (Chat::Status(446), "rejected"),
    ] {
        if let Some(operation) = desk.playground()["operation"].as_str().map(str::to_owned) {
            desk.send(json!({"kind": "clearInference", "operation": operation}))
                .await
                .expect("cleared");
        }
        desk.service().chat = chat;
        let before = desk.chats().len();
        desk.run(&reference, prompt("once")).await.expect("run");
        let view = desk.ended().await;
        assert_eq!(view["outcome"], outcome);
        assert_eq!(view["sent"], true);
        assert_eq!(desk.chats().len(), before + 1, "sent once");
        if outcome == "unknown" {
            traces.push(view["trace"].as_str().expect("trace").to_owned());
        }
    }
    // Every uncertain run is kept with its trace; a refusal is not.
    let kept = || {
        desk.json()["gateway"]["local"]["runs"]
            .as_array()
            .map(|runs| {
                runs.iter()
                    .map(|r| {
                        (
                            r["trace"].as_str().unwrap_or_default().to_owned(),
                            r["outcome"].clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    assert!(
        settle(|| kept().len() == traces.len()).await,
        "{:?}",
        kept()
    );
    for (trace, outcome) in kept() {
        assert!(traces.contains(&trace));
        assert_eq!(outcome, "unknown");
    }
    let tenant = desk.snap().active.expect("active").tenant;
    let path = desktop_core::gateway_local::path(&desk.adapters.sessions_root, tenant.as_str());
    let last = traces.last().expect("trace").clone();
    assert!(settle(|| std::fs::read_to_string(&path).is_ok_and(|f| f.contains(&last))).await);
    let chats = desk.chats().len();
    let mut offline = Offline::start(&desk);
    let local = offline.local().await;
    let restored: Vec<&str> = local["runs"]
        .as_array()
        .expect("runs")
        .iter()
        .filter(|r| r["outcome"] == "unknown")
        .filter_map(|r| r["trace"].as_str())
        .collect();
    assert_eq!(restored.len(), traces.len());
    assert!(traces.iter().all(|t| restored.contains(&t.as_str())));
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(desk.chats().len(), chats, "nothing replayed or retried");
    let _ = (&desk.core, &desk.ca);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replaced_or_removed_key_withdraws_its_run_from_every_snapshot_at_once() {
    let mut desk = desk().await;
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| desk.json()["gateway"]["local"]["state"] == "ready").await);
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    for removal in [false, true] {
        if let Some(operation) = desk.playground()["operation"].as_str().map(str::to_owned) {
            desk.send(json!({"kind": "clearInference", "operation": operation}))
                .await
                .expect("cleared");
        }
        if removal {
            desk.store_key(&reference, INFERENCE_KEY)
                .await
                .expect("stored");
        }
        let closed = desk.server.log.lock().expect("log").closed_by_client;
        desk.service().chat = Chat::Hang;
        desk.run(&reference, prompt("long")).await.expect("run");
        assert!(settle(|| desk.playground()["output"] == "partial").await);
        let trace = desk.playground()["trace"]
            .as_str()
            .expect("trace")
            .to_owned();
        // Subscribe, then drain what was published before the key changes.
        let Observation::Shell { mut updates, .. } =
            desk.handle.observe(ViewScope::Shell).expect("observe")
        else {
            unreachable!("shell scope")
        };
        while updates.try_next().is_some() {}
        let context = desk.context();
        let workspace = desk.json()["gateway"]["selected"].clone();
        if removal {
            desk.send(json!({"kind": "removeInferenceCredential", "context": context}))
                .await
                .expect("removed");
        } else {
            let endpoint = desk.endpoint();
            desk.send(
                json!({"kind": "configureInferenceCredential", "context": context,
                "mode": "keychain", "workspace": reference, "endpoint": endpoint, "secret": OTHER_KEY}),
            )
            .await
            .expect("replaced");
        }
        // From admission on, no observation shows the run or its output,
        // while its cancellation is still under way.
        assert!(desk.playground().is_null(), "withdrawn at admission");
        let mut published = 0;
        for _ in 0..30 {
            assert!(desk.playground().is_null());
            while let Some(update) = updates.try_next() {
                if let desktop_core::Update::Shell(snapshot) = update {
                    published += 1;
                    let json = serde_json::to_value(&*snapshot).expect("json");
                    assert!(
                        json["gateway"]["playground"].is_null(),
                        "published without it"
                    );
                    assert!(!json.to_string().contains("partial"));
                }
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(published > 0, "the change was published");
        assert!(
            settle(|| desk.server.log.lock().expect("log").closed_by_client > closed).await,
            "the stream is closed"
        );
        // Its end is still kept, without its output.
        assert!(
            settle(|| desk.json()["gateway"]["local"]["runs"]
                .as_array()
                .is_some_and(|r| r.iter().any(|r| r["trace"] == trace.as_str())))
            .await
        );
        assert!(desk.playground().is_null());
        assert_eq!(desk.context(), context, "the context stays");
        assert_eq!(
            desk.json()["gateway"]["selected"],
            workspace,
            "the choice stays"
        );
    }
    // The next run uses only the key stored last.
    desk.store_key(&reference, OTHER_KEY).await.expect("stored");
    desk.service().chat = Chat::Text(vec!["ok".into()]);
    desk.run(&reference, prompt("again")).await.expect("run");
    desk.ended().await;
    assert_eq!(
        desk.chats()
            .last()
            .expect("chat")
            .header("x-portkey-api-key"),
        Some(OTHER_KEY)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_responses_run_streams_once_and_reports_how_it_ended() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    desk.run(
        &reference,
        json!({"api": "responses", "model": "@openai-prod/gpt-4o", "route": ROUTE,
            "system": "be brief", "prompt": "hi"}),
    )
    .await
    .expect("run");
    let view = desk.ended().await;
    assert_eq!(view["api"], "responses");
    assert_eq!(view["state"], "completed");
    assert_eq!(view["outcome"], "completed");
    assert_eq!(view["output"], "Hi there");
    assert_eq!(view["servedModel"], "gpt-4o-2024");
    assert_eq!(view["responseId"], "resp_1");
    assert_eq!(view["finish"], "completed");
    assert_eq!(view["usage"]["total"], 5);
    let sent: Vec<Seen> = desk
        .server
        .seen()
        .into_iter()
        .filter(|s| {
            s.request_line
                .starts_with(&format!("POST {RUNTIME}/responses"))
        })
        .collect();
    assert_eq!(sent.len(), 1, "sent once");
    assert_eq!(sent[0].header("x-portkey-api-key"), Some(INFERENCE_KEY));
    assert_eq!(sent[0].header("x-portkey-config"), Some(ROUTE));
    assert_eq!(sent[0].header("x-portkey-trace-id"), view["trace"].as_str());
    let body: Value = serde_json::from_slice(&sent[0].body).expect("body");
    assert_eq!(body["input"], "hi");
    assert_eq!(body["instructions"], "be brief");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["max_output_tokens"], 256);
    assert!(desk.chats().is_empty(), "nothing went to chat completions");
    // A model failure ends it as failed (the Gateway answered); an
    // incomplete response is incomplete, with its reason, its partial
    // output, usage and trace, and is not kept as uncertain.
    for (responses, state, failure, finish) in [
        (
            Responses::Failed,
            "failed",
            json!("modelFailed"),
            Value::Null,
        ),
        (
            Responses::Incomplete,
            "incomplete",
            Value::Null,
            json!("max_output_tokens"),
        ),
    ] {
        let operation = desk.playground()["operation"]
            .as_str()
            .expect("op")
            .to_owned();
        desk.send(json!({"kind": "clearInference", "operation": operation}))
            .await
            .expect("cleared");
        desk.service().responses = responses;
        desk.run(
            &reference,
            json!({"api": "responses", "model": "@openai-prod/gpt-4o", "prompt": "again"}),
        )
        .await
        .expect("run");
        let view = desk.ended().await;
        assert_eq!(view["state"], state);
        assert_eq!(view["failure"], failure);
        assert_eq!(view["finish"], finish);
        assert_eq!(view["output"], "part");
        assert_eq!(view["outcome"], "completed", "the Gateway answered");
        assert!(
            view["trace"]
                .as_str()
                .is_some_and(|t| t.starts_with("airs-"))
        );
        if state == "incomplete" {
            assert_eq!(view["usage"]["total"], 5);
        }
    }
    // A chat answer cut at the token limit is incomplete too.
    let operation = desk.playground()["operation"]
        .as_str()
        .expect("op")
        .to_owned();
    desk.send(json!({"kind": "clearInference", "operation": operation}))
        .await
        .expect("cleared");
    desk.service().chat = Chat::Cut;
    desk.run(&reference, prompt("long")).await.expect("run");
    let view = desk.ended().await;
    assert_eq!(view["state"], "incomplete");
    assert_eq!(view["finish"], "length");
    assert_eq!(view["output"], "part");
    assert_eq!(view["usage"]["completion"], 256);
    assert_eq!(view["outcome"], "completed");
    let total = desk.server.seen().len();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(desk.server.seen().len(), total, "nothing is sent again");
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| desk.json()["gateway"]["local"]["state"] == "ready").await);
    let runs = desk.json()["gateway"]["local"]["runs"].clone();
    assert_eq!(
        runs.as_array().map(Vec::len),
        Some(1),
        "only the failed run is kept: one cut short was answered and ended"
    );
    assert_eq!(runs[0]["failure"], "modelFailed");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_gateway_hold_ends_only_from_a_known_day_of_its_own_workspace() {
    let mut desk = desk().await;
    let item_a = format!("gateway-errors-{WS_A}");
    let set_day = |desk: &Desk, reply: Value| {
        desk.service()
            .replies
            .insert(format!("{DATA}/logs/charts/requests"), (200, reply));
    };
    let overview = |desk: &Desk| desk.json()["overview"].clone();
    let shows = |view: &Value, list: &str, id: &str| {
        view[list]
            .as_array()
            .is_some_and(|items| items.iter().any(|i| i["id"] == id || i["item"]["id"] == id))
    };
    desk.choose(WS_A).await;
    desk.read(json!({"kind": "overview"})).await;
    assert!(settle(|| shows(&overview(&desk), "attention", &item_a)).await);
    let context = desk.context();
    desk.send(
        json!({"kind": "holdInsight", "context": context, "id": item_a, "hold": "acknowledge"}),
    )
    .await
    .expect("acknowledged");
    assert!(shows(&overview(&desk), "held", &item_a));
    // Workspace B is read, known and clean: it says nothing about A.
    set_day(&desk, series(40.0, 0.0));
    desk.choose(WS_B).await;
    desk.read(json!({"kind": "overview"})).await;
    assert_eq!(
        overview(&desk)["read"]["gateway"]["workspaceName"],
        "Team B"
    );
    set_day(&desk, series(40.0, 4.0));
    desk.choose(WS_A).await;
    desk.read(json!({"kind": "overview"})).await;
    let view = overview(&desk);
    assert!(shows(&view, "held", &item_a), "A's hold survives B's read");
    assert!(!shows(&view, "attention", &item_a));
    // A's day with unreported totals and errors is a gap, never all
    // clear, and does not end the hold.
    set_day(
        &desk,
        json!({"success": true, "data": {"records": [], "total": null, "isQuotaExceeded": false}}),
    );
    desk.read(json!({"kind": "overview"})).await;
    let view = overview(&desk);
    assert!(
        view["read"]["gateway"]["errors"].is_null(),
        "errors not reported"
    );
    assert!(view["read"]["gaps"].as_array().is_some_and(|g| {
        g.iter()
            .any(|g| g == "Gateway requests or errors not reported")
    }));
    assert_ne!(view["face"], "allClear");
    set_day(&desk, series(40.0, 4.0));
    desk.read(json!({"kind": "overview"})).await;
    assert!(
        shows(&overview(&desk), "held", &item_a),
        "an unknown day kept it"
    );
    // A known clean day of A ends the hold: new errors are attention again.
    set_day(&desk, series(40.0, 0.0));
    desk.read(json!({"kind": "overview"})).await;
    set_day(&desk, series(40.0, 4.0));
    desk.read(json!({"kind": "overview"})).await;
    let view = overview(&desk);
    assert!(shows(&view, "attention", &item_a), "the hold ended");
    assert!(!shows(&view, "held", &item_a));
    // A late read of B, settling after A was chosen again, is labelled B
    // and does not end A's hold.
    desk.send(json!({"kind": "holdInsight", "context": context, "id": item_a, "hold": "snooze"}))
        .await
        .expect("snoozed");
    set_day(&desk, series(40.0, 0.0));
    desk.choose(WS_B).await;
    desk.service().chart_delay = 600;
    let context = desk.context();
    desk.send(json!({"kind": "management", "context": context, "request": {"kind": "overview"}}))
        .await
        .expect("admitted");
    desk.choose(WS_A).await;
    assert_eq!(
        overview(&desk)["refreshing"],
        true,
        "B's read is still out when A is chosen"
    );
    assert!(
        settle(
            || overview(&desk)["read"]["gateway"]["workspaceName"] == "Team B"
                && overview(&desk)["refreshing"] == false
        )
        .await
    );
    desk.service().chart_delay = 0;
    set_day(&desk, series(40.0, 4.0));
    desk.read(json!({"kind": "overview"})).await;
    let view = overview(&desk);
    assert_eq!(view["read"]["gateway"]["workspaceName"], "Team A");
    assert!(shows(&view, "held", &item_a), "B's late read left A's hold");
}

#[tokio::test(flavor = "multi_thread")]
async fn gateway_files_are_read_uploaded_attached_and_deleted_once_with_the_workspace_key() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    // No key for the workspace: nothing is read.
    assert_eq!(
        desk.file_intent("readGatewayFiles", &reference, None)
            .await
            .err(),
        Some(Refusal::CapabilityMissing)
    );
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    let files = desk.read_files(&reference).await;
    assert_eq!(files["workspace"]["id"], WS_A);
    assert_eq!(files["files"][0]["id"], "file-seed");
    assert_eq!(files["files"][0]["filename"], "seed.txt");
    let read = desk.file_requests("GET");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].header("x-portkey-api-key"), Some(INFERENCE_KEY));
    assert!(read[0].header("x-portkey-trace-id").is_some());
    assert_eq!(
        read[0].header("x-portkey-config"),
        None,
        "no route for files"
    );

    // The sheet's file is held in the core: the presentation sees its name
    // and size only.
    let content = b"CANARY-file-content line\n";
    let token = desk.choose_file(&reference, "notes.txt", content).await;
    assert_eq!(desk.files()["chosen"]["bytes"], content.len());
    assert!(!desk.json().to_string().contains("CANARY-file-content"));
    assert_eq!(
        desk.file_intent("uploadGatewayFile", &reference, Some("gf-other"))
            .await
            .err(),
        Some(Refusal::InvalidInput),
        "only the chosen file's token uploads"
    );
    desk.file_intent("uploadGatewayFile", &reference, Some(&token))
        .await
        .expect("upload admitted");
    let action = desk.file_settled().await;
    assert_eq!(action["action"], "upload");
    assert_eq!(action["outcome"], "completed");
    assert_eq!(action["filename"], "notes.txt");
    let created = action["file"].as_str().expect("created").to_owned();
    assert_eq!(desk.files()["files"][0]["id"], created.as_str());
    assert!(
        desk.files()["chosen"].is_null(),
        "the upload took the choice"
    );
    let uploads = desk.file_requests("POST");
    assert_eq!(uploads.len(), 1);
    assert_eq!(uploads[0].header("x-portkey-api-key"), Some(INFERENCE_KEY));
    assert_eq!(
        uploads[0].header("x-portkey-trace-id"),
        action["trace"].as_str()
    );
    let parts = multipart(&uploads[0]);
    assert_eq!(parts["purpose"].1, b"assistants");
    assert_eq!(parts["file"].0.as_deref(), Some("notes.txt"));
    assert_eq!(parts["file"].1, content, "the bytes as chosen");
    assert_eq!(
        desk.file_intent("uploadGatewayFile", &reference, Some(&token))
            .await
            .err(),
        Some(Refusal::InvalidInput),
        "an upload is never sent again"
    );
    assert_eq!(desk.file_requests("POST").len(), 1);

    // A Responses run attaches a listed file; nothing else attaches.
    let attached = |file: &str| {
        json!({"api": "responses", "model": "@openai-prod/gpt-4o", "prompt": "summarize",
            "file": file})
    };
    assert_eq!(
        desk.run(&reference, attached("file-elsewhere")).await.err(),
        Some(Refusal::InvalidInput),
        "a file this workspace's list does not hold"
    );
    assert_eq!(
        desk.run(
            &reference,
            json!({"model": "@openai-prod/gpt-4o", "prompt": "x", "file": created})
        )
        .await
        .err(),
        Some(Refusal::InvalidInput),
        "only a Responses run carries a file"
    );
    desk.run(&reference, attached(&created)).await.expect("run");
    assert_eq!(desk.ended().await["state"], "completed");
    let sent: Vec<Seen> = desk
        .server
        .seen()
        .into_iter()
        .filter(|s| {
            s.request_line
                .starts_with(&format!("POST {RUNTIME}/responses"))
        })
        .collect();
    let body: Value = serde_json::from_slice(&sent[0].body).expect("body");
    assert_eq!(
        body["input"][0]["content"][1],
        json!({"type": "input_file", "file_id": created})
    );
    assert_eq!(body["input"][0]["content"][0]["text"], "summarize");
    assert_eq!(body["store"], false);

    // A delete removes one listed file, once.
    assert_eq!(
        desk.file_intent("deleteGatewayFile", &reference, Some("file-unlisted"))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    desk.file_intent("deleteGatewayFile", &reference, Some("file-seed"))
        .await
        .expect("delete admitted");
    let action = desk.file_settled().await;
    assert_eq!(action["action"], "delete");
    assert_eq!(action["outcome"], "completed");
    assert_eq!(action["file"], "file-seed");
    let deletes = desk.file_requests("DELETE");
    assert_eq!(deletes.len(), 1);
    assert!(deletes[0].request_line.contains("/files/file-seed"));
    assert!(
        !desk.files()["files"]
            .as_array()
            .expect("files")
            .iter()
            .any(|f| f["id"] == "file-seed")
    );

    // The sheet's bounds: empty, too large and unreadable files are named.
    for (reply, failure) in [
        (Some(Ok(("e.txt".to_owned(), Vec::new()))), "empty"),
        (
            Some(Ok((
                "big.bin".to_owned(),
                vec![0; desktop_core::gateway_files::MAX_UPLOAD_BYTES + 1],
            ))),
            "tooLarge",
        ),
        (
            Some(Err(desktop_core::import::ImportError::TooLarge)),
            "tooLarge",
        ),
        (
            Some(Err(desktop_core::import::ImportError::Unsupported)),
            "unreadable",
        ),
    ] {
        desk.adapters.gateway_files.reply(reply);
        desk.file_intent("chooseGatewayFile", &reference, None)
            .await
            .expect("sheet");
        assert!(
            settle(|| desk.files()["chooseFailure"] == failure).await,
            "{failure}"
        );
        assert!(desk.files()["chosen"].is_null());
    }
    assert_eq!(desk.file_requests("POST").len(), 1, "nothing more uploaded");
}

#[tokio::test(flavor = "multi_thread")]
async fn uncertain_file_changes_are_kept_with_their_trace_and_never_resent() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    desk.read_files(&reference).await;
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| desk.json()["gateway"]["local"]["state"] == "ready").await);
    // A refusal changed nothing and is not kept; a 5xx or a lost answer may
    // have stored or deleted the file and is kept with its trace.
    for (reply, outcome, failure) in [
        (Files::Status(400), "rejected", "rejected"),
        (Files::Status(503), "unknown", "unavailable"),
        (Files::Lost, "unknown", "network"),
    ] {
        desk.service().file_reply = reply;
        let token = desk
            .choose_file(&reference, "report.csv", b"a,b\n1,2\n")
            .await;
        desk.file_intent("uploadGatewayFile", &reference, Some(&token))
            .await
            .expect("upload admitted");
        let action = desk.file_settled().await;
        assert_eq!(action["outcome"], outcome, "{failure}");
        assert_eq!(action["failure"], failure);
    }
    desk.service().file_reply = Files::Status(502);
    desk.file_intent("deleteGatewayFile", &reference, Some("file-seed"))
        .await
        .expect("delete admitted");
    assert_eq!(desk.file_settled().await["outcome"], "unknown");
    assert_eq!(desk.file_requests("POST").len(), 3, "each upload sent once");
    assert_eq!(desk.file_requests("DELETE").len(), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(desk.file_requests("POST").len(), 3, "nothing is sent again");
    let kept = desk.json()["gateway"]["local"]["files"].clone();
    let kept = kept.as_array().expect("kept");
    assert_eq!(kept.len(), 3, "the refusal is not kept");
    assert_eq!(kept[0]["action"], "delete");
    assert_eq!(kept[0]["file"], "file-seed");
    assert_eq!(kept[1]["failure"], "network");
    assert_eq!(kept[2]["failure"], "unavailable");
    assert_eq!(kept[2]["filename"], "report.csv");
    assert_eq!(kept[2]["bytes"], 8);
    assert!(kept.iter().all(|k| k["outcome"] == "unknown"
        && k["trace"].as_str().is_some_and(|t| t.starts_with("airs-"))));
    assert!(
        !kept.iter().any(|k| k.to_string().contains("1,2")),
        "the content is never kept"
    );

    // A Responses draft keeps its attachment.
    desk.send(
        json!({"kind": "savePlaygroundDraft", "context": context, "workspace": reference,
        "draft": {"api": "responses", "model": "@openai-prod/gpt-4o", "prompt": "summarize",
        "file": "file-seed"}}),
    )
    .await
    .expect("draft kept");
    assert_eq!(
        desk.send(
            json!({"kind": "savePlaygroundDraft", "context": context, "workspace": reference,
            "draft": {"api": "responses", "model": "m", "prompt": "p", "file": "../x"}})
        )
        .await
        .err(),
        Some(Refusal::InvalidInput)
    );
    let tenant = desk.snap().active.expect("active").tenant;
    let path = desktop_core::gateway_local::path(&desk.adapters.sessions_root, tenant.as_str());
    assert!(
        settle(|| std::fs::read_to_string(&path)
            .is_ok_and(|f| f.contains("file-seed") && f.contains("summarize")))
        .await
    );
    let on_disk = std::fs::read_to_string(&path).expect("kept");
    assert!(
        !on_disk.contains("1,2") && !on_disk.contains(INFERENCE_KEY),
        "no file content or key on disk"
    );
    // An offline relaunch reads them; nothing is replayed, and the record
    // is dismissed locally.
    let mut offline = Offline::start(&desk);
    let local = offline.local().await;
    assert_eq!(local["files"].as_array().map(Vec::len), Some(3));
    assert_eq!(local["playground"][0]["file"], "file-seed");
    let id = local["files"][0]["id"].as_str().expect("id").to_owned();
    let context = offline.context();
    offline
        .send(json!({"kind": "discardGatewayLocal", "context": context, "id": id}))
        .await
        .expect("dismissed");
    assert_eq!(
        offline.json()["gateway"]["local"]["files"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(desk.file_requests("POST").len(), 3);
    assert_eq!(desk.file_requests("DELETE").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn file_lists_keep_their_bound_deletes_their_identity_and_times_their_range() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    // A full list (exactly the bound): uploads keep it at the bound and
    // report the files no longer shown as more.
    desk.service().files = (0..200)
        .map(|i| gateway_file(&format!("file-{i:03}"), "old.txt", 1))
        .collect();
    let files = desk.read_files(&reference).await;
    assert_eq!(files["files"].as_array().map(Vec::len), Some(200));
    assert_eq!(files["more"], false);
    for n in 1..=2 {
        let token = desk.choose_file(&reference, "new.txt", b"new").await;
        desk.file_intent("uploadGatewayFile", &reference, Some(&token))
            .await
            .expect("upload admitted");
        assert!(
            settle(|| desk.files()["action"]["outcome"] == "completed"
                && desk.files()["action"]["file"] == format!("file-up{n}").as_str())
            .await
        );
        let files = desk.files();
        let list = files["files"].as_array().expect("files");
        assert_eq!(list.len(), 200, "upload {n} keeps the bound");
        assert_eq!(
            list[0]["id"],
            format!("file-up{n}").as_str(),
            "the new file is shown"
        );
        assert_eq!(files["more"], true, "what is no longer shown is reported");
    }
    // An already truncated list stays truncated after an upload.
    desk.service().files = (0..201)
        .map(|i| gateway_file(&format!("file-{i:03}"), "old.txt", 1))
        .collect();
    let files = desk.read_files(&reference).await;
    assert_eq!(files["files"].as_array().map(Vec::len), Some(200));
    assert_eq!(files["more"], true);
    let token = desk.choose_file(&reference, "more.txt", b"more").await;
    desk.file_intent("uploadGatewayFile", &reference, Some(&token))
        .await
        .expect("upload admitted");
    assert!(settle(|| desk.files()["action"]["file"] == "file-up3").await);
    assert_eq!(desk.files()["files"].as_array().map(Vec::len), Some(200));
    assert_eq!(desk.files()["more"], true);

    // A time too late to render is unknown; the file stays listed and usable.
    let mut late = gateway_file("file-late", "late.txt", 3);
    late["created_at"] = json!(9_000_000_000_000_u64);
    desk.service().files = vec![late, gateway_file("file-a", "a.txt", 1)];
    let files = desk.read_files(&reference).await;
    assert!(files["readFailure"].is_null());
    assert_eq!(files["files"][0]["id"], "file-late");
    assert!(files["files"][0]["createdAt"].is_null());
    assert_eq!(files["files"][1]["createdAt"], 1_760_000_000);

    // A delete answered for another file claims nothing about this one: it
    // is uncertain, kept with its trace, still listed and never resent.
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| desk.json()["gateway"]["local"]["state"] == "ready").await);
    let mut traces = Vec::new();
    for deleted in [true, false] {
        desk.service().file_reply = Files::Another(deleted);
        desk.file_intent("deleteGatewayFile", &reference, Some("file-late"))
            .await
            .expect("delete admitted");
        let action = desk.file_settled().await;
        assert_eq!(action["outcome"], "unknown", "deleted: {deleted}");
        assert_eq!(action["failure"], "invalidResponse");
        assert_eq!(action["file"], "file-late");
        traces.push(action["trace"].as_str().expect("trace").to_owned());
        assert!(
            desk.files()["files"]
                .as_array()
                .is_some_and(|l| l.iter().any(|f| f["id"] == "file-late")),
            "an uncertain delete leaves the file listed"
        );
    }
    assert!(
        settle(|| {
            let kept = desk.json()["gateway"]["local"]["files"].clone();
            traces.iter().all(|t| {
                kept.as_array().is_some_and(|k| {
                    k.iter()
                        .any(|r| r["trace"] == t.as_str() && r["file"] == "file-late")
                })
            })
        })
        .await,
        "each uncertain delete is kept with its trace"
    );
    // The file named answers that it was not deleted: a refusal.
    desk.service().file_reply = Files::Kept;
    desk.file_intent("deleteGatewayFile", &reference, Some("file-late"))
        .await
        .expect("delete admitted");
    let action = desk.file_settled().await;
    assert_eq!(action["outcome"], "rejected");
    // The file named answers that it was deleted: only then is it gone.
    desk.service().file_reply = Files::Ok;
    desk.file_intent("deleteGatewayFile", &reference, Some("file-late"))
        .await
        .expect("delete admitted");
    assert!(settle(|| desk.files()["action"]["outcome"] == "completed").await);
    assert!(
        !desk.files()["files"]
            .as_array()
            .is_some_and(|l| l.iter().any(|f| f["id"] == "file-late"))
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        desk.file_requests("DELETE").len(),
        4,
        "each delete sent once"
    );
    assert_eq!(desk.file_requests("POST").len(), 3, "each upload sent once");
}

#[tokio::test(flavor = "multi_thread")]
async fn another_key_or_workspace_withdraws_the_files_and_their_attachments() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, INFERENCE_KEY)
        .await
        .expect("stored");
    desk.read_files(&reference).await;
    let token = desk.choose_file(&reference, "notes.txt", b"hello").await;
    // Replacing the key withdraws the list and the held file at once.
    desk.store_key(&reference, OTHER_KEY)
        .await
        .expect("replaced");
    assert!(desk.files().is_null());
    assert_eq!(
        desk.file_intent("uploadGatewayFile", &reference, Some(&token))
            .await
            .err(),
        Some(Refusal::InvalidInput),
        "the held file went with the key"
    );
    let attached = json!({"api": "responses", "model": "@openai-prod/gpt-4o", "prompt": "x",
        "file": "file-seed"});
    assert_eq!(
        desk.run(&reference, attached.clone()).await.err(),
        Some(Refusal::InvalidInput),
        "a list read with another key attaches nothing"
    );
    let files = desk.read_files(&reference).await;
    assert_eq!(files["files"][0]["id"], "file-seed");
    let reads = desk.file_requests("GET");
    assert_eq!(
        reads.last().and_then(|r| r.header("x-portkey-api-key")),
        Some(OTHER_KEY)
    );
    // Choosing another workspace withdraws them too; its runs attach
    // nothing read for the first.
    let other = desk.choose(WS_B).await;
    assert!(desk.files().is_null());
    desk.store_key(&other, INFERENCE_KEY).await.expect("stored");
    assert_eq!(
        desk.run(&other, attached).await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(desk.file_requests("POST").len(), 0);
    assert!(
        desk.server
            .seen()
            .iter()
            .all(|s| !s.request_line.contains("/responses")),
        "nothing was run"
    );
}
