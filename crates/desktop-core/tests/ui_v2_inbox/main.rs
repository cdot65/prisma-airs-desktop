//! C30 (D058): the Inbox and read scopes through the production authority
//! interface against a synthetic management plane: per-tenant isolation
//! (I2), read status transitions and partial coverage (I4), the snooze
//! return time computed in the core (I11), a fresh tenant's empty scopes
//! (I12), and setup warnings (C29) as entries the operator can hold.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::features::ProductId;
use desktop_core::fixtures::{self, Handles};
use desktop_core::management::ManagementEndpoints;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AttentionFace, AuthorityHandle, Core, CoreConfig, Delivery, HoldKind, InboxKind, Intent,
    Observation, ProductReadState, ReadState, Receipt, Refusal, ScanState, ShellSnapshot,
    ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c30";
const TSG: &str = "1234567890";

struct Script {
    /// A chart whose violating count exceeds its total (inconsistent).
    inconsistent_chart: bool,
    /// The chart's violating sessions (0: the activity item is an update,
    /// not something to act on).
    violating: u32,
    /// The token grant's status (401: the client is refused).
    token_status: u16,
    /// Never answer the key inventory (a read held open).
    hang_keys: bool,
    chart_status: u16,
    keys_status: u16,
    keys: Vec<Value>,
    /// Rows beyond the page the plane claims for each listing (0: the page
    /// is the inventory).
    more_rows: u32,
    /// The other products' sources (C30 R4): Red Team statistics and scans,
    /// model scans, skill statistics and skill scans.
    red_team_status: [u16; 2],
    model_skill_status: [u16; 3],
}

