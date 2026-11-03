//! C36 (D059): the overview contract Home draws its cards from, through the
//! production authority interface against a synthetic management plane:
//! one read yields the Runtime summary and chart the card's figure and bars
//! come from (detection events summed only from usable breakdowns, buckets
//! kept in order), the Gateway day only when a workspace is selected, the
//! per-product read states the pills follow, the needs-action entries the
//! attention list shows minus the operator's holds, and a recheck admitted
//! only for a product the read lists. Nothing here is a new core capability.

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
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, ProductReadState, ReadState,
    Receipt, Refusal, ScanState, ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c36";
const TSG: &str = "1234567890";

/// The synthetic plane: a chart of `buckets` (violating counts; each bucket
/// reports a breakdown totalling its violating count unless `no_breakdown`),
/// an API key inventory, and the other products' sources answering plainly.
struct Script {
    buckets: Vec<u32>,
    no_breakdown: bool,
    chart_status: u16,
    keys: Vec<Value>,
}

impl Script {
    fn route(&mut self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        if path == "/oauth2/access_token" {
            return raw(
                200,
                &json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        if path == "/aisec/v1/mgmt/dashboard/v2/sessions/sessionschart" {
            let buckets: Vec<Value> = self
                .buckets
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let mut b = json!({"time": format!("2026-10-19T{:02}:00:00Z", i), "total": 10 + v, "violating": v});
                    if !self.no_breakdown {
                        b["detection_type_violation_breakdown"] =
                            json!([{"detection_type": "pi", "violation_breakdown": {"total": v}}]);
                    }
                    b
                })
                .collect();
            return raw(
                self.chart_status,
                &json!({"bucket_size_seconds": 3600, "buckets": buckets}),
            );
        }
        if path == format!("/aisec/v1/mgmt/apikeys/tsg/{TSG}") {
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
            return raw(200, &json!({"api_keys": keys}));
        }
        if path == "/ai-red-teaming/data-plane/v1/dashboard/scan-statistics" {
            return raw(
                200,
                &json!({"total_scans": 3, "targets_scanned": 2,
                    "scan_status": [{"name": "COMPLETED", "count": 3}],
                    "risk_profile": [{"risk_rating": "LOW", "total": 2}],
                    "targets_scanned_by_type": [{"name": "APPLICATION", "count": 2}]}),
            );
        }
        if path == "/ai-red-teaming/data-plane/v1/scan" {
            return raw(
                200,
                &json!({"data": [{"uuid": "00000001-2222-4333-8444-555555555555", "status": "COMPLETED",
                    "job_type": "STATIC", "target_id": "66666666-7777-4888-8999-aaaaaaaaaaaa",
                    "target_type": "APPLICATION", "created_at": "2026-10-18T10:00:00Z"}],
                    "pagination": {"total_items": 1}}),
            );
        }
        if path == "/aims/data/v1/scans" {
            return raw(
                200,
                &json!({"scans": [{"uuid": "00000001-3333-4333-8444-555555555555", "eval_outcome": "ALLOWED",
                    "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 1}}),
            );
        }
        if path == "/aiag/data/v1/stats/scans" {
            return raw(
                200,
                &json!({"unique_skills_scanned": {"count": 1, "percent_change": null},
                    "total_vulnerabilities_found": {"count": 0, "percent_change": null},
                    "top_vulnerability": null}),
            );
        }
        if path == "/aiag/data/v1/scans" {
            return raw(
                200,
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

struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    _adapters: Handles,
    script: Arc<Mutex<Script>>,
    _server: TlsServer,
}

async fn desk(buckets: Vec<u32>) -> Desk {
    let ca = TestCa::new("airs-v2 c36 ca");
    let script = Arc::new(Mutex::new(Script {
        buckets,
        no_breakdown: false,
        chart_status: 200,
        keys: vec![],
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
    };
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Northwind Health", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    let id = desk.snap().tenants[0].id.as_str().to_owned();
    desk.send(json!({"kind": "selectTenant", "tenant": id}))
        .await
        .expect("selected");
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("management key");
    assert!(
        eventually(|| desk
            .snap()
            .tenants
            .iter()
            .any(|t| t.management.state == ScanState::Ready))
        .await
    );
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
async fn the_runtime_card_draws_the_summary_and_the_chart_of_one_read() {
    let mut desk = desk(vec![0, 2, 1, 4]).await;
    let snap = desk.read().await;
    let read = snap.overview.read.expect("a read");
    // The figure: detection events summed from every bucket's usable breakdown.
    assert_eq!(read.summary.detection_events, Some(7));
    assert_eq!(read.summary.violating, Some(7));
    assert_eq!(read.summary.sessions, Some(47));
    // The bars: the buckets as the service reported them, in order.
    let chart = read.chart.expect("chart");
    let violating: Vec<u32> = chart.buckets.iter().map(|b| b.violating).collect();
    assert_eq!(violating, [0, 2, 1, 4]);
    assert_eq!(chart.bucket_seconds, 3600);
    // No Gateway workspace is selected: the Gateway card has no day to draw.
    assert!(read.gateway.is_none());
    // The pill follows the product's read state.
    let runtime = snap
        .inbox
        .read
        .products
        .iter()
        .find(|p| p.product == ProductId::RuntimeSecurity)
        .expect("runtime read");
    assert_eq!(runtime.state, ProductReadState::Ok);
    assert_eq!(snap.inbox.read.state, ReadState::Done);
    assert!(snap.inbox.read.last_read_at.is_some());
    // Without usable breakdowns the figure is unknown, never zero (the card shows an em dash).
    desk.script().no_breakdown = true;
    let snap = desk.read().await;
    let read = snap.overview.read.expect("a read");
    assert_eq!(read.summary.detection_events, None);
    assert_eq!(read.summary.violating, Some(7));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_attention_list_is_the_needs_action_entries_minus_the_operators_holds() {
    let mut desk = desk(vec![3]).await;
    let snap = desk.read().await;
    // Violating sessions make the activity entry something to act on.
    let needing: Vec<&str> = snap
        .inbox
        .items
        .iter()
        .filter(|i| i.needs_action && i.hold.is_none())
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(needing, ["runtime-activity"]);
    assert_eq!(snap.overview.attention.len(), 1);
    // Acknowledged from the card list: held, so Home's count drops; the entry itself stays.
    let context = desk.context();
    desk.send(json!({"kind": "acknowledgeItem", "context": context, "id": "runtime-activity"}))
        .await
        .expect("acknowledged");
    let snap = desk.snap();
    let entry = snap
        .inbox
        .items
        .iter()
        .find(|i| i.id == "runtime-activity")
        .expect("entry kept");
    assert!(entry.hold.is_some());
    assert!(
        snap.inbox
            .items
            .iter()
            .all(|i| !(i.needs_action && i.hold.is_none()))
    );
    // A quiet chart leaves nothing to act on: the empty state names the read.
    desk.script().buckets = vec![0, 0];
    let snap = desk.read().await;
    assert!(
        snap.inbox
            .items
            .iter()
            .all(|i| !(i.needs_action && i.hold.is_none()))
    );
    assert_eq!(snap.inbox.read.state, ReadState::Done);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_recheck_is_admitted_only_for_a_product_the_read_lists_and_a_failed_source_is_partial() {
    let mut desk = desk(vec![1]).await;
    let snap = desk.read().await;
    let listed: Vec<ProductId> = snap.inbox.read.products.iter().map(|p| p.product).collect();
    assert!(listed.contains(&ProductId::RuntimeSecurity));
    // A failed chart: the product is partial and its card offers a recheck, which names it until the read lands.
    desk.script().chart_status = 503;
    let snap = desk.read().await;
    let runtime = snap
        .inbox
        .read
        .products
        .iter()
        .find(|p| p.product == ProductId::RuntimeSecurity)
        .expect("runtime read");
    assert_eq!(runtime.state, ProductReadState::Incomplete);
    assert!(
        snap.inbox
            .read
            .sources_missing
            .contains(&"sessions chart".to_owned())
    );
    desk.script().chart_status = 200;
    let context = desk.context();
    let before = desk.snap().inbox.read.generation;
    desk.send(json!({"kind": "recheckProduct", "context": context, "product": "runtimeSecurity"}))
        .await
        .expect("recheck admitted");
    assert_eq!(
        desk.snap().inbox.read.rechecking,
        Some(ProductId::RuntimeSecurity)
    );
    assert!(eventually(|| desk.snap().inbox.read.generation != before).await);
    let snap = desk.snap();
    let runtime = snap
        .inbox
        .read
        .products
        .iter()
        .find(|p| p.product == ProductId::RuntimeSecurity)
        .expect("runtime read");
    assert_eq!(runtime.state, ProductReadState::Ok);
    assert_eq!(snap.inbox.read.rechecking, None);
    // A product the read does not list is not a card that can be rechecked: refused, no request.
    for product in listed_absent(&snap) {
        let outcome = desk
            .send(json!({"kind": "recheckProduct", "context": context, "product": product}))
            .await;
        assert_eq!(outcome, Err(Refusal::InvalidInput), "{product}");
    }
    // The gated products have no read state at all: they are not even a product id.
    assert!(
        serde_json::from_value::<Intent>(
            json!({"kind": "recheckProduct", "context": context, "product": "agenticIdentity"})
        )
        .is_err()
    );
}

/// The admitted products the read does not list for this tenant.
fn listed_absent(snap: &ShellSnapshot) -> Vec<&'static str> {
    let listed: Vec<ProductId> = snap.inbox.read.products.iter().map(|p| p.product).collect();
    [
        (ProductId::RuntimeSecurity, "runtimeSecurity"),
        (ProductId::Gateway, "gateway"),
        (ProductId::RedTeam, "redTeam"),
        (ProductId::ModelSkill, "modelSkill"),
    ]
    .into_iter()
    .filter(|(id, _)| !listed.contains(id))
    .map(|(_, name)| name)
    .collect()
}
