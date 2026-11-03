//! C31 (D058): reviews, the Administration inventory and Runtime API key
//! rotation through the production authority interface against a synthetic
//! management plane. The inventory is pinned to the change matrices (V2-T20,
//! V2-R11, V2-R10, V2-T19); a review binds tenant, resource and typed name
//! (V2-R03); a held one-time secret is dropped on every review exit including
//! a tenant switch (V2-R06); rotation is the admitted sequence with its
//! reminder, its named failure cases and its two timestamps (V2-R07, T07,
//! T08). The plane, desk and helpers mirror `runtime_changes`.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::changes::{ChangeAction, ChangeKind, ChangeProblem};
use desktop_core::fixtures::{self, Handles};
use desktop_core::management::ManagementEndpoints;
use desktop_core::runtime::Outcome;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, ChangeFailure, ChangeResultView, ChangeReviewState, ChangeReviewView, Core,
    CoreConfig, Delivery, InboxKind, Intent, Observation, Readback, Receipt, Refusal, ScanState,
    SecretState, ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c09";
const TSG: &str = "1234567890";
const TOPIC_ID: &str = "11111111-1111-4111-8111-111111111111";
const PROFILE_ID: &str = "22222222-2222-4222-8222-222222222222";
const ONE_TIME: &str = "ONE-TIME-SECRET-";
const AUTH_CODE: &str = "SYNTHETIC-DP-AUTH-CODE";

/// A stateful synthetic AIRS management service.
struct Service {
    issued: u32,
    profiles: Vec<Value>,
    topics: Vec<Value>,
    apps: Vec<Value>,
    keys: Vec<Value>,
    serial: u32,
    /// (method, path prefix) → reply, used once each.
    overrides: Vec<(String, String, Reply)>,
    /// Once a write is answered, reads are held this long (a readback the
    /// operator may stop).
    hold_reads_after_write: Option<u64>,
    wrote: bool,
}

impl Service {
    fn new() -> Self {
        Self {
            issued: 0,
            profiles: vec![
                json!({"profile_id": PROFILE_ID, "profile_name": "Production guardrails",
                                  "revision": 3, "active": true, "policy": {}, "last_modified_ts": "2026-10-01"}),
            ],
            topics: vec![
                json!({"topic_id": TOPIC_ID, "topic_name": "Finance", "revision": 1,
                                "description": "Money talk", "examples": ["wire transfer"]}),
            ],
            apps: vec![
                json!({"customer_appId": "app-1", "app_name": "Support bot", "cloud_provider": "aws",
                              "environment": "production", "tsg_id": TSG,
                              "api_keys_dp_info": [{"api_key_name": "support", "auth_code": "OTHER-CODE",
                                                    "dp_name": "dp-2"}]}),
            ],
            keys: vec![
                json!({"api_key_id": "key-1", "api_key_name": "Support key", "cust_app": "Support bot",
                              "api_key_last8": "abcd1234", "expiration": "2027-01-01T00:00:00Z",
                              "revoked": false, "auth_code": "STORED-CODE"}),
            ],
            serial: 0,
            overrides: Vec::new(),
            hold_reads_after_write: None,
            wrote: false,
        }
    }