impl Script {
    fn route(&mut self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        if path == "/oauth2/access_token" {
            if self.token_status != 200 {
                return raw(
                    self.token_status,
                    &json!({"error": "invalid_client", "error_description": "SERVER-TEXT-NEVER-SHOWN"}),
                );
            }
            return raw(
                200,
                &json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        if path == "/aisec/v1/mgmt/dashboard/v2/sessions/sessionschart" {
            return raw(
                self.chart_status,
                &json!({"bucket_size_seconds": 3600, "buckets": [
                    {"time": "2026-10-19T00:00:00Z", "total": 10, "violating": if self.inconsistent_chart { 30 } else { self.violating },
                     "detection_type_violation_breakdown": [{"detection_type": "pi", "violation_breakdown": {"total": 3}}]}]}),
            );
        }
        if path == format!("/aisec/v1/mgmt/apikeys/tsg/{TSG}") {
            if self.hang_keys {
                return Reply::Hang;
            }
            let keys: Vec<Value> = self
                .keys
                .iter()
                .map(|k| {
                    let mut k = k.clone();
                    for (field, value) in [
                        ("api_key_last8", json!("abcd1234")),
                        ("auth_code", json!("SYNTHETIC-AUTH-CODE")),
                        ("cust_app", json!("Support bot")),
                        ("revoked", json!(false)),
                    ] {
                        if k.get(field).is_none() {
                            k[field] = value;
                        }
                    }
                    k
                })
                .collect();
            return raw(self.keys_status, &json!({"api_keys": keys}));
        }
        // Red Team (two sources) and model/skill (three) in the shapes the
        // synthetic plane answers with (tools/native-acceptance/mgmt_server.py).
        if path == "/ai-red-teaming/data-plane/v1/dashboard/scan-statistics" {
            return raw(
                self.red_team_status[0],
                &json!({"total_scans": 3, "targets_scanned": 2,
                    "scan_status": [{"name": "COMPLETED", "count": 3}],
                    "risk_profile": [{"risk_rating": "LOW", "total": 2}],
                    "targets_scanned_by_type": [{"name": "APPLICATION", "count": 2}]}),
            );
        }
        if path == "/ai-red-teaming/data-plane/v1/scan" {
            return raw(
                self.red_team_status[1],
                &json!({"data": [{"uuid": "00000001-2222-4333-8444-555555555555", "status": "COMPLETED",
                    "job_type": "STATIC", "target_id": "66666666-7777-4888-8999-aaaaaaaaaaaa",
                    "target_type": "APPLICATION", "created_at": "2026-10-18T10:00:00Z"}],
                    "pagination": {"total_items": 1 + self.more_rows}}),
            );
        }
        if path == "/aims/data/v1/scans" {
            return raw(
                self.model_skill_status[0],
                &json!({"scans": [{"uuid": "00000001-3333-4333-8444-555555555555", "eval_outcome": "ALLOWED",
                    "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 1 + self.more_rows}}),
            );
        }
        if path == "/aiag/data/v1/stats/scans" {
            return raw(
                self.model_skill_status[1],
                &json!({"unique_skills_scanned": {"count": 1, "percent_change": null},
                    "total_vulnerabilities_found": {"count": 0, "percent_change": null},
                    "top_vulnerability": null}),
            );
        }
        if path == "/aiag/data/v1/scans" {
            return raw(
                self.model_skill_status[2],
                &json!({"scans": [{"uuid": "00000001-4444-4333-8444-555555555555", "name": "AG-PRIVATE-SKILL",
                    "status": "COMPLETED", "eval_outcome": "ALLOWED", "artifact_type": "skill",
                    "summary": {"vulnerability_count": 0, "attack_chain_count": 0},
                    "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 1}}),
            );
        }
        raw(404, &json!({"error": "not found"}))
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

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
}

struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    _adapters: Handles,
    script: Arc<Mutex<Script>>,
    _server: TlsServer,
    origin: String,
}

async fn desk() -> Desk {
    let ca = TestCa::new("airs-v2 c30 ca");
    let script = Arc::new(Mutex::new(Script {
        inconsistent_chart: false,
        violating: 3,
        token_status: 200,
        hang_keys: false,
        chart_status: 200,
        keys_status: 200,
        keys: vec![],
        more_rows: 0,
        red_team_status: [200, 200],
        model_skill_status: [200, 200, 200],
    }));
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
        deadline_tick: Duration::from_millis(100),
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
        _core: core,
        handle,
        session,
        sequence: 0,
        _adapters: handles,
        script,
        _server: server,
        origin: origin.clone(),
    };
    desk.tenant("Tenant A").await;
    desk
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

    fn script(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script.lock().expect("script")
    }

    /// Creates a tenant on the synthetic plane and makes it active.
    async fn tenant(&mut self, label: &str) -> String {
        let origin = self.origin.clone();
        self.send(
            json!({"kind": "createTenant", "profile": {"label": label, "tsgId": TSG,
            "destination": {"kind": "custom", "origin": origin}}}),
        )
        .await
        .expect("tenant");
        let id = self
            .snap()
            .tenants
            .iter()
            .find(|t| t.label == label)
            .expect("tenant")
            .id
            .as_str()
            .to_owned();
        self.send(json!({"kind": "selectTenant", "tenant": id}))
            .await
            .expect("selected");
        id
    }

    async fn management_key(&mut self) {
        let context = self.context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        let active = self.snap().active.expect("active").tenant;
        assert!(
            eventually(|| self
                .snap()
                .tenants
                .iter()
                .any(|t| t.id == active && t.management.state == ScanState::Ready))
            .await
        );
    }

    /// Adds a tenant through the setup draft with the chosen products (no
    /// credentials: the lines skip), completes it (active) and keys it.
    async fn tenant_with_products(&mut self, label: &str, products: &[&str]) -> String {
        let origin = self.origin.clone();
        self.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": label, "tsgId": TSG,
            "destination": {"kind": "custom", "origin": origin}}}))
            .await
            .expect("draft");
        let draft = self
            .snap()
            .setup
            .expect("setup")
            .context
            .as_str()
            .to_owned();
        self.send(json!({"kind": "chooseSetupProducts", "context": draft, "products": products}))
            .await
            .expect("products");
        self.send(json!({"kind": "verifyTenantSetup", "context": draft}))
            .await
            .expect("verify");
        assert!(
            eventually(|| self
                .snap()
                .setup
                .and_then(|s| s.verify)
                .is_some_and(|v| v.done))
            .await
        );
        self.send(json!({"kind": "completeTenantSetup", "context": draft, "packs": []}))
            .await
            .expect("completed");
        let id = self.snap().active.expect("active").tenant;
        self.management_key().await;
        id.as_str().to_owned()
    }

    /// `readTenant`, waited to completion.
    async fn read(&mut self) -> ShellSnapshot {
        let context = self.context();
        let before = self.snap().inbox.read.generation;
        self.send(json!({"kind": "readTenant", "context": context}))
            .await
            .expect("read admitted");
        assert!(
            eventually(|| {
                let s = self.snap();
                s.inbox.read.generation != before && !s.overview.refreshing
            })
            .await,
            "the read completed"
        );
        self.snap()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_tenant_shows_pending_reads_and_an_empty_inbox() {
    let desk = desk().await;
    let snap = desk.snap();
    assert_eq!(snap.inbox.read.state, ReadState::Pending);
    assert!(snap.inbox.items.is_empty());
    assert_eq!(snap.overview.face, AttentionFace::FirstLaunch);
    assert!(snap.overview.attention.is_empty() && snap.overview.held.is_empty());
    assert_eq!(
        snap.inbox.read.products.len(),
        1,
        "Runtime Security until the setup chooses more"
    );
    assert_eq!(snap.inbox.read.products[0].state, ProductReadState::Pending);
    assert_eq!(snap.inbox.read.generation, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_report_per_product_coverage_and_a_recheck_names_its_product() {
    let mut desk = desk().await;
    desk.management_key().await;
    let expiring = desktop_core::sessions::rfc3339(now() + 5 * 86_400 + 60);
    desk.script().keys = vec![
        json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring, "api_key": "SYNTHETIC-KEY-NEVER-SHOWN"}),
    ];
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Done);
    assert!(read.last_read_at.is_some());
    assert_eq!(read.generation, 1);
    assert_eq!(read.products[0].state, ProductReadState::Ok);
    assert_eq!(
        (
            read.products[0].sources_read,
            read.products[0].sources_total
        ),
        (2, 2)
    );
    assert!(read.sources_missing.is_empty());
    // Items carry their kind and product; the key is never in the snapshot.
    let ids: Vec<&str> = snap.inbox.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["runtime-activity", "runtime-key:k1"]);
    assert!(
        snap.inbox
            .items
            .iter()
            .all(|i| i.kind == InboxKind::Runtime && i.product == Some(ProductId::RuntimeSecurity))
    );
    assert!(
        !serde_json::to_string(&snap)
            .expect("json")
            .contains("SYNTHETIC-KEY-NEVER-SHOWN")
    );
    // A failed chart is partial coverage, named.
    desk.script().chart_status = 503;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Done);
    assert_eq!(read.products[0].state, ProductReadState::Incomplete);
    assert_eq!(read.sources_missing, vec!["sessions chart".to_owned()]);
    assert_eq!(read.generation, 2);
    // A refused source needs a role; a recheck names the product until its read lands.
    desk.script().chart_status = 200;
    desk.script().keys_status = 403;
    let context = desk.context();
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "runtimeSecurity"}))
        .await
        .expect("recheck admitted");
    assert_eq!(
        desk.snap().inbox.read.rechecking,
        Some(ProductId::RuntimeSecurity)
    );
    assert!(eventually(|| desk.snap().inbox.read.generation == 3).await);
    let read = desk.snap().inbox.read;
    assert_eq!(read.products[0].state, ProductReadState::NeedsRole);
    assert!(read.sources_missing.contains(&"API keys".to_owned()));
    assert_eq!(read.rechecking, None, "answered by the read");
}

#[tokio::test(flavor = "multi_thread")]
async fn snooze_returns_at_a_core_computed_time_shown_the_same_everywhere() {
    let mut desk = desk().await;
    desk.management_key().await;
    let expiring = desktop_core::sessions::rfc3339(now() + 3 * 86_400 + 60);
    desk.script().keys =
        vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring})];
    desk.read().await;
    let context = desk.context();
    let before = u32::try_from(now()).expect("fits");
    desk.send(json!({"kind": "snoozeItem", "context": context, "id": "runtime-key:k1"}))
        .await
        .expect("snoozed");
    let snap = desk.snap();
    let held = snap
        .overview
        .held
        .iter()
        .find(|h| h.item.id == "runtime-key:k1")
        .expect("held");
    let returns = held.returns_at.expect("a return time");
    assert!(
        (before + 86_400..=before + 86_400 + 5).contains(&returns),
        "24 hours, computed in the core"
    );
    let entry = snap
        .inbox
        .items
        .iter()
        .find(|i| i.id == "runtime-key:k1")
        .expect("entry");
    let hold = entry.hold.as_ref().expect("hold");
    assert_eq!(hold.kind, HoldKind::Snooze);
    assert_eq!(
        hold.returns_at,
        Some(returns),
        "the Inbox states the same time"
    );
    assert!(
        !snap
            .overview
            .attention
            .iter()
            .any(|i| i.id == "runtime-key:k1"),
        "off Home's attention list"
    );
    // Unsnooze brings it back; acknowledge hides it without a return time.
    desk.send(json!({"kind": "unsnoozeItem", "context": context, "id": "runtime-key:k1"}))
        .await
        .expect("unsnoozed");
    assert!(
        desk.snap()
            .overview
            .attention
            .iter()
            .any(|i| i.id == "runtime-key:k1")
    );
    desk.send(json!({"kind": "acknowledgeItem", "context": context, "id": "runtime-key:k1"}))
        .await
        .expect("acknowledged");
    let entry = desk
        .snap()
        .inbox
        .items
        .into_iter()
        .find(|i| i.id == "runtime-key:k1")
        .expect("entry");
    assert_eq!(
        entry.hold.as_ref().map(|h| (h.kind, h.returns_at)),
        Some((HoldKind::Acknowledge, None))
    );
    assert_eq!(
        desk.send(json!({"kind": "snoozeItem", "context": context, "id": "no-such-item"}))
            .await,
        Err(Refusal::InvalidInput)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn every_scope_is_per_tenant_across_a_switch() {
    let mut desk = desk().await;
    desk.management_key().await;
    let expiring = desktop_core::sessions::rfc3339(now() + 3 * 86_400 + 60);
    desk.script().keys =
        vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring})];
    desk.read().await;
    let a = desk.snap().active.expect("A").tenant;
    let context = desk.context();
    desk.send(json!({"kind": "snoozeItem", "context": context, "id": "runtime-key:k1"}))
        .await
        .expect("snoozed in A");
    assert_eq!(desk.snap().inbox.items.len(), 2);
    // B: nothing of A's shows; B starts pending and empty, on its Home.

    desk.send(json!({"kind": "navigate", "feature": "inbox"}))
        .await
        .expect("inbox");
    let b = desk.tenant("Tenant B").await;
    let snap = desk.snap();
    assert_eq!(
        snap.active_feature,
        desktop_core::features::FeatureId::Home,
        "a switch shows the new tenant's Home"
    );
    assert_eq!(
        snap.active.as_ref().map(|x| x.tenant.as_str()),
        Some(b.as_str())
    );
    assert!(snap.inbox.items.is_empty(), "B's Inbox is empty, not A's");
    assert_eq!(snap.inbox.read.state, ReadState::Pending);
    assert!(snap.overview.held.is_empty(), "A's holds are not B's");
    assert_eq!(snap.overview.face, AttentionFace::FirstLaunch);
    // Back to A: its hold is still its own.
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    desk.management_key().await;
    let snap = desk.read().await;
    let held = snap
        .overview
        .held
        .iter()
        .find(|h| h.item.id == "runtime-key:k1");
    assert!(
        held.is_some_and(|h| h.kind == HoldKind::Snooze),
        "A's snooze survived the round trip"
    );
    assert_eq!(
        snap.inbox.items.iter().filter(|i| i.hold.is_some()).count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn setup_warnings_are_inbox_entries_the_operator_can_hold_across_reads() {
    let mut desk = desk().await;
    // A second tenant through the setup draft, without a scan key: its
    // completion records a warning (C29).
    let origin = desk.origin.clone();
    desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("draft");
    let draft = desk
        .snap()
        .setup
        .expect("setup")
        .context
        .as_str()
        .to_owned();
    desk.send(json!({"kind": "verifyTenantSetup", "context": draft}))
        .await
        .expect("verify");
    assert!(
        eventually(|| desk
            .snap()
            .setup
            .and_then(|s| s.verify)
            .is_some_and(|v| v.done))
        .await
    );
    desk.send(json!({"kind": "completeTenantSetup", "context": draft, "packs": []}))
        .await
        .expect("completed");
    let snap = desk.snap();
    let entry = snap
        .inbox
        .items
        .iter()
        .find(|i| i.kind == InboxKind::SetupWarning)
        .expect("the warning as an entry");
    assert_eq!(entry.id, "vw-1");
    assert!(entry.needs_action && entry.product == Some(ProductId::RuntimeSecurity));
    assert!(entry.detail.contains("No scan key saved"));
    assert!(
        snap.overview.attention.iter().any(|i| i.id == "vw-1"),
        "and on Home's attention list"
    );
    assert_eq!(
        snap.inbox.read.state,
        ReadState::Pending,
        "a warning is not a read"
    );
    // Acknowledged, it stays held across reads (reads do not report it).
    let context = desk.context();
    desk.send(json!({"kind": "acknowledgeItem", "context": context, "id": "vw-1"}))
        .await
        .expect("acknowledged");
    desk.management_key().await;
    let snap = desk.read().await;
    let entry = snap
        .inbox
        .items
        .iter()
        .find(|i| i.id == "vw-1")
        .expect("still listed");
    assert_eq!(
        entry.hold.as_ref().map(|h| h.kind),
        Some(HoldKind::Acknowledge)
    );
    assert!(!snap.overview.attention.iter().any(|i| i.id == "vw-1"));
}

/// C30 (I2): a tenant owns its read. Switching A → B → A without another
/// read shows A's cached items, read time, holds and generation again (B
/// stays independently pending), and a recheck pending in one tenant is
/// never reported for the other. A reply admitted under the context A had
/// before the switch is withdrawn, never recorded.
#[tokio::test(flavor = "multi_thread")]
async fn a_tenants_read_survives_a_switch_away_and_back_without_a_new_read() {
    let mut desk = desk().await;
    desk.management_key().await;
    let expiring = desktop_core::sessions::rfc3339(now() + 3 * 86_400 + 60);
    desk.script().keys =
        vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring})];
    let snap = desk.read().await;
    let a = snap.active.expect("A").tenant;
    let read_at = snap.inbox.read.last_read_at.expect("read");
    let context = desk.context();
    desk.send(json!({"kind": "snoozeItem", "context": context, "id": "runtime-key:k1"}))
        .await
        .expect("snoozed in A");
    let before = desk.snap();
    assert_eq!(before.inbox.read.generation, 1);
    // A recheck admitted in A, answered only after the switch: its reply
    // belongs to a retired context.
    desk.script().keys_status = 503;
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "runtimeSecurity"}))
        .await
        .expect("recheck admitted");
    assert_eq!(
        desk.snap().inbox.read.rechecking,
        Some(ProductId::RuntimeSecurity)
    );
    let b = desk.tenant("Tenant B").await;
    let snap = desk.snap();
    assert_eq!(
        snap.active.as_ref().map(|x| x.tenant.as_str()),
        Some(b.as_str())
    );
    assert_eq!(
        snap.inbox.read.state,
        ReadState::Pending,
        "B is its own, unread"
    );
    assert_eq!(snap.inbox.read.rechecking, None, "A's recheck is not B's");
    assert_eq!(snap.inbox.read.generation, 0);
    assert!(snap.inbox.items.is_empty());
    assert!(
        snap.inbox.last_snooze.is_none(),
        "A's snooze receipt is not B's"
    );
    // Back to A: no new read, and A's read is shown again as it was.
    desk.script().keys_status = 200;
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    tokio::time::sleep(Duration::from_millis(600)).await;
    let snap = desk.snap();
    assert_eq!(snap.inbox.read.state, ReadState::Done);
    assert_eq!(
        snap.inbox.read.last_read_at,
        Some(read_at),
        "the cached read time"
    );
    assert_eq!(
        snap.inbox
            .items
            .iter()
            .map(|i| i.id.clone())
            .collect::<Vec<_>>(),
        before
            .inbox
            .items
            .iter()
            .map(|i| i.id.clone())
            .collect::<Vec<_>>(),
        "the cached items"
    );
    assert!(
        snap.inbox.items.iter().any(|i| i.id == "runtime-key:k1"
            && i.hold.as_ref().is_some_and(|h| h.kind == HoldKind::Snooze)),
        "the hold"
    );
    assert_eq!(
        snap.inbox.read.products[0].state,
        ProductReadState::Ok,
        "not the withdrawn 503 reply"
    );
    assert_eq!(
        snap.inbox.read.rechecking, None,
        "the retired recheck is not pending"
    );
    assert_eq!(snap.inbox.last_snooze.as_ref().map(|r| r.sequence), Some(1));
    assert!(snap.overview.read.is_some(), "A's read is shown again");
    // Reading A again is one more completion of A only.
    let snap = desk.read().await;
    assert_eq!(snap.inbox.read.generation, 2);
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("to B");
    assert_eq!(desk.snap().inbox.read.generation, 0, "B's count is its own");
}

