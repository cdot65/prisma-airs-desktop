//! C16 Gateway workspace and configuration reads through the production
//! authority interface against synthetic TLS Gateway planes (data, admin and
//! IAM at the management origin): the listed workspaces and the native
//! choice of one, every advertised read with its exact method, path, query
//! and tenant header, allow-listed projections that keep key material and
//! credential-bearing configuration inside the core, completeness that is
//! never claimed from a partial inventory, details proven to belong to the
//! selected workspace, a choice that retires the reads of the one before,
//! restore only when listed again, and prerequisites that report each plane
//! and the IAM scope binding as they were read.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::gateway::{
    GatewayDetail, GatewayList, GatewayPrerequisites, GatewayResult, PlaneAccess, ScopeBinding,
};
use desktop_core::management::{
    DetailFound, ManagementEndpoints, ManagementFailure, ManagementResult,
};
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal, ScanState,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c16";
const TSG: &str = "1234567890";
const DATA: &str = "/ai_gw/v2";
const ADMIN: &str = "/ai_gw/admin/v2";
const IAM: &str = "/iam/v1";
const WS_A: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";
const WS_B: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8298";
const WS_C: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8299";
const PROVIDER: &str = "7e3a0000-0000-4000-8000-000000000001";
const ROUTE: &str = "7e3a0000-0000-4000-8000-000000000002";
const GUARDRAIL: &str = "7e3a0000-0000-4000-8000-000000000003";
const SERVICE_KEY: &str = "7e3a0000-0000-4000-8000-000000000004";
const USER_KEY: &str = "7e3a0000-0000-4000-8000-000000000005";
const INTEGRATION: &str = "f6692544-3265-49be-9711-bbdcebc079eb";
/// Values that must never leave the core.
const CANARIES: [&str; 6] = [
    "sk-CANARY-provider",
    "CANARY-model-config",
    "CANARY-integration-secret",
    "CANARY-route-key",
    "CANARY-guardrail-key",
    "CANARY-key-value",
];

/// What the synthetic planes answer: a status and body per path; requests
/// whose target is listed in `hang` are never answered.
#[derive(Default)]
struct Script {
    routes: BTreeMap<String, (u16, Value)>,
    hang: BTreeSet<String>,
}

impl Script {
    fn route(&self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("");
        if self.hang.contains(&target) {
            return Reply::Hang;
        }
        let (status, body) = if path == "/oauth2/access_token" {
            (
                200,
                json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            )
        } else {
            let key = match path.strip_prefix(DATA) {
                Some(rest) => format!("{rest}?{}", target.split('?').nth(1).unwrap_or("")),
                None => path.to_owned(),
            };
            self.routes
                .get(&key)
                .or_else(|| self.routes.get(path))
                .cloned()
                .unwrap_or((404, json!({"error": "not found"})))
        };
        let body = serde_json::to_vec(&body).expect("json");
        Reply::Raw {
            head: format!(
                "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
                body.len()
            ),
            body,
        }
    }
}

fn workspace(id: &str, name: &str, slug: &str) -> Value {
    json!({"id": id, "slug": slug, "name": name, "icon": null, "description": null,
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "is_default": 0, "status": "active", "scope_name": format!("scope-{slug}"),
           "object": "workspace"})
}

fn listing(rows: Vec<Value>, total: usize, more: Option<bool>) -> Value {
    let mut reply = json!({"object": "list", "total": total, "success": true, "data": rows});
    if let Some(more) = more {
        reply["has_more"] = json!(more);
    }
    reply
}

fn provider_row(id: &str) -> Value {
    json!({"id": id, "name": "openai-prod", "slug": "openai-prod", "object": "provider",
           "integration_id": INTEGRATION, "status": "active",
           "usage_limits": {"credit_limit": 100, "type": "cost", "current_usage": 7},
           "rate_limits": [{"type": "requests", "unit": "rpm", "value": 60}]})
}

fn route_row(id: &str, ws: &str) -> Value {
    json!({"id": id, "name": "default-route", "slug": "default-route", "organisation_id": "org-1",
           "is_default": 1, "status": "active", "owner_id": "owner-1", "updated_by": "owner-1",
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z",
           "workspace_id": ws, "object": "config"})
}

fn guardrail_row(id: &str, ws: &str) -> Value {
    json!({"id": id, "name": "pii", "slug": "pii", "organisation_id": "org-1", "status": "active",
           "owner_id": "owner-1", "updated_by": null, "created_at": "2026-10-01T00:00:00Z",
           "last_updated_at": "2026-10-02T00:00:00Z", "workspace_id": ws, "object": "guardrail"})
}

fn key_row(id: &str, scopes: &[&str]) -> Value {
    json!({"id": id, "name": "ci-key", "object": "api-key", "status": "active",
           "key": "CANARY-key-value", "scopes": scopes})
}