    fn route(&mut self, seen: &Seen) -> Reply {
        let mut parts = seen.request_line.split(' ');
        let method = parts.next().unwrap_or("").to_owned();
        let target = parts.next().unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        if let Some(i) = self
            .overrides
            .iter()
            .position(|(m, p, _)| *m == method && path.starts_with(p.as_str()))
        {
            return self.overrides.remove(i).2;
        }
        let query = |name: &str| {
            target.split_once('?').and_then(|(_, q)| {
                q.split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.replace("%40", "@").replace('+', " ").replace("%20", " "))
            })
        };
        let body: Value = serde_json::from_slice(&seen.body).unwrap_or(Value::Null);
        let list = |items: &[Value], key: &str| {
            let offset: usize = query("offset").and_then(|v| v.parse().ok()).unwrap_or(0);
            let limit: usize = query("limit").and_then(|v| v.parse().ok()).unwrap_or(100);
            let end = (offset + limit).min(items.len());
            raw(200, &json!({ key: items[offset.min(end)..end].to_vec() }))
        };
        let tsg = format!("/tsg/{TSG}");
        match (method.as_str(), path.as_str()) {
            ("POST", "/oauth2/access_token") => {
                self.issued += 1;
                raw(
                    200,
                    &json!({"access_token": format!("c09-access-{}", self.issued),
                                 "token_type": "Bearer", "expires_in": 3600}),
                )
            }
            ("GET", p) if p == format!("/aisec/v1/mgmt/profiles{tsg}") => {
                list(&self.profiles, "ai_profiles")
            }
            ("GET", p) if p == format!("/aisec/v1/mgmt/topics{tsg}") => {
                list(&self.topics, "custom_topics")
            }
            ("GET", p) if p == format!("/aisec/v1/mgmt/customerapp{tsg}") => {
                list(&self.apps, "customer_apps")
            }
            ("GET", p) if p == format!("/aisec/v1/mgmt/apikeys{tsg}") => {
                list(&self.keys, "api_keys")
            }
            ("GET", "/aisec/v1/mgmt/deploymentprofiles") => raw(
                200,
                &json!({"status": "success",
                "deployment_profiles": [{"dp_name": "dp-1", "auth_code": AUTH_CODE, "status": "active"},
                                        {"dp_name": "dp-2", "auth_code": "OTHER-CODE", "status": "active"}]}),
            ),
            ("POST", "/aisec/v1/mgmt/topic") => {
                self.serial += 1;
                let mut topic = body.clone();
                topic["topic_id"] = json!(format!("33333333-3333-4333-8333-{:012}", self.serial));
                topic["revision"] = json!(1);
                if topic.get("description").is_none() {
                    topic["description"] = json!("");
                }
                self.topics.push(topic.clone());
                raw(200, &topic)
            }
            ("PUT", p) if p.starts_with("/aisec/v1/mgmt/topic/uuid/") => {
                let id = p.rsplit('/').next().unwrap_or("");
                let Some(topic) = self.topics.iter_mut().find(|t| t["topic_id"] == id) else {
                    return raw(404, &json!({"error": "not found"}));
                };
                let revision = topic["revision"].as_i64().unwrap_or(0) + 1;
                *topic = body.clone();
                topic["topic_id"] = json!(id);
                topic["revision"] = json!(revision);
                if topic.get("description").is_none() {
                    topic["description"] = json!("");
                }
                raw(200, &topic.clone())
            }
            ("DELETE", p) if p.starts_with("/aisec/v1/mgmt/topic/") => {
                let id = p.rsplit('/').next().unwrap_or("").to_owned();
                self.topics.retain(|t| t["topic_id"] != id.as_str());
                raw(200, &json!({"message": "deleted"}))
            }
            ("POST", "/aisec/v1/mgmt/profile") => {
                self.serial += 1;
                let mut profile = body.clone();
                profile["profile_id"] =
                    json!(format!("44444444-4444-4444-8444-{:012}", self.serial));
                profile["revision"] = json!(1);
                self.profiles.push(profile.clone());
                raw(200, &profile)
            }
            ("DELETE", p) if p.starts_with("/aisec/v1/mgmt/profile/") => {
                let id = p.rsplit('/').next().unwrap_or("").to_owned();
                self.profiles.retain(|t| t["profile_id"] != id.as_str());
                raw(200, &json!({"message": "deleted"}))
            }
            ("PUT", p) if p.starts_with("/aisec/v1/mgmt/profile/uuid/") => {
                let id = p.rsplit('/').next().unwrap_or("");
                let Some(profile) = self.profiles.iter_mut().find(|t| t["profile_id"] == id) else {
                    return raw(404, &json!({"error": "not found"}));
                };
                let revision = profile["revision"].as_i64().unwrap_or(0) + 1;
                *profile = body.clone();
                profile["profile_id"] = json!(id);
                profile["revision"] = json!(revision);
                raw(200, &profile.clone())
            }
            ("PUT", "/aisec/v1/mgmt/customerapp") => {
                let id = query("customer_app_id").unwrap_or_default();
                let Some(app) = self
                    .apps
                    .iter_mut()
                    .find(|a| a["customer_appId"] == id.as_str())
                else {
                    return raw(404, &json!({"error": "not found"}));
                };
                // As the live service (C25): an update without the
                // application's deployment auth code is refused.
                if body.get("auth_code").and_then(Value::as_str).is_none() {
                    return raw(400, &json!("auth_code is required"));
                }
                // The service keeps its own fields (tsg_id, key list) on update.
                let tsg = app.get("tsg_id").cloned();
                let infos = app.get("api_keys_dp_info").cloned();
                *app = body.clone();
                app["customer_appId"] = json!(id);
                if let Some(tsg) = tsg {
                    app["tsg_id"] = tsg;
                }
                if let Some(infos) = infos {
                    app["api_keys_dp_info"] = infos;
                }
                if let Some(o) = app.as_object_mut() {
                    o.remove("auth_code");
                }
                raw(200, &app.clone())
            }
            ("DELETE", "/aisec/v1/mgmt/customerapp") => {
                let name = query("app_name").unwrap_or_default();
                self.apps.retain(|a| a["app_name"] != name.as_str());
                self.keys.retain(|k| k["cust_app"] != name.as_str());
                raw(200, &json!({"message": "deleted"}))
            }
            ("POST", "/aisec/v1/mgmt/apikey") => {
                self.serial += 1;
                let secret = format!("{ONE_TIME}{}", self.serial);
                let key = json!({"api_key_id": format!("key-{}", self.serial + 1), "api_key_name": body["api_key_name"],
                                 "cust_app": body["cust_app"], "api_key_last8": &secret[secret.len() - 8..],
                                 "expiration": "2027-06-01T00:00:00Z", "revoked": false,
                                 "auth_code": body["auth_code"]});
                if !self.apps.iter().any(|a| a["app_name"] == body["cust_app"]) {
                    self.apps.push(json!({"customer_appId": format!("app-{}", self.serial + 1),
                                          "app_name": body["cust_app"], "cloud_provider": body["cust_cloud_provider"],
                                          "environment": body["cust_env"],
                                          "api_keys_dp_info": [{"api_key_name": body["api_key_name"],
                                                                "auth_code": body["auth_code"],
                                                                "dp_name": body["dp_name"]}]}));
                }
                self.keys.push(key.clone());
                let mut reply = key;
                reply["api_key"] = json!(secret);
                raw(200, &reply)
            }
            ("POST", p) if p.starts_with("/aisec/v1/mgmt/apikey/regenerate/") => {
                let id = p.rsplit('/').next().unwrap_or("");
                self.serial += 1;
                let secret = format!("{ONE_TIME}{}", self.serial);
                let Some(key) = self.keys.iter_mut().find(|k| k["api_key_id"] == id) else {
                    return raw(404, &json!({"error": "not found"}));
                };
                key["api_key_last8"] = json!(&secret[secret.len() - 8..]);
                let mut reply = key.clone();
                reply["api_key"] = json!(secret);
                raw(200, &reply)
            }
            ("DELETE", p) if p.starts_with("/aisec/v1/mgmt/apikey/delete/") => {
                let name = p.rsplit('/').next().unwrap_or("").replace("%20", " ");
                self.keys.retain(|k| k["api_key_name"] != name.as_str());
                raw(200, &json!({"message": "deleted"}))
            }
            _ => raw(404, &json!({"error": "not found"})),
        }
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
    adapters: Handles,
    service: Arc<Mutex<Service>>,
    server: TlsServer,
    _ca: TestCa,
    /// Applied changes already returned by [`Desk::settled`].
    results_seen: usize,
}

async fn desk() -> Desk {
    desk_with(|_| {}).await
}