/// C30 (I4): `readTenant` reads every chosen product finitely: the Runtime
/// sources and the chosen Gateway workspace through the overview, the Red
/// Team scans and statistics, the model and skill scans; each settles into
/// its own per-source outcome. A chosen Gateway without a workspace is
/// named as missing coverage, never announced complete; a refused source
/// needs a role; `recheckProduct` re-reads one product and refuses one the
/// tenant did not choose.
#[tokio::test(flavor = "multi_thread")]
async fn reads_cover_every_chosen_product_with_finite_per_source_outcomes() {
    let mut desk = desk().await;
    desk.script().model_skill_status = [403, 200, 500];
    desk.tenant_with_products(
        "Tenant M",
        &["runtimeSecurity", "gateway", "redTeam", "modelSkill"],
    )
    .await;
    let snap = desk.snap();
    assert_eq!(snap.inbox.read.products.len(), 4);
    assert!(
        snap.inbox
            .read
            .products
            .iter()
            .all(|p| p.state == ProductReadState::Pending)
    );
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Done);
    let of = |product: ProductId| {
        read.products
            .iter()
            .find(|p| p.product == product)
            .expect("product")
    };
    assert_eq!(of(ProductId::RuntimeSecurity).state, ProductReadState::Ok);
    let gateway = of(ProductId::Gateway);
    assert_eq!(
        gateway.state,
        ProductReadState::Incomplete,
        "no workspace chosen is unread coverage"
    );
    assert_eq!((gateway.sources_read, gateway.sources_total), (0, 2));
    let red_team = of(ProductId::RedTeam);
    assert_eq!(red_team.state, ProductReadState::Ok, "{red_team:?}");
    assert_eq!((red_team.sources_read, red_team.sources_total), (2, 2));
    let model_skill = of(ProductId::ModelSkill);
    assert_eq!(
        model_skill.state,
        ProductReadState::NeedsRole,
        "{model_skill:?}"
    );
    assert_eq!(
        (model_skill.sources_read, model_skill.sources_total),
        (1, 3)
    );
    assert_eq!(
        read.sources_missing,
        vec![
            "Gateway workspace (none chosen)".to_owned(),
            "model scan list".to_owned(),
            "skill scan list".to_owned()
        ],
        "each source not established, by name"
    );
    assert_eq!(read.generation, 1, "one completion for the whole read");
    // Nothing of another product's private data reaches the snapshot.
    assert!(
        !serde_json::to_string(&snap)
            .expect("json")
            .contains("AG-PRIVATE-SKILL")
    );
    // The role granted: a recheck of that product alone re-reads it.
    desk.script().model_skill_status = [200, 200, 200];
    let context = desk.context();
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "modelSkill"}))
        .await
        .expect("recheck admitted");
    assert_eq!(
        desk.snap().inbox.read.rechecking,
        Some(ProductId::ModelSkill)
    );
    assert!(eventually(|| desk.snap().inbox.read.generation == 2).await);
    let read = desk.snap().inbox.read;
    let model_skill = read
        .products
        .iter()
        .find(|p| p.product == ProductId::ModelSkill)
        .expect("product");
    assert_eq!(model_skill.state, ProductReadState::Ok);
    assert_eq!(model_skill.sources_read, 3);
    assert_eq!(read.rechecking, None);
    assert_eq!(
        read.sources_missing,
        vec!["Gateway workspace (none chosen)".to_owned()]
    );
    // Every Red Team source refused: a finite failure needing a role, named.
    desk.script().red_team_status = [403, 403];
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "redTeam"}))
        .await
        .expect("recheck admitted");
    assert!(eventually(|| desk.snap().inbox.read.generation == 3).await);
    let read = desk.snap().inbox.read;
    let red_team = read
        .products
        .iter()
        .find(|p| p.product == ProductId::RedTeam)
        .expect("product");
    assert_eq!(red_team.state, ProductReadState::NeedsRole, "{red_team:?}");
    assert!(
        read.sources_missing
            .contains(&"Red Team statistics".to_owned())
    );
    assert!(
        read.sources_missing
            .contains(&"Red Team scan list".to_owned())
    );
    // A product the tenant did not choose cannot be rechecked.
    desk.tenant("Tenant R").await;
    desk.management_key().await;
    let context = desk.context();
    assert_eq!(
        desk.send(json!({"kind": "recheckProduct", "context": context, "product": "redTeam"}))
            .await,
        Err(Refusal::InvalidInput)
    );
}

