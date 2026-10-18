// Shared synthetic Academy + Clerk harness for the C04 and C05 Academy
// suites (textually included; each suite uses a subset).
#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::learner::AcademyView;
#[allow(unused_imports)]
use desktop_core::runtime::Outcome;
use desktop_core::transport::{ProxyMode, TransportPolicy};
#[allow(unused_imports)]
use desktop_core::vault::{Vault, VaultError};
use desktop_core::{
    AcademyCallResult, AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt,
    Refusal, ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT: &str = "desktop_public_client";
const SCOPE: &str = "openid profile email offline_access academy:learn";

/// What the synthetic identity provider and Academy server answer.
struct Script {
    origin: String,
    config: Value,
    config_status: u16,
    polls: Vec<(u16, Value)>,
    refresh: Vec<(u16, Value)>,
    refresh_delay_ms: u64,
    trpc: Vec<(String, u16, Value)>,
    trpc_delay_ms: u64,
    /// Per-procedure reply delays (milliseconds), overriding `trpc_delay_ms`.
    trpc_delays: Vec<(String, u64)>,
    poll_delay_ms: u64,
    revoke_delay_ms: u64,
    me_delays: Vec<u64>,
    /// Other paths (upload destinations, avatar images): path without the
    /// query, status (0: the reply is lost; 1: no reply ever), content type and
    /// body; each
    /// answers once.
    files: Vec<(String, u16, String, Vec<u8>)>,
    metadata_issuer: Option<String>,
    token_endpoint: Option<String>,
    issued: u32,
}

fn bearer(access: &str, refresh: &str, expires_in: u64) -> Value {
    json!({"token_type": "Bearer", "access_token": access, "refresh_token": refresh,
           "expires_in": expires_in, "scope": SCOPE})
}

fn raw(status: u16, body: &Value) -> Reply {
    let body = body.to_string().into_bytes();
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} S\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}

impl Script {
    fn new() -> Self {
        Self {
            origin: String::new(),
            config: Value::Null,
            config_status: 200,
            polls: vec![(200, bearer("access-1", "refresh-1", 3600))],
            refresh: Vec::new(),
            refresh_delay_ms: 0,
            trpc: Vec::new(),
            trpc_delay_ms: 0,
            trpc_delays: Vec::new(),
            poll_delay_ms: 0,
            revoke_delay_ms: 0,
            me_delays: Vec::new(),
            files: Vec::new(),
            metadata_issuer: None,
            token_endpoint: None,
            issued: 0,
        }
    }

    fn route(&mut self, seen: &Seen) -> Reply {
        let line = seen.request_line.clone();
        let path = line.split(' ').nth(1).unwrap_or("").to_owned();
        let form = String::from_utf8_lossy(&seen.body).to_string();
        if path == "/api/desktop/config" {
            let config = if self.config.is_null() {
                json!({"issuer": self.origin, "clientId": CLIENT, "scope": SCOPE})
            } else {
                self.config.clone()
            };
            return raw(self.config_status, &config);
        }
        if path == "/.well-known/oauth-authorization-server" {
            let token = self
                .token_endpoint
                .clone()
                .unwrap_or(format!("{}/oauth/token", self.origin));
            return raw(
                200,
                &json!({
                "issuer": self.metadata_issuer.clone().unwrap_or(self.origin.clone()),
                "device_authorization_endpoint": format!("{}/oauth/device", self.origin),
                "token_endpoint": token,
                "revocation_endpoint": format!("{}/oauth/revoke", self.origin)}),
            );
        }
        if path == "/oauth/device" {
            return raw(
                200,
                &json!({"device_code": "device-secret-1", "user_code": "WDJB-MJHT",
                "verification_uri": format!("{}/activate", self.origin),
                "verification_uri_complete": format!("{}/activate?code=WDJB-MJHT", self.origin),
                "expires_in": 600, "interval": 1}),
            );
        }
        if path == "/oauth/token" && form.contains("device_code") {
            let (status, body) = if self.polls.len() > 1 {
                self.polls.remove(0)
            } else {
                self.polls[0].clone()
            };
            return delay(self.poll_delay_ms, raw(status, &body));
        }
        if path == "/oauth/token" && form.contains("refresh_token") {
            self.issued += 1;
            let reply = if self.refresh.is_empty() {
                raw(
                    200,
                    &bearer(&format!("access-r{}", self.issued), "refresh-1", 3600),
                )
            } else {
                let (status, body) = self.refresh.remove(0);
                raw(status, &body)
            };
            return delay(self.refresh_delay_ms, reply);
        }
        if path == "/oauth/revoke" {
            return delay(self.revoke_delay_ms, raw(200, &json!({})));
        }
        if let Some(rest) = path.strip_prefix("/api/trpc/") {
            let procedure = rest.split('?').next().unwrap_or("").to_owned();
            if procedure == "desktop.me" && !self.trpc.iter().any(|(p, ..)| p == "desktop.me") {
                let wait = if self.me_delays.is_empty() {
                    0
                } else {
                    self.me_delays.remove(0)
                };
                return delay(
                    wait,
                    raw(
                        200,
                        &json!({"result": {"data": {"json": {"accountId": "user_2fixture",
                    "displayName": "Ada Lovelace", "serverTime": "2026-10-17T00:00:00.000Z"}}}}),
                    ),
                );
            }
            if let Some(i) = self.trpc.iter().position(|(p, ..)| *p == procedure) {
                let (_, status, body) = self.trpc.remove(i);
                let wait = self
                    .trpc_delays
                    .iter()
                    .find(|(p, _)| *p == procedure)
                    .map_or(self.trpc_delay_ms, |(_, ms)| *ms);
                return delay(wait, raw(status, &body));
            }
            return raw(
                404,
                &json!({"error": {"json": {"message": "No procedure", "code": -32004,
                "data": {"code": "NOT_FOUND", "httpStatus": 404}}}}),
            );
        }
        let bare = path.split('?').next().unwrap_or("");
        if let Some(i) = self.files.iter().position(|(p, ..)| p == bare) {
            let (_, status, content_type, body) = self.files.remove(i);
            // Status 0: read the request, then close without a reply;
            // status 1: never answer.
            if status == 0 {
                return Reply::Close;
            }
            if status == 1 {
                return Reply::Hang;
            }
            return Reply::Raw {
                head: format!(
                    "HTTP/1.1 {status} S\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close",
                    body.len()
                ),
                body,
            };
        }
        raw(404, &json!({}))
    }
}