async fn desk_with(configure: impl FnOnce(&mut CoreConfig)) -> Desk {
    let ca = TestCa::new("airs-v2 c09 ca");
    let service = Arc::new(Mutex::new(Service::new()));
    let routed = Arc::clone(&service);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            let mut service = routed.lock().expect("service");
            let reply = service.route(seen);
            let read = seen.request_line.starts_with("GET ");
            match service.hold_reads_after_write {
                Some(millis) if read && service.wrote => Reply::Delayed {
                    millis,
                    then: Box::new(reply),
                },
                _ => {
                    service.wrote |= !read && !seen.request_line.contains("/oauth2/");
                    reply
                }
            }
        })),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = fixtures::adapters();
    let mut config = CoreConfig {
        management: ManagementEndpoints::at_origin(&origin),
        ..CoreConfig::default()
    };
    configure(&mut config);
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
        adapters: handles,
        service,
        server,
        _ca: ca,
        results_seen: 0,
    };
    for label in ["Tenant A", "Tenant B"] {
        desk.send(
            json!({"kind": "createTenant", "profile": {"label": label, "tsgId": TSG,
            "destination": {"kind": "custom", "origin": origin}}}),
        )
        .await
        .expect("tenant");
    }
    desk.select("Tenant A").await;
    desk.management_key().await;
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

    async fn select(&mut self, label: &str) {
        let id = self
            .snap()
            .tenants
            .iter()
            .find(|t| t.label == label)
            .expect("tenant")
            .id
            .clone();
        self.send(json!({"kind": "selectTenant", "tenant": id}))
            .await
            .expect("selected");
    }

    async fn management_key(&mut self) {
        let context = self.context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "session",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        assert!(
            eventually(|| self
                .snap()
                .tenants
                .iter()
                .find(|t| Some(&t.id) == self.snap().active.as_ref().map(|a| &a.tenant))
                .is_some_and(|t| t.management.state == ScanState::Ready))
            .await
        );
    }

    /// ReviewChange, waiting until the review is ready or failed.
    async fn review(&mut self, draft: &Value) -> ChangeReviewView {
        let context = self.context();
        self.send(json!({"kind": "reviewChange", "context": context, "draft": draft}))
            .await
            .expect("review admitted");
        assert!(
            eventually(|| self
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

    /// The next applied change once it settled.
    async fn settled(&mut self) -> ChangeResultView {
        let seen = self.results_seen;
        assert!(
            eventually(|| {
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
}

fn topic_update(description: &str) -> Value {
    json!({"kind": "topics", "action": "update", "target": TOPIC_ID,
           "body": json!({"topic_name": "Finance", "description": description, "examples": ["wire transfer"],
                          "revision": 1}).to_string()})
}

#[allow(dead_code)]
fn key_create(app: &str, name: &str) -> Value {
    json!({"kind": "apiKeys", "action": "create", "target": "",
           "body": json!({"cust_app": app, "cust_env": "production", "cust_cloud_provider": "aws",
                          "api_key_name": name, "dp_name": "dp-1", "revoked": false,
                          "rotation_time_interval": 30, "rotation_time_unit": "days"}).to_string()})
}

#[allow(dead_code)]
fn body(seen: &Seen) -> Value {
    serde_json::from_slice(&seen.body).unwrap_or(Value::Null)
}

fn key_version(source: &str, app: &str, name: &str) -> Value {
    json!({"kind": "apiKeys", "action": "version", "target": source,
           "body": json!({"cust_app": app, "cust_env": "production", "cust_cloud_provider": "aws",
                          "api_key_name": name, "dp_name": "dp-1", "revoked": false,
                          "rotation_time_interval": 30, "rotation_time_unit": "days"}).to_string()})
}

fn key_delete(id: &str) -> Value {
    json!({"kind": "apiKeys", "action": "delete", "target": id, "body": "{}"})
}

fn rotation_of(snap: &ShellSnapshot, app: &str) -> Option<desktop_core::RotationView> {
    snap.rotations
        .iter()
        .find(|r| r.application == app)
        .cloned()
}

fn reminder(snap: &ShellSnapshot, app: &str) -> Option<desktop_core::InboxEntry> {
    snap.inbox
        .items
        .iter()
        .find(|i| i.id == format!("rotation-{app}"))
        .cloned()
}

async fn read_keys(desk: &mut Desk) {
    read_keys_at(desk, 0).await;
}

async fn read_keys_at(desk: &mut Desk, offset: u32) {
    let context = desk.context();
    let before = desk.snap().revision.clone();
    desk.send(json!({"kind": "management", "context": context,
        "request": {"kind": "page", "resource": "apiKeys", "offset": offset, "limit": 100}}))
        .await
        .expect("read admitted");
    assert!(eventually(|| desk.snap().revision != before).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// V2-T20 / V2-R11 / V2-R10 / V2-T19 (core): the Administration inventory
/// the snapshot carries offers only admitted (kind, action) pairs, every
/// denied pair is refused by the review before any read, an unavailable
/// product has no tabs and no action, and a read-only product offers none.
#[tokio::test(flavor = "multi_thread")]
async fn the_inventory_offers_only_admitted_actions_and_the_core_refuses_the_denied_ones() {
    let mut desk = desk().await;
    // C44: one presentation; the inventory is always carried (the switch intent is accepted and ignored).
    desk.send(json!({"kind": "setUiVariant", "variant": "v2"}))
        .await
        .expect("v2");
    let inventory = desk
        .snap()
        .administration
        .expect("the v2 shell's inventory");
    assert_eq!(inventory.products.len(), 6);
    let mut denied = Vec::new();
    for product in &inventory.products {
        if let desktop_core::admin_inventory::AdminAvailability::Unavailable { reason } =
            &product.availability
        {
            assert!(product.tabs.is_empty(), "{:?} has tabs", product.id);
            assert!(reason.contains("Not available in this release"));
            continue;
        }
        for tab in &product.tabs {
            if product.read_only {
                assert!(tab.actions.is_empty());
            }
            for a in &tab.actions {
                assert!(desktop_core::changes::is_allowed(a.kind, a.action));
            }
            for d in &tab.denied {
                assert!(!desktop_core::changes::is_allowed(d.kind, d.action));
                denied.push((d.kind, d.action));
            }
        }
    }
    assert!(denied.contains(&(ChangeKind::ApiKeys, ChangeAction::Update)));
    assert!(denied.contains(&(ChangeKind::Applications, ChangeAction::Create)));
    assert!(denied.contains(&(ChangeKind::GatewayServiceKeys, ChangeAction::Version)));
    // Every denied Runtime pair is refused by the review itself.
    let context = desk.context();
    for (kind, action) in denied.iter().filter(|(k, _)| k.gateway().is_none()) {
        let draft = json!({"kind": kind, "action": action, "target": "key-1", "body": "{}"});
        let outcome = desk
            .send(json!({"kind": "reviewChange", "context": context, "draft": draft}))
            .await;
        match outcome {
            Err(Refusal::InvalidInput) => {}
            Err(other) => panic!("{kind:?} {action:?}: {other:?}"),
            Ok(_) => {
                assert!(
                    eventually(|| desk
                        .snap()
                        .changes
                        .review
                        .is_some_and(|r| r.state == ChangeReviewState::Failed))
                    .await
                );
                let review = desk.snap().changes.review.expect("review");
                assert_eq!(
                    review.failure,
                    Some(ChangeFailure::Problem {
                        problem: ChangeProblem::Unsupported
                    }),
                    "{kind:?} {action:?}"
                );
                desk.send(json!({"kind": "discardChange", "review": review.review}))
                    .await
                    .expect("discarded");
            }
        }
    }
    assert!(desk.writes().is_empty(), "nothing was sent");
}

/// V2-R03 (I3): the typed name must match what the core expects, and a
/// review of one tenant cannot apply after a switch to another.
#[tokio::test(flavor = "multi_thread")]
async fn confirm_review_refuses_a_wrong_name_and_a_changed_tenant() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = topic_update("Renamed");
    let review = desk.review(&draft).await;
    assert_eq!(review.confirmation.as_deref(), Some("Finance"));
    assert_eq!(
        desk.request(&review, &draft, "finance").await.err(),
        Some(Refusal::InvalidInput),
        "the typed name must match exactly"
    );
    assert!(desk.writes().is_empty());
    // Another tenant: the review belongs to the first and is gone.
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Other", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    desk.select("Other").await;
    assert!(desk.snap().changes.review.is_none());
    assert_eq!(
        desk.request(&review, &draft, "Finance").await.err(),
        Some(Refusal::ReviewInvalid)
    );
    assert!(desk.writes().is_empty());
}

/// V2-R07 / V2-T07 / V2-T08 (I7): the admitted sequence. An additional key
/// read back starts the rotation (Issued while its secret is held, Resumed
/// once it is not), the reminder is a distinct Inbox item on Home and in the
/// Inbox, the deployment acknowledgement is a local timestamp stored once,
/// the old key's delete read back absent completes it with the removal
/// time, and both timestamps survive reads, a switch round trip and a
/// relaunch.
#[tokio::test(flavor = "multi_thread")]
async fn a_rotation_is_the_admitted_sequence_with_its_reminder_and_two_timestamps() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    assert_eq!(review.confirmation.as_deref(), Some("CREATE"));
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let issued = desk.settled().await;
    assert_eq!(issued.outcome, Some(Outcome::Completed));
    assert_eq!(issued.secret, SecretState::Held);
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Issued);
    assert_eq!(rotation.old_key.id, "key-1");
    assert_eq!(rotation.old_key.name, "Support key");
    assert_eq!(
        rotation.new_key.as_ref().map(|k| k.name.as_str()),
        Some("Support key 2")
    );
    assert_eq!(
        rotation.issued_by.as_deref(),
        Some(issued.operation.as_str())
    );
    assert_eq!(rotation.deployment_acknowledged_at, None);
    assert_eq!(rotation.old_key_removed_at, None);
    let item = reminder(&snap, "Support bot").expect("reminder");
    assert!(item.needs_action);
    assert_eq!(item.kind, InboxKind::Rotation);
    assert_eq!(item.action_label.as_deref(), Some("Finish rotation…"));
    assert_eq!(
        item.target.as_ref().and_then(|t| t.item.clone()).as_deref(),
        Some("Support key")
    );
    assert!(
        snap.overview
            .attention
            .iter()
            .any(|i| i.id == "rotation-Support bot"),
        "on Home too"
    );
    // The secret cleared: resumed, nothing to reveal.
    desk.send(json!({"kind": "forgetSecret", "operation": issued.operation}))
        .await
        .expect("forgotten");
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Resumed);
    assert_eq!(rotation.issued_by, None);
    // The operator's local statement, timestamped once.
    let context = desk.context();
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged");
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Acknowledged);
    let acknowledged_at = rotation.deployment_acknowledged_at.expect("acknowledged");
    desk._core.advance_clock(120);
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged again");
    assert_eq!(
        rotation_of(&desk.snap(), "Support bot")
            .expect("rotation")
            .deployment_acknowledged_at,
        Some(acknowledged_at),
        "stored once"
    );
    // A read changes nothing of it.
    read_keys(&mut desk).await;
    assert_eq!(
        rotation_of(&desk.snap(), "Support bot")
            .expect("rotation")
            .deployment_acknowledged_at,
        Some(acknowledged_at)
    );
    // Old key removed and read back absent: complete, with the readback time.
    let delete = key_delete("key-1");
    let review = desk.review(&delete).await;
    assert_eq!(review.confirmation.as_deref(), Some("Support key"));
    desk.request(&review, &delete, "Support key")
        .await
        .expect("requested");
    let removed = desk.settled().await;
    assert_eq!(
        (removed.outcome, removed.readback),
        (Some(Outcome::Completed), Some(Readback::Absent))
    );
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Complete);
    let removed_at = rotation.old_key_removed_at.expect("removed at");
    assert!(removed_at >= acknowledged_at + 120);
    assert!(
        reminder(&snap, "Support bot").is_none(),
        "the reminder clears"
    );
    assert!(
        !snap
            .overview
            .attention
            .iter()
            .any(|i| i.id.starts_with("rotation-"))
    );
    // Stable across a switch round trip.
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Other", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    desk.select("Other").await;
    assert!(
        desk.snap().rotations.is_empty(),
        "another tenant's rotation is not shown"
    );
    desk.select("Tenant A").await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(
        (
            rotation.deployment_acknowledged_at,
            rotation.old_key_removed_at
        ),
        (Some(acknowledged_at), Some(removed_at))
    );
    // And a relaunch from the saved state.
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(
        !format!("{saved:?}").contains(ONE_TIME),
        "no secret persisted"
    );
    let (mut adapters, _handles) =
        fixtures::adapters_with(fixtures::ScriptedStore::with_state(saved));
    adapters.keychain = desk.adapters.vault.clone();
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
    let rotation = rotation_of(&snapshot, "Support bot").expect("persisted rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Complete);
    assert_eq!(
        (
            rotation.deployment_acknowledged_at,
            rotation.old_key_removed_at
        ),
        (Some(acknowledged_at), Some(removed_at))
    );
}

/// V2-R07 (a), (b): a failed issuance records nothing and reissues nothing;
/// an unknown issuance is a rotation with no key confirmed and no reminder
/// until the key inventory shows the application's new key.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_or_unknown_issuance_claims_nothing_until_a_key_is_read_back() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    desk.service.lock().expect("service").overrides.push((
        "POST".into(),
        "/aisec/v1/mgmt/apikey".into(),
        raw(403, &json!({"message": "SERVER-TEXT"})),
    ));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let failed = desk.settled().await;
    assert_eq!(failed.outcome, Some(Outcome::Rejected));
    let snap = desk.snap();
    assert!(snap.rotations.is_empty(), "nothing issued, nothing kept");
    assert!(reminder(&snap, "Support bot").is_none());
    assert_eq!(desk.writes().len(), 1, "never reissued");
    // Unknown: the connection closed after the write was sent.
    desk.service.lock().expect("service").overrides.push((
        "POST".into(),
        "/aisec/v1/mgmt/apikey".into(),
        Reply::Close,
    ));
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let unknown = desk.settled().await;
    assert_eq!(unknown.outcome, Some(Outcome::Unknown));
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("unconfirmed rotation");
    assert_eq!(
        rotation.stage,
        desktop_core::RotationStage::IssuanceUnconfirmed
    );
    assert_eq!(rotation.new_key, None);
    assert!(
        reminder(&snap, "Support bot").is_none(),
        "no reminder until a key is read back"
    );
    assert_eq!(desk.writes().len(), 2, "never reissued");
    // Another key of the application says nothing about this issuance.
    desk.service.lock().expect("service").keys.push(json!({
        "api_key_id": "key-8", "api_key_name": "Support key 8", "cust_app": "Support bot",
        "api_key_last8": "yyyy8888", "expiration": "2027-01-01T00:00:00Z", "revoked": false, "auth_code": "STORED-CODE"}));
    read_keys(&mut desk).await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(
        rotation.stage,
        desktop_core::RotationStage::IssuanceUnconfirmed,
        "only the reviewed key name confirms"
    );
    assert!(reminder(&desk.snap(), "Support bot").is_none());
    // Reconciliation: the inventory shows the key the review named.
    desk.service.lock().expect("service").keys.push(json!({
        "api_key_id": "key-9", "api_key_name": "Support key 2", "cust_app": "Support bot",
        "api_key_last8": "zzzz9999", "expiration": "2027-01-01T00:00:00Z", "revoked": false, "auth_code": "STORED-CODE"}));
    read_keys(&mut desk).await;
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(
        rotation.stage,
        desktop_core::RotationStage::Resumed,
        "{rotation:?}"
    );
    assert_eq!(
        rotation.new_key.as_ref().map(|k| k.id.as_str()),
        Some("key-9")
    );
    assert!(reminder(&snap, "Support bot").is_some());
}