/// C30 (I4): a listing establishes its source only when the page is the
/// inventory. Rows beyond the first page (or a capped count) leave the scan
/// list unread coverage: the product is incomplete and the completion is
/// not announced as complete; a page that is the inventory stays ok.
#[tokio::test(flavor = "multi_thread")]
async fn a_truncated_listing_is_incomplete_coverage_not_a_complete_read() {
    let mut desk = desk().await;
    desk.script().more_rows = 500;
    desk.tenant_with_products("Tenant T", &["runtimeSecurity", "redTeam", "modelSkill"])
        .await;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Done);
    let of = |product: ProductId| {
        read.products
            .iter()
            .find(|p| p.product == product)
            .expect("product")
    };
    let red_team = of(ProductId::RedTeam);
    assert_eq!(red_team.state, ProductReadState::Incomplete, "{red_team:?}");
    assert_eq!((red_team.sources_read, red_team.sources_total), (1, 2));
    assert!(
        red_team
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("1 of 501 scans listed")),
        "{red_team:?}"
    );
    let model_skill = of(ProductId::ModelSkill);
    assert_eq!(
        model_skill.state,
        ProductReadState::Incomplete,
        "{model_skill:?}"
    );
    assert_eq!(
        (model_skill.sources_read, model_skill.sources_total),
        (2, 3)
    );
    assert_eq!(
        read.sources_missing,
        vec![
            "Red Team scan list".to_owned(),
            "model scan list".to_owned()
        ]
    );
    // The inventory in one page: complete.
    desk.script().more_rows = 0;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert!(
        read.sources_missing.is_empty(),
        "{:?}",
        read.sources_missing
    );
    assert!(
        read.products
            .iter()
            .all(|p| p.state == ProductReadState::Ok),
        "{:?}",
        read.products
    );
}

