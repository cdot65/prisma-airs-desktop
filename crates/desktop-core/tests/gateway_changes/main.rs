//! C17 reviewed Gateway changes through the production authority interface
//! against a stateful synthetic Gateway (data, admin and IAM planes at the
//! management origin): changes bound to the workspace chosen natively and
//! to the baseline they were reviewed with, the native confirmation's
//! single-use completion, exactly one write per apply with its readback,
//! deletes blocked by what references the item, drafts that never carry
//! credentials, one-time keys held for one reveal, and workspace
//! provisioning whose every step keeps its own outcome and is reconciled by
//! reading the tenant back. Nothing is retried or rolled back.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::changes::ChangeProblem;
use desktop_core::fixtures::{self, Handles};
use desktop_core::gateway_changes::{Reconciled, ScopeCleanup, StepState};
use desktop_core::management::{ManagementEndpoints, ManagementFailure};
use desktop_core::runtime::Outcome;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, ChangeFailure, ChangeResultView, ChangeReviewState, ChangeReviewView, Core,
    CoreConfig, Delivery, Intent, Observation, Readback, Receipt, Refusal, ScanState, SecretState,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c17";
const TSG: &str = "1234567890";
const DATA: &str = "/ai_gw/v2";
const ADMIN: &str = "/ai_gw/admin/v2";
const IAM: &str = "/iam/v1";
const WS_A: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";
const WS_B: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8298";
const WS_EMPTY: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8299";
const PROVIDER: &str = "7e3a0000-0000-4000-8000-000000000001";
const ROUTE: &str = "7e3a0000-0000-4000-8000-000000000002";
const GUARDRAIL: &str = "7e3a0000-0000-4000-8000-000000000003";
const SERVICE_KEY: &str = "7e3a0000-0000-4000-8000-000000000004";
const OTHER_PROVIDER: &str = "7e3a0000-0000-4000-8000-000000000009";
const INTEGRATION: &str = "f6692544-3265-49be-9711-bbdcebc079eb";
const OPENAI_FAMILY: &str = "f6692544-3265-49be-9711-bbdcebc079e6";
/// Values that must never leave the core.
const CANARIES: [&str; 6] = [
    "sk-CANARY-provider",
    "CANARY-model-config",
    "CANARY-route-key",
    "CANARY-guardrail-key",
    "CANARY-key-value",
    "gw-ONE-TIME",
];

/// A one-shot fault for the next request to a path.
#[derive(Clone)]
enum Fault {
    /// Reply with this status (a sanitized error body).
    Status(u16),
    /// Apply the request, then close without replying (a lost reply).
    Lost,
    /// Never answer.
    Hang,
    /// Answer after this many milliseconds.
    Delay(u64),
}

/// A small stateful Gateway: each write changes what later reads return.
struct Gateway {
    workspaces: Vec<Value>,
    items: BTreeMap<&'static str, Vec<Value>>,
    integrations: Vec<Value>,
    models: BTreeMap<String, Value>,
    access: BTreeMap<String, Value>,
    scopes: BTreeMap<String, Value>,
    faults: BTreeMap<String, Fault>,
    /// The service ignores model and routing updates (a readback must say).
    ignores_updates: bool,
    /// The service ignores the all-models setting of a models update.
    ignores_all_models: bool,
    serial: u32,
}

fn workspace(id: &str, name: &str, slug: &str) -> Value {
    json!({"id": id, "slug": slug, "name": name, "icon": null, "description": null,
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "is_default": 0, "status": "active", "scope_name": format!("scope-{slug}"),
           "object": "workspace", "defaults": {"config_id": ROUTE, "allow_config_override": true},
           "usage_limits": null, "rate_limits": null})
}

fn provider(id: &str, ws: &str, name: &str) -> Value {
    json!({"id": id, "ai_provider_name": "openai", "slug": name, "name": name,
           "usage_limits": {"credit_limit": 100, "type": "cost", "current_usage": 7},
           "status": "active", "note": null, "created_at": "2026-10-01T00:00:00Z",
           "expires_at": null, "last_reset_at": null, "rate_limits": [],
           "integration_id": INTEGRATION, "object": "provider", "workspace_id": ws,
           "model_config": {"api_key": "CANARY-model-config"}, "key": "sk-CANARY-provider",
           "masked_api_key": "sk-****ider"})
}

fn route(id: &str, ws: &str, config: Value) -> Value {
    json!({"id": id, "name": "default-route", "slug": "default-route", "organisation_id": "org-1",
           "is_default": 1, "status": "active", "owner_id": "owner-1", "updated_by": "owner-1",
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "workspace_id": ws, "object": "config", "format": "json", "type": "ROUTING",
           "version_id": "v-1", "config": config.to_string()})
}

fn guardrail(id: &str, ws: &str, checks: Value) -> Value {
    json!({"id": id, "name": "pii", "slug": "pii", "organisation_id": "org-1", "status": "active",
           "owner_id": "owner-1", "updated_by": null, "created_at": "2026-10-01T00:00:00Z",
           "last_updated_at": "2026-10-02T00:00:00Z", "workspace_id": ws, "object": "guardrail",
           "checks": checks, "actions": {"deny": true, "async": false, "sequential": false},
           "version_id": "v-1"})
}

fn key(id: &str, ws: &str, name: &str) -> Value {
    json!({"id": id, "name": name, "object": "api-key", "status": "active",
           "key": "CANARY-key-value", "scopes": ["completions.write"], "workspace_id": ws})
}

fn integration() -> Value {
    json!({"id": INTEGRATION, "name": "OpenAI", "owner_id": "owner-1", "status": "active",
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": null, "slug": "openai",
           "description": null, "workspace_id": null, "ai_provider_id": OPENAI_FAMILY,
           "object": "integration", "configurations": "{}"})
}

fn binding(ws: &str) -> Value {
    json!({"id": ws, "enabled": true, "usage_limits": null, "rate_limits": null, "status": "active",
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-01T00:00:00Z",
           "last_reset_at": "2026-10-01T00:00:00Z"})
}

fn listing(rows: Vec<Value>) -> Value {
    let total = rows.len();
    json!({"object": "list", "total": total, "success": true, "has_more": false, "data": rows})
}

impl Gateway {
    fn new() -> Self {
        let mut items: BTreeMap<&'static str, Vec<Value>> = BTreeMap::new();
        items.insert(
            "providers",
            vec![
                provider(PROVIDER, WS_A, "openai-prod"),
                provider(OTHER_PROVIDER, WS_B, "team-b-openai"),
            ],
        );
        items.insert(
            "configs",
            vec![route(
                ROUTE,
                WS_A,
                json!({"strategy": {"mode": "fallback"}, "targets": [
                    {"provider": "@openai-prod", "api_key": "CANARY-route-key"}]}),
            )],
        );
        items.insert(
            "guardrails",
            vec![guardrail(
                GUARDRAIL,
                WS_A,
                json!([{"id": "pii-check", "parameters": {"apiKey": "CANARY-guardrail-key"}, "is_enabled": true}]),
            )],
        );
        items.insert("api-keys/service", vec![key(SERVICE_KEY, WS_A, "ci-key")]);
        items.insert("api-keys/user", Vec::new());
        let mut models = BTreeMap::new();
        models.insert(
            INTEGRATION.to_owned(),
            json!({"object": "list", "allow_all_models": false, "total": 2, "models": [
                {"slug": "gpt-4o", "enabled": true}, {"slug": "o3", "enabled": false}]}),
        );
        let mut access = BTreeMap::new();
        access.insert(
            INTEGRATION.to_owned(),
            json!({"object": "list", "total": 1, "workspaces": [binding(WS_A)],
                   "global_workspace_access": {"enabled": false}}),
        );
        let mut scopes = BTreeMap::new();
        for slug in ["team-a", "team-b", "team-empty"] {
            scopes.insert(
                format!("scope-{slug}"),
                json!({"name": format!("scope-{slug}"), "description": slug, "tsg_id": TSG,
                       "id": format!("id-{slug}"),
                       "resources": [{"resource_type": "workspace", "resource_id": slug}]}),
            );
        }
        Self {
            workspaces: vec![
                workspace(WS_A, "Team A", "team-a"),
                workspace(WS_B, "Team B", "team-b"),
                workspace(WS_EMPTY, "Empty", "team-empty"),
            ],
            items,
            integrations: vec![integration()],
            models,
            access,
            scopes,
            faults: BTreeMap::new(),
            ignores_updates: false,
            ignores_all_models: false,
            serial: 100,
        }
    }

    fn id(&mut self) -> String {
        self.serial += 1;
        format!("7e3a0000-0000-4000-8000-{:012}", self.serial)
    }

    fn item_mut(&mut self, collection: &str, id: &str) -> Option<&mut Value> {
        self.items
            .get_mut(collection)?
            .iter_mut()
            .find(|i| i["id"] == id)
    }

