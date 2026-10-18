//! C08 management onboarding and reads through the production authority
//! interface: a separate management OAuth credential (never the scan key,
//! never a replacement for it), the grant sent only to the admitted token
//! origin, reads through the pinned SDK with literal synthetic replies,
//! bounded pages whose completeness only the service can claim, details by
//! a bounded walk, credentials replaced mid-pagination, denial, unknown
//! values and secret fields, redirects, protected-storage failures, restart
//! and reset.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::{
    DetailFound, ManagementEndpoints, ManagementFailure, ManagementResult,
};
use desktop_core::store::PersistedState;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::vault::VaultError;
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal, ScanState,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const SCAN_KEY: &str = "synthetic-scan-key-c08";
const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c08";
const TSG: &str = "1234567890";

/// What the synthetic management, DLP and scan endpoints answer.
struct Script {
    /// Profiles in the inventory (generated).
    profiles: usize,
    /// Token replies in order (then the default grant).
    tokens: Vec<(u16, Value)>,
    issued: u32,
    /// Path prefix → (status, body) overrides, used once each.
    overrides: Vec<(String, u16, Value)>,
    /// Path prefix → delay (ms), used once each.
    delays: Vec<(String, u64)>,
    /// Path prefix → redirect target, used once each.
    redirects: Vec<(String, String)>,
}

impl Script {
    fn new() -> Self {
        Self {
            profiles: 3,
            tokens: Vec::new(),
            issued: 0,
            overrides: Vec::new(),
            delays: Vec::new(),
            redirects: Vec::new(),
        }
    }