/// C30 (I4): an admitted first read that fails before any source answers
/// (the token grant refused) is a failed read: never "pending" (it was
/// attempted) and never done; its products are failed, its sources named
/// as missing, and the completion counts so the presentation announces the
/// failure. A later read recovers; a failed refresh after a good read keeps
/// the good read and says when the refresh failed.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_first_read_is_failed_not_pending_and_a_failed_refresh_keeps_the_older_read() {
    let mut desk = desk().await;
    desk.management_key().await;
    desk.script().token_status = 401;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Failed, "{read:?}");
    assert_eq!(read.last_read_at, None, "nothing was read");
    assert!(read.failed_at.is_some());
    assert_eq!(read.generation, 1, "the attempt completed (failed)");
    assert_eq!(
        read.products[0].state,
        ProductReadState::NeedsRole,
        "{:?}",
        read.products[0]
    );
    assert_eq!(
        read.sources_missing,
        vec!["sessions chart".to_owned(), "API keys".to_owned()]
    );
    assert!(snap.inbox.items.is_empty());
    assert_eq!(
        snap.overview.face,
        AttentionFace::Gap,
        "a failed read is a gap, never an all-clear"
    );
    // Recovered.
    desk.script().token_status = 200;
    let snap = desk.read().await;
    assert_eq!(snap.inbox.read.state, ReadState::Done);
    assert_eq!(snap.inbox.read.failed_at, None);
    let read_at = snap.inbox.read.last_read_at.expect("read");
    // A failed refresh (every source down; the token is still cached) keeps
    // the good read, labelled by the failure time.
    desk.script().chart_status = 503;
    desk.script().keys_status = 503;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Failed);
    assert_eq!(
        read.last_read_at,
        Some(read_at),
        "the older read is still shown"
    );
    assert!(read.failed_at.is_some_and(|f| f >= read_at));
    assert!(snap.overview.read.is_some());
}