/// V2-R07 (c), (d): a rejected removal keeps the reminder, the old key and
/// the stage, with the reason and no completion time; an unknown removal
/// is reconciled by a complete inventory read without the old key.
#[tokio::test(flavor = "multi_thread")]
async fn a_rejected_or_unknown_removal_keeps_the_reminder_until_the_key_is_read_back_gone() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    desk.settled().await;
    let context = desk.context();
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged");
    // Rejected delete.
    desk.service.lock().expect("service").overrides.push((
        "DELETE".into(),
        "/aisec/v1/mgmt/apikey/delete/".into(),
        raw(409, &json!({"message": "SERVER-TEXT"})),
    ));
    let delete = key_delete("key-1");
    let review = desk.review(&delete).await;
    desk.request(&review, &delete, "Support key")
        .await
        .expect("requested");
    let rejected = desk.settled().await;
    assert_eq!(rejected.outcome, Some(Outcome::Rejected));
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(
        rotation.stage,
        desktop_core::RotationStage::Acknowledged,
        "{rotation:?}"
    );
    assert!(rotation.removal_rejected.is_some());
    assert_eq!(rotation.old_key_removed_at, None);
    assert!(reminder(&snap, "Support bot").is_some());
    assert!(
        desk.service
            .lock()
            .expect("service")
            .keys
            .iter()
            .any(|k| k["api_key_id"] == "key-1"),
        "the old key stays"
    );
    // Unknown delete: the connection closed.
    desk.service.lock().expect("service").overrides.push((
        "DELETE".into(),
        "/aisec/v1/mgmt/apikey/delete/".into(),
        Reply::Close,
    ));
    let review = desk.review(&delete).await;
    desk.request(&review, &delete, "Support key")
        .await
        .expect("requested");
    let unknown = desk.settled().await;
    assert_eq!(unknown.outcome, Some(Outcome::Unknown));
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(
        rotation.stage,
        desktop_core::RotationStage::RemovalUnconfirmed
    );
    assert_eq!(rotation.old_key_removed_at, None);
    assert!(
        reminder(&snap, "Support bot").is_some(),
        "retained until reconciled"
    );
    // A final page that is not the first establishes nothing.
    desk.service
        .lock()
        .expect("service")
        .keys
        .retain(|k| k["api_key_id"] != "key-1");
    read_keys_at(&mut desk, 1).await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(
        rotation.stage,
        desktop_core::RotationStage::RemovalUnconfirmed,
        "absence needs the whole inventory in one read"
    );
    // Reconciliation by a whole inventory read without the old key.
    read_keys(&mut desk).await;
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Complete);
    assert!(rotation.old_key_removed_at.is_some());
    assert!(reminder(&snap, "Support bot").is_none());
}

