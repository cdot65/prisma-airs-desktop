//! C29 (D058): a setup draft's reads over the real management pipeline
//! against a synthetic management plane, beside an active tenant: the
//! draft's Gateway listing and choice never touch the active tenant's, its
//! verify lines settle from real reads (management verify, the chosen
//! workspace's prerequisites), an optional service's failure is carried as
//! a warning the operator may continue over, and a discard leaves the
//! active tenant exactly as it was.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::features::{ProductId, VerifyLine, VerifyState};
use desktop_core::fixtures::{self, Handles};
use desktop_core::management::{ManagementEndpoints, ManagementResult};
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
const PROVIDER: &str = "7e3a0000-0000-4000-8000-000000000001";
const ROUTE: &str = "7e3a0000-0000-4000-8000-000000000002";
const GUARDRAIL: &str = "7e3a0000-0000-4000-8000-000000000003";
const SERVICE_KEY: &str = "7e3a0000-0000-4000-8000-000000000004";
const USER_KEY: &str = "7e3a0000-0000-4000-8000-000000000005";
const INTEGRATION: &str = "f6692544-3265-49be-9711-bbdcebc079eb";

/// What the synthetic planes answer: a status and body per path; requests
/// whose target is listed in `hang` are never answered.
#[derive(Default)]
struct Script {
    routes: BTreeMap<String, (u16, Value)>,
    hang: BTreeSet<String>,
    /// What the token endpoint answers (200 grants; 401 refuses the client).
    token_status: u16,
}