/// C30 (I2): per-operation bookkeeping is separate from the tenant's cached
/// record. With a read of A held open, switching A → B → A and then
/// snoozing and reading again in A must not leave A reading for ever: the
/// old part is withdrawn when it lands (it counts nothing and never replaces
/// A's data), the new read completes once, and generations count only valid
/// completions.
#[tokio::test(flavor = "multi_thread")]
async fn a_read_held_open_across_a_switch_round_trip_never_leaves_the_tenant_reading() {
    let mut desk = desk().await;
    desk.management_key().await;
    let expiring = desktop_core::sessions::rfc3339(now() + 3 * 86_400 + 60);
    desk.script().keys =
        vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring})];
    desk.read().await;
    let a = desk.snap().active.expect("A").tenant;
    let items_before: Vec<String> = desk
        .snap()
        .inbox
        .items
        .iter()
        .map(|i| i.id.clone())
        .collect();
    // A read of A held open by the plane (never answered).
    desk.script().hang_keys = true;
    let context = desk.context();
    desk.send(json!({"kind": "readTenant", "context": context}))
        .await
        .expect("read admitted");
    assert!(desk.snap().overview.refreshing);
    assert_eq!(desk.snap().inbox.read.state, ReadState::Reading);
    // A → B → A while it is still open.
    let b = desk.tenant("Tenant B").await;
    assert_eq!(
        desk.snap().inbox.read.state,
        ReadState::Pending,
        "B reads nothing"
    );
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    let snap = desk.snap();
    assert!(
        !snap.overview.refreshing,
        "A is not reading under its new context"
    );
    assert_eq!(
        snap.inbox.read.state,
        ReadState::Done,
        "A's cached read is shown"
    );
    assert_eq!(
        snap.inbox.read.generation, 1,
        "the held read never completed"
    );
    // A snooze and a fresh read in A under the new context.
    let context = desk.context();
    desk.send(json!({"kind": "snoozeItem", "context": context, "id": "runtime-key:k1"}))
        .await
        .expect("snoozed");
    desk.script().hang_keys = false;
    let snap = desk.read().await;
    assert_eq!(
        snap.inbox.read.generation, 2,
        "exactly one valid completion more"
    );
    assert!(!snap.overview.refreshing, "nothing left under way");
    assert_eq!(snap.inbox.read.state, ReadState::Done);
    assert_eq!(
        snap.inbox
            .items
            .iter()
            .map(|i| i.id.clone())
            .collect::<Vec<_>>(),
        items_before
    );
    assert!(
        snap.inbox
            .items
            .iter()
            .any(|i| i.id == "runtime-key:k1" && i.hold.is_some())
    );
    // Read now works again and again: no leaked part.
    let snap = desk.read().await;
    assert_eq!(snap.inbox.read.generation, 3);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!desk.snap().overview.refreshing);
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("to B");
    assert_eq!(desk.snap().inbox.read.generation, 0);
}

/// C30 (I4): a source counts as read only when it established what it is
/// for. A complete key page with an unreported expiration and a chart with
/// inconsistent totals are each named incomplete coverage (the Inbox gap
/// and the completion agree); fully reported healthy answers stay complete.
#[tokio::test(flavor = "multi_thread")]
async fn established_coverage_not_mere_answers_counts_as_read() {
    let mut desk = desk().await;
    desk.management_key().await;
    let snap = desk.read().await;
    assert!(
        snap.inbox.read.sources_missing.is_empty(),
        "{:?}",
        snap.inbox.read
    );
    assert_eq!(snap.inbox.read.products[0].state, ProductReadState::Ok);
    // A key whose expiration is not reported: the inventory answered in full
    // but did not establish every key's expiry.
    desk.script().keys = vec![
        json!({"api_key_id": "k9", "api_key_name": "No expiry reported", "expiration": "unknown"}),
    ];
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(
        read.products[0].state,
        ProductReadState::Incomplete,
        "{:?}",
        read.products[0]
    );
    assert_eq!(read.sources_missing, vec!["API keys".to_owned()]);
    assert!(
        read.products[0]
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("unreported")),
        "{:?}",
        read.products[0]
    );
    assert!(
        snap.overview
            .read
            .as_ref()
            .is_some_and(|r| r.gaps.iter().any(|g| g.contains("unreported"))),
        "the Inbox gap agrees"
    );
    // Inconsistent chart totals: the chart answered but established nothing usable.
    desk.script().keys = vec![];
    desk.script().inconsistent_chart = true;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(
        read.products[0].state,
        ProductReadState::Incomplete,
        "{:?}",
        read.products[0]
    );
    assert_eq!(read.sources_missing, vec!["sessions chart".to_owned()]);
    assert_eq!(read.state, ReadState::Done, "incomplete, not failed");
    desk.script().inconsistent_chart = false;
    let snap = desk.read().await;
    assert!(snap.inbox.read.sources_missing.is_empty());
}