/// V2-R06 (I6): a held one-time key is dropped by a tenant switch (the
/// context retires with its results): the rotation resumes without a secret,
/// Reveal refuses, and coming back revives nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_held_secret_is_dropped_by_a_tenant_switch_and_never_revived() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let issued = desk.settled().await;
    assert_eq!(issued.secret, SecretState::Held);
    assert_eq!(
        rotation_of(&desk.snap(), "Support bot")
            .expect("rotation")
            .stage,
        desktop_core::RotationStage::Issued
    );
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Other", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    desk.select("Other").await;
    assert!(
        desk.snap().changes.results.is_empty(),
        "another context's results are not shown"
    );
    desk.select("Tenant A").await;
    let snap = desk.snap();
    assert!(
        snap.changes.results.is_empty(),
        "the retired context's results are gone"
    );
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Resumed);
    assert_eq!(rotation.issued_by, None);
    assert_eq!(
        desk.send(json!({"kind": "revealSecret", "operation": issued.operation}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    assert!(desk.adapters.secrets.shown().is_empty());
}

/// The one-time key a version returns, as the service sends it.
fn issued_key(id: &str, name: &str, secret: &str) -> Value {
    json!({"api_key_id": id, "api_key_name": name, "cust_app": "Support bot",
           "api_key_last8": &secret[secret.len() - 8..], "expiration": "2027-06-01T00:00:00Z",
           "revoked": false, "auth_code": AUTH_CODE, "api_key": secret})
}

/// V2-R06 (I6): the operator ends the review before the issuance settles;
/// (DiscardChange on the applied review, as every review exit sends it)
/// the key that lands afterwards is dropped on arrival, never held, never
/// offered, and Reveal refuses it.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_that_lands_after_the_review_closed_is_dropped_on_arrival() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let secret = format!("{ONE_TIME}77");
    {
        let mut service = desk.service.lock().expect("service");
        service.overrides.push((
            "POST".into(),
            "/aisec/v1/mgmt/apikey".into(),
            Reply::Delayed {
                millis: 1500,
                then: Box::new(raw(200, &issued_key("key-7", "Support key 2", &secret))),
            },
        ));
    }
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .results
            .first()
            .is_some_and(|r| !r.settled))
        .await,
        "under way"
    );
    assert!(
        eventually(|| !desk.writes().is_empty()).await,
        "the write is on the wire"
    );
    // The service stores the key while its answer is still on the way.
    let mut stored = issued_key("key-7", "Support key 2", &secret);
    stored.as_object_mut().expect("object").remove("api_key");
    desk.service.lock().expect("service").keys.push(stored);
    let operation = desk.snap().changes.results[0].operation.clone();
    // The operator leaves the review (close, navigation or unmount).
    desk.send(json!({"kind": "discardChange", "review": review.review}))
        .await
        .expect("the applied review ends before its change settled");
    let settled = desk.settled().await;
    assert_eq!(settled.outcome, Some(Outcome::Completed));
    assert_eq!(settled.secret, SecretState::Forgotten, "never held");
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Resumed);
    assert_eq!(rotation.issued_by, None);
    assert!(
        desk.send(json!({"kind": "revealSecret", "operation": operation}))
            .await
            .is_err()
    );
    assert!(desk.adapters.secrets.shown().is_empty());
}