    fn route(&mut self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        let query: Vec<(String, String)> = target
            .split_once('?')
            .map(|(_, q)| {
                q.split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let q = |name: &str| {
            query
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        };
        let reply = self.reply(&path, &q);
        match self
            .delays
            .iter()
            .position(|(p, _)| path.starts_with(p.as_str()))
        {
            Some(i) => Reply::Delayed {
                millis: self.delays.remove(i).1,
                then: Box::new(reply),
            },
            None => reply,
        }
    }

    fn reply(&mut self, path: &str, q: &dyn Fn(&str) -> Option<String>) -> Reply {
        if let Some(i) = self
            .redirects
            .iter()
            .position(|(p, _)| path.starts_with(p.as_str()))
        {
            let to = self.redirects.remove(i).1;
            return Reply::Raw {
                head: format!(
                    "HTTP/1.1 302 Found\r\nlocation: {to}\r\ncontent-length: 0\r\nconnection: close"
                ),
                body: Vec::new(),
            };
        }
        if let Some(i) = self
            .overrides
            .iter()
            .position(|(p, ..)| path.starts_with(p.as_str()))
        {
            let (_, status, body) = self.overrides.remove(i);
            return raw(status, &body);
        }
        if path == "/v1/scan/sync/request" {
            return raw(
                200,
                &json!({"report_id": "R-c08", "scan_id": "S-c08", "category": "benign",
                "action": "allow", "timeout": false, "error": false, "errors": [],
                "profile_name": "Production guardrails"}),
            );
        }
        if path == "/oauth2/access_token" {
            if !self.tokens.is_empty() {
                let (status, body) = self.tokens.remove(0);
                return raw(status, &body);
            }
            self.issued += 1;
            return raw(
                200,
                &json!({"access_token": format!("mgmt-access-{}", self.issued),
                "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        let offset: usize = q("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
        let limit: usize = q("limit").and_then(|v| v.parse().ok()).unwrap_or(100);
        if path == format!("/aisec/v1/mgmt/profiles/tsg/{TSG}") {
            let end = (offset + limit).min(self.profiles);
            let items: Vec<Value> = (offset..end).map(profile).collect();
            let mut body = json!({"ai_profiles": items});
            if end < self.profiles {
                body["next_offset"] = json!(end);
            }
            return raw(200, &body);
        }
        if path == format!("/aisec/v1/mgmt/topics/tsg/{TSG}") {
            return raw(
                200,
                &json!({"custom_topics": [{"topic_id": "11111111-1111-4111-8111-111111111111",
                "topic_name": "Finance", "revision": 1, "description": "Money talk"}]}),
            );
        }
        if path == format!("/aisec/v1/mgmt/customerapp/tsg/{TSG}") {
            return raw(
                200,
                &json!({"customer_apps": [{"customer_appId": "app-1", "tsg_id": TSG,
                "app_name": "Support bot", "cloud_provider": "aws", "environment": "production"}]}),
            );
        }
        if path == format!("/aisec/v1/mgmt/apikeys/tsg/{TSG}") {
            return raw(
                200,
                &json!({"api_keys": [{"api_key_id": "key-1", "api_key_name": "Runtime",
                "api_key_last8": "abcd1234", "api_key": "SYNTHETIC-KEY-VALUE", "auth_code": "SYNTHETIC-AUTH-CODE",
                "expiration": "2027-01-01T00:00:00Z", "revoked": false, "status": "FUTURE_STATUS"}]}),
            );
        }
        if path == "/aisec/v1/mgmt/deploymentprofiles" {
            let items: Vec<Value> = (0..3)
                .map(|n| json!({"dp_name": format!("dp-{n}"), "auth_code": "SYNTHETIC-DP-CODE", "status": "active"}))
                .collect();
            return raw(
                200,
                &json!({"deployment_profiles": items, "status": "success"}),
            );
        }
        if let Some(id) = path.strip_prefix("/dlp/v2/api/data-patterns/") {
            return if id == "pattern-0" {
                raw(
                    200,
                    &json!({"id": "pattern-0", "name": "Card numbers", "status": "active",
                    "future_field": {"kept": 1},
                    "detection_config": {"technique": "regex", "token": "SYNTHETIC-PATTERN-TOKEN"}}),
                )
            } else if id == "pattern-new" {
                raw(
                    200,
                    &json!({"id": "pattern-new", "status": "FUTURE_STATUS"}),
                )
            } else if id == "pattern-bad" {
                // Breaks the SDK's response contract (technique is required).
                raw(200, &json!({"id": "pattern-bad", "detection_config": {}}))
            } else {
                raw(404, &json!({"error": "not found"}))
            };
        }
        if path == "/dlp/v2/api/data-patterns" {
            let page: u64 = q("page").and_then(|v| v.parse().ok()).unwrap_or(0);
            return raw(
                200,
                &json!({"content": [{"id": format!("pattern-{page}"), "name": "Card numbers"}],
                "last": page >= 1, "totalElements": 2, "number": page}),
            );
        }
        raw(404, &json!({"error": "not found"}))
    }
}

fn profile(n: usize) -> Value {
    json!({"profile_id": format!("00000000-0000-4000-8000-{n:012}"), "profile_name": format!("Profile {n}"),
           "revision": 1, "active": true})
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

fn basic(user: &str, secret: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    // RFC 6749 2.3.1: both parts are form-urlencoded before encoding.
    let form = |text: &str| -> String {
        text.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                b' ' => "+".to_owned(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    };
    let input = format!("{}:{}", form(user), form(secret));
    let bytes = input.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    format!("Basic {out}")
}

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
    ca: TestCa,
    origin: String,
}

async fn desk() -> Desk {
    desk_with(Script::new()).await
}

async fn desk_with(script: Script) -> Desk {
    let ca = TestCa::new("airs-v2 c08 ca");
    let script = Arc::new(Mutex::new(script));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("script").route(seen)
        })),
    )
    .await;
    let mut desk = start(ca, server, script, None, None).await;
    let origin = desk.origin.clone();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    desk
}

async fn start(
    ca: TestCa,
    server: TlsServer,
    script: Arc<Mutex<Script>>,
    restart: Option<(Handles, PersistedState)>,
    management: Option<ManagementEndpoints>,
) -> Desk {
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = match restart {
        None => fixtures::adapters(),
        Some((old, state)) => {
            let (mut adapters, mut handles) =
                fixtures::adapters_with(fixtures::ScriptedStore::with_state(state));
            adapters.keychain = old.vault.clone();
            handles.vault = old.vault;
            (adapters, handles)
        }
    };
    let config = CoreConfig {
        management: management.unwrap_or_else(|| ManagementEndpoints::at_origin(&origin)),
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
        origin,
    }
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

    fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    async fn scan_key(&mut self) {
        let context = self.context();
        self.send(json!({"kind": "configureScanCredential", "context": context, "mode": "keychain", "key": SCAN_KEY}))
            .await
            .expect("scan key");
        assert!(eventually(|| self.snap().tenants[0].scan.state == ScanState::Ready).await);
    }

    async fn management_key(&mut self, secret: &str) {
        let context = self.context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
            "clientId": CLIENT_ID, "secret": secret}),
        )
        .await
        .expect("management key");
        assert!(eventually(|| self.snap().tenants[0].management.state == ScanState::Ready).await);
    }

