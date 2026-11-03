//! C37 (D059): the Inbox contract the ported list and detail draw from,
//! through the production authority interface against a synthetic
//! management plane: entries newest first with the facts the detail shows,
//! the tabs' membership and counts (held entries never "needs action"), the
//! footer's coverage line from the read status, and the C30 holds unchanged:
//! snooze returns at the core's time, acknowledge marks, unsnooze restores.
//! Nothing here is a new core capability.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::ManagementEndpoints;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, HoldKind, Intent, Observation, ReadState, Receipt,
    Refusal, ScanState, ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c37";
const TSG: &str = "1234567890";

/// The synthetic plane: a chart with `violating` sessions, an API key
/// inventory (an expiring key makes a needs-action entry), the other
/// products answering plainly; `keys_status` 403 makes the key source need a role.
struct Script {
    violating: u32,
    keys: Vec<Value>,
    keys_status: u16,
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
            return raw(
                200,
                &json!({"bucket_size_seconds": 3600, "buckets": [
                    {"time": "2026-10-19T00:00:00Z", "total": 10, "violating": self.violating,
                     "detection_type_violation_breakdown": [{"detection_type": "pi", "violation_breakdown": {"total": self.violating}}]}]}),
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
            return raw(self.keys_status, &json!({"api_keys": keys}));
        }
        if path == "/ai-red-teaming/data-plane/v1/dashboard/scan-statistics" {
            return raw(
                200,
                &json!({"total_scans": 3, "targets_scanned": 2, "scan_status": [{"name": "COMPLETED", "count": 3}], "risk_profile": [{"risk_rating": "LOW", "total": 2}], "targets_scanned_by_type": [{"name": "APPLICATION", "count": 2}]}),
            );
        }
        if path == "/ai-red-teaming/data-plane/v1/scan" {
            return raw(
                200,
                &json!({"data": [{"uuid": "00000001-2222-4333-8444-555555555555", "status": "COMPLETED", "job_type": "STATIC", "target_id": "66666666-7777-4888-8999-aaaaaaaaaaaa", "target_type": "APPLICATION", "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 1}}),
            );
        }
        if path == "/aims/data/v1/scans" {
            return raw(
                200,
                &json!({"scans": [{"uuid": "00000001-3333-4333-8444-555555555555", "eval_outcome": "ALLOWED", "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 1}}),
            );
        }
        if path == "/aiag/data/v1/stats/scans" {
            return raw(
                200,
                &json!({"unique_skills_scanned": {"count": 1, "percent_change": null}, "total_vulnerabilities_found": {"count": 0, "percent_change": null}, "top_vulnerability": null}),
            );
        }
        if path == "/aiag/data/v1/scans" {
            return raw(
                200,
                &json!({"scans": [{"uuid": "00000001-4444-4333-8444-555555555555", "name": "AG-PRIVATE-SKILL", "status": "COMPLETED", "eval_outcome": "ALLOWED", "artifact_type": "skill", "summary": {"vulnerability_count": 0, "attack_chain_count": 0}, "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 1}}),
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
}

async fn desk(violating: u32, keys: Vec<Value>) -> Desk {
    let ca = TestCa::new("airs-v2 c37 ca");
    let script = Arc::new(Mutex::new(Script {
        violating,
        keys,
        keys_status: 200,
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
    desk.send(json!({"kind": "createTenant", "profile": {"label": "Northwind Health", "tsgId": TSG, "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("tenant");
    let id = desk.snap().tenants[0].id.as_str().to_owned();
    desk.send(json!({"kind": "selectTenant", "tenant": id}))
        .await
        .expect("selected");
    let context = desk.context();
    desk.send(json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain", "clientId": CLIENT_ID, "secret": CLIENT_SECRET}))
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
async fn the_list_is_newest_first_with_the_facts_the_detail_shows_and_the_footer_names_the_coverage()
 {
    let expiring = desktop_core::sessions::rfc3339(now() + 5 * 86_400 + 60);
    let mut desk = desk(3, vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring, "api_key": "SYNTHETIC-KEY-NEVER-SHOWN"})]).await;
    let snap = desk.read().await;
    let items = &snap.inbox.items;
    assert_eq!(items.len(), 2);
    // Newest first: the activity entry (observed now) precedes the key's expiry (dated by the key).
    assert!(
        items.windows(2).all(|w| w[0].when >= w[1].when),
        "newest first"
    );
    // Every entry carries what the detail card draws: a title, a body, a source and facts; the key is never there.
    for e in items {
        assert!(!e.title.is_empty() && !e.detail.is_empty() && !e.source.is_empty());
        assert!(!e.facts.is_empty(), "{} has facts", e.id);
    }
    assert!(
        !serde_json::to_string(&snap)
            .expect("json")
            .contains("SYNTHETIC-KEY-NEVER-SHOWN")
    );
    // The footer: all sources read at the read's time.
    let read = &snap.inbox.read;
    assert_eq!(read.state, ReadState::Done);
    assert!(read.sources_missing.is_empty());
    let (r, t) = read.products.iter().fold((0, 0), |(r, t), p| {
        (r + p.sources_read, t + p.sources_total)
    });
    assert_eq!((r, t), (2, 2));
    // A refused source: the footer counts the read sources and names the missing one.
    desk.script().keys_status = 403;
    let snap = desk.read().await;
    let read = &snap.inbox.read;
    assert!(read.sources_missing.contains(&"API keys".to_owned()));
    let (r, t) = read.products.iter().fold((0, 0), |(r, t), p| {
        (r + p.sources_read, t + p.sources_total)
    });
    assert_eq!((r, t), (1, 2));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tabs_count_needs_action_updates_and_snoozed_and_the_holds_are_the_c30_intents() {
    let mut desk = desk(3, vec![]).await;
    let snap = desk.read().await;
    let needing = |s: &ShellSnapshot| {
        s.inbox
            .items
            .iter()
            .filter(|i| i.needs_action && i.hold.is_none())
            .count()
    };
    let snoozed = |s: &ShellSnapshot| {
        s.inbox
            .items
            .iter()
            .filter(|i| i.hold.as_ref().is_some_and(|h| h.kind == HoldKind::Snooze))
            .count()
    };
    assert_eq!(
        (snap.inbox.items.len(), needing(&snap), snoozed(&snap)),
        (1, 1, 0)
    );
    let id = snap.inbox.items[0].id.clone();
    let context = desk.context();
    // Snooze: the core computes the return time the row, the detail and the toast all state.
    desk.send(json!({"kind": "snoozeItem", "context": context, "id": id}))
        .await
        .expect("snoozed");
    let snap = desk.snap();
    let hold = snap.inbox.items[0].hold.as_ref().expect("held");
    assert_eq!(hold.kind, HoldKind::Snooze);
    let returns = u64::from(hold.returns_at.expect("return time"));
    assert!(returns > now() + 23 * 3600 && returns <= now() + 24 * 3600 + 5);
    assert_eq!(
        snap.inbox
            .last_snooze
            .as_ref()
            .map(|s| (s.id.as_str(), u64::from(s.returns_at))),
        Some((id.as_str(), returns))
    );
    assert_eq!((needing(&snap), snoozed(&snap)), (0, 1));
    // Unsnooze restores the entry to Needs action.
    desk.send(json!({"kind": "unsnoozeItem", "context": context, "id": id}))
        .await
        .expect("unsnoozed");
    let snap = desk.snap();
    assert_eq!((needing(&snap), snoozed(&snap)), (1, 0));
    // Acknowledge marks it: an update, listed, never needing action until unmarked.
    desk.send(json!({"kind": "acknowledgeItem", "context": context, "id": id}))
        .await
        .expect("acknowledged");
    let snap = desk.snap();
    assert_eq!(
        snap.inbox.items[0].hold.as_ref().map(|h| h.kind),
        Some(HoldKind::Acknowledge)
    );
    assert_eq!((snap.inbox.items.len(), needing(&snap)), (1, 0));
    desk.send(json!({"kind": "unsnoozeItem", "context": context, "id": id}))
        .await
        .expect("unmarked");
    assert_eq!(needing(&desk.snap()), 1);
    // A quiet reread leaves nothing to act on: the empty state names the read, never sample data.
    desk.script().violating = 0;
    let snap = desk.read().await;
    assert_eq!(needing(&snap), 0);
    assert_eq!(snap.inbox.read.state, ReadState::Done);
}