/// V2-R06 (I6): acknowledging the deployment forgets the held copy of the
/// new key; nothing is offered for reveal afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn acknowledging_the_deployment_forgets_the_held_key() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let issued = desk.settled().await;
    assert_eq!(issued.secret, SecretState::Held);
    let context = desk.context();
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged");
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Acknowledged);
    assert_eq!(
        rotation.issued_by, None,
        "nothing offered after acknowledgement"
    );
    assert_eq!(snap.changes.results[0].secret, SecretState::Forgotten);
    assert!(
        desk.send(json!({"kind": "revealSecret", "operation": issued.operation}))
            .await
            .is_err()
    );
    assert!(desk.adapters.secrets.shown().is_empty());
}

/// V2-R07 (e): a version that completed but whose readback did not find the
/// key is an unconfirmed issuance, not an empty state: the held key stays
/// revealable, no reminder is raised, and the reviewed key name confirms it.
#[tokio::test(flavor = "multi_thread")]
async fn a_completed_version_whose_readback_missed_is_an_unconfirmed_issuance() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let secret = format!("{ONE_TIME}55");
    desk.service.lock().expect("service").overrides.push((
        "POST".into(),
        "/aisec/v1/mgmt/apikey".into(),
        // Answered, but the service's inventory does not show it yet.
        raw(200, &issued_key("key-5", "Support key 2", &secret)),
    ));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let issued = desk.settled().await;
    assert_eq!(issued.outcome, Some(Outcome::Completed));
    assert_ne!(issued.readback, Some(Readback::Found));
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(
        rotation.stage,
        desktop_core::RotationStage::IssuanceUnconfirmed
    );
    if issued.secret == SecretState::Held {
        assert_eq!(
            rotation.issued_by.as_deref(),
            Some(issued.operation.as_str()),
            "the held key stays revealable"
        );
    }
    assert!(reminder(&snap, "Support bot").is_none());
    let mut stored = issued_key("key-5", "Support key 2", &secret);
    stored.as_object_mut().expect("object").remove("api_key");
    desk.service.lock().expect("service").keys.push(stored);
    read_keys(&mut desk).await;
    let snap = desk.snap();
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(
        rotation.new_key.as_ref().map(|k| k.id.as_str()),
        Some("key-5")
    );
    assert!(reminder(&snap, "Support bot").is_some());
}

/// V2-R06 (I6): leaving a review whose issuance settled with the key held
/// retires the key; the rotation resumes and nothing can be revealed.
#[tokio::test(flavor = "multi_thread")]
async fn leaving_an_applied_review_retires_its_held_key() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let issued = desk.settled().await;
    assert_eq!(issued.secret, SecretState::Held);
    desk.send(json!({"kind": "discardChange", "review": review.review}))
        .await
        .expect("review exit");
    let snap = desk.snap();
    assert_eq!(snap.changes.results[0].secret, SecretState::Forgotten);
    let rotation = rotation_of(&snap, "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Resumed);
    assert_eq!(rotation.issued_by, None);
    assert!(
        desk.send(json!({"kind": "revealSecret", "operation": issued.operation}))
            .await
            .is_err()
    );
    // An unrelated or unknown review is refused.
    assert_eq!(
        desk.send(json!({"kind": "discardChange", "review": "rv-unknown"}))
            .await
            .err(),
        Some(Refusal::ReviewInvalid)
    );
}