fn integration_row() -> Value {
    json!({"id": INTEGRATION, "name": "OpenAI", "owner_id": "owner-1", "status": "active",
           "created_at": "2026-10-01T00:00:00Z", "last_updated_at": null, "slug": "openai",
           "description": null, "workspace_id": null, "ai_provider_id": "openai",
           "object": "integration", "masked_key": "sk-****abcd"})
}

fn default_script() -> Script {
    let mut routes = BTreeMap::new();
    let mut put = |key: &str, status: u16, body: Value| {
        routes.insert(key.to_owned(), (status, body));
    };
    put(
        "/workspaces?",
        200,
        listing(
            vec![
                workspace(WS_A, "Team A", "team-a"),
                workspace(WS_B, "Team B", "team-b"),
            ],
            2,
            Some(false),
        ),
    );
    put(
        &format!("/workspaces/{WS_A}?"),
        200,
        json!({"id": WS_A, "name": "Team A", "description": null, "created_at": "2026-10-01T00:00:00Z",
               "last_updated_at": "2026-10-02T00:00:00Z", "is_default": 0, "slug": "team-a",
               "icon": null, "defaults": {"config_id": ROUTE, "allow_config_override": true},
               "usage_limits": null, "rate_limits": null, "status": "active"}),
    );
    for ws in [WS_A, WS_B] {
        put(
            &format!("/providers?workspace_id={ws}"),
            200,
            listing(vec![provider_row(PROVIDER)], 1, Some(false)),
        );
        put(
            &format!("/configs?workspace_id={ws}"),
            200,
            listing(vec![route_row(ROUTE, ws)], 1, Some(false)),
        );
        put(
            &format!("/guardrails?workspace_id={ws}"),
            200,
            listing(vec![guardrail_row(GUARDRAIL, ws)], 1, Some(false)),
        );
        put(
            &format!("/api-keys/service?workspace_id={ws}"),
            200,
            listing(
                vec![
                    key_row(SERVICE_KEY, &["completions.write", "models.read"]),
                    key_row("7e3a0000-0000-4000-8000-000000000006", &["models.read"]),
                ],
                2,
                Some(false),
            ),
        );
        put(
            &format!("/api-keys/user?workspace_id={ws}"),
            200,
            listing(vec![key_row(USER_KEY, &["completions.write"])], 1, None),
        );
    }
    put(
        &format!("/providers/{PROVIDER}?"),
        200,
        json!({"id": PROVIDER, "ai_provider_name": "openai", "model_config": {"api_key": "CANARY-model-config"},
               "key": "sk-CANARY-provider", "masked_api_key": "sk-****ider", "slug": "openai-prod",
               "name": "openai-prod", "usage_limits": null, "status": "active", "note": null,
               "created_at": "2026-10-01T00:00:00Z", "expires_at": null, "last_reset_at": null,
               "rate_limits": [], "integration_id": INTEGRATION, "object": "provider"}),
    );
    put(&format!("/configs/{ROUTE}?"), 200, {
        let mut detail = route_row(ROUTE, WS_A);
        detail["config"] = json!(json!({"strategy": {"mode": "fallback"}, "targets": [
                {"provider": "@openai-prod", "api_key": "CANARY-route-key", "forward_headers": ["x-trace"]},
                {"provider": "@backup", "weight": 1}
            ]}).to_string());
        detail["format"] = json!("json");
        detail["type"] = json!("ROUTING");
        detail["version_id"] = json!("v-1");
        detail
    });
    put(&format!("/guardrails/{GUARDRAIL}?"), 200, {
        let mut detail = guardrail_row(GUARDRAIL, WS_A);
        detail["checks"] = json!([{"id": "pii-check", "parameters": {"apiKey": "CANARY-guardrail-key"},
                                       "is_enabled": true}]);
        detail["actions"] = json!({"deny": true, "async": false, "sequential": false});
        detail["version_id"] = json!("v-1");
        detail
    });
    put(
        &format!("/api-keys/service/{SERVICE_KEY}?"),
        200,
        key_row(SERVICE_KEY, &["completions.write"]),
    );
    put(
        &format!("/api-keys/user/{USER_KEY}?"),
        200,
        key_row(USER_KEY, &["completions.write"]),
    );
    put(
        &format!("{ADMIN}/integrations"),
        200,
        listing(vec![integration_row()], 1, Some(false)),
    );
    put(&format!("{ADMIN}/integrations/{INTEGRATION}"), 200, {
        let mut detail = integration_row();
        detail["configurations"] = json!(
            json!({"api_key": "CANARY-integration-secret", "region": "us-east-1"}).to_string()
        );
        detail
    });
    put(
        &format!("{ADMIN}/integrations/{INTEGRATION}/models"),
        200,
        json!({"object": "list", "allow_all_models": false, "total": 2, "models": [
            {"slug": "gpt-4o", "enabled": true, "name": "GPT-4o", "pricing_config": {"pay_as_you_go": {}}},
            {"slug": "o3", "enabled": false}
        ]}),
    );
    put(
        &format!("{IAM}/scopes/scope-team-a"),
        200,
        json!({"name": "scope-team-a", "description": "Team A", "tsg_id": TSG, "id": "scope-1",
               "resources": [{"resource_type": "workspace", "resource_id": "team-a"}]}),
    );
    put(
        &format!("{IAM}/scopes/scope-team-b"),
        200,
        json!({"name": "scope-team-b", "description": "Team B", "tsg_id": TSG, "id": "scope-2",
               "resources": [{"resource_type": "workspace", "resource_id": "someone-else"}]}),
    );
    Script {
        routes,
        hang: BTreeSet::new(),
    }
}

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
    origin: String,
    ca_pem: Vec<u8>,
}