    fn route(&mut self, seen: &Seen) -> Reply {
        let mut parts = seen.request_line.split(' ');
        let method = parts.next().unwrap_or("").to_owned();
        let target = parts.next().unwrap_or("").to_owned();
        let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
        let (path, query) = (path.to_owned(), query.to_owned());
        if path == "/oauth2/access_token" {
            return json_reply(
                200,
                &json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        let fault = self.faults.remove(&format!("{method} {path}"));
        if let Some(Fault::Status(status)) = fault {
            return json_reply(status, &json!({"error": "synthetic failure"}));
        }
        if let Some(Fault::Hang) = fault {
            return Reply::Hang;
        }
        let body: Value = serde_json::from_slice(&seen.body).unwrap_or(Value::Null);
        let (status, reply) = self.answer(&method, &path, &query, &body);
        if let Some(Fault::Lost) = fault {
            return Reply::Close;
        }
        let reply = match reply {
            None => Reply::Raw {
                head: format!("HTTP/1.1 {status} Status\r\ncontent-length: 0\r\nconnection: close"),
                body: Vec::new(),
            },
            Some(reply) => json_reply(status, &reply),
        };
        match fault {
            Some(Fault::Delay(millis)) => Reply::Delayed {
                millis,
                then: Box::new(reply),
            },
            _ => reply,
        }
    }

    fn answer(
        &mut self,
        method: &str,
        path: &str,
        query: &str,
        body: &Value,
    ) -> (u16, Option<Value>) {
        let not_found = (404, Some(json!({"error": "not found"})));
        if let Some(rest) = path.strip_prefix(IAM) {
            let name = rest.strip_prefix("/scopes/").unwrap_or("").to_owned();
            return match (method, rest) {
                ("POST", "/scopes") => {
                    let name = body["name"].as_str().unwrap_or("").to_owned();
                    let scope = json!({"name": name, "description": body["description"], "tsg_id": TSG,
                                       "id": format!("id-{name}"), "resources": []});
                    self.scopes.insert(name, scope.clone());
                    (200, Some(scope))
                }
                ("GET", _) => match self.scopes.get(&name) {
                    Some(scope) => (200, Some(scope.clone())),
                    None => not_found,
                },
                ("PUT", _) => match self.scopes.get_mut(&name) {
                    Some(scope) => {
                        scope["resources"] = body["resources"].clone();
                        scope["description"] = body["description"].clone();
                        (200, Some(scope.clone()))
                    }
                    None => not_found,
                },
                ("DELETE", _) => {
                    self.scopes.remove(&name);
                    (204, None)
                }
                _ => not_found,
            };
        }
        let (admin, rest) = match (path.strip_prefix(ADMIN), path.strip_prefix(DATA)) {
            (Some(rest), _) => (true, rest.to_owned()),
            (None, Some(rest)) => (false, rest.to_owned()),
            _ => return not_found,
        };
        let segments: Vec<&str> = rest.trim_start_matches('/').split('/').collect();
        let workspace_query = query
            .split('&')
            .find_map(|p| p.strip_prefix("workspace_id="))
            .unwrap_or("")
            .to_owned();
        match (method, segments.as_slice()) {
            ("GET", ["workspaces"]) => {
                let active: Vec<Value> = self
                    .workspaces
                    .iter()
                    .filter(|w| w["status"] == "active")
                    .cloned()
                    .collect();
                (200, Some(listing(active)))
            }
            ("GET", ["workspaces", id]) => match self.workspaces.iter().find(|w| w["id"] == *id) {
                Some(w) => (200, Some(w.clone())),
                None => not_found,
            },
            ("POST", ["workspaces"]) if admin => {
                let id = self.id();
                let name = body["name"].as_str().unwrap_or("new").to_owned();
                let slug = name.to_lowercase().replace(' ', "-");
                let mut created = workspace(&id, &name, &slug);
                created["scope_name"] = body["scope_name"].clone();
                self.workspaces.push(created.clone());
                (
                    200,
                    Some(
                        json!({"id": id, "name": name, "slug": slug, "description": null,
                                  "created_at": "2026-10-20T00:00:00Z",
                                  "last_updated_at": "2026-10-20T00:00:00Z",
                                  "scope_name": body["scope_name"], "object": "workspace"}),
                    ),
                )
            }
            ("PUT", ["workspaces", id]) if admin => {
                let Some(w) = self.workspaces.iter_mut().find(|w| w["id"] == *id) else {
                    return not_found;
                };
                merge(w, body);
                (200, Some(json!({"success": true, "id": id})))
            }
            ("DELETE", ["workspaces", id]) if admin => {
                let Some(w) = self.workspaces.iter_mut().find(|w| w["id"] == *id) else {
                    return not_found;
                };
                w["status"] = json!("archived");
                (204, None)
            }
            ("GET", ["integrations"]) if admin => (200, Some(listing(self.integrations.clone()))),
            ("GET", ["integrations", id]) if admin => {
                match self.integrations.iter().find(|i| i["id"] == *id) {
                    Some(i) => (200, Some(i.clone())),
                    None => not_found,
                }
            }
            ("POST", ["integrations"]) if admin => {
                let id = self.id();
                let mut created = integration();
                merge(&mut created, body);
                created["id"] = json!(id);
                self.integrations.push(created);
                (200, Some(json!({"success": true, "id": id})))
            }
            ("PUT", ["integrations", id]) if admin => {
                let Some(i) = self.integrations.iter_mut().find(|i| i["id"] == *id) else {
                    return not_found;
                };
                merge(i, body);
                (200, Some(json!({"success": true, "id": id})))
            }
            ("DELETE", ["integrations", id]) if admin => {
                self.integrations.retain(|i| i["id"] != *id);
                (204, None)
            }
            ("GET", ["integrations", id, "models"]) if admin => match self.models.get(*id) {
                Some(m) => (200, Some(m.clone())),
                None => not_found,
            },
            ("PUT", ["integrations", id, "models"]) if admin => {
                if !self.ignores_updates
                    && let Some(m) = self.models.get_mut(*id)
                {
                    m["models"] = body["models"].clone();
                    if let Some(all) = body.get("allow_all_models")
                        && !self.ignores_all_models
                    {
                        m["allow_all_models"] = all.clone();
                    }
                }
                (200, Some(json!({"success": true, "id": id})))
            }
            ("GET", ["integrations", id, "workspaces"]) if admin => match self.access.get(*id) {
                Some(a) => (200, Some(a.clone())),
                None => (
                    200,
                    Some(json!({"object": "list", "total": 0, "workspaces": [],
                                          "global_workspace_access": {"enabled": false}})),
                ),
            },
            ("GET", ["utils", "static-resources", "ai-providers"]) if admin => (
                200,
                Some(json!({
                "success": true, "data": [{"id": OPENAI_FAMILY, "slug": "open-ai", "name": "OpenAI",
                                           "status": "active"}]})),
            ),
            ("GET", ["utils", "static-resources", "schema"]) if admin => (
                200,
                Some(json!({
                "object": "list", "evals": [{
                    "id": "pii-check", "name": "PII", "type": "guardrail", "guardrailCategory": "basic",
                    "description": [], "parameters": {"type": "object", "properties": {
                        "scan_scope": {"type": "string", "enum": ["last_message", "all_messages"]}},
                        "additionalProperties": false}}]})),
            ),
            ("GET", ["plugins"]) if admin => {
                (200, Some(json!({"object": "list", "total": 0, "data": []})))
            }
            (
                _,
                [
                    collection @ ("providers" | "configs" | "guardrails"),
                    tail @ ..,
                ],
            ) => self.collection(method, collection, tail, &workspace_query, body),
            (_, ["api-keys", kind @ ("service" | "user"), tail @ ..]) => {
                let collection = if *kind == "service" {
                    "api-keys/service"
                } else {
                    "api-keys/user"
                };
                self.collection(method, collection, tail, &workspace_query, body)
            }
            _ => not_found,
        }
    }

    fn collection(
        &mut self,
        method: &str,
        collection: &str,
        tail: &[&str],
        workspace: &str,
        body: &Value,
    ) -> (u16, Option<Value>) {
        let not_found = (404, Some(json!({"error": "not found"})));
        let name: &'static str = match collection {
            "providers" => "providers",
            "configs" => "configs",
            "guardrails" => "guardrails",
            "api-keys/service" => "api-keys/service",
            _ => "api-keys/user",
        };
        match (method, tail) {
            ("GET", []) => {
                let rows: Vec<Value> = self.items[name]
                    .iter()
                    .filter(|i| i["workspace_id"] == workspace)
                    .cloned()
                    .collect();
                (200, Some(listing(rows)))
            }
            ("GET", [id]) => match self.items[name].iter().find(|i| i["id"] == *id) {
                Some(i) => (200, Some(i.clone())),
                None => not_found,
            },
            ("POST", []) => {
                let id = self.id();
                let mut created = match name {
                    "providers" => provider(&id, workspace, "new"),
                    "configs" => route(&id, workspace, json!({})),
                    "guardrails" => guardrail(&id, workspace, json!([])),
                    _ => key(&id, workspace, "new"),
                };
                merge(&mut created, body);
                if name == "configs" {
                    created["config"] = json!(body["config"].to_string());
                }
                created["id"] = json!(id);
                let slug = created["name"].clone();
                self.items.get_mut(name).expect("collection").push(created);
                let reply = match name {
                    "providers" => json!({"id": id, "slug": slug, "object": "provider"}),
                    "configs" | "guardrails" => {
                        json!({"id": id, "version_id": "v-1", "slug": slug, "object": "config"})
                    }
                    _ => json!({"id": id, "success": true, "key": format!("gw-ONE-TIME-{id}"),
                                "object": "api-key"}),
                };
                (200, Some(reply))
            }
            ("POST", [id, "rotate"]) => {
                if self.item_mut(name, id).is_none() {
                    return not_found;
                }
                (
                    200,
                    Some(json!({"id": id, "key": "gw-ONE-TIME-rotated",
                                  "key_transition_expires_at": "2026-10-21T00:00:00Z"})),
                )
            }
            ("PUT", [id]) => {
                let ignores = self.ignores_updates;
                let Some(item) = self.item_mut(name, id) else {
                    return not_found;
                };
                if !(ignores && name == "configs") {
                    merge(item, body);
                    if name == "configs" {
                        if let Some(config) = body.get("config") {
                            item["config"] = json!(config.to_string());
                        }
                        item["version_id"] = json!("v-2");
                    }
                }
                (200, Some(json!({"success": true, "id": id})))
            }
            ("DELETE", [id]) => {
                self.items
                    .get_mut(name)
                    .expect("collection")
                    .retain(|i| i["id"] != *id);
                (204, None)
            }
            _ => not_found,
        }
    }
}

fn merge(target: &mut Value, edits: &Value) {
    if let (Some(target), Some(edits)) = (target.as_object_mut(), edits.as_object()) {
        for (k, v) in edits {
            target.insert(k.clone(), v.clone());
        }
    }
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

struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    gateway: Arc<Mutex<Gateway>>,
    server: TlsServer,
    _ca: TestCa,
    results_seen: usize,
}

async fn desk() -> Desk {
    let ca = TestCa::new("airs-v2 c17 ca");
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
        _core: core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        gateway,
        server,
        _ca: ca,
        results_seen: 0,
    };
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    desk.configure().await;
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

    async fn configure(&mut self) {
        let context = self.context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        assert!(settle(|| self.snap().tenants[0].management.state == ScanState::Ready).await);
    }

    /// Configures the active tenant's management credential.
    async fn configure_active(&mut self) {
        let context = self.context();
        let tenant = self.snap().active.expect("active").tenant;
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        assert!(
            settle(|| self
                .snap()
                .tenants
                .iter()
                .any(|t| t.id == tenant && t.management.state == ScanState::Ready))
            .await
        );
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

    fn reference(&self) -> Option<String> {
        serde_json::to_value(self.snap().gateway).expect("json")["selected"]["reference"]
            .as_str()
            .map(str::to_owned)
    }

    fn service(&self) -> std::sync::MutexGuard<'_, Gateway> {
        self.gateway.lock().expect("gateway")
    }