/// V2-R07 (d): the reviewed key's detail read walks the whole inventory, so
/// its absence completes an unconfirmed removal; a detail that still finds
/// the key changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_detail_walk_reconciles_an_unconfirmed_removal() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    desk.settled().await;
    let context = desk.context();
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged");
    desk.service.lock().expect("service").overrides.push((
        "DELETE".into(),
        "/aisec/v1/mgmt/apikey/delete/".into(),
        Reply::Close,
    ));
    let delete = key_delete("key-1");
    let review = desk.review(&delete).await;
    desk.request(&review, &delete, "Support key")
        .await
        .expect("requested");
    assert_eq!(desk.settled().await.outcome, Some(Outcome::Unknown));
    let detail = |desk: &Desk| {
        json!({"kind": "management", "context": desk.context(),
            "request": {"kind": "detail", "resource": "apiKeys", "id": "key-1"}})
    };
    // Still there: nothing changes.
    let before = desk.snap().revision.clone();
    desk.send(detail(&desk)).await.expect("detail admitted");
    assert!(eventually(|| desk.snap().revision != before).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        rotation_of(&desk.snap(), "Support bot")
            .expect("rotation")
            .stage,
        desktop_core::RotationStage::RemovalUnconfirmed
    );
    // Gone from the whole inventory: complete.
    desk.service
        .lock()
        .expect("service")
        .keys
        .retain(|k| k["api_key_id"] != "key-1");
    let before = desk.snap().revision.clone();
    desk.send(detail(&desk)).await.expect("detail admitted");
    assert!(eventually(|| desk.snap().revision != before).await);
    assert!(
        eventually(|| rotation_of(&desk.snap(), "Support bot")
            .is_some_and(|r| r.stage == desktop_core::RotationStage::Complete))
        .await
    );
    assert!(reminder(&desk.snap(), "Support bot").is_none());
}

/// Starts the change `draft` reviewed with `typed`, its write answered after
/// `millis` (the reply `reply`), and returns once the write is on the wire.
async fn apply_delayed(
    desk: &mut Desk,
    draft: &Value,
    typed: &str,
    method: &str,
    path: &str,
    millis: u64,
    reply: Reply,
) {
    let writes = desk.writes().len();
    desk.service.lock().expect("service").overrides.push((
        method.into(),
        path.into(),
        Reply::Delayed {
            millis,
            then: Box::new(reply),
        },
    ));
    let review = desk.review(draft).await;
    desk.request(&review, draft, typed)
        .await
        .expect("requested");
    assert!(
        eventually(|| desk.writes().len() > writes).await,
        "the write is on the wire"
    );
}

/// The saved state reloaded into a fresh core (a relaunch).
async fn relaunched(desk: &Desk) -> ShellSnapshot {
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(
        !format!("{saved:?}").contains(ONE_TIME),
        "no secret persisted"
    );
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
    snapshot
}

/// R1 (C31 attempt-02): a tenant switch while a key Version is in flight
/// keeps the issuance with the tenant it was made in. The other tenant sees
/// none of it; coming back shows the rotation (confirmed, or unconfirmed and
/// reconcilable by a read) without its secret, and nothing is resent.
#[tokio::test(flavor = "multi_thread")]
async fn a_switch_during_issuance_keeps_the_rotation_with_its_tenant() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let secret = format!("{ONE_TIME}61");
    let draft = key_version("key-1", "Support bot", "Support key 2");
    apply_delayed(
        &mut desk,
        &draft,
        "CREATE",
        "POST",
        "/aisec/v1/mgmt/apikey",
        1200,
        raw(200, &issued_key("key-6", "Support key 2", &secret)),
    )
    .await;
    let mut stored = issued_key("key-6", "Support key 2", &secret);
    stored.as_object_mut().expect("object").remove("api_key");
    desk.service.lock().expect("service").keys.push(stored);
    desk.select("Tenant B").await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    let snap = desk.snap();
    assert!(
        snap.rotations.is_empty(),
        "the other tenant sees none of it"
    );
    assert!(reminder(&snap, "Support bot").is_none());
    desk.select("Tenant A").await;
    desk.management_key().await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("kept with its tenant");
    assert!(
        matches!(
            rotation.stage,
            desktop_core::RotationStage::Resumed | desktop_core::RotationStage::IssuanceUnconfirmed
        ),
        "{rotation:?}"
    );
    assert_eq!(rotation.issued_by, None, "no secret revives");
    assert!(desk.adapters.secrets.shown().is_empty());
    // Reconcilable without another mutation.
    read_keys(&mut desk).await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Resumed);
    assert_eq!(rotation.new_key.map(|k| k.id), Some("key-6".to_owned()));
    let posts = desk
        .writes()
        .iter()
        .filter(|w| w.request_line.starts_with("POST /aisec/v1/mgmt/apikey"))
        .count();
    assert_eq!(posts, 1, "never resent");
}

/// R1 (C31 attempt-02): a tenant switch while the old key's Delete is in
/// flight never leaves an acknowledged rotation whose key was deleted: the
/// removal is complete or unconfirmed with its tenant, and a read settles it.
#[tokio::test(flavor = "multi_thread")]
async fn a_switch_during_removal_keeps_the_outcome_with_its_tenant() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    desk.settled().await;
    let context = desk.context();
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged");
    apply_delayed(
        &mut desk,
        &key_delete("key-1"),
        "Support key",
        "DELETE",
        "/aisec/v1/mgmt/apikey/delete/",
        1200,
        raw(200, &json!({"message": "deleted"})),
    )
    .await;
    // The service deletes it while the answer is still on the way.
    desk.service
        .lock()
        .expect("service")
        .keys
        .retain(|k| k["api_key_id"] != "key-1");
    desk.select("Tenant B").await;
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert!(desk.snap().rotations.is_empty());
    desk.select("Tenant A").await;
    desk.management_key().await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert!(
        matches!(
            rotation.stage,
            desktop_core::RotationStage::Complete | desktop_core::RotationStage::RemovalUnconfirmed
        ),
        "never left acknowledged: {rotation:?}"
    );
    read_keys(&mut desk).await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Complete);
    assert!(reminder(&desk.snap(), "Support bot").is_none());
    let deletes = desk
        .writes()
        .iter()
        .filter(|w| w.request_line.starts_with("DELETE "))
        .count();
    assert_eq!(deletes, 1, "never resent");
}