async fn desk() -> Desk {
    let ca = TestCa::new("airs-v2 c16 ca");
    let script = Arc::new(Mutex::new(default_script()));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("script").route(seen)
        })),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = fixtures::adapters();
    let ca_pem = ca.pem.as_bytes().to_vec();
    let core = start(adapters, &origin, &ca_pem);
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    let mut desk = Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        script,
        server,
        origin: origin.clone(),
        ca_pem,
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

fn start(adapters: desktop_core::Adapters, origin: &str, ca_pem: &[u8]) -> Core {
    let config = CoreConfig {
        management: ManagementEndpoints::at_origin(origin),
        ..CoreConfig::default()
    };
    let core = Core::new(config, adapters);
    core.start();
    core.set_transport_policy(TransportPolicy {
        revision: 2,
        proxy: ProxyMode::Direct,
        extra_roots: vec![ca_pem.to_vec()],
    })
    .expect("policy");
    core
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

    /// Stores (or replaces) the management credential: a new context.
    async fn configure(&mut self) {
        let context = self.context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        assert!(eventually(|| self.snap().tenants[0].management.state == ScanState::Ready).await);
    }

    fn snap(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).expect("observe") {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell scope"),
        }
    }

    fn gateway(&self) -> Value {
        serde_json::to_value(self.snap().gateway).expect("json")
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
        self.gateway()["selected"]["reference"]
            .as_str()
            .map(str::to_owned)
    }

    fn script(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script.lock().expect("script")
    }

    /// Admits one Gateway read under `workspace`; returns its operation.
    async fn start_read(
        &mut self,
        workspace: Option<String>,
        read: Value,
    ) -> Result<desktop_core::runtime::OperationId, Refusal> {
        let context = self.context();
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = self
            .send(json!({"kind": "management", "context": context,
                         "request": {"kind": "gateway", "workspace": workspace, "read": read}}))
            .await?
        else {
            panic!("no operation");
        };
        Ok(operation)
    }

    async fn result(&self, operation: desktop_core::runtime::OperationId) -> ManagementResult {
        let Ok(Observation::Management(handle)) =
            self.handle.observe(ViewScope::Management(operation))
        else {
            panic!("management observation");
        };
        tokio::time::timeout(Duration::from_secs(20), handle.result())
            .await
            .expect("result in time")
    }

    async fn read(&mut self, read: Value) -> ManagementResult {
        let scoped = !matches!(
            read["resource"].as_str(),
            Some("workspaces" | "integrations")
        ) && read["kind"] != "integrationModels";
        let workspace = if scoped { self.reference() } else { None };
        let operation = self.start_read(workspace, read).await.expect("admitted");
        self.result(operation).await
    }

    async fn list(&mut self, resource: &str) -> GatewayList {
        match self
            .read(json!({"kind": "inventory", "resource": resource}))
            .await
        {
            ManagementResult::Gateway {
                gateway: GatewayResult::List { list },
            } => list,
            other => panic!("not a list: {other:?}"),
        }
    }

    async fn detail(
        &mut self,
        resource: &str,
        id: &str,
    ) -> Result<GatewayDetail, ManagementFailure> {
        match self
            .read(json!({"kind": "detail", "resource": resource, "id": id}))
            .await
        {
            ManagementResult::Gateway {
                gateway: GatewayResult::Detail { detail },
            } => Ok(detail),
            ManagementResult::Failed { failure, .. } => Err(failure),
            other => panic!("not a detail: {other:?}"),
        }
    }

    async fn prerequisites(&mut self) -> GatewayPrerequisites {
        match self.read(json!({"kind": "prerequisites"})).await {
            ManagementResult::Gateway {
                gateway: GatewayResult::Prerequisites { prerequisites },
            } => *prerequisites,
            other => panic!("not prerequisites: {other:?}"),
        }
    }

    async fn select(&mut self, workspace: &str) -> Result<Receipt, Refusal> {
        let context = self.context();
        self.send(
            json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": workspace}),
        )
        .await
    }

    /// Lists the workspaces and chooses `workspace`.
    async fn choose(&mut self, workspace: &str) -> String {
        self.list("workspaces").await;
        self.select(workspace).await.expect("selected");
        self.reference().expect("reference")
    }

    fn gateway_requests(&self) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| !s.request_line.contains("/oauth2/"))
            .collect()
    }
}