impl Script {
    fn route(&self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("");
        if self.hang.contains(&target) {
            return Reply::Hang;
        }
        let (status, body) = if path == "/oauth2/access_token" {
            // The plane checks the client's Basic credentials: a replaced or
            // wrong secret is refused, so a cached binding cannot pass for it.
            let expected = basic(CLIENT_ID, CLIENT_SECRET);
            let presented = seen.header("authorization").unwrap_or("");
            if self.token_status == 200 && presented == expected {
                (
                    200,
                    json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
                )
            } else {
                // A wrong client, or a plane told to refuse grants: 401.
                let status = if self.token_status == 200 {
                    401
                } else {
                    self.token_status
                };
                (
                    status,
                    json!({"error": "invalid_client", "error_description": "SERVER-TEXT-NEVER-SHOWN"}),
                )
            }
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

/// RFC 6749 2.3.1: both parts are form-urlencoded, then Basic-encoded.
fn basic(user: &str, secret: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
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
    routes.insert(
        format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"),
        (200, json!({"ai_profiles": [profile(0)]})),
    );
    routes.insert(
        "/ai-red-teaming/data-plane/v1/scan".to_owned(),
        (200, json!({"data": [{"uuid": "00000000-2222-4333-8444-555555555555", "tsg_id": TSG,
            "name": "Campaign", "target": {"name": "Bot"}, "target_id": "t-1", "target_type": "APPLICATION",
            "job_type": "STATIC", "status": "COMPLETED", "created_at": "2026-10-18T10:00:00Z"}],
            "pagination": {"total_items": 1}})),
    );
    routes.insert(
        "/ai-red-teaming/data-plane/v1/dashboard/scan-statistics".to_owned(),
        (503, json!({"error": "statistics unavailable"})),
    );
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
        token_status: 200,
    }
}

fn profile(n: usize) -> Value {
    json!({"profile_id": format!("00000000-0000-4000-8000-{n:012}"), "profile_name": format!("Profile {n}"),
           "revision": 1, "active": true})
}

struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    script: Arc<Mutex<Script>>,
    _server: TlsServer,
    origin: String,
}

async fn desk() -> Desk {
    desk_with_fixture_creation(true).await
}

async fn desk_with_fixture_creation(fixture_tenant_creation: bool) -> Desk {
    let ca = TestCa::new("airs-v2 c29 ca");
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
    let config = CoreConfig {
        management: ManagementEndpoints::at_origin(&origin),
        fixture_tenant_creation,
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
        _core: core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        script,
        _server: server,
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

    fn setup(&self) -> desktop_core::features::SetupView {
        self.snap().setup.expect("a setup in progress")
    }

    fn active_context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    /// The draft's current context (it changes when its inputs change).
    fn draft_context(&self) -> String {
        self.setup().context.as_str().to_owned()
    }

    async fn tenant_a(&mut self) {
        let origin = self.origin.clone();
        self.send(
            json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
            "destination": {"kind": "custom", "origin": origin}}}),
        )
        .await
        .expect("tenant A");
        let context = self.active_context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        assert!(eventually(|| self.snap().tenants[0].management.state == ScanState::Ready).await);
        // A lists and chooses its workspace.
        let context = self.active_context();
        let list = self.read(&context, json!({"kind": "gateway", "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}})).await;
        assert!(
            matches!(list, ManagementResult::Gateway { .. }),
            "A's listing: {list:?}"
        );
        assert!(eventually(|| self.snap().gateway.workspaces.is_some()).await);
        let context = self.active_context();
        self.send(json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": WS_A}))
            .await
            .expect("A chose");
    }

    async fn read(&mut self, context: &str, request: Value) -> ManagementResult {
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = self
            .send(json!({"kind": "management", "context": context, "request": request}))
            .await
            .expect("admitted")
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
            .expect("result in time")
    }

    async fn verified(&self) -> desktop_core::features::VerifyView {
        for _ in 0..400 {
            if let Some(v) = self.setup().verify
                && v.done
            {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("verification did not finish");
    }
}

#[tokio::test]
async fn a_draft_reads_beside_the_active_tenant_without_touching_its_gateway_choice() {
    let mut desk = desk().await;
    desk.tenant_a().await;
    let a_before = desk.snap().gateway;
    let a_reference = a_before
        .selected
        .as_ref()
        .expect("A's choice")
        .reference
        .clone();
    assert_eq!(
        a_before.selected.as_ref().map(|s| s.workspace.id.as_str()),
        Some(WS_A)
    );
    let active_before = desk.snap().active.expect("active");

    // Draft B over the same synthetic plane (add mode, key optional).
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("draft");
    let first_context = desk.draft_context();
    desk.send(json!({"kind": "configureManagementCredential", "context": first_context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}))
        .await
        .expect("draft client");
    // The credential change retired the draft's context: the old one is refused, the new one works.
    let context = desk.draft_context();
    assert_ne!(context, first_context, "a fresh draft context");
    assert_eq!(
        desk.send(json!({"kind": "verifyTenantSetup", "context": first_context}))
            .await,
        Err(Refusal::StaleContext)
    );
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    desk.send(json!({"kind": "chooseSetupProducts", "context": context, "products": ["gateway"]}))
        .await
        .expect("products");
    // The draft lists through its own context and chooses WS_B; A keeps WS_A.
    let list = desk.read(&context, json!({"kind": "gateway", "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}})).await;
    assert!(
        matches!(list, ManagementResult::Gateway { .. }),
        "draft listing delivered, not withdrawn: {list:?}"
    );
    desk.send(json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": WS_B}))
        .await
        .expect("draft chose");
    let setup = desk.setup();
    let chosen = setup.workspace.expect("draft's workspace");
    assert_eq!(chosen.workspace.id, WS_B);
    assert_ne!(chosen.reference, a_reference);
    let a_during = desk.snap().gateway;
    assert_eq!(
        a_during.selected, a_before.selected,
        "A's choice untouched by the draft's listing and choice"
    );
    assert_eq!(a_during.workspaces, a_before.workspaces);
    assert_eq!(
        desk.snap().active,
        Some(active_before.clone()),
        "A's context untouched"
    );

    // Replacing the draft's client retires its context and the choice under it;
    // the next listing restores the draft's remembered workspace.
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("replaced client");
    let context = desk.draft_context();
    assert!(
        desk.setup().workspace.is_none(),
        "the choice went with the old context"
    );
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    let list = desk.read(&context, json!({"kind": "gateway", "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}})).await;
    assert!(matches!(list, ManagementResult::Gateway { .. }));
    let restored = desk
        .setup()
        .workspace
        .expect("restored from the draft's remembered workspace");
    assert_eq!(restored.workspace.id, WS_B);
    assert!(restored.restored);
    assert_eq!(
        desk.snap().gateway.selected,
        a_before.selected,
        "A still untouched"
    );

    // Verify: real reads settle the lines; AIS/AES are listed as unavailable.
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    let line = |l: VerifyLine| v.steps.iter().find(|s| s.line == l).cloned().expect("line");
    assert_eq!(
        line(VerifyLine::ScanKey).state,
        VerifyState::Warning,
        "no key in add mode"
    );
    assert_eq!(line(VerifyLine::ManagementClient).state, VerifyState::Pass);
    // WS_B's IAM scope lists another workspace: the prerequisites read is
    // partial, and says so, instead of passing on the four inventories alone.
    let gateway = line(VerifyLine::Gateway);
    assert_eq!(gateway.state, VerifyState::Warning, "{}", gateway.reason);
    assert!(
        gateway
            .reason
            .contains("scope scope-team-b does not list the workspace"),
        "{}",
        gateway.reason
    );
    assert_eq!(line(VerifyLine::Ais).state, VerifyState::Unavailable);
    assert_eq!(line(VerifyLine::Aes).state, VerifyState::Unavailable);
    assert!(line(VerifyLine::Ais).product.is_none());
    assert!(v.done && !v.failed && !v.blocking && !v.stale);
    assert_eq!(
        v.warnings.len(),
        2,
        "the missing key and the partial prerequisites"
    );
    assert_eq!(v.warnings[1].product, ProductId::Gateway);
    assert!(desk.setup().completable);

    // Discard: A is exactly as it was.
    desk.send(json!({"kind": "discardTenantSetup", "context": context}))
        .await
        .expect("discard");
    let snap = desk.snap();
    assert!(snap.setup.is_none());
    assert_eq!(snap.gateway.selected, a_before.selected);
    assert_eq!(snap.active, Some(active_before));
    assert_eq!(snap.tenants.len(), 1);
    // A's chosen workspace still answers scoped reads under its reference.
    let a_context = desk.active_context();
    let prerequisites = desk
        .read(
            &a_context,
            json!({"kind": "gateway", "workspace": a_reference, "read": {"kind": "prerequisites"}}),
        )
        .await;
    assert!(
        matches!(prerequisites, ManagementResult::Gateway { .. }),
        "{prerequisites:?}"
    );
}

#[tokio::test]
async fn an_optional_service_failure_is_a_warning_the_operator_may_continue_over() {
    let mut desk = desk().await;
    desk.tenant_a().await;
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("draft");
    let context = desk.draft_context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("draft client");
    let context = desk.draft_context();
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    desk.send(json!({"kind": "chooseSetupProducts", "context": context, "products": ["redTeam"]}))
        .await
        .expect("products");
    // The management plane refuses this client: the verify read fails.
    desk.script.lock().expect("script").token_status = 401;
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = tokio::time::timeout(Duration::from_secs(60), desk.verified())
        .await
        .expect("settled");
    let management = v
        .steps
        .iter()
        .find(|s| s.line == VerifyLine::ManagementClient)
        .expect("line");
    assert_eq!(management.state, VerifyState::Fail, "{}", management.reason);
    assert!(
        v.failed && !v.blocking,
        "an optional service's failure does not block"
    );
    assert!(
        v.warnings
            .iter()
            .any(|w| w.product == ProductId::RuntimeSecurity && w.reason.starts_with("Failed: "))
    );
    assert!(
        desk.setup().completable,
        "Continue with warnings is admitted"
    );
    desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
        .await
        .expect("completed over the warning");
    let snap = desk.snap();
    let b = snap
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("B");
    assert!(
        b.setup_warnings
            .iter()
            .any(|w| w.reason.starts_with("Failed: ")),
        "the failure is recorded, not converted to a pass"
    );
    assert_eq!(snap.active.as_ref().map(|a| &a.tenant), Some(&b.id));
    let _ = &desk.adapters;
}

#[tokio::test]
async fn a_source_that_fails_inside_a_read_is_a_warning_that_reaches_the_tenant() {
    let mut desk = desk().await;
    desk.tenant_a().await;
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("draft");
    let context = desk.draft_context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("draft client");
    let context = desk.draft_context();
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    desk.send(json!({"kind": "chooseSetupProducts", "context": context, "products": ["redTeam"]}))
        .await
        .expect("products");
    // Red Team scans list, but the statistics source answers 503: partial, never Pass.
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    let red_team = v
        .steps
        .iter()
        .find(|s| s.line == VerifyLine::RedTeam)
        .expect("line");
    assert_eq!(red_team.state, VerifyState::Warning, "{}", red_team.reason);
    assert!(
        red_team.reason.contains("statistics did not"),
        "{}",
        red_team.reason
    );
    assert!(v.warnings.iter().any(|w| w.product == ProductId::RedTeam));
    desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
        .await
        .expect("completed");
    let b = desk
        .snap()
        .tenants
        .into_iter()
        .find(|t| t.label == "Tenant B")
        .expect("B");
    assert!(
        b.setup_warnings
            .iter()
            .any(|w| w.product == ProductId::RedTeam && w.reason.contains("statistics did not"))
    );
}

#[tokio::test]
async fn discarding_a_draft_cancels_its_reads_in_flight_and_leaves_the_active_tenant_alone() {
    let mut desk = desk().await;
    desk.tenant_a().await;
    let a_before = desk.snap().gateway;
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("draft");
    let context = desk.draft_context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("draft client");
    let context = desk.draft_context();
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    // The draft's fresh binding must fetch a token first; that request hangs.
    desk.script
        .lock()
        .expect("script")
        .hang
        .insert("/oauth2/access_token".to_owned());
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = desk
        .send(json!({"kind": "management", "context": context,
            "request": {"kind": "gateway", "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}}}))
        .await
        .expect("admitted")
    else {
        panic!("no operation");
    };
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify started");
    tokio::time::sleep(Duration::from_millis(200)).await;
    desk.send(json!({"kind": "discardTenantSetup", "context": context}))
        .await
        .expect("discarded while reads were in flight");
    // The read queued under the draft settles cancelled, not with a result.
    let Ok(Observation::Management(handle)) = desk.handle.observe(ViewScope::Management(operation))
    else {
        panic!("management observation");
    };
    let result = tokio::time::timeout(Duration::from_secs(20), handle.result())
        .await
        .expect("settled in time");
    assert!(
        matches!(
            result,
            ManagementResult::Failed {
                failure: desktop_core::management::ManagementFailure::Cancelled,
                ..
            }
        ),
        "{result:?}"
    );
    desk.script.lock().expect("script").hang.clear();
    let snap = desk.snap();
    assert!(snap.setup.is_none());
    assert_eq!(
        snap.gateway.selected, a_before.selected,
        "A's choice untouched"
    );
    // A's own reads still work under its context and workspace reference.
    let a_context = desk.active_context();
    let a_reference = a_before.selected.expect("A").reference;
    let prerequisites = desk
        .read(
            &a_context,
            json!({"kind": "gateway", "workspace": a_reference, "read": {"kind": "prerequisites"}}),
        )
        .await;
    assert!(
        matches!(prerequisites, ManagementResult::Gateway { .. }),
        "{prerequisites:?}"
    );
}

#[tokio::test]
async fn a_replaced_client_authenticates_afresh_and_profile_changes_retire_the_binding() {
    let mut desk = desk().await;
    desk.tenant_a().await;
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("draft");
    let context = desk.draft_context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("good client");
    let context = desk.draft_context();
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    assert_eq!(v.steps[1].state, VerifyState::Pass, "{}", v.steps[1].reason);
    // Replace the secret with one the plane rejects: the cached binding must not pass for it.
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": "wrong-synthetic-secret"}),
    )
    .await
    .expect("replaced client");
    let wrong_context = desk.draft_context();
    assert_ne!(wrong_context, context);
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    desk.send(json!({"kind": "verifyTenantSetup", "context": wrong_context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    assert_eq!(v.steps[1].state, VerifyState::Fail, "{}", v.steps[1].reason);
    assert!(
        v.steps[1].reason.contains("not accepted"),
        "{}",
        v.steps[1].reason
    );
    assert!(
        !v.blocking && desk.setup().completable,
        "an optional failure"
    );
    // Remove the client, add the right one back: it authenticates afresh.
    desk.send(json!({"kind": "removeManagementCredential", "context": wrong_context}))
        .await
        .expect("removed");
    let removed_context = desk.draft_context();
    assert_ne!(removed_context, wrong_context);
    assert_eq!(desk.setup().tenant.management.state, ScanState::Missing);
    desk.send(json!({"kind": "configureManagementCredential", "context": removed_context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}))
        .await
        .expect("re-added");
    let context = desk.draft_context();
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let v = desk.verified().await;
    assert_eq!(v.steps[1].state, VerifyState::Pass, "{}", v.steps[1].reason);
    // A TSG change retires the management credential (it was bound to the TSG) and the context.
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "updateTenantSetup", "context": context, "profile": {"label": "Tenant B", "tsgId": "9876543210",
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("tsg changed");
    let setup = desk.setup();
    assert_ne!(setup.context.as_str(), context);
    assert_eq!(setup.tenant.tsg_id.as_deref(), Some("9876543210"));
    assert_eq!(
        setup.tenant.management.state,
        ScanState::Missing,
        "the client went with the old TSG"
    );
    assert!(setup.verify.is_none());
    assert_eq!(
        desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
            .await,
        Err(Refusal::StaleContext)
    );
}

// Owner refinement: management authentication gates every new tenant; a
// product permission is a separate observation and scan keys remain optional.
#[tokio::test]
async fn refinement_oauth_authentication_does_not_require_profile_permission() {
    let mut desk = desk().await;
    desk.script.lock().expect("script").routes.insert(
        format!("/aisec/v1/mgmt/profiles/tsg/{TSG}"),
        (403, json!({"error": "profile permission denied"})),
    );
    desk.send(
        json!({"kind": "beginTenantSetup", "mode": "firstRun", "profile": {
            "label": "OAuth only", "tsgId": TSG,
            "destination": {"kind": "region", "region": "europe"}
        }}),
    )
    .await
    .expect("draft");
    let context = desk.draft_context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context,
            "mode": "keychain", "clientId": CLIENT_ID, "secret": CLIENT_SECRET
        }),
    )
    .await
    .expect("management credential");
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    let context = desk.draft_context();
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let verify = desk.verified().await;
    let auth = verify
        .steps
        .iter()
        .find(|s| s.line == VerifyLine::ManagementClient)
        .expect("auth line");
    assert_eq!(
        auth.state,
        VerifyState::Pass,
        "OAuth succeeds independently of Runtime profile permission: {}",
        auth.reason
    );
    assert!(
        !verify.blocking && desk.setup().completable,
        "a missing scan key does not block authenticated first-run"
    );
    assert!(!desk.setup().scan_required);
    assert!(
        desk._server
            .seen()
            .iter()
            .all(|request| !request.request_line.contains("/profiles/")),
        "authentication must not inventory Runtime profiles"
    );
    desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
        .await
        .expect("complete");
    let snapshot = desk.snap();
    assert_eq!(snapshot.tenants.len(), 1);
    assert_eq!(snapshot.tenants[0].scan.state, ScanState::Missing);
    assert_eq!(
        snapshot.onboarding,
        desktop_core::features::OnboardingState::Done
    );
}

#[tokio::test]
async fn refinement_missing_oauth_cannot_complete_an_add_draft() {
    let mut desk = desk().await;
    desk.send(
        json!({"kind": "beginTenantSetup", "mode": "add", "profile": {
            "label": "No OAuth", "tsgId": TSG,
            "destination": {"kind": "region", "region": "europe"}
        }}),
    )
    .await
    .expect("draft");
    let context = desk.draft_context();
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let verify = desk.verified().await;
    assert!(
        verify.blocking && !desk.setup().completable,
        "missing management OAuth must block Add as well as first-run"
    );
    assert_eq!(
        desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
            .await,
        Err(Refusal::InvalidInput)
    );
    assert!(desk.snap().tenants.is_empty());
    assert!(
        desk._server.seen().is_empty(),
        "missing credentials initiate no network call"
    );
}

#[tokio::test]
async fn refinement_invalid_oauth_cannot_complete_an_add_draft() {
    let mut desk = desk().await;
    desk.script.lock().expect("script").token_status = 401;
    desk.send(
        json!({"kind": "beginTenantSetup", "mode": "add", "profile": {
            "label": "Invalid OAuth", "tsgId": TSG,
            "destination": {"kind": "region", "region": "europe"}
        }}),
    )
    .await
    .expect("draft");
    let context = desk.draft_context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context,
            "mode": "keychain", "clientId": CLIENT_ID, "secret": CLIENT_SECRET
        }),
    )
    .await
    .expect("management credential");
    assert!(eventually(|| desk.setup().tenant.management.state == ScanState::Ready).await);
    let context = desk.draft_context();
    desk.send(json!({"kind": "verifyTenantSetup", "context": context}))
        .await
        .expect("verify");
    let verify = desk.verified().await;
    assert!(
        verify.blocking && !desk.setup().completable,
        "invalid OAuth is a blocking authentication failure, not an optional product warning"
    );
    assert_eq!(
        desk.send(json!({"kind": "completeTenantSetup", "context": context, "packs": []}))
            .await,
        Err(Refusal::InvalidInput)
    );
    assert!(desk.snap().tenants.is_empty());
}

#[tokio::test]
async fn refinement_production_policy_rejects_the_legacy_create_tenant_bypass() {
    // This uses the same actor/intent path as production, with the fixture-only
    // compatibility capability explicitly disabled as it is in shipping builds.
    let mut desk = desk_with_fixture_creation(false).await;
    assert_eq!(
        desk.send(json!({"kind": "createTenant", "profile": {
            "label": "Bypass", "tsgId": TSG,
            "destination": {"kind": "region", "region": "europe"}
        }}))
        .await,
        Err(Refusal::InvalidInput)
    );
    assert!(desk.snap().tenants.is_empty());
    assert!(desk.snap().active.is_none());
    assert!(desk._server.seen().is_empty());
}