/// C30 (I4): a product read that established nothing is a failed attempt,
/// kept apart from the product's last read that did: a first all-source
/// failure with a chosen product yields a failed read with no read time and
/// a real failure time; a failed recheck keeps the previous outcome (shown
/// as older) and reports the failure explicitly.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_product_attempt_is_reported_and_never_replaces_its_last_read() {
    let mut desk = desk().await;
    desk.tenant_with_products("Tenant P", &["runtimeSecurity", "redTeam"])
        .await;
    // First read: every source fails at the token.
    desk.script().token_status = 401;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Failed, "{read:?}");
    assert_eq!(
        read.last_read_at, None,
        "no read was invented from the failure"
    );
    assert!(read.failed_at.is_some());
    let of = |read: &desktop_core::ReadStatus, product: ProductId| {
        read.products
            .iter()
            .find(|p| p.product == product)
            .cloned()
            .expect("product")
    };
    assert_eq!(
        of(read, ProductId::RedTeam).state,
        ProductReadState::NeedsRole,
        "{:?}",
        of(read, ProductId::RedTeam)
    );
    assert_eq!(of(read, ProductId::RedTeam).sources_read, 0);
    // Recovered: a good read of both.
    desk.script().token_status = 200;
    let snap = desk.read().await;
    let read = snap.inbox.read.clone();
    assert_eq!(read.state, ReadState::Done, "{read:?}");
    assert_eq!(of(&read, ProductId::RedTeam).state, ProductReadState::Ok);
    let good_at = read.last_read_at.expect("read");
    // A recheck of Red Team that reaches nothing (every source 500): a
    // failure, reported as such; the good outcome is kept as older.
    desk.script().red_team_status = [500, 500];
    let context = desk.context();
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "redTeam"}))
        .await
        .expect("recheck admitted");
    assert!(eventually(|| desk.snap().inbox.read.generation == read.generation + 1).await);
    let snap = desk.snap();
    let read = snap.inbox.read.clone();
    assert_eq!(
        read.state,
        ReadState::Failed,
        "a failed recheck is reported: {read:?}"
    );
    assert_eq!(
        read.last_read_at,
        Some(good_at),
        "the good read's time is kept"
    );
    assert!(read.failed_at.is_some_and(|f| f >= good_at));
    let rt = of(&read, ProductId::RedTeam);
    assert_eq!(rt.state, ProductReadState::Failed, "{rt:?}");
    assert!(
        rt.detail.as_deref().is_some_and(|d| d.contains("is kept")),
        "{rt:?}"
    );
    assert!(
        read.sources_missing
            .contains(&"Red Team scan list".to_owned())
    );
    // The next good recheck clears the failure and shows the new outcome.
    desk.script().red_team_status = [200, 200];
    let context = desk.context();
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "redTeam"}))
        .await
        .expect("recheck admitted");
    assert!(eventually(|| desk.snap().inbox.read.generation == read.generation + 1).await);
    let read = desk.snap().inbox.read;
    assert_eq!(read.state, ReadState::Done);
    assert_eq!(read.failed_at, None);
    assert_eq!(of(&read, ProductId::RedTeam).state, ProductReadState::Ok);
}

/// C30 (I4): once anything was attempted, a chosen product nothing has
/// reached yet is unread coverage: a standalone Runtime overview (the
/// retained Runtime view's own read) and a first product-only recheck each
/// keep every other chosen source in `sourcesMissing`, so Home and the
/// completion never announce "all sources complete" over unread products.
#[tokio::test(flavor = "multi_thread")]
async fn unread_chosen_products_stay_in_coverage_after_a_partial_attempt() {
    let mut desk = desk().await;
    desk.tenant_with_products("Tenant U", &["runtimeSecurity", "redTeam", "modelSkill"])
        .await;
    // The retained Runtime view's own read: only the overview.
    let context = desk.context();
    let before = desk.snap().inbox.read.generation;
    desk.send(json!({"kind": "management", "context": context, "request": {"kind": "overview"}}))
        .await
        .expect("overview admitted");
    assert!(eventually(|| desk.snap().inbox.read.generation != before).await);
    let read = desk.snap().inbox.read;
    assert_eq!(read.state, ReadState::Done);
    let of = |read: &desktop_core::ReadStatus, product: ProductId| {
        read.products
            .iter()
            .find(|p| p.product == product)
            .cloned()
            .expect("product")
    };
    assert_eq!(
        of(&read, ProductId::RuntimeSecurity).state,
        ProductReadState::Ok
    );
    assert_eq!(
        of(&read, ProductId::RedTeam).state,
        ProductReadState::Pending
    );
    assert_eq!(
        read.sources_missing,
        vec![
            "Red Team statistics".to_owned(),
            "Red Team scan list".to_owned(),
            "model scan list".to_owned(),
            "skill statistics".to_owned(),
            "skill scan list".to_owned()
        ],
        "every unread chosen source is named"
    );
    // A fresh tenant whose first attempt is a product-only recheck.
    desk.tenant_with_products("Tenant V", &["runtimeSecurity", "redTeam"])
        .await;
    let context = desk.context();
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "redTeam"}))
        .await
        .expect("recheck admitted");
    assert!(eventually(|| desk.snap().inbox.read.generation == 1).await);
    let read = desk.snap().inbox.read;
    assert_eq!(read.state, ReadState::Done);
    assert_eq!(of(&read, ProductId::RedTeam).state, ProductReadState::Ok);
    assert_eq!(
        of(&read, ProductId::RuntimeSecurity).state,
        ProductReadState::Pending
    );
    assert_eq!(
        read.sources_missing,
        vec!["sessions chart".to_owned(), "API keys".to_owned()]
    );
    assert!(read.last_read_at.is_some());
    assert_eq!(read.settled_at, read.last_read_at);
}

/// C30 (I4): a first product-only recheck refused for a role everywhere is
/// a settled attempt: done with that product needing a role, every chosen
/// source unread, no read time invented (`lastReadAt` null) but a settled
/// time for the announcement; never "first read pending".
#[tokio::test(flavor = "multi_thread")]
async fn a_first_product_recheck_refused_for_a_role_is_a_settled_attempt() {
    let mut desk = desk().await;
    desk.script().red_team_status = [403, 403];
    desk.tenant_with_products("Tenant W", &["runtimeSecurity", "redTeam"])
        .await;
    let context = desk.context();
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "redTeam"}))
        .await
        .expect("recheck admitted");
    assert!(eventually(|| desk.snap().inbox.read.generation == 1).await);
    let read = desk.snap().inbox.read;
    assert_eq!(read.state, ReadState::Done, "{read:?}");
    assert_eq!(read.last_read_at, None, "nothing established");
    assert!(read.settled_at.is_some(), "but the attempt settled");
    assert_eq!(read.failed_at, None, "a missing role is not a failed read");
    let rt = read
        .products
        .iter()
        .find(|p| p.product == ProductId::RedTeam)
        .expect("product");
    assert_eq!(rt.state, ProductReadState::NeedsRole, "{rt:?}");
    assert_eq!(
        read.sources_missing,
        vec![
            "Red Team statistics".to_owned(),
            "Red Team scan list".to_owned(),
            "sessions chart".to_owned(),
            "API keys".to_owned()
        ]
    );
}

