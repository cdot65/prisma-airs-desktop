//! C39a (D059): the Administration contract the ported board draws from,
//! through the production authority interface against a synthetic
//! management plane: the inventory's six products with Runtime Security's
//! tabs in the board's order and the agentic products unavailable, the
//! Runtime read the Overview tiles and bars come from (summary and buckets),
//! the per-product read state the cards' pills follow, and a management page
//! read of custom topics (the table's rows: name, id, complete) through the
//! same `management` intent. Nothing here is a new core capability.

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
const CLIENT_SECRET: &str = "synthetic-client-secret-c39";
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
        if path == format!("/aisec/v1/mgmt/topics/tsg/{TSG}") {
            return raw(
                200,
                &json!({"custom_topics": [
                    {"topic_id": "11111111-1111-4111-8111-111111111111", "topic_name": "competitor-pricing",
                     "description": "Questions about competitor pricing", "revision": 3},
                    {"topic_id": "22222222-2222-4222-8222-222222222222", "topic_name": "medical-advice",
                     "description": "Requests for diagnosis or treatment", "revision": 5}]}),
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
    #[allow(dead_code)]
    script: Arc<Mutex<Script>>,
    _server: TlsServer,
}

async fn desk(buckets: Vec<u32>) -> Desk {
    let ca = TestCa::new("airs-v2 c39 ca");
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

    #[allow(dead_code)]
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
async fn the_inventory_lists_the_products_and_runtime_tabs_the_board_draws() {
    let mut desk = desk(vec![0, 1]).await;
    // The inventory is the v2 shell's (the legacy shell carries none).

    let snap = desk.read().await;
    let inventory = snap.administration.expect("inventory");
    let ids: Vec<String> = inventory
        .products
        .iter()
        .map(|p| {
            serde_json::to_value(p.id)
                .expect("id")
                .as_str()
                .expect("str")
                .to_owned()
        })
        .collect();
    assert_eq!(
        ids,
        [
            "runtimeSecurity",
            "gateway",
            "redTeam",
            "modelSkill",
            "agenticIdentity",
            "agenticEndpoint"
        ]
    );
    let runtime = &inventory.products[0];
    let tabs: Vec<&str> = runtime.tabs.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(
        tabs,
        [
            "Overview",
            "Playground",
            "Security profiles",
            "Custom topics",
            "Applications",
            "API keys",
            "DLP",
            "Deployment profiles"
        ]
    );
    assert!(!runtime.read_only);
    for p in &inventory.products[4..] {
        assert!(
            matches!(
                serde_json::to_value(&p.availability).expect("availability")["state"].as_str(),
                Some("unavailable")
            ),
            "{:?}",
            p.label
        );
    }
    // Read-only products say so to the card (the header's label).
    assert!(inventory.products[2].read_only && inventory.products[3].read_only);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_overview_tiles_and_bars_come_from_one_read_and_the_pill_from_its_state() {
    let mut desk = desk(vec![0, 2, 1, 4, 0, 0]).await;
    let snap = desk.read().await;
    let read = snap.overview.read.expect("a read");
    assert_eq!(read.summary.detection_events, Some(7));
    assert_eq!(read.summary.violating, Some(7));
    assert_eq!(read.summary.sessions, Some(67));
    let chart = read.chart.expect("chart");
    let violating: Vec<u32> = chart.buckets.iter().map(|b| b.violating).collect();
    assert_eq!(violating, [0, 2, 1, 4, 0, 0]);
    assert!(read.keys_complete && read.keys_failure.is_none());
    let runtime = snap
        .inbox
        .read
        .products
        .iter()
        .find(|p| p.product == ProductId::RuntimeSecurity)
        .expect("runtime read");
    assert_eq!(runtime.state, ProductReadState::Ok);
    assert_eq!(snap.inbox.read.state, ReadState::Done);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_page_read_of_custom_topics_is_the_table_rows() {
    let mut desk = desk(vec![0]).await;
    let context = desk.context();
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = desk
        .send(json!({"kind": "management", "context": context,
            "request": {"kind": "page", "resource": "topics", "offset": 0, "limit": 25}}))
        .await
        .expect("read admitted")
    else {
        panic!("no operation");
    };
    let Ok(Observation::Management(handle)) = desk.handle.observe(ViewScope::Management(operation))
    else {
        panic!("management observation");
    };
    let result = tokio::time::timeout(Duration::from_secs(20), handle.result())
        .await
        .expect("read settles");
    let result = serde_json::to_value(result).expect("result");
    assert_eq!(result["kind"], "page");
    let page = &result["page"];
    assert_eq!(page["resource"], "topics");
    assert_eq!(page["count"], 2);
    assert_eq!(page["complete"], true);
    let items: Value = serde_json::from_str(page["items"].as_str().expect("items")).expect("json");
    let names: Vec<&str> = items
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|i| i["topic_name"].as_str())
        .collect();
    assert_eq!(names, ["competitor-pricing", "medical-advice"]);
    assert_eq!(items[0]["topic_id"], "11111111-1111-4111-8111-111111111111");
    assert_eq!(items[0]["revision"], 3);
}