    async fn admit(
        &mut self,
        request: Value,
    ) -> Result<desktop_core::runtime::OperationId, Refusal> {
        let context = self.context();
        match self
            .send(json!({"kind": "management", "context": context, "request": request}))
            .await?
        {
            Receipt::Admitted {
                operation: Some(operation),
                ..
            } => Ok(operation),
            other => panic!("unexpected {other:?}"),
        }
    }

    async fn result(&self, operation: &desktop_core::runtime::OperationId) -> ManagementResult {
        let Ok(Observation::Management(handle)) = self
            .handle
            .observe(ViewScope::Management(operation.clone()))
        else {
            panic!("management observation");
        };
        tokio::time::timeout(Duration::from_secs(20), handle.result())
            .await
            .expect("result in time")
    }

    async fn read(&mut self, request: Value) -> ManagementResult {
        let operation = self.admit(request).await.expect("admitted");
        self.result(&operation).await
    }

    async fn page(
        &mut self,
        resource: &str,
        offset: u32,
        limit: u32,
    ) -> desktop_core::management::ManagementPage {
        match self
            .read(json!({"kind": "page", "resource": resource, "offset": offset, "limit": limit}))
            .await
        {
            ManagementResult::Page { page } => page,
            other => panic!("not a page: {other:?}"),
        }
    }

    async fn scan(&mut self) -> desktop_core::ScanRecordView {
        let context = self.context();
        let receipt = self
            .send(json!({"kind": "scan", "context": context, "draft": {
                "profile": {"kind": "name", "name": "Production guardrails"},
                "prompt": "hello", "response": "", "appName": "Desktop workbench"}}))
            .await
            .expect("scan admitted");
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = receipt
        else {
            panic!("no operation")
        };
        assert!(
            eventually(|| self
                .snap()
                .scans
                .iter()
                .any(|s| s.operation == operation
                    && s.progress == desktop_core::ScanProgress::Settled))
            .await
        );
        self.snap()
            .scans
            .into_iter()
            .find(|s| s.operation == operation)
            .expect("scan")
    }

    fn requests(&self, prefix: &str) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| {
                s.request_line
                    .split(' ')
                    .nth(1)
                    .is_some_and(|p| p.starts_with(prefix))
            })
            .collect()
    }

    async fn restart(self, management: Option<ManagementEndpoints>) -> Desk {
        let state = self.adapters.store.saved().expect("saved");
        let Desk {
            adapters,
            script,
            server,
            ca,
            core,
            ..
        } = self;
        core.shutdown();
        drop(core);
        start(ca, server, script, Some((adapters, state)), management).await
    }
}

fn items(page: &desktop_core::management::ManagementPage) -> Vec<Value> {
    serde_json::from_str(&page.items).expect("items")
}