/// R2 (C31 attempt-02): when storage fails, what the service established
/// (the issuance, then the removal) is kept in memory and reported unsaved;
/// once storage recovers the next publication saves the original facts and
/// timestamps, which a relaunch shows, with no write resent.
#[tokio::test(flavor = "multi_thread")]
async fn rotation_facts_survive_a_storage_failure_and_are_saved_on_recovery() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    desk.adapters.store.fail_saves(true);
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let issued = desk.settled().await;
    assert_eq!(issued.outcome, Some(Outcome::Completed));
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("kept in memory");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Issued);
    assert!(rotation.unsaved, "reported unsaved");
    let issued_at = rotation.issued_at;
    assert!(reminder(&desk.snap(), "Support bot").is_some());
    // An intent's own save still refuses while storage fails, and keeps nothing.
    let context = desk.context();
    assert_eq!(
        desk.send(json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}))
            .await
            .err(),
        Some(Refusal::DeniedStore)
    );
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(rotation.deployment_acknowledged_at, None);
    assert_eq!(
        rotation.issued_by.as_deref(),
        Some(issued.operation.as_str()),
        "the key is not forgotten by a refused acknowledgement"
    );
    // Storage recovers: the next publication saves the issuance.
    desk.adapters.store.fail_saves(false);
    read_keys(&mut desk).await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert!(!rotation.unsaved);
    let saved = relaunched(&desk).await;
    let persisted = rotation_of(&saved, "Support bot").expect("saved issuance");
    assert_eq!(persisted.issued_at, issued_at);
    assert_eq!(
        persisted.new_key.map(|k| k.name),
        Some("Support key 2".to_owned())
    );
    // The removal, under a failing store again.
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged");
    desk.adapters.store.fail_saves(true);
    let delete = key_delete("key-1");
    let review = desk.review(&delete).await;
    desk.request(&review, &delete, "Support key")
        .await
        .expect("requested");
    desk.settled().await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Complete);
    assert!(rotation.unsaved);
    let removed_at = rotation.old_key_removed_at.expect("removed at");
    desk.adapters.store.fail_saves(false);
    read_keys(&mut desk).await;
    assert!(
        !rotation_of(&desk.snap(), "Support bot")
            .expect("rotation")
            .unsaved
    );
    let saved = relaunched(&desk).await;
    let persisted = rotation_of(&saved, "Support bot").expect("saved removal");
    assert_eq!(persisted.stage, desktop_core::RotationStage::Complete);
    assert_eq!(persisted.old_key_removed_at, Some(removed_at));
    let writes: Vec<String> = desk
        .writes()
        .iter()
        .map(|w| w.request_line.clone())
        .collect();
    assert_eq!(
        writes.iter().filter(|w| w.starts_with("POST ")).count(),
        1,
        "{writes:?}"
    );
    assert_eq!(
        writes.iter().filter(|w| w.starts_with("DELETE ")).count(),
        1,
        "{writes:?}"
    );
}

/// A relaunch of the desk's core from its saved state, on the same
/// synthetic service (operation IDs start again from the beginning).
async fn restart(old: Desk) -> Desk {
    let saved = old.adapters.store.saved().expect("saved");
    let origin = format!("https://localhost:{}", old.server.port);
    let Desk {
        _core,
        adapters: old_handles,
        service,
        server,
        _ca,
        ..
    } = old;
    drop(_core);
    let (mut adapters, handles) =
        fixtures::adapters_with(fixtures::ScriptedStore::with_state(saved));
    adapters.keychain = old_handles.vault.clone();
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
        extra_roots: vec![_ca.pem.as_bytes().to_vec()],
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
        service,
        server,
        _ca,
        results_seen: 0,
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.select("Tenant A").await;
    desk.management_key().await;
    desk.adapters.confirmer.auto(Some(true));
    desk
}

/// R1 (C31 attempt-03): the link from a rotation to the result holding its
/// one-time key is session-only and exact. After a relaunch the unfinished
/// rotation is Resumed with nothing to reveal; an issuance for another
/// application that reuses the former operation ID neither revives it nor
/// can be forgotten through it, and each key stays revealable only from its
/// own result.
#[tokio::test(flavor = "multi_thread")]
async fn a_reused_operation_id_after_relaunch_never_links_another_key() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    {
        let mut service = desk.service.lock().expect("service");
        service.keys.push(json!({"api_key_id": "key-b", "api_key_name": "Billing key", "cust_app": "Billing bot",
            "api_key_last8": "bbbb1111", "expiration": "2027-01-01T00:00:00Z", "revoked": false, "auth_code": "STORED-CODE"}));
    }
    let draft = key_version("key-1", "Support bot", "Support key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let first = desk.settled().await;
    assert_eq!(first.secret, SecretState::Held);
    let saved = serde_json::to_string(&desk.adapters.store.saved().expect("saved")).expect("json");
    assert!(
        !saved.contains(first.operation.as_str()),
        "the session link is not persisted"
    );
    // Relaunch, then the same sequence for another application.
    let mut desk = restart(desk).await;
    let rotation = rotation_of(&desk.snap(), "Support bot").expect("persisted rotation");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Resumed);
    assert_eq!(rotation.issued_by, None);
    let draft = key_version("key-b", "Billing bot", "Billing key 2");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let second = desk.settled().await;
    assert_eq!(second.secret, SecretState::Held);
    assert_eq!(
        second.operation, first.operation,
        "the former operation ID is reused after the relaunch"
    );
    let snap = desk.snap();
    let old = rotation_of(&snap, "Support bot").expect("old rotation");
    assert_eq!(
        old.stage,
        desktop_core::RotationStage::Resumed,
        "not revived by the reused ID"
    );
    assert_eq!(old.issued_by, None);
    let new = rotation_of(&snap, "Billing bot").expect("new rotation");
    assert_eq!(new.stage, desktop_core::RotationStage::Issued);
    assert_eq!(new.issued_by.as_deref(), Some(second.operation.as_str()));
    // Acknowledging the old rotation cannot forget the new key.
    let context = desk.context();
    desk.send(
        json!({"kind": "acknowledgeDeployment", "context": context, "application": "Support bot"}),
    )
    .await
    .expect("acknowledged");
    let snap = desk.snap();
    assert_eq!(
        snap.changes.results[0].secret,
        SecretState::Held,
        "the new key is untouched"
    );
    assert_eq!(
        rotation_of(&snap, "Billing bot")
            .expect("new")
            .issued_by
            .as_deref(),
        Some(second.operation.as_str())
    );
    // The new key is revealable from its own result.
    desk.send(json!({"kind": "revealSecret", "operation": second.operation}))
        .await
        .expect("revealed");
    assert_eq!(desk.adapters.secrets.shown().len(), 1);
}