fn rows(list: &GatewayList) -> Vec<Value> {
    serde_json::from_str(&list.rows).expect("rows")
}

fn leaks(value: &impl serde::Serialize) -> Vec<&'static str> {
    let text = serde_json::to_string(value).expect("json");
    CANARIES
        .iter()
        .copied()
        .filter(|c| text.contains(c))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_advertised_read_sends_its_exact_get_with_the_tenant_header() {
    let mut desk = desk().await;
    desk.choose(WS_A).await;
    for resource in [
        "providers",
        "routes",
        "guardrails",
        "serviceKeys",
        "userKeys",
        "integrations",
    ] {
        let list = desk.list(resource).await;
        assert_eq!(list.count as usize, rows(&list).len(), "{resource}");
    }
    for (resource, id) in [
        ("workspaces", WS_A),
        ("providers", PROVIDER),
        ("routes", ROUTE),
        ("guardrails", GUARDRAIL),
        ("serviceKeys", SERVICE_KEY),
        ("userKeys", USER_KEY),
        ("integrations", INTEGRATION),
    ] {
        let detail = desk.detail(resource, id).await.expect(resource);
        assert_eq!(detail.found, DetailFound::Found, "{resource}");
    }
    let models = match desk
        .read(json!({"kind": "integrationModels", "id": INTEGRATION}))
        .await
    {
        ManagementResult::Gateway {
            gateway: GatewayResult::Models { models },
        } => models,
        other => panic!("not models: {other:?}"),
    };
    assert_eq!(
        (models.count, models.complete, models.allow_all),
        (2, true, false)
    );
    assert!(!models.models.contains("pricing"));
    desk.prerequisites().await;
    let seen = desk.gateway_requests();
    let targets: BTreeSet<String> = seen
        .iter()
        .map(|s| {
            s.request_line
                .split(' ')
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect();
    let expected: BTreeSet<String> = [
        format!("GET {DATA}/workspaces"),
        format!("GET {DATA}/workspaces/{WS_A}"),
        format!("GET {DATA}/providers?workspace_id={WS_A}"),
        format!("GET {DATA}/providers/{PROVIDER}"),
        format!("GET {DATA}/configs?workspace_id={WS_A}"),
        format!("GET {DATA}/configs/{ROUTE}"),
        format!("GET {DATA}/guardrails?workspace_id={WS_A}"),
        format!("GET {DATA}/guardrails/{GUARDRAIL}"),
        format!("GET {DATA}/api-keys/service?workspace_id={WS_A}"),
        format!("GET {DATA}/api-keys/service/{SERVICE_KEY}"),
        format!("GET {DATA}/api-keys/user?workspace_id={WS_A}"),
        format!("GET {DATA}/api-keys/user/{USER_KEY}"),
        format!("GET {ADMIN}/integrations"),
        format!("GET {ADMIN}/integrations/{INTEGRATION}"),
        format!("GET {ADMIN}/integrations/{INTEGRATION}/models"),
        format!("GET {IAM}/scopes/scope-team-a"),
    ]
    .into_iter()
    .collect();
    assert_eq!(targets, expected);
    for s in &seen {
        assert!(s.request_line.starts_with("GET "), "{}", s.request_line);
        assert_eq!(s.header("x-tsg-id"), Some(TSG), "{}", s.request_line);
        assert_eq!(
            s.header("authorization"),
            Some("Bearer mgmt-access"),
            "{}",
            s.request_line
        );
        assert!(s.body.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn key_material_and_credential_bearing_configuration_stay_in_the_core() {
    let mut desk = desk().await;
    desk.choose(WS_A).await;
    let mut shown: Vec<Value> = Vec::new();
    for resource in ["providers", "serviceKeys", "userKeys", "integrations"] {
        shown.push(json!(desk.list(resource).await));
    }
    let provider = desk.detail("providers", PROVIDER).await.expect("provider");
    let route = desk.detail("routes", ROUTE).await.expect("route");
    let guardrail = desk
        .detail("guardrails", GUARDRAIL)
        .await
        .expect("guardrail");
    let integration = desk
        .detail("integrations", INTEGRATION)
        .await
        .expect("integration");
    let key = desk.detail("serviceKeys", SERVICE_KEY).await.expect("key");
    shown.extend([
        json!(provider),
        json!(route),
        json!(guardrail),
        json!(integration),
        json!(key),
    ]);
    shown.push(json!(desk.prerequisites().await));
    shown.push(json!(desk.snap()));
    assert!(leaks(&shown).is_empty(), "{:?}", leaks(&shown));
    let item = |d: &GatewayDetail| -> Value {
        serde_json::from_str(d.item.as_deref().expect("item")).expect("json")
    };
    let provider = item(&provider);
    assert!(provider.get("key").is_none() && provider.get("model_config").is_none());
    assert_eq!(provider["verified_workspace_id"], WS_A);
    let route = item(&route);
    assert_eq!(route["credential_bearing"], true);
    assert_eq!(
        route["config"]["targets"][0]["forward_headers"],
        json!(["x-trace"])
    );
    assert_eq!(
        route["protected_routing_fields"],
        json!([{"path": ["targets", 0, "api_key"], "type": "string", "masked": false}])
    );
    let guardrail = item(&guardrail);
    assert_eq!(guardrail["credential_bearing"], true);
    assert!(guardrail.get("checks").is_none());
    let integration = item(&integration);
    assert_eq!(
        integration["configurations"],
        json!({"region": "us-east-1"})
    );
    assert_eq!(integration["protected_fields"], json!(["api_key"]));
    assert!(integration.get("masked_key").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn choosing_another_workspace_retires_the_reads_of_the_one_before() {
    let mut desk = desk().await;
    let a = desk.choose(WS_A).await;
    desk.script()
        .hang
        .insert(format!("{DATA}/providers?workspace_id={WS_A}"));
    let pending = desk
        .start_read(
            Some(a.clone()),
            json!({"kind": "inventory", "resource": "providers"}),
        )
        .await
        .expect("admitted");
    assert!(
        eventually(|| desk.gateway_requests().iter().any(|s| s
            .request_line
            .contains(&format!("providers?workspace_id={WS_A}"))))
        .await
    );
    let context = desk.context();
    desk.select(WS_B).await.expect("B");
    let b = desk.reference().expect("B reference");
    assert_ne!(a, b);
    assert!(matches!(
        desk.result(pending).await,
        ManagementResult::Failed {
            failure: ManagementFailure::Cancelled,
            ..
        }
    ));
    // The tenant context was not touched.
    assert_eq!(desk.context(), context);
    // A read asked under A's reference is refused; B's goes to B.
    assert_eq!(
        desk.start_read(
            Some(a.clone()),
            json!({"kind": "inventory", "resource": "routes"})
        )
        .await
        .err(),
        Some(Refusal::StaleContext)
    );
    let routes = desk.list("routes").await;
    assert_eq!(routes.workspace.as_deref(), Some(WS_B));
    // A→B→A issues a third reference; the first never becomes current.
    desk.script().hang.clear();
    desk.select(WS_A).await.expect("A again");
    let again = desk.reference().expect("reference");
    assert!(again != a && again != b);
    assert_eq!(
        desk.start_read(Some(a), json!({"kind": "inventory", "resource": "routes"}))
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
    assert_eq!(desk.list("routes").await.workspace.as_deref(), Some(WS_A));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_result_settled_before_another_choice_is_withdrawn_when_observed() {
    let mut desk = desk().await;
    let a = desk.choose(WS_A).await;
    let before = desk.gateway_requests().len();
    let settled = desk
        .start_read(
            Some(a),
            json!({"kind": "inventory", "resource": "providers"}),
        )
        .await
        .expect("admitted");
    // The read completes while A is still chosen; it is observed only after
    // B is chosen, and never shown as B's (or anyone's) data.
    assert!(eventually(|| desk.gateway_requests().len() > before).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.select(WS_B).await.expect("B");
    assert!(matches!(
        desk.result(settled).await,
        ManagementResult::Failed {
            failure: ManagementFailure::Cancelled,
            outcome: desktop_core::runtime::Outcome::Completed,
        }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_are_admitted_only_for_listed_workspaces_and_their_current_reference() {
    let mut desk = desk().await;
    // Nothing listed yet: no workspace can be chosen, and no scoped read goes.
    assert_eq!(desk.select(WS_A).await.err(), Some(Refusal::InvalidInput));
    assert_eq!(
        desk.start_read(None, json!({"kind": "prerequisites"}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    desk.list("workspaces").await;
    assert_eq!(desk.select(WS_C).await.err(), Some(Refusal::InvalidInput));
    assert_eq!(desk.gateway()["selected"], Value::Null);
    let a = desk.choose(WS_A).await;
    // A tenant-wide read carries no workspace; a scoped one must carry one.
    assert_eq!(
        desk.start_read(
            Some(a),
            json!({"kind": "inventory", "resource": "integrations"})
        )
        .await
        .err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.start_read(None, json!({"kind": "inventory", "resource": "providers"}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.start_read(
            Some("gw-000000000000000000000000".to_owned()),
            json!({"kind": "inventory", "resource": "providers"})
        )
        .await
        .err(),
        Some(Refusal::StaleContext)
    );
    // Identifiers are plain.
    let reference = desk.reference();
    assert_eq!(
        desk.start_read(
            reference,
            json!({"kind": "detail", "resource": "providers", "id": "../x"})
        )
        .await
        .err(),
        Some(Refusal::InvalidInput)
    );
    assert!(
        desk.gateway_requests()
            .iter()
            .all(|s| !s.request_line.contains("/providers")),
        "nothing refused was sent"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_context_change_clears_the_choice_which_returns_only_when_listed_again() {
    let mut desk = desk().await;
    let a = desk.choose(WS_A).await;
    desk.script()
        .hang
        .insert(format!("{DATA}/configs?workspace_id={WS_A}"));
    let pending = desk
        .start_read(Some(a), json!({"kind": "inventory", "resource": "routes"}))
        .await
        .expect("admitted");
    assert!(
        eventually(|| desk
            .gateway_requests()
            .iter()
            .any(|s| s.request_line.contains("/configs?")))
        .await
    );
    // Replacing the credential issues a new context: the read, the listing
    // and the choice go with the old one.
    desk.configure().await;
    assert!(matches!(
        desk.result(pending).await,
        ManagementResult::Failed {
            failure: ManagementFailure::Cancelled,
            ..
        }
    ));
    let view = desk.gateway();
    assert_eq!(view["selected"], Value::Null, "{view}");
    assert_eq!(view["workspaces"], Value::Null, "{view}");
    assert_eq!(view["remembered"], WS_A, "{view}");
    // Restored once a listing under the new context lists it.
    desk.list("workspaces").await;
    let view = desk.gateway();
    assert_eq!(view["selected"]["workspace"]["id"], WS_A, "{view}");
    assert_eq!(view["selected"]["restored"], true);
    assert_eq!(view["remembered"], Value::Null);
    // A partial listing without it neither restores nor claims it is gone.
    desk.configure().await;
    desk.script().routes.insert(
        "/workspaces?".to_owned(),
        (
            200,
            listing(vec![workspace(WS_B, "Team B", "team-b")], 3, Some(true)),
        ),
    );
    desk.list("workspaces").await;
    let view = desk.gateway();
    assert_eq!(view["selected"], Value::Null, "{view}");
    assert_eq!(
        (view["complete"].clone(), view["rememberedMissing"].clone()),
        (json!(false), json!(false))
    );
    // A complete listing without it says so; nothing is chosen for the
    // operator.
    desk.script().routes.insert(
        "/workspaces?".to_owned(),
        (
            200,
            listing(vec![workspace(WS_B, "Team B", "team-b")], 1, Some(false)),
        ),
    );
    desk.list("workspaces").await;
    let view = desk.gateway();
    assert_eq!(view["selected"], Value::Null, "{view}");
    assert_eq!(view["rememberedMissing"], true, "{view}");
    // A choice a later complete listing no longer lists is cleared.
    desk.select(WS_B).await.expect("B");
    desk.script().routes.insert(
        "/workspaces?".to_owned(),
        (
            200,
            listing(vec![workspace(WS_C, "Team C", "team-c")], 1, Some(false)),
        ),
    );
    desk.list("workspaces").await;
    assert_eq!(desk.gateway()["selected"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_choice_is_remembered_across_restarts_and_forgotten_with_the_tenant() {
    let mut desk = desk().await;
    desk.choose(WS_A).await;
    // A choice that cannot be saved is refused; the previous one stands.
    desk.adapters.store.fail_saves(true);
    assert_eq!(desk.select(WS_B).await.err(), Some(Refusal::DeniedStore));
    assert_eq!(desk.gateway()["selected"]["workspace"]["id"], WS_A);
    desk.adapters.store.fail_saves(false);
    desk.select(WS_B).await.expect("B");
    let state = desk.adapters.store.durable().expect("saved");
    assert_eq!(state.gateway_workspaces.len(), 1);
    let vault = Arc::clone(&desk.adapters.vault);
    let (origin, ca_pem) = (desk.origin.clone(), desk.ca_pem.clone());
    drop(desk.core);
    let (mut adapters, handles) =
        fixtures::adapters_with(fixtures::ScriptedStore::with_state(state));
    adapters.keychain = vault.clone();
    let handles = Handles { vault, ..handles };
    let core = start(adapters, &origin, &ca_pem);
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    let mut again = Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        script: Arc::clone(&desk.script),
        server: desk.server,
        origin,
        ca_pem,
    };
    assert!(
        eventually(|| again
            .snap()
            .tenants
            .first()
            .is_some_and(|t| t.management.state == ScanState::Ready))
        .await
    );
    assert_eq!(again.gateway()["remembered"], WS_B);
    again.list("workspaces").await;
    assert_eq!(again.gateway()["selected"]["workspace"]["id"], WS_B);
    // Reset forgets it with everything else.
    again
        .send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| again.snap().tenants.is_empty()).await);
    assert_eq!(again.gateway()["remembered"], Value::Null);
    assert_eq!(again.gateway()["selected"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_and_malformed_inventories_are_never_complete_or_empty() {
    let mut desk = desk().await;
    desk.choose(WS_A).await;
    let set = |key: String, body: Value| {
        desk.script
            .lock()
            .expect("script")
            .routes
            .insert(key, (200, body));
    };
    set(
        format!("/providers?workspace_id={WS_A}"),
        listing(vec![provider_row(PROVIDER)], 4, None),
    );
    let partial = desk.list("providers").await;
    assert!(!partial.complete && partial.total == Some(4) && partial.count == 1);
    let set = |key: String, body: Value| {
        desk.script
            .lock()
            .expect("script")
            .routes
            .insert(key, (200, body));
    };
    set(
        format!("/providers?workspace_id={WS_A}"),
        listing(vec![provider_row(PROVIDER)], 1, Some(true)),
    );
    assert!(!desk.list("providers").await.complete, "has_more decides");
    let invalid = |desk: &Desk, key: String, body: Value| {
        desk.script
            .lock()
            .expect("script")
            .routes
            .insert(key, (200, body));
    };
    for body in [
        // More rows than the total.
        listing(
            vec![provider_row(PROVIDER), provider_row("p-2")],
            1,
            Some(false),
        ),
        // The same item twice.
        listing(
            vec![provider_row(PROVIDER), provider_row(PROVIDER)],
            2,
            Some(false),
        ),
        // Over 200 rows.
        listing(
            (0..201).map(|i| provider_row(&format!("p-{i}"))).collect(),
            201,
            Some(false),
        ),
        // A row from another workspace.
        listing(
            vec![{
                let mut row = provider_row(PROVIDER);
                row["workspace_id"] = json!(WS_B);
                row
            }],
            1,
            Some(false),
        ),
        // No total: the SDK contract refuses it.
        json!({"object": "list", "data": [provider_row(PROVIDER)]}),
    ] {
        invalid(&desk, format!("/providers?workspace_id={WS_A}"), body);
        assert!(
            matches!(
                desk.read(json!({"kind": "inventory", "resource": "providers"}))
                    .await,
                ManagementResult::Failed {
                    failure: ManagementFailure::InvalidResponse,
                    ..
                }
            ),
            "invalid inventory accepted"
        );
    }
    // A workspace listing that is partial says so.
    invalid(
        &desk,
        "/workspaces?".to_owned(),
        listing(vec![workspace(WS_A, "Team A", "team-a")], 5, None),
    );
    let workspaces = desk.list("workspaces").await;
    assert!(!workspaces.complete);
    assert_eq!(desk.gateway()["complete"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn details_are_shown_only_when_they_belong_to_the_chosen_workspace() {
    let mut desk = desk().await;
    desk.choose(WS_A).await;
    // A route naming another workspace is refused.
    {
        let mut script = desk.script();
        let mut detail = script.routes[&format!("/configs/{ROUTE}?")].1.clone();
        detail["workspace_id"] = json!(WS_B);
        script
            .routes
            .insert(format!("/configs/{ROUTE}?"), (200, detail));
    }
    assert_eq!(
        desk.detail("routes", ROUTE).await.err(),
        Some(ManagementFailure::InvalidResponse)
    );
    // A provider (which names no workspace) not in a complete inventory is
    // absent; not in a partial one, nothing is claimed.
    desk.script().routes.insert(
        format!("/providers?workspace_id={WS_A}"),
        (200, listing(vec![provider_row("p-other")], 1, Some(false))),
    );
    let absent = desk.detail("providers", PROVIDER).await.expect("detail");
    assert_eq!((absent.found, absent.item), (DetailFound::Absent, None));
    desk.script().routes.insert(
        format!("/providers?workspace_id={WS_A}"),
        (200, listing(vec![provider_row("p-other")], 9, Some(true))),
    );
    let incomplete = desk.detail("providers", PROVIDER).await.expect("detail");
    assert_eq!(
        (incomplete.found, incomplete.item),
        (DetailFound::Incomplete, None)
    );
    // A reply for another item is refused; a missing one is not found.
    {
        let mut script = desk.script();
        let mut detail = script.routes[&format!("/guardrails/{GUARDRAIL}?")]
            .1
            .clone();
        detail["id"] = json!("7e3a0000-0000-4000-8000-00000000ffff");
        script
            .routes
            .insert(format!("/guardrails/{GUARDRAIL}?"), (200, detail));
    }
    assert_eq!(
        desk.detail("guardrails", GUARDRAIL).await.err(),
        Some(ManagementFailure::InvalidResponse)
    );
    assert_eq!(
        desk.detail("userKeys", "7e3a0000-0000-4000-8000-00000000eeee")
            .await
            .err(),
        Some(ManagementFailure::NotFound)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn prerequisites_report_each_plane_and_the_scope_binding_as_read() {
    let mut desk = desk().await;
    desk.choose(WS_A).await;
    let p = desk.prerequisites().await;
    assert_eq!(p.workspace, WS_A);
    assert_eq!(
        (p.scope_name.as_str(), p.scope),
        ("scope-team-a", ScopeBinding::Bound)
    );
    assert_eq!(p.inference_keys, 1);
    assert_eq!(
        (p.data_plane, p.admin_plane, p.iam_plane),
        (
            PlaneAccess::Granted,
            PlaneAccess::Granted,
            PlaneAccess::Granted
        )
    );
    assert!(p.routes.is_some() && p.providers.is_some() && p.integrations.is_some());
    // B's scope does not bind B.
    desk.select(WS_B).await.expect("B");
    assert_eq!(desk.prerequisites().await.scope, ScopeBinding::Unbound);
    // Denied admin and IAM planes are shown as denied, the data plane's
    // reads still stand.
    {
        let mut script = desk.script();
        script.routes.insert(
            format!("{ADMIN}/integrations"),
            (403, json!({"error": "forbidden"})),
        );
        script.routes.insert(
            format!("{IAM}/scopes/scope-team-b"),
            (403, json!({"error": "forbidden"})),
        );
    }
    let p = desk.prerequisites().await;
    assert_eq!(
        (p.data_plane, p.admin_plane, p.iam_plane, p.scope),
        (
            PlaneAccess::Granted,
            PlaneAccess::Denied,
            PlaneAccess::Denied,
            ScopeBinding::Denied
        )
    );
    assert_eq!(p.integrations, None);
    assert_eq!(p.integrations_failure, Some(ManagementFailure::Denied));
    // A missing scope is absent; a failed one is not "unbound".
    desk.script()
        .routes
        .remove(&format!("{IAM}/scopes/scope-team-b"));
    assert_eq!(desk.prerequisites().await.scope, ScopeBinding::Absent);
    desk.script().routes.insert(
        format!("{IAM}/scopes/scope-team-b"),
        (500, json!({"error": "SERVER-TEXT"})),
    );
    let p = desk.prerequisites().await;
    assert_eq!(
        (p.scope, p.iam_plane),
        (ScopeBinding::Failed, PlaneAccess::Failed)
    );
    // Every source failing fails the read, with a sanitized failure.
    {
        let mut script = desk.script();
        for path in [
            format!("/providers?workspace_id={WS_B}"),
            format!("/configs?workspace_id={WS_B}"),
            format!("/api-keys/service?workspace_id={WS_B}"),
        ] {
            script
                .routes
                .insert(path, (500, json!({"error": "SERVER-TEXT"})));
        }
        script.routes.insert(
            format!("{ADMIN}/integrations"),
            (500, json!({"error": "SERVER-TEXT"})),
        );
    }
    let failed = desk.read(json!({"kind": "prerequisites"})).await;
    assert!(
        matches!(
            failed,
            ManagementResult::Failed {
                failure: ManagementFailure::Unavailable,
                ..
            }
        ),
        "{failed:?}"
    );
    assert!(
        !serde_json::to_string(&failed)
            .expect("json")
            .contains("SERVER-TEXT")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pending_review_and_the_tenant_context_survive_gateway_use() {
    let mut desk = desk().await;
    let context = desk.context();
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "reviewEndpointChange", "context": context,
                     "destination": {"kind": "custom", "origin": "https://airs-proxy.example.com:8443/"}}))
        .await
        .expect("review");
    let review = desk.snap().endpoint_review.expect("review open");
    desk.choose(WS_A).await;
    desk.list("providers").await;
    desk.select(WS_B).await.expect("B");
    desk.prerequisites().await;
    assert_eq!(desk.context(), context);
    assert_eq!(desk.snap().endpoint_review, Some(review));
    assert!(origin.starts_with("https://localhost:"));
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_are_sanitized_and_name_no_server_text() {
    let mut desk = desk().await;
    desk.choose(WS_A).await;
    for (status, failure) in [
        (401, ManagementFailure::Unauthorized),
        (403, ManagementFailure::Denied),
        (429, ManagementFailure::RateLimited),
        (503, ManagementFailure::Unavailable),
    ] {
        desk.script().routes.insert(
            format!("/guardrails?workspace_id={WS_A}"),
            (
                status,
                json!({"error": "SERVER-TEXT", "detail": "sk-CANARY-provider"}),
            ),
        );
        let result = desk
            .read(json!({"kind": "inventory", "resource": "guardrails"}))
            .await;
        let text = serde_json::to_string(&result).expect("json");
        assert!(
            !text.contains("SERVER-TEXT") && !text.contains("CANARY"),
            "{text}"
        );
        assert!(
            matches!(result, ManagementResult::Failed { failure: f, .. } if f == failure),
            "{status}: {result:?}"
        );
    }
    let models_missing = desk
        .read(json!({"kind": "integrationModels", "id": "f6692544-3265-49be-9711-bbdcebc0ffff"}))
        .await;
    assert!(matches!(
        models_missing,
        ManagementResult::Failed {
            failure: ManagementFailure::NotFound,
            ..
        }
    ));
}