fn failed(result: &ManagementResult) -> Option<ManagementFailure> {
    match result {
        ManagementResult::Failed { failure, .. } => Some(*failure),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_scan_only_tenant_scans_and_management_never_uses_or_replaces_the_scan_key() {
    let mut desk = desk().await;
    desk.scan_key().await;
    // Scan-only: management reads are not admitted; scanning works.
    assert_eq!(
        desk.admit(json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 25}))
            .await
            .err(),
        Some(Refusal::CapabilityMissing)
    );
    assert_eq!(
        desk.scan().await.outcome,
        Some(desktop_core::runtime::Outcome::Completed)
    );
    // A separate management credential: verified with its own grant.
    desk.management_key(CLIENT_SECRET).await;
    let view = desk.snap().tenants[0].management.clone();
    assert_eq!(view.client_id.as_deref(), Some(CLIENT_ID));
    assert_eq!(view.token_origin.as_deref(), Some(desk.origin.as_str()));
    assert_eq!(
        desk.snap().tenants[0].scan.state,
        ScanState::Ready,
        "the scan key stays"
    );
    assert_eq!(
        desk.read(json!({"kind": "verify"})).await,
        ManagementResult::Verified
    );
    let grants = desk.requests("/oauth2/access_token");
    assert_eq!(grants.len(), 1);
    assert_eq!(
        grants[0].header("authorization"),
        Some(basic(CLIENT_ID, CLIENT_SECRET).as_str())
    );
    let form = String::from_utf8_lossy(&grants[0].body).to_string();
    assert!(
        form.contains("grant_type=client_credentials") && form.contains(&format!("tsg_id%3A{TSG}")),
        "{form}"
    );
    // The scan key never reaches management; the management grant never reaches scanning.
    assert_eq!(
        desk.scan().await.outcome,
        Some(desktop_core::runtime::Outcome::Completed)
    );
    for seen in desk.server.seen() {
        let path = seen.request_line.clone();
        let text = format!("{seen:?}");
        if path.contains("/v1/scan/") {
            assert_eq!(seen.header("x-pan-token"), Some(SCAN_KEY));
            assert!(seen.header("authorization").is_none(), "{path}");
            assert!(!text.contains(CLIENT_SECRET) && !text.contains("mgmt-access"));
        } else {
            assert!(!text.contains(SCAN_KEY), "{path}");
        }
        if path.contains("/aisec/") {
            assert_eq!(
                seen.header("authorization"),
                Some("Bearer mgmt-access-1"),
                "{path}"
            );
        }
    }
    // Nothing secret in the snapshot.
    let snapshot = serde_json::to_string(&desk.snap()).expect("json");
    assert!(!snapshot.contains(CLIENT_SECRET) && !snapshot.contains(SCAN_KEY));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_management_grant_never_affects_scanning() {
    let mut script = Script::new();
    script
        .tokens
        .push((401, json!({"error": "invalid_client"})));
    script
        .tokens
        .push((400, json!({"error": "invalid_client"})));
    let mut desk = desk_with(script).await;
    desk.scan_key().await;
    desk.management_key("wrong-secret").await;
    assert_eq!(
        failed(&desk.read(json!({"kind": "verify"})).await),
        Some(ManagementFailure::Unauthorized)
    );
    // The next read does not use a cached failure as if it were the scan identity.
    let second = desk
        .read(json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 25}))
        .await;
    assert_eq!(failed(&second), Some(ManagementFailure::Unauthorized));
    assert_eq!(desk.snap().tenants[0].scan.state, ScanState::Ready);
    let scan = desk.scan().await;
    assert_eq!(
        scan.outcome,
        Some(desktop_core::runtime::Outcome::Completed)
    );
    assert!(scan.result.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn later_pages_are_read_one_at_a_time_and_only_the_last_claims_completeness() {
    let mut script = Script::new();
    script.profiles = 60;
    let mut desk = desk_with(script).await;
    desk.management_key(CLIENT_SECRET).await;
    let first = desk.page("profiles", 0, 25).await;
    assert_eq!(
        (first.count, first.next_offset, first.complete),
        (25, Some(25), false)
    );
    let second = desk.page("profiles", 25, 25).await;
    assert_eq!(
        (second.count, second.next_offset, second.complete),
        (25, Some(50), false)
    );
    assert_eq!(items(&second)[0]["profile_name"], "Profile 25");
    let last = desk.page("profiles", 50, 25).await;
    assert_eq!(
        (last.count, last.next_offset, last.complete),
        (10, None, true)
    );
    // Each page was its own bounded request with the asked offset/limit.
    let reads = desk.requests(&format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"));
    assert_eq!(reads.len(), 3);
    assert!(
        reads[1].request_line.contains("offset=25") && reads[1].request_line.contains("limit=25")
    );
    // One token for the generation (the SDK's cache), never re-granted per page.
    assert_eq!(desk.requests("/oauth2/access_token").len(), 1);
    // A detail on a later page is found by a bounded walk; a missing one is
    // absent only because the whole inventory was read.
    let found = desk
        .read(json!({"kind": "detail", "resource": "profiles", "id": "00000000-0000-4000-8000-000000000055"}))
        .await;
    let ManagementResult::Detail { detail } = found else {
        panic!("{found:?}")
    };
    assert_eq!(detail.found, DetailFound::Found);
    assert!(detail.item.expect("item").contains("Profile 55"));
    let missing = desk
        .read(json!({"kind": "detail", "resource": "profiles", "id": "missing"}))
        .await;
    let ManagementResult::Detail { detail } = missing else {
        panic!("{missing:?}")
    };
    assert_eq!(detail.found, DetailFound::Absent);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_walk_that_ends_before_the_inventory_claims_nothing() {
    let mut script = Script::new();
    script.profiles = 1_100;
    let mut desk = desk_with(script).await;
    desk.management_key(CLIENT_SECRET).await;
    let result = desk
        .read(json!({"kind": "detail", "resource": "profiles", "id": "missing"}))
        .await;
    let ManagementResult::Detail { detail } = result else {
        panic!("{result:?}")
    };
    assert_eq!(
        detail.found,
        DetailFound::Incomplete,
        "a missing page does not prove absence"
    );
    assert_eq!(
        desk.requests(&format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"))
            .len(),
        40
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_replaced_mid_pagination_retire_the_old_pages() {
    let mut script = Script::new();
    script.profiles = 60;
    let mut desk = desk_with(script).await;
    desk.management_key(CLIENT_SECRET).await;
    desk.page("profiles", 0, 25).await;
    desk.script
        .lock()
        .expect("script")
        .delays
        .push((format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"), 1_500));
    let pending = desk
        .admit(json!({"kind": "page", "resource": "profiles", "offset": 25, "limit": 25}))
        .await
        .expect("admitted");
    tokio::time::sleep(Duration::from_millis(300)).await;
    // The operator replaces the management credential while page 2 is in flight.
    desk.management_key("replacement-secret").await;
    let old = desk.result(&pending).await;
    assert_eq!(
        failed(&old),
        Some(ManagementFailure::Cancelled),
        "the old page is never shown"
    );
    // The new generation starts over with its own grant.
    let fresh = desk.page("profiles", 0, 25).await;
    assert_eq!(fresh.count, 25);
    let grants = desk.requests("/oauth2/access_token");
    assert_eq!(grants.len(), 2);
    assert_eq!(
        grants[1].header("authorization"),
        Some(basic(CLIENT_ID, "replacement-secret").as_str())
    );
    let last = desk
        .requests(&format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"))
        .pop()
        .expect("read");
    assert_eq!(last.header("authorization"), Some("Bearer mgmt-access-2"));
}

#[tokio::test(flavor = "multi_thread")]
async fn denial_unknown_values_secret_fields_and_catalogs_are_reported_faithfully() {
    let mut desk = desk().await;
    desk.management_key(CLIENT_SECRET).await;
    desk.script.lock().expect("script").overrides.push((
        format!("/aisec/v1/mgmt/topics/tsg/{TSG}"),
        403,
        json!({"error": "forbidden", "message": "SERVER-TEXT-NEVER-SHOWN"}),
    ));
    let denied = desk
        .read(json!({"kind": "page", "resource": "topics", "offset": 0, "limit": 25}))
        .await;
    assert_eq!(failed(&denied), Some(ManagementFailure::Denied));
    assert!(!format!("{denied:?}").contains("SERVER-TEXT"));
    // API keys: unknown status kept, key value and auth code removed.
    let keys = desk.page("apiKeys", 0, 25).await;
    assert!(
        !keys.items.contains("SYNTHETIC-KEY-VALUE") && !keys.items.contains("SYNTHETIC-AUTH-CODE")
    );
    assert_eq!(items(&keys)[0]["status"], "FUTURE_STATUS");
    assert_eq!(items(&keys)[0]["api_key_last8"], "abcd1234");
    // Unpaginated catalogs are sliced with their total; DLP pages follow `last`.
    let deployments = desk.page("deploymentProfiles", 0, 2).await;
    assert_eq!(
        (deployments.count, deployments.total, deployments.complete),
        (2, Some(3), false)
    );
    assert!(!deployments.items.contains("SYNTHETIC-DP-CODE"));
    let patterns = desk.page("dataPatterns", 0, 1).await;
    assert_eq!(
        (patterns.next_offset, patterns.complete, patterns.total),
        (Some(1), false, Some(2))
    );
    let more = desk.page("dataPatterns", 1, 1).await;
    assert!(more.complete);
    // A topic, an app: read with their own endpoints.
    assert_eq!(desk.page("topics", 0, 25).await.count, 1);
    assert_eq!(desk.page("applications", 0, 25).await.count, 1);
    let dlp = desk.requests("/dlp/v2/api/data-patterns");
    // DLP reads carry a management grant (the DLP client keeps its own
    // token cache); every grant went to the token origin with the client.
    let grants = desk.requests("/oauth2/access_token");
    assert!(
        grants
            .iter()
            .all(|g| g.header("authorization") == Some(basic(CLIENT_ID, CLIENT_SECRET).as_str()))
    );
    assert!(
        !dlp.is_empty()
            && dlp.iter().all(|s| s
                .header("authorization")
                .is_some_and(|h| h.starts_with("Bearer mgmt-access-"))),
        "{:?}",
        dlp.iter().map(|s| s.headers.clone()).collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_redirect_is_never_followed_to_another_origin() {
    let mut desk = desk().await;
    desk.management_key(CLIENT_SECRET).await;
    desk.script.lock().expect("script").redirects.push((
        format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"),
        "https://elsewhere.example/aisec/v1/mgmt/profiles".to_owned(),
    ));
    let result = desk
        .read(json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 25}))
        .await;
    assert!(
        matches!(
            failed(&result),
            Some(ManagementFailure::Destination | ManagementFailure::Rejected)
        ),
        "{result:?}"
    );
    assert_eq!(
        desk.requests(&format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"))
            .len(),
        1,
        "not retried or followed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn protected_storage_failures_are_reported_without_any_fallback() {
    let mut desk = desk().await;
    desk.adapters.vault.fail_with(Some(VaultError::Denied));
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("admitted");
    assert!(eventually(|| desk.snap().tenants[0].management.state == ScanState::Failed).await);
    assert_eq!(
        desk.snap().tenants[0].management.failure,
        Some(VaultError::Denied)
    );
    assert_eq!(
        desk.admit(json!({"kind": "verify"})).await.err(),
        Some(Refusal::CapabilityMissing),
        "no read without a stored credential"
    );
    // Stored, then the Keychain refuses the read: reported, nothing sent.
    desk.adapters.vault.fail_with(None);
    desk.management_key(CLIENT_SECRET).await;
    desk.adapters.vault.fail_with(Some(VaultError::Locked));
    let locked = desk.read(json!({"kind": "verify"})).await;
    assert_eq!(failed(&locked), Some(ManagementFailure::CredentialLocked));
    assert!(desk.requests("/oauth2/access_token").is_empty());
    // Unlocked, the next explicit read recovers without reconfiguration: a
    // failed binding is not cached.
    desk.adapters.vault.fail_with(None);
    assert_eq!(
        desk.read(json!({"kind": "verify"})).await,
        ManagementResult::Verified
    );
    assert_eq!(desk.requests("/oauth2/access_token").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_result_settled_before_its_context_retired_is_never_delivered_afterwards() {
    let mut desk = desk().await;
    desk.management_key(CLIENT_SECRET).await;
    // Read A's inventory and detail to completion without observing them.
    let page = desk
        .admit(json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 25}))
        .await
        .expect("admitted");
    let detail = desk
        .admit(json!({"kind": "detail", "resource": "topics", "id": "11111111-1111-4111-8111-111111111111"}))
        .await
        .expect("admitted");
    assert!(
        eventually(|| desk.requests("/aisec/v1/mgmt/topics").len() == 1).await,
        "both reads reached the service"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    // The credential is replaced: A's results are withdrawn, not delivered.
    desk.management_key("replacement-secret").await;
    for operation in [&page, &detail] {
        assert_eq!(
            desk.result(operation).await,
            ManagementResult::Failed {
                failure: ManagementFailure::Cancelled,
                outcome: desktop_core::runtime::Outcome::Completed
            }
        );
    }
    // Likewise across a tenant switch.
    let page = desk
        .admit(json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 25}))
        .await
        .expect("admitted");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let context = desk.context();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "region", "region": "europe"}}}),
    )
    .await
    .expect("created");
    let other = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("tenant")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": other}))
        .await
        .expect("selected");
    assert_ne!(desk.context(), context, "B is active");
    assert_eq!(
        failed(&desk.result(&page).await),
        Some(ManagementFailure::Cancelled)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_management_credential_survives_restart_bound_to_its_token_origin() {
    let mut desk = desk().await;
    desk.management_key(CLIENT_SECRET).await;
    let saved = desk.adapters.store.saved().expect("saved");
    let stored = saved
        .credentials
        .iter()
        .find(|c| c.class == desktop_core::store::CredentialClass::Management)
        .expect("stored");
    assert_eq!(stored.client_id.as_deref(), Some(CLIENT_ID));
    assert_eq!(stored.destination, desk.origin);
    let mut desk = desk.restart(None).await;
    assert!(eventually(|| desk.snap().tenants[0].management.state == ScanState::Ready).await);
    assert_eq!(
        desk.read(json!({"kind": "verify"})).await,
        ManagementResult::Verified
    );
    // With another token origin configured, the stored secret is never sent there.
    let elsewhere = ManagementEndpoints::at_origin("https://elsewhere.example");
    let mut desk = desk.restart(Some(elsewhere)).await;
    assert_eq!(
        desk.admit(json!({"kind": "verify"})).await.err(),
        Some(Refusal::CapabilityMissing)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn removal_deletion_and_reset_retire_each_capability_as_asked() {
    let mut desk = desk().await;
    desk.scan_key().await;
    desk.management_key(CLIENT_SECRET).await;
    let context = desk.context();
    desk.send(json!({"kind": "removeManagementCredential", "context": context}))
        .await
        .expect("removed");
    assert_eq!(desk.snap().tenants[0].management.state, ScanState::Missing);
    assert_eq!(
        desk.snap().tenants[0].scan.state,
        ScanState::Ready,
        "the scan key stays"
    );
    desk.management_key(CLIENT_SECRET).await;
    let context = desk.context();
    desk.send(json!({"kind": "removeScanCredential", "context": context}))
        .await
        .expect("removed");
    assert_eq!(
        desk.snap().tenants[0].management.state,
        ScanState::Ready,
        "management stays"
    );
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(
        eventually(|| desk.adapters.vault.records().is_empty()).await,
        "every credential purged"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn only_bounded_requests_from_a_tenant_with_a_tsg_are_admitted() {
    let mut desk = desk().await;
    desk.management_key(CLIENT_SECRET).await;
    for request in [
        json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 0}),
        json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 101}),
        json!({"kind": "page", "resource": "dataPatterns", "offset": 3, "limit": 25}),
        json!({"kind": "detail", "resource": "topics", "id": ""}),
    ] {
        assert_eq!(desk.admit(request).await.err(), Some(Refusal::InvalidInput));
    }
    // A tenant without a TSG cannot hold a management credential.
    let origin = desk.origin.clone();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "No TSG",
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    let other = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "No TSG")
        .expect("tenant")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": other}))
        .await
        .expect("selected");
    let context = desk.context();
    assert_eq!(
        desk.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "session",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET})
        )
        .await
        .err(),
        Some(Refusal::CapabilityMissing)
    );
    // A stale context is refused.
    assert_eq!(
        desk.send(json!({"kind": "management", "context": "cx-stale0000000000000000", "request": {"kind": "verify"}}))
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
    let _ = (&desk.core, &desk.ca);
}

#[tokio::test(flavor = "multi_thread")]
async fn dlp_details_use_their_own_read_and_a_404_means_absent() {
    let mut desk = desk().await;
    desk.management_key(CLIENT_SECRET).await;
    let found = desk
        .read(json!({"kind": "detail", "resource": "dataPatterns", "id": "pattern-0"}))
        .await;
    let ManagementResult::Detail { detail } = found else {
        panic!("not a detail: {found:?}")
    };
    assert_eq!(detail.found, desktop_core::management::DetailFound::Found);
    let item = detail.item.expect("item");
    assert!(item.contains("future_field") && !item.contains("SYNTHETIC-PATTERN-TOKEN"));
    let absent = desk
        .read(json!({"kind": "detail", "resource": "dataPatterns", "id": "pattern-9"}))
        .await;
    let ManagementResult::Detail { detail } = absent else {
        panic!("not a detail: {absent:?}")
    };
    assert_eq!(
        (detail.found, detail.item),
        (desktop_core::management::DetailFound::Absent, None)
    );
    // An unknown enum value is kept; a reply the SDK's contract rejects is
    // an invalid reply, never absent.
    let unknown = desk
        .read(json!({"kind": "detail", "resource": "dataPatterns", "id": "pattern-new"}))
        .await;
    let ManagementResult::Detail { detail } = unknown else {
        panic!("not a detail: {unknown:?}")
    };
    assert!(detail.item.expect("item").contains("FUTURE_STATUS"));
    assert_eq!(
        failed(
            &desk
                .read(json!({"kind": "detail", "resource": "dataPatterns", "id": "pattern-bad"}))
                .await
        ),
        Some(ManagementFailure::InvalidResponse)
    );
    // One read each, no list walk.
    let reads: Vec<String> = desk
        .requests("/dlp/")
        .iter()
        .filter_map(|s| s.request_line.split(' ').nth(1).map(str::to_owned))
        .collect();
    assert_eq!(
        reads,
        [
            "/dlp/v2/api/data-patterns/pattern-0",
            "/dlp/v2/api/data-patterns/pattern-9",
            "/dlp/v2/api/data-patterns/pattern-new",
            "/dlp/v2/api/data-patterns/pattern-bad"
        ]
    );
}