fn delay(millis: u64, reply: Reply) -> Reply {
    if millis == 0 {
        reply
    } else {
        Reply::Delayed {
            millis,
            then: Box::new(reply),
        }
    }
}

#[allow(dead_code)]
struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
    ca: TestCa,
}

async fn desk() -> Desk {
    desk_with(Script::new(), None).await
}

async fn desk_with(
    script: Script,
    restart: Option<(Handles, desktop_core::store::PersistedState)>,
) -> Desk {
    let ca = TestCa::new("airs-v2 c04 ca");
    let script = Arc::new(Mutex::new(script));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("script").route(seen)
        })),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    script.lock().expect("script").origin = origin.clone();
    start(ca, server, script, restart).await
}

async fn start(
    ca: TestCa,
    server: TlsServer,
    script: Arc<Mutex<Script>>,
    restart: Option<(Handles, desktop_core::store::PersistedState)>,
) -> Desk {
    let origin = script.lock().expect("script").origin.clone();
    let (adapters, handles) = match restart {
        None => fixtures::adapters(),
        Some((old, state)) => {
            let (mut adapters, mut handles) =
                fixtures::adapters_with(fixtures::ScriptedStore::with_state(state));
            adapters.keychain = old.vault.clone();
            adapters.learner_keychain = old.learner_vault.clone();
            handles.vault = old.vault;
            handles.learner_vault = old.learner_vault;
            (adapters, handles)
        }
    };
    let config = CoreConfig {
        academy_origin: origin,
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
    Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        script,
        server,
        ca,
    }
}

#[allow(dead_code)]
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

    fn academy(&self) -> AcademyView {
        self.snap().academy
    }

    /// The current learner reference: the account's, or the guest one.
    fn current_learner(&self) -> String {
        self.snap().learner.expect("learner").as_str().to_owned()
    }

    fn learner(&self) -> String {
        match self.academy() {
            AcademyView::SignedIn { learner, .. } => learner.as_str().to_owned(),
            other => panic!("not signed in: {other:?}"),
        }
    }

    fn requests(&self, path: &str) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| {
                s.request_line
                    .split(' ')
                    .nth(1)
                    .is_some_and(|p| p.starts_with(path))
            })
            .collect()
    }

    async fn sign_in(&mut self) {
        self.send(json!({"kind": "startAcademySignIn"}))
            .await
            .expect("start");
        assert!(
            eventually(|| matches!(self.academy(), AcademyView::SignedIn { .. })).await,
            "signed in: {:?}",
            self.academy()
        );
    }

    async fn call(
        &mut self,
        procedure: &str,
        kind: &str,
        input: Option<&str>,
    ) -> Result<AcademyCallResult, Refusal> {
        let learner = self.learner();
        self.call_as(&learner, procedure, kind, input).await
    }

    async fn call_as(
        &mut self,
        learner: &str,
        procedure: &str,
        kind: &str,
        input: Option<&str>,
    ) -> Result<AcademyCallResult, Refusal> {
        let receipt = self
            .send(
                json!({"kind": "academyCall", "learner": learner, "procedure": procedure,
                         "procedureKind": kind, "input": input}),
            )
            .await?;
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = receipt
        else {
            panic!("no operation");
        };
        Ok(self.result(&operation).await)
    }

    async fn result(&self, operation: &desktop_core::runtime::OperationId) -> AcademyCallResult {
        let Ok(Observation::AcademyCall(handle)) = self
            .handle
            .observe(ViewScope::AcademyCall(operation.clone()))
        else {
            panic!("call observation");
        };
        tokio::time::timeout(Duration::from_secs(20), handle.result())
            .await
            .expect("result in time")
    }
}

#[allow(dead_code)]
fn form(seen: &Seen) -> Vec<(String, String)> {
    url::form_urlencoded::parse(&seen.body)
        .into_owned()
        .collect()
}