    /// Lists the workspaces (natively) and chooses `workspace`.
    async fn choose(&mut self, workspace: &str) -> String {
        let context = self.context();
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = self
            .send(json!({"kind": "management", "context": context, "request":
                {"kind": "gateway", "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}}}))
            .await
            .expect("listing admitted")
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
            .expect("listing");
        assert!(settle(|| self.snap().gateway.workspaces.is_some()).await);
        let context = self.context();
        self.send(
            json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": workspace}),
        )
        .await
        .expect("selected");
        self.reference().expect("reference")
    }

    async fn review(&mut self, draft: &Value) -> ChangeReviewView {
        let context = self.context();
        self.send(json!({"kind": "reviewChange", "context": context, "draft": draft}))
            .await
            .expect("review admitted");
        assert!(
            settle(|| self
                .snap()
                .changes
                .review
                .as_ref()
                .is_some_and(|r| matches!(
                    r.state,
                    ChangeReviewState::Ready | ChangeReviewState::Failed
                )))
            .await,
            "review settles"
        );
        self.snap().changes.review.expect("review")
    }

    async fn request(
        &mut self,
        review: &ChangeReviewView,
        draft: &Value,
        typed: &str,
    ) -> Result<Receipt, Refusal> {
        self.send(json!({"kind": "requestApply", "review": review.review, "draft": draft, "confirmation": typed}))
            .await
    }

    /// Reviews, requests and confirms `draft`; returns the settled result.
    async fn apply(&mut self, draft: &Value) -> ChangeResultView {
        let review = self.review(draft).await;
        assert_eq!(review.state, ChangeReviewState::Ready, "{review:?}");
        let typed = review.confirmation.clone().expect("confirmation");
        self.adapters.confirmer.auto(Some(true));
        self.request(&review, draft, &typed)
            .await
            .expect("requested");
        self.settled().await
    }

    async fn settled(&mut self) -> ChangeResultView {
        let seen = self.results_seen;
        assert!(
            settle(|| {
                let results = self.snap().changes.results;
                results.len() > seen && results[0].settled
            })
            .await,
            "change settles"
        );
        let results = self.snap().changes.results;
        self.results_seen = results.len();
        results.into_iter().next().expect("result")
    }

    fn writes(&self) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| !s.request_line.starts_with("GET ") && !s.request_line.contains("/oauth2/"))
            .collect()
    }

    fn requests(&self) -> usize {
        self.server
            .seen()
            .iter()
            .filter(|s| !s.request_line.contains("/oauth2/"))
            .count()
    }
}

fn line(seen: &Seen) -> String {
    seen.request_line
        .split(' ')
        .take(2)
        .collect::<Vec<_>>()
        .join(" ")
}

fn draft(kind: &str, action: &str, target: &str, body: Value, workspace: Option<&str>) -> Value {
    let mut draft =
        json!({"kind": kind, "action": action, "target": target, "body": body.to_string()});
    if let Some(workspace) = workspace {
        draft["workspace"] = json!(workspace);
    }
    draft
}

fn leaks(value: &impl serde::Serialize) -> Vec<&'static str> {
    let text = serde_json::to_string(value).expect("json");
    CANARIES
        .iter()
        .copied()
        .filter(|c| text.contains(c))
        .collect()
}

fn problem(review: &ChangeReviewView) -> Option<ChangeProblem> {
    match review.failure {
        Some(ChangeFailure::Problem { problem }) => Some(problem),
        _ => None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reviewed_change_inside_the_chosen_workspace_applies_once_after_confirmation() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"name": "openai-primary"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    assert_eq!(review.state, ChangeReviewState::Ready, "{review:?}");
    assert_eq!(review.confirmation.as_deref(), Some("openai-prod"));
    let shown = review.workspace.clone().expect("workspace shown");
    assert_eq!(
        (shown.id.as_str(), shown.scope_name.as_str()),
        (WS_A, "scope-team-a")
    );
    assert!(
        review
            .after
            .as_deref()
            .is_some_and(|a| a.contains("openai-primary"))
    );
    assert!(leaks(&desk.snap()).is_empty());
    // RequestApply opens the native confirmation naming the exact scope.
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert!(settle(|| desk.adapters.confirmer.held() == 1).await);
    let (_, request) = desk.adapters.confirmer.requests().pop().expect("request");
    assert!(
        request
            .message
            .contains("Workspace: Team A (team-a), IAM scope scope-team-a."),
        "{}",
        request.message
    );
    assert!(
        desk.writes().is_empty(),
        "nothing written before confirmation"
    );
    assert_eq!(
        desk.request(&review, &update, "openai-prod").await,
        Err(Refusal::Busy)
    );
    assert!(desk.adapters.confirmer.complete(true));
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::Completed));
    assert_eq!(result.readback, Some(Readback::Found));
    assert!(
        result
            .current
            .as_deref()
            .is_some_and(|c| c.contains("openai-primary"))
    );
    assert_eq!(result.workspace.as_ref().map(|w| w.id.as_str()), Some(WS_A));
    let writes = desk.writes();
    assert_eq!(writes.len(), 1, "exactly one write");
    assert_eq!(line(&writes[0]), format!("PUT {DATA}/providers/{PROVIDER}"));
    assert_eq!(writes[0].header("x-tsg-id"), Some(TSG));
    let sent: Value = serde_json::from_slice(&writes[0].body).expect("body");
    assert_eq!(sent, json!({"name": "openai-primary"}));
    // Single use: the review is consumed and a late completion is ignored.
    assert_eq!(
        desk.request(&review, &update, "openai-prod").await,
        Err(Refusal::ReviewConsumed)
    );
    assert!(!desk.adapters.confirmer.complete(true));
    assert_eq!(desk.writes().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_review_is_bound_to_its_workspace_and_a_draft_cannot_choose_another() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    // A draft naming another workspace is refused before any request.
    let before = desk.requests();
    let elsewhere = draft(
        "gatewayServiceKeys",
        "create",
        "",
        json!({"name": "x", "scopes": ["completions.write"], "type": "service", "workspace_id": WS_B}),
        Some(&reference),
    );
    let review = desk.review(&elsewhere).await;
    assert_eq!(
        problem(&review),
        Some(ChangeProblem::WorkspaceMismatch),
        "{review:?}"
    );
    assert_eq!(desk.requests(), before);
    // A scoped change needs the current reference.
    let context = desk.context();
    let stale = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"name": "x"}),
        None,
    );
    assert_eq!(
        desk.send(json!({"kind": "reviewChange", "context": context, "draft": stale}))
            .await,
        Err(Refusal::StaleContext)
    );
    // An item of another workspace is never changed through this one.
    let foreign = draft(
        "gatewayProviders",
        "update",
        OTHER_PROVIDER,
        json!({"name": "x"}),
        Some(&reference),
    );
    let review = desk.review(&foreign).await;
    assert_eq!(problem(&review), Some(ChangeProblem::WorkspaceMismatch));
    // Choosing another workspace ends the review and its open sheet.
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"name": "y"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert!(settle(|| desk.adapters.confirmer.held() == 1).await);
    let nonce = desk
        .adapters
        .confirmer
        .requests()
        .last()
        .expect("request")
        .0;
    let context = desk.context();
    desk.send(json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": WS_B}))
        .await
        .expect("selected");
    assert!(settle(|| desk.snap().changes.review.is_none()).await);
    assert!(desk.adapters.confirmer.dismissed().contains(&nonce));
    assert!(!desk.adapters.confirmer.complete(true));
    assert_eq!(
        desk.request(&review, &update, "openai-prod").await,
        Err(Refusal::ReviewInvalid)
    );
    assert!(desk.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_change_made_since_review_is_a_conflict_but_traffic_counters_are_not() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "x"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    // Only usage moves: the change still applies.
    desk.service().item_mut("providers", PROVIDER).expect("p")["usage_limits"]["current_usage"] =
        json!(55);
    desk.adapters.confirmer.auto(Some(true));
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert_eq!(desk.settled().await.outcome, Some(Outcome::Completed));
    // Another writer renames it after review: nothing is written over it.
    let again = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "y"}),
        Some(&reference),
    );
    let review = desk.review(&again).await;
    desk.service().item_mut("providers", PROVIDER).expect("p")["name"] = json!("renamed");
    let writes = desk.writes().len();
    desk.request(&review, &again, "openai-prod")
        .await
        .expect("requested");
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::NotSent));
    assert_eq!(result.failure, Some(ChangeFailure::Conflict));
    assert_eq!(
        desk.writes().len(),
        writes,
        "nothing written over the other change"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn deletes_are_blocked_while_anything_references_the_item() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    // The route names the provider by slug: listed, and Apply is refused.
    let delete = draft(
        "gatewayProviders",
        "delete",
        PROVIDER,
        json!({}),
        Some(&reference),
    );
    let review = desk.review(&delete).await;
    assert_eq!(review.state, ChangeReviewState::Ready);
    assert_eq!(review.references, vec!["Route: default-route".to_owned()]);
    assert_eq!(
        desk.request(&review, &delete, "openai-prod").await,
        Err(Refusal::ReviewInvalid)
    );
    // A workspace with configuration cannot be archived; an empty one can.
    let archive = draft("gatewayWorkspaces", "delete", WS_A, json!({}), None);
    let review = desk.review(&archive).await;
    assert!(
        review
            .references
            .contains(&"Provider: openai-prod".to_owned()),
        "{review:?}"
    );
    assert!(
        review
            .references
            .contains(&"Service key: ci-key".to_owned())
    );
    // The integration is used in another workspace and granted to one.
    let integration = draft(
        "gatewayIntegrations",
        "delete",
        INTEGRATION,
        json!({}),
        None,
    );
    let review = desk.review(&integration).await;
    assert!(
        review
            .references
            .iter()
            .any(|r| r.contains("team-b-openai")),
        "{review:?}"
    );
    assert!(
        review
            .references
            .contains(&format!("Workspace access: {WS_A}"))
    );
    assert!(desk.writes().is_empty());
    // Once the route no longer names it, the delete applies and reads back
    // absent.
    desk.service().items.get_mut("configs").expect("routes")[0]["config"] =
        json!(json!({"targets": [{"provider": "@backup"}]}).to_string());
    let result = desk.apply(&delete).await;
    assert_eq!(result.outcome, Some(Outcome::Completed));
    assert_eq!(result.readback, Some(Readback::Absent));
    let result = desk
        .apply(&draft(
            "gatewayWorkspaces",
            "delete",
            WS_EMPTY,
            json!({}),
            None,
        ))
        .await;
    assert_eq!(result.readback, Some(Readback::Absent), "{result:?}");
    let writes: Vec<String> = desk.writes().iter().map(line).collect();
    assert_eq!(
        writes,
        vec![
            format!("DELETE {DATA}/providers/{PROVIDER}"),
            format!("DELETE {ADMIN}/workspaces/{WS_EMPTY}"),
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn credentials_never_travel_in_a_draft_and_stored_ones_are_protected() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let before = desk.requests();
    for body in [
        json!({"ai_provider_id": OPENAI_FAMILY, "name": "x", "slug": "x", "key": "sk-live"}),
        json!({"ai_provider_id": OPENAI_FAMILY, "name": "x", "slug": "x",
               "configurations": {"api_key": "sk-live"}}),
    ] {
        let review = desk
            .review(&draft("gatewayIntegrations", "create", "", body, None))
            .await;
        assert_eq!(problem(&review), Some(ChangeProblem::CredentialPolicy));
    }
    assert_eq!(desk.requests(), before, "nothing read or sent");
    // The stored route carries a credential: replacing its document would
    // lose it, so it is refused after reading it, with nothing written.
    let review = desk
        .review(&draft(
            "gatewayRoutes",
            "update",
            ROUTE,
            json!({"config": {"strategy": {"mode": "single"}, "targets": [{"provider": "@openai-prod"}]}}),
            Some(&reference),
        ))
        .await;
    assert_eq!(
        problem(&review),
        Some(ChangeProblem::ProtectedConfiguration)
    );
    let review = desk
        .review(&draft(
            "gatewayGuardrails",
            "update",
            GUARDRAIL,
            json!({"checks": [{"id": "pii-check", "parameters": {"scan_scope": "all_messages"}}]}),
            Some(&reference),
        ))
        .await;
    assert_eq!(
        problem(&review),
        Some(ChangeProblem::ProtectedConfiguration)
    );
    // Renaming the route keeps its document and applies.
    let result = desk
        .apply(&draft(
            "gatewayRoutes",
            "update",
            ROUTE,
            json!({"name": "primary"}),
            Some(&reference),
        ))
        .await;
    assert_eq!(result.outcome, Some(Outcome::Completed));
    assert!(leaks(&desk.snap()).is_empty());
    assert_eq!(desk.writes().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn guardrail_checks_are_validated_against_the_catalog() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let create = |parameters: Value| {
        draft(
            "gatewayGuardrails",
            "create",
            "",
            json!({"name": "pii-2", "checks": [{"id": "pii-check", "parameters": parameters}],
                   "actions": {"deny": true}}),
            Some(&reference),
        )
    };
    let review = desk
        .review(&create(json!({"scan_scope": "everything"})))
        .await;
    assert_eq!(problem(&review), Some(ChangeProblem::Configuration));
    let review = desk
        .review(&draft(
            "gatewayGuardrails",
            "create",
            "",
            json!({"name": "x", "checks": [{"id": "not-in-catalog"}], "actions": {"deny": true}}),
            Some(&reference),
        ))
        .await;
    assert_eq!(problem(&review), Some(ChangeProblem::Configuration));
    assert!(desk.writes().is_empty());
    let result = desk
        .apply(&create(json!({"scan_scope": "all_messages"})))
        .await;
    assert_eq!(result.outcome, Some(Outcome::Completed));
    let writes = desk.writes();
    let sent: Value = serde_json::from_slice(&writes[0].body).expect("body");
    assert_eq!(
        (
            sent["workspace_id"].as_str(),
            sent["organisation_id"].as_str()
        ),
        (Some(WS_A), Some(TSG))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn one_time_keys_are_held_in_memory_and_shown_once() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let create = draft(
        "gatewayServiceKeys",
        "create",
        "",
        json!({"name": "deploy-key", "scopes": ["completions.write"], "type": "service"}),
        Some(&reference),
    );
    let created = desk.apply(&create).await;
    assert_eq!(created.secret, SecretState::Held);
    assert_eq!(created.readback, Some(Readback::Found), "{created:?}");
    assert!(
        leaks(&desk.snap()).is_empty(),
        "the key never leaves the core"
    );
    let sent: Value = serde_json::from_slice(&desk.writes()[0].body).expect("body");
    assert_eq!(sent["workspace_id"], WS_A);
    assert_eq!(sent["organisation_id"], TSG);
    desk.send(json!({"kind": "revealSecret", "operation": created.operation}))
        .await
        .expect("revealed");
    let shown = desk.adapters.secrets.shown();
    assert_eq!(shown.len(), 1);
    assert!(
        shown[0].0.starts_with("New Gateway key for deploy-key"),
        "{}",
        shown[0].0
    );
    assert!(shown[0].1.starts_with("gw-ONE-TIME-"));
    assert_eq!(
        desk.send(json!({"kind": "revealSecret", "operation": created.operation}))
            .await,
        Err(Refusal::CapabilityMissing)
    );
    // Rotation returns a new value once, held the same way.
    let rotated = desk
        .apply(&draft(
            "gatewayServiceKeys",
            "rotate",
            SERVICE_KEY,
            json!({"key_transition_period_ms": 3_600_000}),
            Some(&reference),
        ))
        .await;
    assert_eq!(rotated.secret, SecretState::Held);
    assert_eq!(
        line(desk.writes().last().expect("rotate")),
        format!("POST {DATA}/api-keys/service/{SERVICE_KEY}/rotate")
    );
    desk.send(json!({"kind": "forgetSecret", "operation": rotated.operation}))
        .await
        .expect("forgotten");
    assert_eq!(
        desk.snap().changes.results[0].secret,
        SecretState::Forgotten
    );
    assert!(leaks(&desk.snap()).is_empty());
}

fn provision(name: &str, scope: &str, reuse: bool) -> Value {
    draft(
        "gatewayWorkspaces",
        "create",
        "",
        json!({"name": name, "scope_name": scope, "existing_scope": reuse}),
        None,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn provisioning_reports_every_step_and_reconciles_what_exists() {
    let mut desk = desk().await;
    let draft = provision("Team C", "scope-team-c", false);
    let review = desk.review(&draft).await;
    assert_eq!(review.confirmation.as_deref(), Some("CREATE"));
    desk.adapters.confirmer.auto(Some(true));
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let result = desk.settled().await;
    let (_, request) = desk.adapters.confirmer.requests().pop().expect("request");
    assert!(
        request.message.contains("IAM scope: scope-team-c."),
        "{}",
        request.message
    );
    assert_eq!(result.outcome, Some(Outcome::Completed));
    let steps = result.provisioning.expect("steps");
    assert_eq!(
        (steps.scope, steps.workspace, steps.binding),
        (
            StepState::Completed,
            StepState::Completed,
            StepState::Completed
        )
    );
    assert_eq!(
        (steps.scope_read, steps.bound_read, steps.workspace_read),
        (Reconciled::Found, Some(true), Reconciled::Found)
    );
    assert_eq!(steps.cleanup, ScopeCleanup::NotNeeded);
    assert!(steps.verified());
    let writes: Vec<String> = desk.writes().iter().map(line).collect();
    assert_eq!(
        writes,
        vec![
            format!("POST {IAM}/scopes"),
            format!("POST {ADMIN}/workspaces"),
            format!("PUT {IAM}/scopes/scope-team-c"),
        ]
    );
    // Reviewing an existing scope as new is refused; reusing a missing one too.
    let review = desk
        .review(&provision("Team D", "scope-team-a", false))
        .await;
    assert_eq!(problem(&review), Some(ChangeProblem::ScopeExists));
    let review = desk
        .review(&provision("Team D", "scope-missing", true))
        .await;
    assert_eq!(problem(&review), Some(ChangeProblem::ScopeMissing));
    // Reusing an existing scope sends no scope create.
    let writes = desk.writes().len();
    let result = desk.apply(&provision("Team E", "scope-team-b", true)).await;
    let steps = result.provisioning.expect("steps");
    assert_eq!(steps.scope, StepState::Existing);
    assert!(
        desk.writes()[writes..]
            .iter()
            .all(|w| line(w) != format!("POST {IAM}/scopes"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_uncertain_or_failed_step_keeps_its_outcome_and_nothing_is_retried_or_undone() {
    let mut desk = desk().await;
    // The workspace create reaches the service but its reply is lost.
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Lost);
    let result = desk
        .apply(&provision("Team C", "scope-team-c", false))
        .await;
    assert_eq!(result.outcome, Some(Outcome::Unknown));
    let steps = result.provisioning.clone().expect("steps");
    assert_eq!(
        (steps.scope, steps.workspace, steps.binding),
        (
            StepState::Completed,
            StepState::Unknown,
            StepState::NotStarted
        )
    );
    // Reading the tenant back finds what the lost reply created; the scope
    // is kept because the workspace may exist.
    assert_eq!(steps.workspace_read, Reconciled::Found);
    assert_eq!(steps.bound_read, Some(false));
    assert_eq!(steps.cleanup, ScopeCleanup::Retained);
    assert!(!steps.verified());
    let writes: Vec<String> = desk.writes().iter().map(line).collect();
    assert_eq!(
        writes,
        vec![
            format!("POST {IAM}/scopes"),
            format!("POST {ADMIN}/workspaces")
        ],
        "no retry and no cleanup after an uncertain create"
    );
    // A refused workspace: the scope created for it is deleted again.
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Status(400));
    let result = desk
        .apply(&provision("Team F", "scope-team-f", false))
        .await;
    let steps = result.provisioning.clone().expect("steps");
    assert_eq!(
        (steps.scope, steps.workspace),
        (StepState::Deleted, StepState::Rejected)
    );
    assert_eq!(result.outcome, Some(Outcome::Rejected));
    assert_eq!(steps.scope_read, Reconciled::Absent);
    assert_eq!(steps.cleanup, ScopeCleanup::Deleted);
    // The binding fails after the workspace exists: partially completed.
    desk.service()
        .faults
        .insert(format!("PUT {IAM}/scopes/scope-team-g"), Fault::Status(500));
    let result = desk
        .apply(&provision("Team G", "scope-team-g", false))
        .await;
    let steps = result.provisioning.clone().expect("steps");
    assert_eq!(steps.workspace, StepState::Completed);
    assert!(
        matches!(steps.binding, StepState::Rejected | StepState::Unknown),
        "{steps:?}"
    );
    // Partially completed: the workspace exists; an uncertain binding keeps
    // the whole outcome unknown.
    let expected = if steps.binding == StepState::Unknown {
        Outcome::Unknown
    } else {
        Outcome::Completed
    };
    assert_eq!(result.outcome, Some(expected));
    assert!(result.failure.is_some());
    assert_eq!(
        (steps.workspace_read, steps.bound_read),
        (Reconciled::Found, Some(false))
    );
    // A lost provider update is unknown and sent once.
    let reference = desk.choose(WS_A).await;
    desk.service()
        .faults
        .insert(format!("PUT {DATA}/providers/{PROVIDER}"), Fault::Lost);
    let writes = desk.writes().len();
    let result = desk
        .apply(&draft(
            "gatewayProviders",
            "update",
            PROVIDER,
            json!({"note": "n"}),
            Some(&reference),
        ))
        .await;
    assert_eq!(result.outcome, Some(Outcome::Unknown));
    assert!(
        matches!(
            result.failure,
            Some(ChangeFailure::Service {
                failure: ManagementFailure::Network | ManagementFailure::InvalidResponse
            })
        ),
        "{result:?}"
    );
    assert_eq!(desk.writes().len(), writes + 1, "sent once");
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_an_apply_never_claims_a_rollback() {
    let mut desk = desk().await;
    // Cancelled while the workspace create is unanswered: unknown, and the
    // tenant is read back.
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Hang);
    let draft_c = provision("Team C", "scope-team-c", false);
    let review = desk.review(&draft_c).await;
    desk.adapters.confirmer.auto(Some(true));
    desk.request(&review, &draft_c, "CREATE")
        .await
        .expect("requested");
    assert!(
        settle(|| desk.writes().len() == 2).await,
        "scope and workspace sent"
    );
    let operation = desk.snap().changes.results[0].operation.clone();
    desk.send(json!({"kind": "cancelChange", "operation": operation}))
        .await
        .expect("cancelled");
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::Unknown));
    assert_eq!(
        result.failure,
        Some(ChangeFailure::Service {
            failure: ManagementFailure::Cancelled
        })
    );
    let steps = result.provisioning.expect("steps");
    assert_eq!(steps.workspace, StepState::Unknown);
    assert_eq!(
        steps.scope_read,
        Reconciled::Found,
        "the scope it created is reported"
    );
    assert!(
        desk.writes()
            .iter()
            .all(|w| !w.request_line.starts_with("DELETE ")),
        "nothing undone"
    );
    // Cancelled before its write (the baseline re-read is unanswered):
    // not sent.
    let reference = desk.choose(WS_A).await;
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "n"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    desk.service()
        .faults
        .insert(format!("GET {DATA}/providers/{PROVIDER}"), Fault::Hang);
    let writes = desk.writes().len();
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert!(settle(|| desk.snap().changes.results.iter().any(|r| !r.settled)).await);
    let operation = desk.snap().changes.results[0].operation.clone();
    desk.send(json!({"kind": "cancelChange", "operation": operation}))
        .await
        .expect("cancelled");
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::NotSent));
    assert_eq!(desk.writes().len(), writes);
    // Only an applying change of the active context can be cancelled.
    assert_eq!(
        desk.send(json!({"kind": "cancelChange", "operation": result.operation}))
            .await,
        Err(Refusal::InvalidInput)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn readbacks_say_when_the_service_did_not_take_the_reviewed_change() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let models = draft(
        "gatewayIntegrations",
        "models",
        INTEGRATION,
        json!({"models": [{"slug": "o3", "enabled": true}]}),
        None,
    );
    let result = desk.apply(&models).await;
    assert_eq!(result.readback, Some(Readback::Found), "{result:?}");
    assert_eq!(
        line(desk.writes().last().expect("models")),
        format!("PUT {ADMIN}/integrations/{INTEGRATION}/models")
    );
    desk.service().ignores_updates = true;
    let again = draft(
        "gatewayIntegrations",
        "models",
        INTEGRATION,
        json!({"models": [{"slug": "gpt-4o", "enabled": false}]}),
        None,
    );
    assert_eq!(desk.apply(&again).await.readback, Some(Readback::Differs));
    // A routing document the service did not store under a new version.
    desk.service().items.get_mut("configs").expect("routes")[0]["config"] =
        json!(json!({"targets": [{"provider": "@openai-prod"}]}).to_string());
    let route = draft(
        "gatewayRoutes",
        "update",
        ROUTE,
        json!({"config": {"strategy": {"mode": "single"}, "targets": [{"provider": "@openai-prod"}]}}),
        Some(&reference),
    );
    let result = desk.apply(&route).await;
    assert_eq!(result.outcome, Some(Outcome::Completed));
    assert_eq!(result.readback, Some(Readback::Differs));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_credential_replacement_ends_the_review_and_nothing_is_replayed_after_restart() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "x"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert!(settle(|| desk.adapters.confirmer.held() == 1).await);
    // Replacing the management credential is a new context.
    desk.configure().await;
    assert!(settle(|| desk.snap().changes.review.is_none()).await);
    assert!(!desk.adapters.confirmer.complete(true));
    assert!(desk.writes().is_empty());
    // An applied change and a key, then a restart from the saved state.
    let reference = desk.choose(WS_A).await;
    let created = desk
        .apply(&draft(
            "gatewayServiceKeys",
            "create",
            "",
            json!({"name": "deploy-key", "scopes": ["completions.write"], "type": "service"}),
            Some(&reference),
        ))
        .await;
    assert_eq!(created.secret, SecretState::Held);
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(leaks(&format!("{saved:?}")).is_empty());
    let writes = desk.writes().len();
    let (mut adapters, _handles) =
        fixtures::adapters_with(fixtures::ScriptedStore::with_state(saved));
    adapters.keychain = desk.adapters.vault.clone();
    let origin = format!("https://localhost:{}", desk.server.port);
    let restarted = Core::new(
        CoreConfig {
            management: ManagementEndpoints::at_origin(&origin),
            ..CoreConfig::default()
        },
        adapters,
    );
    restarted.start();
    tokio::time::sleep(Duration::from_millis(500)).await;
    let Observation::Shell { snapshot, .. } = restarted
        .operator()
        .observe(ViewScope::Shell)
        .expect("observe")
    else {
        unreachable!("shell scope")
    };
    assert!(snapshot.changes.review.is_none() && snapshot.changes.results.is_empty());
    assert_eq!(desk.writes().len(), writes, "nothing replayed");
}

/// A literal sequence through the production interface for every Gateway
/// operation the desktop advertises (D043): exactly one write with the
/// reviewed body at the operation's path, its readback, a refusal that is
/// rejected and a lost reply that is unknown, each sent once.
struct Case {
    name: &'static str,
    kind: &'static str,
    action: &'static str,
    target: &'static str,
    body: Value,
    /// Inside the chosen workspace (A).
    scoped: bool,
    line: String,
    sent: Value,
    readback: Readback,
}

const ROUTE_2: &str = "7e3a0000-0000-4000-8000-000000000012";
const USER_KEY: &str = "7e3a0000-0000-4000-8000-000000000014";
const INTEGRATION_2: &str = "f6692544-3265-49be-9711-bbdcebc07902";

fn cases() -> Vec<Case> {
    let case = |name, kind, action, target, body: Value, scoped, line: String, sent: Value| Case {
        name,
        kind,
        action,
        target,
        body,
        scoped,
        line,
        sent,
        readback: if action == "delete" {
            Readback::Absent
        } else {
            Readback::Found
        },
    };
    vec![
        case(
            "workspace update",
            "gatewayWorkspaces",
            "update",
            WS_A,
            json!({"description": "Team A's workspace"}),
            false,
            format!("PUT {ADMIN}/workspaces/{WS_A}"),
            json!({"description": "Team A's workspace"}),
        ),
        case(
            "provider create",
            "gatewayProviders",
            "create",
            "",
            json!({"ai_provider_id": OPENAI_FAMILY, "integration_id": INTEGRATION, "name": "p2", "slug": "p2"}),
            true,
            format!("POST {DATA}/providers"),
            json!({"ai_provider_id": OPENAI_FAMILY, "integration_id": INTEGRATION, "name": "p2",
                   "slug": "p2", "workspace_id": WS_A}),
        ),
        case(
            "route create",
            "gatewayRoutes",
            "create",
            "",
            json!({"name": "r2", "config": {"targets": [{"provider": "@openai-prod"}]}}),
            true,
            format!("POST {DATA}/configs"),
            json!({"name": "r2", "config": {"targets": [{"provider": "@openai-prod"}]}, "workspace_id": WS_A}),
        ),
        case(
            "route delete",
            "gatewayRoutes",
            "delete",
            ROUTE_2,
            json!({}),
            true,
            format!("DELETE {DATA}/configs/{ROUTE_2}"),
            Value::Null,
        ),
        case(
            "guardrail update",
            "gatewayGuardrails",
            "update",
            GUARDRAIL,
            json!({"name": "pii-strict"}),
            true,
            format!("PUT {DATA}/guardrails/{GUARDRAIL}"),
            json!({"name": "pii-strict"}),
        ),
        case(
            "guardrail delete",
            "gatewayGuardrails",
            "delete",
            GUARDRAIL,
            json!({}),
            true,
            format!("DELETE {DATA}/guardrails/{GUARDRAIL}"),
            Value::Null,
        ),
        case(
            "service key update",
            "gatewayServiceKeys",
            "update",
            SERVICE_KEY,
            json!({"description": "CI"}),
            true,
            format!("PUT {DATA}/api-keys/service/{SERVICE_KEY}"),
            json!({"description": "CI"}),
        ),
        case(
            "service key delete",
            "gatewayServiceKeys",
            "delete",
            SERVICE_KEY,
            json!({}),
            true,
            format!("DELETE {DATA}/api-keys/service/{SERVICE_KEY}"),
            Value::Null,
        ),
        case(
            "user key create",
            "gatewayUserKeys",
            "create",
            "",
            json!({"name": "alice-key", "scopes": ["completions.write"], "type": "user", "user_id": "5c1e0000-0000-4000-8000-0000000000a1"}),
            true,
            format!("POST {DATA}/api-keys/user"),
            json!({"name": "alice-key", "scopes": ["completions.write"], "type": "user", "user_id": "5c1e0000-0000-4000-8000-0000000000a1",
                   "workspace_id": WS_A, "organisation_id": TSG}),
        ),
        case(
            "user key update",
            "gatewayUserKeys",
            "update",
            USER_KEY,
            json!({"description": "Alice"}),
            true,
            format!("PUT {DATA}/api-keys/user/{USER_KEY}"),
            json!({"description": "Alice"}),
        ),
        case(
            "user key rotate",
            "gatewayUserKeys",
            "rotate",
            USER_KEY,
            json!({"key_transition_period_ms": 3_600_000}),
            true,
            format!("POST {DATA}/api-keys/user/{USER_KEY}/rotate"),
            json!({"key_transition_period_ms": 3_600_000}),
        ),
        case(
            "user key delete",
            "gatewayUserKeys",
            "delete",
            USER_KEY,
            json!({}),
            true,
            format!("DELETE {DATA}/api-keys/user/{USER_KEY}"),
            Value::Null,
        ),
        case(
            "integration create",
            "gatewayIntegrations",
            "create",
            "",
            json!({"ai_provider_id": OPENAI_FAMILY, "name": "Azure", "slug": "azure"}),
            false,
            format!("POST {ADMIN}/integrations"),
            json!({"ai_provider_id": OPENAI_FAMILY, "name": "Azure", "slug": "azure", "organisation_id": TSG}),
        ),
        case(
            "integration update",
            "gatewayIntegrations",
            "update",
            INTEGRATION,
            json!({"description": "Production OpenAI"}),
            false,
            format!("PUT {ADMIN}/integrations/{INTEGRATION}"),
            json!({"description": "Production OpenAI"}),
        ),
        case(
            "integration delete",
            "gatewayIntegrations",
            "delete",
            INTEGRATION_2,
            json!({}),
            false,
            format!("DELETE {ADMIN}/integrations/{INTEGRATION_2}?organisation_id={TSG}"),
            Value::Null,
        ),
    ]
}

/// A desk with the unreferenced items the matrix deletes, and workspace A
/// chosen.
async fn matrix_desk() -> (Desk, String) {
    let mut desk = desk().await;
    {
        let mut service = desk.service();
        let mut spare_route = route(
            ROUTE_2,
            WS_A,
            json!({"targets": [{"provider": "@openai-prod"}]}),
        );
        spare_route["name"] = json!("spare-route");
        spare_route["slug"] = json!("spare-route");
        spare_route["is_default"] = json!(0);
        service
            .items
            .get_mut("configs")
            .expect("routes")
            .push(spare_route);
        let mut user = key(USER_KEY, WS_A, "alice");
        user["object"] = json!("api-key");
        service
            .items
            .get_mut("api-keys/user")
            .expect("user keys")
            .push(user);
        let mut spare = integration();
        spare["id"] = json!(INTEGRATION_2);
        spare["slug"] = json!("spare");
        service.integrations.push(spare);
    }
    let reference = desk.choose(WS_A).await;
    (desk, reference)
}

fn case_draft(case: &Case, reference: &str) -> Value {
    draft(
        case.kind,
        case.action,
        case.target,
        case.body.clone(),
        case.scoped.then_some(reference),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn every_advertised_operation_sends_its_literal_request_once_and_settles_by_outcome() {
    for case in cases() {
        let name = case.name;
        // Faults are keyed by method and path (without the query).
        let path = case.line.split('?').next().expect("line").to_owned();
        // A refusal: rejected, sent once, nothing read back.
        let (mut desk, reference) = matrix_desk().await;
        let draft = case_draft(&case, &reference);
        desk.service()
            .faults
            .insert(path.clone(), Fault::Status(400));
        let refused = desk.apply(&draft).await;
        assert_eq!(
            refused.outcome,
            Some(Outcome::Rejected),
            "{name}: {refused:?}"
        );
        assert_eq!(refused.readback, None, "{name}");
        assert_eq!(desk.writes().len(), 1, "{name}: sent once");
        // Then the same change, reviewed again: one literal write and its
        // readback.
        let applied = desk.apply(&draft).await;
        assert_eq!(
            applied.outcome,
            Some(Outcome::Completed),
            "{name}: {applied:?}"
        );
        assert_eq!(applied.readback, Some(case.readback), "{name}: {applied:?}");
        let writes = desk.writes();
        assert_eq!(writes.len(), 2, "{name}: one more write");
        assert_eq!(line(&writes[1]), case.line, "{name}");
        assert_eq!(writes[1].header("x-tsg-id"), Some(TSG), "{name}");
        let sent: Value = serde_json::from_slice(&writes[1].body).unwrap_or(Value::Null);
        assert_eq!(sent, case.sent, "{name}");
        let shown = applied.workspace.as_ref().map(|w| w.id.as_str());
        let expected = (case.scoped || case.kind == "gatewayWorkspaces").then_some(WS_A);
        assert_eq!(shown, expected, "{name}: the scope shown");
        let secret = matches!(case.action, "rotate")
            || (case.action == "create" && case.kind.ends_with("Keys"));
        assert_eq!(applied.secret == SecretState::Held, secret, "{name}");
        assert!(leaks(&desk.snap()).is_empty(), "{name}");
        // A lost reply: unknown, sent once, no key and no retry.
        let (mut desk, reference) = matrix_desk().await;
        desk.service().faults.insert(path, Fault::Lost);
        let lost = desk.apply(&case_draft(&case, &reference)).await;
        assert_eq!(lost.outcome, Some(Outcome::Unknown), "{name}: {lost:?}");
        assert_eq!(lost.secret, SecretState::None, "{name}");
        assert_eq!(desk.writes().len(), 1, "{name}: never retried");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn choosing_another_workspace_before_the_write_sends_nothing() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "held"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    // The apply's baseline re-read is held while the operator switches A to
    // B and back to A.
    let baseline = format!("GET {DATA}/providers/{PROVIDER}");
    desk.service()
        .faults
        .insert(baseline.clone(), Fault::Delay(1_500));
    let reads = |desk: &Desk| {
        desk.server
            .seen()
            .iter()
            .filter(|s| line(s) == baseline)
            .count()
    };
    let before = reads(&desk);
    desk.adapters.confirmer.auto(Some(true));
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert!(settle(|| reads(&desk) > before).await, "apply is reading");
    desk.choose(WS_B).await;
    let again = desk.choose(WS_A).await;
    assert_ne!(again, reference, "a new reference for the same workspace");
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::NotSent));
    assert_eq!(result.failure, Some(ChangeFailure::WorkspaceChanged));
    assert!(desk.writes().is_empty(), "nothing written to A or B");
    // A write already dispatched keeps its own outcome.
    let review = desk
        .review(&draft(
            "gatewayProviders",
            "update",
            PROVIDER,
            json!({"note": "sent"}),
            Some(&again),
        ))
        .await;
    desk.service().faults.insert(
        format!("PUT {DATA}/providers/{PROVIDER}"),
        Fault::Delay(1_500),
    );
    desk.request(
        &review,
        &draft(
            "gatewayProviders",
            "update",
            PROVIDER,
            json!({"note": "sent"}),
            Some(&again),
        ),
        "openai-prod",
    )
    .await
    .expect("requested");
    assert!(
        settle(|| desk.writes().len() == 1).await,
        "write dispatched"
    );
    desk.choose(WS_B).await;
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::Completed), "{result:?}");
    assert_eq!(result.workspace.as_ref().map(|w| w.id.as_str()), Some(WS_A));
    assert_eq!(desk.writes().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn stopping_during_the_readback_keeps_the_answered_write_and_its_key() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let create = draft(
        "gatewayServiceKeys",
        "create",
        "",
        json!({"name": "deploy-key", "scopes": ["completions.write"], "type": "service"}),
        Some(&reference),
    );
    let review = desk.review(&create).await;
    // The first ID this service issues; its readback is held.
    let created = "7e3a0000-0000-4000-8000-000000000101";
    let readback = format!("GET {DATA}/api-keys/service/{created}");
    desk.service()
        .faults
        .insert(readback.clone(), Fault::Delay(5_000));
    desk.adapters.confirmer.auto(Some(true));
    desk.request(&review, &create, "CREATE")
        .await
        .expect("requested");
    assert!(
        settle(|| desk.server.seen().iter().any(|s| line(s) == readback)).await,
        "reading back"
    );
    let operation = desk.snap().changes.results[0].operation.clone();
    desk.send(json!({"kind": "cancelChange", "operation": operation}))
        .await
        .expect("cancelled");
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::Completed), "{result:?}");
    assert_eq!(result.failure, None);
    assert_eq!(result.readback, Some(Readback::Stopped));
    assert_eq!(result.secret, SecretState::Held);
    assert!(leaks(&desk.snap()).is_empty());
    desk.send(json!({"kind": "revealSecret", "operation": result.operation}))
        .await
        .expect("revealed once");
    let shown = desk.adapters.secrets.shown();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].1, format!("gw-ONE-TIME-{created}"));
    assert_eq!(desk.writes().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_models_readback_checks_the_all_models_setting() {
    let mut desk = desk().await;
    let models = |all: bool| {
        draft(
            "gatewayIntegrations",
            "models",
            INTEGRATION,
            json!({"models": [{"slug": "gpt-4o", "enabled": true}], "allow_all_models": all}),
            None,
        )
    };
    // Both values, taken by the service: read back as reviewed.
    for all in [true, false] {
        let result = desk.apply(&models(all)).await;
        assert_eq!(result.readback, Some(Readback::Found), "{all}: {result:?}");
        let sent: Value =
            serde_json::from_slice(&desk.writes().last().expect("models").body).expect("body");
        assert_eq!(sent["allow_all_models"], all);
    }
    // The service keeps all models allowed while every model row matches.
    desk.service().models.get_mut(INTEGRATION).expect("models")["allow_all_models"] = json!(true);
    desk.service().ignores_all_models = true;
    let result = desk.apply(&models(false)).await;
    assert_eq!(result.outcome, Some(Outcome::Completed));
    assert_eq!(result.readback, Some(Readback::Differs), "{result:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reused_scope_never_attributes_another_workspace_to_the_attempt() {
    // Team B already uses scope-team-b and is listed first.
    let mut desk = desk().await;
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Lost);
    let result = desk.apply(&provision("Team E", "scope-team-b", true)).await;
    assert_eq!(result.outcome, Some(Outcome::Unknown));
    let steps = result.provisioning.expect("steps");
    assert_eq!(steps.workspace_read, Reconciled::Found);
    assert_eq!(steps.workspace_slug.as_deref(), Some("team-e"));
    assert_ne!(steps.workspace_id.as_deref(), Some(WS_B));
    assert_eq!(steps.bound_read, Some(false), "Team E is not bound");
    assert_eq!(steps.cleanup, ScopeCleanup::NotNeeded);
    // A refused binding: the created workspace is reconciled, not Team B.
    let mut desk = self::desk().await;
    desk.service()
        .faults
        .insert(format!("PUT {IAM}/scopes/scope-team-b"), Fault::Status(400));
    let result = desk.apply(&provision("Team F", "scope-team-b", true)).await;
    let steps = result.provisioning.expect("steps");
    assert_eq!(
        (steps.workspace, steps.binding),
        (StepState::Completed, StepState::Rejected)
    );
    assert_eq!(
        result.outcome,
        Some(Outcome::Completed),
        "partially completed"
    );
    assert_eq!(steps.workspace_slug.as_deref(), Some("team-f"));
    assert_eq!(steps.bound_read, Some(false));
    assert!(!steps.verified());
    // The scope's earlier workspaces cannot be read and the create's reply
    // is lost: no workspace is attributed.
    let mut desk = self::desk().await;
    desk.service()
        .faults
        .insert(format!("GET {ADMIN}/workspaces"), Fault::Status(400));
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Lost);
    let result = desk.apply(&provision("Team G", "scope-team-b", true)).await;
    let steps = result.provisioning.expect("steps");
    assert_eq!(steps.workspace_read, Reconciled::Ambiguous);
    assert_eq!((steps.workspace_id, steps.bound_read), (None, None));
    assert_eq!(result.readback, Some(Readback::Incomplete));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_cleanup_is_reported_and_never_called_undone() {
    let mut desk = desk().await;
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Status(400));
    desk.service().faults.insert(
        format!("DELETE {IAM}/scopes/scope-team-h"),
        Fault::Status(400),
    );
    let result = desk
        .apply(&provision("Team H", "scope-team-h", false))
        .await;
    let steps = result.provisioning.expect("steps");
    assert_eq!(steps.cleanup, ScopeCleanup::Failed);
    assert_eq!(
        (steps.scope, steps.workspace),
        (StepState::Completed, StepState::Rejected)
    );
    assert_eq!(steps.scope_read, Reconciled::Found, "the scope remains");
    assert_eq!(
        result.outcome,
        Some(Outcome::Completed),
        "the scope was created"
    );
    assert!(result.failure.is_some());
    // The cleanup's own reply is lost: whether the scope remains is unknown.
    let mut desk = self::desk().await;
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Status(400));
    desk.service()
        .faults
        .insert(format!("DELETE {IAM}/scopes/scope-team-j"), Fault::Lost);
    let result = desk
        .apply(&provision("Team J", "scope-team-j", false))
        .await;
    let steps = result.provisioning.expect("steps");
    assert_eq!(steps.cleanup, ScopeCleanup::Unknown, "{steps:?}");
    assert_eq!(result.outcome, Some(Outcome::Unknown));
}

/// The core restarted from what the first one saved on this Mac.
struct Restarted {
    _core: Core,
    adapters: Handles,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
}

impl Restarted {
    fn start(desk: &Desk, origin: &str, ca: Option<&TestCa>) -> Self {
        let saved = desk.adapters.store.saved().expect("saved");
        let (mut adapters, handles) =
            fixtures::adapters_with(fixtures::ScriptedStore::with_state(saved));
        adapters.keychain = desk.adapters.vault.clone();
        adapters.sessions_root = Some(desk.adapters.sessions_root.clone());
        let core = Core::new(
            CoreConfig {
                management: ManagementEndpoints::at_origin(origin),
                ..CoreConfig::default()
            },
            adapters,
        );
        core.start();
        if let Some(ca) = ca {
            core.set_transport_policy(TransportPolicy {
                revision: 2,
                proxy: ProxyMode::Direct,
                extra_roots: vec![ca.pem.as_bytes().to_vec()],
            })
            .expect("policy");
        }
        let handle = core.operator();
        let session = handle.register_delivery_session().expect("session").nonce;
        Self {
            _core: core,
            adapters: handles,
            handle,
            session,
            sequence: 0,
        }
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

    async fn open(&mut self) -> Value {
        let context = self.context();
        self.send(json!({"kind": "openGatewayLocal", "context": context}))
            .await
            .expect("opened");
        assert!(settle(|| local(&self.snap())["state"] == "ready").await);
        local(&self.snap())
    }
}

fn local(snap: &ShellSnapshot) -> Value {
    serde_json::to_value(&snap.gateway).expect("json")["local"].clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn drafts_and_uncertain_outcomes_survive_an_offline_restart_and_nothing_is_replayed() {
    let mut desk = desk().await;
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| local(&desk.snap())["state"] == "ready").await);
    let reference = desk.choose(WS_A).await;
    let context = desk.context();
    let save = |id: Option<&str>, body: &str| {
        let mut draft = json!({"kind": "gatewayProviders", "action": "update",
            "target": PROVIDER, "body": body, "workspace": reference});
        if let Some(id) = id {
            draft["id"] = json!(id);
        }
        json!({"kind": "saveGatewayDraft", "context": context, "draft": draft})
    };
    desk.send(save(None, r#"{"note": "draft one"}"#))
        .await
        .expect("kept");
    // Credentials are never kept.
    assert_eq!(
        desk.send(save(None, r#"{"key": "sk-CANARY-provider"}"#))
            .await,
        Err(Refusal::InvalidInput)
    );
    assert_eq!(local(&desk.snap())["problem"], "credential");
    let kept = local(&desk.snap());
    assert_eq!(kept["drafts"].as_array().map(Vec::len), Some(1));
    assert_eq!(kept["drafts"][0]["workspace"]["id"], WS_A);
    assert_eq!(kept["drafts"][0]["body"], r#"{"note": "draft one"}"#);
    let draft_id = kept["drafts"][0]["id"].as_str().expect("id").to_owned();
    // A provisioning whose workspace create is lost settles uncertain.
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Lost);
    let uncertain = desk
        .apply(&provision("Team C", "scope-team-c", false))
        .await;
    assert_eq!(uncertain.outcome, Some(Outcome::Unknown));
    assert!(
        settle(|| local(&desk.snap())["records"]
            .as_array()
            .is_some_and(|r| r.len() == 1))
        .await
    );
    // A review left open, and the key a create returns, are not kept.
    let review = desk
        .review(&draft(
            "gatewayProviders",
            "update",
            PROVIDER,
            json!({"note": "draft one"}),
            Some(&reference),
        ))
        .await;
    assert!(settle(|| local(&desk.snap())["unsaved"] == false).await);
    let file = std::fs::read_to_string(desktop_core::gateway_local::path(
        &desk.adapters.sessions_root,
        desk.snap().active.expect("active").tenant.as_str(),
    ))
    .expect("kept on this Mac");
    assert!(
        leaks(&file).is_empty()
            && !file.contains(
                serde_json::to_value(&review.review)
                    .expect("id")
                    .as_str()
                    .expect("id")
            )
    );
    let writes = desk.writes().len();
    let requests = desk.requests();

    // Offline restart: the management service cannot be reached.
    let mut offline = Restarted::start(&desk, "https://localhost:9", None);
    let restored = offline.open().await;
    assert!(offline.snap().changes.review.is_none() && offline.snap().changes.results.is_empty());
    assert_eq!(restored["drafts"][0]["body"], r#"{"note": "draft one"}"#);
    assert_eq!(restored["drafts"][0]["workspace"]["name"], "Team A");
    let record = &restored["records"][0];
    assert_eq!(record["outcome"], "unknown");
    assert_eq!(record["kind"], "gatewayWorkspaces");
    assert_eq!(record["provisioning"]["workspace"], "unknown");
    assert_eq!(record["provisioning"]["cleanup"], "retained");
    assert!(
        record["body"]
            .as_str()
            .is_some_and(|b| b.contains("scope-team-c"))
    );
    // Local edits work offline; the draft keeps its workspace.
    let context = offline.context();
    offline
        .send(
            json!({"kind": "saveGatewayDraft", "context": context, "draft": {
            "id": draft_id, "kind": "gatewayProviders", "action": "update", "target": PROVIDER,
            "body": r#"{"note": "edited offline"}"#}}),
        )
        .await
        .expect("edited offline");
    let edited = local(&offline.snap());
    assert_eq!(edited["drafts"][0]["body"], r#"{"note": "edited offline"}"#);
    assert_eq!(edited["drafts"][0]["workspace"]["id"], WS_A);
    // Nothing restored carries authority: the old review cannot be applied
    // and a review needs a workspace chosen from a fresh listing.
    assert!(
        offline
            .send(json!({"kind": "requestApply", "review": review.review, "draft":
                draft("gatewayProviders", "update", PROVIDER, json!({"note": "draft one"}), Some(&reference)),
                "confirmation": "openai-prod"}))
            .await
            .is_err()
    );
    assert!(
        offline
            .send(json!({"kind": "reviewChange", "context": context, "draft":
                draft("gatewayProviders", "update", PROVIDER, json!({"note": "edited offline"}), Some(&reference))}))
            .await
            .is_err()
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    drop(offline);
    assert_eq!(desk.requests(), requests, "nothing reached the service");

    // Reconnected: still nothing is sent until the operator reviews again.
    let origin = format!("https://localhost:{}", desk.server.port);
    let mut online = Restarted::start(&desk, &origin, Some(&desk._ca));
    let restored = online.open().await;
    assert_eq!(
        restored["drafts"][0]["body"],
        r#"{"note": "edited offline"}"#
    );
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(desk.writes().len(), writes, "nothing replayed on reconnect");
    // An explicit retry is a fresh review and confirmation of the draft.
    let context = online.context();
    online
        .send(json!({"kind": "management", "context": context, "request":
            {"kind": "gateway", "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}}}))
        .await
        .expect("listing");
    assert!(settle(|| online.snap().gateway.workspaces.is_some()).await);
    online
        .send(json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": WS_A}))
        .await
        .expect("chosen");
    let fresh = serde_json::to_value(online.snap().gateway).expect("json")["selected"]["reference"]
        .as_str()
        .expect("reference")
        .to_owned();
    assert_ne!(fresh, reference);
    let retry = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "edited offline"}),
        Some(&fresh),
    );
    online
        .send(json!({"kind": "reviewChange", "context": context, "draft": retry}))
        .await
        .expect("reviewed");
    assert!(
        settle(|| online
            .snap()
            .changes
            .review
            .as_ref()
            .is_some_and(|r| r.state == ChangeReviewState::Ready))
        .await
    );
    let review = online.snap().changes.review.expect("review");
    online.adapters.confirmer.auto(Some(true));
    online
        .send(json!({"kind": "requestApply", "review": review.review, "draft": retry, "confirmation": "openai-prod"}))
        .await
        .expect("requested");
    assert!(
        settle(|| online
            .snap()
            .changes
            .results
            .first()
            .is_some_and(|r| r.settled))
        .await
    );
    assert_eq!(desk.writes().len(), writes + 1, "written once, when asked");
    // Applied and read back: its draft is no longer kept.
    assert!(
        settle(|| local(&online.snap())["drafts"]
            .as_array()
            .is_some_and(Vec::is_empty))
        .await
    );
    let id = local(&online.snap())["records"][0]["id"]
        .as_str()
        .expect("record")
        .to_owned();
    online
        .send(json!({"kind": "discardGatewayLocal", "context": context, "id": id}))
        .await
        .expect("discarded");
    assert!(
        local(&online.snap())["records"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
}

/// Opens the active tenant's local work on `desk`.
async fn open_local(desk: &mut Desk) {
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened");
    assert!(settle(|| local(&desk.snap())["state"] == "ready").await);
}

fn local_file(desk: &Desk, tenant: &str) -> std::path::PathBuf {
    desktop_core::gateway_local::path(&desk.adapters.sessions_root, tenant)
}

#[tokio::test(flavor = "multi_thread")]
async fn credential_text_never_reaches_this_mac_even_while_it_is_being_typed() {
    let mut desk = desk().await;
    open_local(&mut desk).await;
    let reference = desk.choose(WS_A).await;
    let context = desk.context();
    let save = |id: Option<&str>, body: &str| {
        let mut draft = json!({"kind": "gatewayIntegrations", "action": "update",
            "target": INTEGRATION, "body": body, "workspace": reference});
        if let Some(id) = id {
            draft["id"] = json!(id);
        }
        json!({"kind": "saveGatewayDraft", "context": context, "draft": draft})
    };
    desk.send(save(None, r#"{"name": "safe"}"#))
        .await
        .expect("kept");
    let id = local(&desk.snap())["drafts"][0]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    let canary = "CANARY-local-draft";
    let refused = [
        // Truncated while typing: the check cannot see every field.
        (format!(r#"{{"key":"{canary}","#), "incomplete"),
        (
            format!(r#"{{"name": "safe", "key": "{canary}"#),
            "incomplete",
        ),
        (
            format!(r#"{{"config": {{"api_key": "{canary}"}}"#),
            "incomplete",
        ),
        (format!(r#"{{"key": "{canary}""#), "incomplete"),
        // Complete: named, nested or escaped credential fields.
        (format!(r#"{{"key": "{canary}"}}"#), "credential"),
        (
            format!(r#"{{"secret_mappings": {{"a": "{canary}"}}}}"#),
            "credential",
        ),
        (
            format!(r#"{{"config": {{"api_key": "{canary}"}}}}"#),
            "credential",
        ),
        (format!(r#"{{"key": "{canary}"}}"#), "credential"),
        (
            format!(r#"{{"config": {{"api_key": "{canary}"}}}}"#),
            "credential",
        ),
    ];
    for (body, problem) in &refused {
        for id in [None, Some(id.as_str())] {
            assert_eq!(
                desk.send(save(id, body)).await,
                Err(Refusal::InvalidInput),
                "{body}"
            );
            assert_eq!(local(&desk.snap())["problem"], *problem, "{body}");
        }
    }
    // The size bound still holds.
    assert_eq!(
        desk.send(save(Some(&id), &"x".repeat(65_537))).await,
        Err(Refusal::InvalidInput)
    );
    // Only the last complete text is kept and shown.
    let snapshot = serde_json::to_string(&desk.snap()).expect("json");
    assert!(!snapshot.contains(canary));
    assert_eq!(
        local(&desk.snap())["drafts"][0]["body"],
        r#"{"name": "safe"}"#
    );
    let tenant = desk.snap().active.expect("active").tenant;
    let file = local_file(&desk, tenant.as_str());
    assert!(settle(|| file.exists() && local(&desk.snap())["unsaved"] == false).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let kept = std::fs::read_to_string(&file).expect("kept");
    assert!(!kept.contains(canary) && kept.contains("safe"));
    // Restarted: nothing of it is restored.
    let mut restarted = Restarted::start(&desk, "https://localhost:9", None);
    let restored = restarted.open().await;
    assert_eq!(restored["drafts"][0]["body"], r#"{"name": "safe"}"#);
    assert!(
        !serde_json::to_string(&restarted.snap())
            .expect("json")
            .contains(canary)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn local_work_held_across_a_reset_never_reappears() {
    let mut desk = desk().await;
    open_local(&mut desk).await;
    let reference = desk.choose(WS_A).await;
    let context = desk.context();
    let tenant = desk.snap().active.expect("active").tenant;
    let file = local_file(&desk, tenant.as_str());
    let save = |body: &str| {
        json!({"kind": "saveGatewayDraft", "context": context, "draft": {"kind": "gatewayProviders",
            "action": "update", "target": PROVIDER, "body": body, "workspace": reference}})
    };
    desk.send(save(r#"{"note": "kept"}"#)).await.expect("kept");
    assert!(settle(|| file.exists()).await);
    // Tenant B's book has not been read; an uncertain change settling in B
    // queues for that read.
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant b");
    let tenant_b = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("b")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": tenant_b}))
        .await
        .expect("select b");
    desk.configure_active().await;
    let file_b = local_file(&desk, tenant_b.as_str());
    // Every file read and write now waits: A's edit is written later, and
    // the change settling in B leaves a read of B's book waiting.
    let held = desk._core.hold_report_writes().await;
    desk.send(json!({"kind": "selectTenant", "tenant": tenant}))
        .await
        .expect("select a");
    desk.configure_active().await;
    let reference = desk.choose(WS_A).await;
    let context = desk.context();
    desk.send(json!({"kind": "saveGatewayDraft", "context": context, "draft": {"kind": "gatewayProviders",
            "action": "update", "target": PROVIDER, "body": r#"{"note": "pending"}"#, "workspace": reference}}))
        .await
        .expect("edited");
    desk.send(json!({"kind": "selectTenant", "tenant": tenant_b}))
        .await
        .expect("select b");
    desk.configure_active().await;
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Lost);
    let uncertain = desk
        .apply(&provision("Team C", "scope-team-c", false))
        .await;
    assert_eq!(uncertain.outcome, Some(Outcome::Unknown));
    // The reset is admitted while all of that waits, then everything runs.
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    drop(held);
    assert!(
        settle(|| desk.snap().tenants.is_empty() && !desk.adapters.sessions_root.exists()).await
    );
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(!desk.adapters.sessions_root.exists(), "nothing recreated");
    assert!(!file.exists() && !file_b.exists());
    assert_eq!(local(&desk.snap())["state"], "idle");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_in_flight_when_the_context_changes_is_kept_under_its_own_tenant() {
    let mut desk = desk().await;
    open_local(&mut desk).await;
    let tenant_a = desk.snap().active.expect("active").tenant;
    // 1. The credential is replaced while a provider update awaits its
    // reply: the result leaves the screen, its uncertain outcome is kept.
    let reference = desk.choose(WS_A).await;
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "in flight"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    desk.service()
        .faults
        .insert(format!("PUT {DATA}/providers/{PROVIDER}"), Fault::Hang);
    desk.adapters.confirmer.auto(Some(true));
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert!(
        settle(|| desk.writes().len() == 1).await,
        "write dispatched"
    );
    desk.configure().await;
    assert!(
        settle(|| local(&desk.snap())["records"]
            .as_array()
            .is_some_and(|r| r.len() == 1))
        .await,
        "kept under tenant A"
    );
    let record = local(&desk.snap())["records"][0].clone();
    assert_eq!(record["outcome"], "unknown");
    assert_eq!(record["kind"], "gatewayProviders");
    assert_eq!(record["workspace"]["id"], WS_A);
    assert_eq!(record["body"], json!({"note": "in flight"}).to_string());
    assert!(desk.snap().changes.results.is_empty(), "withdrawn");
    // 2. Another tenant is chosen while a provisioning awaits its reply.
    let reference = desk.choose(WS_A).await;
    let _ = reference;
    desk.service()
        .faults
        .insert(format!("POST {ADMIN}/workspaces"), Fault::Hang);
    let provisioning = provision("Team C", "scope-team-c", false);
    let review = desk.review(&provisioning).await;
    desk.request(&review, &provisioning, "CREATE")
        .await
        .expect("requested");
    assert!(
        settle(|| desk.writes().len() == 3).await,
        "scope and workspace sent"
    );
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant b");
    let tenant_b = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("b")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": tenant_b}))
        .await
        .expect("select b");
    let file_a = local_file(&desk, tenant_a.as_str());
    assert!(
        settle(|| desktop_core::gateway_local::load(&file_a).is_ok_and(|b| b.records.len() == 2))
            .await,
        "kept under tenant A while B is chosen"
    );
    assert!(desk.snap().changes.results.is_empty() && desk.snap().changes.review.is_none());
    assert_eq!(local(&desk.snap())["state"], "idle", "B shows only its own");
    let kept = std::fs::read_to_string(&file_a).expect("file");
    assert!(leaks(&kept).is_empty());
    let book = desktop_core::gateway_local::load(&file_a).expect("book");
    assert_eq!(
        book.records[0].change.kind,
        desktop_core::changes::ChangeKind::GatewayWorkspaces
    );
    assert_eq!(book.records[0].outcome, Outcome::Unknown);
    let writes = desk.writes().len();
    // Restarted offline in tenant A: both records are there, nothing sent.
    let mut offline = Restarted::start(&desk, "https://localhost:9", None);
    offline
        .send(json!({"kind": "selectTenant", "tenant": tenant_a}))
        .await
        .expect("select a");
    let restored = offline.open().await;
    assert_eq!(restored["records"].as_array().map(Vec::len), Some(2));
    assert!(offline.snap().changes.results.is_empty());
    drop(offline);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(desk.writes().len(), writes, "nothing replayed");
    // 3. A tenant deleted while its write awaits a reply keeps nothing.
    desk.configure_active().await;
    let reference = desk.choose(WS_A).await;
    let context = desk.context();
    desk.send(json!({"kind": "openGatewayLocal", "context": context}))
        .await
        .expect("opened b");
    assert!(settle(|| local(&desk.snap())["state"] == "ready").await);
    let update = draft(
        "gatewayProviders",
        "update",
        PROVIDER,
        json!({"note": "deleted tenant"}),
        Some(&reference),
    );
    let review = desk.review(&update).await;
    desk.service()
        .faults
        .insert(format!("PUT {DATA}/providers/{PROVIDER}"), Fault::Hang);
    desk.request(&review, &update, "openai-prod")
        .await
        .expect("requested");
    assert!(settle(|| desk.writes().len() == writes + 1).await);
    desk.send(json!({"kind": "deleteTenant", "tenant": tenant_b}))
        .await
        .expect("deleted");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        !local_file(&desk, tenant_b.as_str()).exists(),
        "nothing kept for B"
    );
}