/// C12/C30 (I4): a healthy empty read is "all clear" only while fresh; past
/// the ten-minute window it is a gap, kept as it was (never re-read on its
/// own), and still a gap after a switch away and back.
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_read_is_a_gap_not_an_all_clear_and_stays_so_across_a_switch() {
    let mut desk = desk().await;
    desk.management_key().await;
    let snap = desk.read().await;
    let a = snap.active.expect("A").tenant;
    let read_at = snap.inbox.read.last_read_at.expect("read");
    assert!(snap.overview.fresh);
    desk._core.advance_clock(660);
    let snap = desk.snap();
    assert!(!snap.overview.fresh, "past the window");
    assert_ne!(snap.overview.face, AttentionFace::AllClear);
    assert_eq!(snap.inbox.read.state, ReadState::Done);
    assert_eq!(
        snap.inbox.read.last_read_at,
        Some(read_at),
        "the cached read is kept"
    );
    assert_eq!(snap.inbox.read.generation, 1, "nothing re-read on its own");
    let b = desk.tenant("Tenant B").await;
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    let snap = desk.snap();
    assert!(!snap.overview.fresh);
    assert_eq!(snap.inbox.read.last_read_at, Some(read_at));
    assert_eq!(snap.inbox.read.generation, 1);
    assert_ne!(snap.overview.face, AttentionFace::AllClear);
    let _ = b;
}

/// C12/C30 (I4): a refresh admitted over an expired cached read changes
/// nothing until it lands: the read is `reading` with the old read's time
/// still its `lastReadAt` and the overview still not fresh; a successful
/// settlement is fresh again; a failed one keeps the older read and names
/// the failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_refresh_over_an_expired_read_restores_nothing_until_it_lands() {
    let mut desk = desk().await;
    desk.management_key().await;
    let snap = desk.read().await;
    let read_at = snap.inbox.read.last_read_at.expect("read");
    desk._core.advance_clock(660);
    assert!(!desk.snap().overview.fresh);
    // Held open: admitted, not landed.
    desk.script().hang_keys = true;
    let context = desk.context();
    desk.send(json!({"kind": "readTenant", "context": context}))
        .await
        .expect("read admitted");
    let snap = desk.snap();
    assert_eq!(snap.inbox.read.state, ReadState::Reading);
    assert_eq!(
        snap.inbox.read.last_read_at,
        Some(read_at),
        "the expired read is still the one shown"
    );
    assert!(!snap.overview.fresh, "admission restores no freshness");
    assert_ne!(snap.overview.face, AttentionFace::AllClear);
    assert_eq!(snap.inbox.read.generation, 1);
    // Released: a fresh read lands.
    desk.script().hang_keys = false;
    assert!(eventually(|| desk.snap().inbox.read.generation == 2).await);
    let snap = desk.snap();
    assert_eq!(snap.inbox.read.state, ReadState::Done);
    // (The read's own time comes from the wall clock the pipeline stamps,
    // not the fixture's moved clock: freshness is asserted by the
    // presentation test; here the landed read replaces the expired one.)
    assert!(snap.inbox.read.last_read_at.is_some_and(|t| t >= read_at));
    assert_eq!(snap.inbox.read.generation, 2);
    let fresh_at = snap.inbox.read.last_read_at.expect("read");
    // A failed refresh keeps that read, named by the failure time.
    desk.script().chart_status = 503;
    desk.script().keys_status = 503;
    let snap = desk.read().await;
    assert_eq!(snap.inbox.read.state, ReadState::Failed);
    assert_eq!(snap.inbox.read.last_read_at, Some(fresh_at));
    assert!(snap.inbox.read.failed_at.is_some_and(|f| f >= fresh_at));
}

/// C30 (I11): a snooze on an update (a normal, non-actionable entry) ends
/// at its return time like any other, and the deadline ticker publishes it
/// without another intent or read, even while the read is already stale.
#[tokio::test(flavor = "multi_thread")]
async fn a_snoozed_update_returns_at_its_deadline_and_is_published() {
    let mut desk = desk().await;
    desk.management_key().await;
    // No violating sessions: the activity item is an update, not actionable.
    desk.script().violating = 0;
    let snap = desk.read().await;
    let update = snap
        .inbox
        .items
        .iter()
        .find(|i| !i.needs_action)
        .expect("a non-actionable update")
        .id
        .clone();
    let context = desk.context();
    desk.send(json!({"kind": "snoozeItem", "context": context, "id": update}))
        .await
        .expect("snoozed");
    assert!(
        desk.snap()
            .inbox
            .items
            .iter()
            .any(|i| i.id == update && i.hold.as_ref().is_some_and(|h| h.kind == HoldKind::Snooze))
    );
    let Ok(Observation::Shell { mut updates, .. }) = desk.handle.observe(ViewScope::Shell) else {
        panic!("shell");
    };
    // The read goes stale first (published), then the snooze ends (published too).
    desk._core.advance_clock(601);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(desktop_core::Update::Shell(snapshot)) = updates.next().await
                && !snapshot.overview.fresh
            {
                return snapshot;
            }
        }
    })
    .await
    .expect("stale published");
    desk._core.advance_clock(86_400);
    let back = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(desktop_core::Update::Shell(snapshot)) = updates.next().await
                && snapshot
                    .inbox
                    .items
                    .iter()
                    .any(|i| i.id == update && i.hold.is_none())
            {
                return snapshot;
            }
        }
    })
    .await
    .expect("the ended snooze published");
    assert_eq!(back.inbox.read.generation, 1, "no read was needed");
}
