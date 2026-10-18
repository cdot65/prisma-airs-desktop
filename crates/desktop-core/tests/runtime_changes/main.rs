//! C09 reviewed Runtime changes through the production authority interface
//! against a stateful synthetic management service: v1's write subset,
//! reviews bound to context, draft and baseline, the native confirmation's
//! private single-use completion, conflict on a changed resource, lost and
//! refused writes without retry, operation-specific readback and one-time
//! secrets that never reach a snapshot.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::{ManagementEndpoints, ManagementFailure};
use desktop_core::runtime::{OperationId, Outcome};
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, ChangeFailure, ChangeResultView, ChangeReviewState, ChangeReviewView, Core,
    CoreConfig, Delivery, Intent, Observation, Readback, Receipt, Refusal, ScanState, SecretState,
    ShellSnapshot, ViewScope,
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
                              "environment": "production", "tsg_id": TSG}),
            ],
            keys: vec![
                json!({"api_key_id": "key-1", "api_key_name": "Support key", "cust_app": "Support bot",
                              "api_key_last8": "abcd1234", "expiration": "2027-01-01T00:00:00Z",
                              "revoked": false, "auth_code": "STORED-CODE"}),
            ],
            serial: 0,
            overrides: Vec::new(),
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
                *app = body.clone();
                app["customer_appId"] = json!(id);
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
                                          "environment": body["cust_env"]}));
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
    let ca = TestCa::new("airs-v2 c09 ca");
    let service = Arc::new(Mutex::new(Service::new()));
    let routed = Arc::clone(&service);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("service").route(seen)
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

fn key_create(app: &str, name: &str) -> Value {
    json!({"kind": "apiKeys", "action": "create", "target": "",
           "body": json!({"cust_app": app, "cust_env": "production", "cust_cloud_provider": "aws",
                          "api_key_name": name, "dp_name": "dp-1", "revoked": false,
                          "rotation_time_interval": 30, "rotation_time_unit": "days"}).to_string()})
}

fn body(seen: &Seen) -> Value {
    serde_json::from_slice(&seen.body).unwrap_or(Value::Null)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reviewed_update_applies_once_after_the_native_confirmation() {
    let mut desk = desk().await;
    let draft = topic_update("Money and payments");
    let review = desk.review(&draft).await;
    assert_eq!(review.state, ChangeReviewState::Ready);
    assert_eq!(review.confirmation.as_deref(), Some("Finance"));
    let before: Value =
        serde_json::from_str(review.before.as_deref().expect("before")).expect("json");
    assert_eq!(before["description"], "Money talk");
    assert!(
        review
            .after
            .as_deref()
            .expect("after")
            .contains("Money and payments")
    );
    assert!(desk.writes().is_empty(), "review sends nothing");
    // RequestApply opens the native confirmation; nothing is written yet.
    desk.request(&review, &draft, "Finance")
        .await
        .expect("requested");
    assert!(eventually(|| desk.adapters.confirmer.held() == 1).await);
    let (_, shown) = desk
        .adapters
        .confirmer
        .requests()
        .pop()
        .expect("confirmation");
    assert!(
        shown.title.contains("Finance") && shown.title.starts_with("Update custom topic"),
        "{shown:?}"
    );
    assert!(shown.message.contains("Tenant A"));
    assert_eq!(
        desk.snap().changes.review.expect("review").state,
        ChangeReviewState::Confirming
    );
    assert!(desk.writes().is_empty());
    // A second RequestApply while confirming is refused.
    assert_eq!(
        desk.request(&review, &draft, "Finance").await.err(),
        Some(Refusal::Busy)
    );
    // Only the confirmation's own completion applies it, once.
    assert!(desk.adapters.confirmer.complete(true));
    let result = desk.settled().await;
    assert_eq!(
        (result.outcome, result.failure),
        (Some(Outcome::Completed), None)
    );
    assert_eq!(result.readback, Some(Readback::Found));
    assert!(
        result
            .current
            .as_deref()
            .expect("current")
            .contains("Money and payments")
    );
    let writes = desk.writes();
    assert_eq!(writes.len(), 1, "one write");
    assert!(
        writes[0]
            .request_line
            .starts_with(&format!("PUT /aisec/v1/mgmt/topic/uuid/{TOPIC_ID}"))
    );
    let sent = body(&writes[0]);
    assert_eq!(
        sent["updated_by"], CLIENT_ID,
        "authorship from the management client"
    );
    assert_eq!(sent["description"], "Money and payments");
    // The review was consumed.
    assert!(desk.snap().changes.review.is_none());
    assert_eq!(
        desk.request(&review, &draft, "Finance").await.err(),
        Some(Refusal::ReviewConsumed)
    );
    assert!(
        !desk.adapters.confirmer.complete(true),
        "nothing left to complete"
    );
    assert_eq!(desk.writes().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_or_stale_confirmation_sends_nothing() {
    let mut desk = desk().await;
    let draft = topic_update("Denied");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "Finance")
        .await
        .expect("requested");
    assert!(eventually(|| desk.adapters.confirmer.held() == 1).await);
    assert!(desk.adapters.confirmer.complete(false));
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .review
            .as_ref()
            .is_some_and(|r| r.state == ChangeReviewState::Ready))
        .await
    );
    assert!(desk.writes().is_empty() && desk.snap().changes.results.is_empty());
    // Another attempt needs another confirmation; switching tenants while it
    // is open ends the sheet and its later completion applies nothing.
    desk.request(&review, &draft, "Finance")
        .await
        .expect("requested again");
    assert!(eventually(|| desk.adapters.confirmer.held() == 1).await);
    let nonce = desk.adapters.confirmer.requests().last().expect("second").0;
    desk.select("Tenant B").await;
    assert!(
        desk.adapters.confirmer.dismissed().contains(&nonce),
        "the sheet is ended on retirement"
    );
    assert!(
        !desk.adapters.confirmer.complete(true),
        "dismissal resolved it"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(desk.writes().is_empty());
    assert!(
        desk.snap().changes.review.is_none(),
        "no review in tenant B"
    );
    // Back in tenant A the old review is gone: it cannot be applied there.
    desk.select("Tenant A").await;
    assert_eq!(
        desk.request(&review, &draft, "Finance").await.err(),
        Some(Refusal::ReviewInvalid)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn fresh_input_and_the_typed_confirmation_must_match_the_review() {
    let mut desk = desk().await;
    let draft = topic_update("Reviewed text");
    let review = desk.review(&draft).await;
    // Edited after Review (even if no debounce saw it): refused.
    assert_eq!(
        desk.request(&review, &topic_update("Edited after review"), "Finance")
            .await
            .err(),
        Some(Refusal::ReviewInvalid)
    );
    assert_eq!(
        desk.request(&review, &draft, "finance").await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.adapters.confirmer.requests().len(),
        0,
        "no confirmation opened"
    );
    // A forged review reference is refused.
    let forged = json!({"kind": "requestApply", "review": "rv-forged0000000000", "draft": draft, "confirmation": "Finance"});
    assert_eq!(desk.send(forged).await.err(), Some(Refusal::ReviewInvalid));
    assert!(desk.writes().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resource_changed_since_review_is_a_conflict_and_is_not_written() {
    let mut desk = desk().await;
    let draft = topic_update("Mine");
    let review = desk.review(&draft).await;
    // Another writer changes the topic between review and apply.
    desk.service.lock().expect("service").topics[0]["description"] = json!("Theirs");
    desk.request(&review, &draft, "Finance")
        .await
        .expect("requested");
    assert!(eventually(|| desk.adapters.confirmer.held() == 1).await);
    desk.adapters.confirmer.complete(true);
    let result = desk.settled().await;
    assert_eq!(result.failure, Some(ChangeFailure::Conflict));
    assert_eq!(result.outcome, Some(Outcome::NotSent));
    assert!(
        desk.writes().is_empty(),
        "nothing written over the other change"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn lost_and_refused_writes_keep_their_outcome_and_are_never_retried() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    // The connection closes after the write was sent: unknown, not retried.
    desk.service.lock().expect("service").overrides.push((
        "PUT".into(),
        "/aisec/v1/mgmt/topic/uuid/".into(),
        Reply::Close,
    ));
    let draft = topic_update("Lost");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "Finance")
        .await
        .expect("requested");
    let lost = desk.settled().await;
    assert_eq!(lost.outcome, Some(Outcome::Unknown));
    assert_eq!(
        lost.failure,
        Some(ChangeFailure::Service {
            failure: ManagementFailure::Network
        })
    );
    assert_eq!(desk.writes().len(), 1, "sent once, never resent");
    // A 409 is a conflict the service rejected.
    desk.service.lock().expect("service").overrides.push((
        "PUT".into(),
        "/aisec/v1/mgmt/topic/uuid/".into(),
        raw(409, &json!({"message": "SERVER-TEXT"})),
    ));
    let draft = topic_update("Conflicted");
    let review = desk.review(&draft).await;
    desk.request(&review, &draft, "Finance")
        .await
        .expect("requested");
    let refused = desk.settled().await;
    assert_eq!(
        (refused.outcome, refused.failure),
        (Some(Outcome::Rejected), Some(ChangeFailure::Conflict))
    );
    assert!(!format!("{:?}", desk.snap()).contains("SERVER-TEXT"));
    assert_eq!(desk.writes().len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn key_creation_checks_names_injects_the_deployment_code_and_holds_the_secret() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    // An existing application or key name is refused before any write.
    let exists = desk.review(&key_create("support BOT", "New key")).await;
    assert_eq!(
        exists.failure,
        Some(ChangeFailure::Problem {
            problem: desktop_core::changes::ChangeProblem::ApplicationExists
        })
    );
    let dup = desk.review(&key_create("New app", "support key")).await;
    assert_eq!(
        dup.failure,
        Some(ChangeFailure::Problem {
            problem: desktop_core::changes::ChangeProblem::KeyExists
        })
    );
    let draft = key_create("New app", "New key");
    let review = desk.review(&draft).await;
    assert_eq!(review.confirmation.as_deref(), Some("CREATE"));
    assert!(
        !review.after.as_deref().unwrap_or("").contains(AUTH_CODE),
        "auth code never shown"
    );
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let created = desk.settled().await;
    assert_eq!(created.outcome, Some(Outcome::Completed));
    assert_eq!(created.secret, SecretState::Held);
    assert_eq!(created.readback, Some(Readback::Found));
    let writes = desk.writes();
    let sent = body(&writes[0]);
    assert_eq!(
        sent["auth_code"], AUTH_CODE,
        "from the one matching deployment profile"
    );
    assert_eq!(sent["created_by"], CLIENT_ID);
    // The one-time secret is in no snapshot, and only the native sheet shows it.
    let snapshot = serde_json::to_string(&desk.snap()).expect("json");
    assert!(!snapshot.contains(ONE_TIME) && !snapshot.contains(AUTH_CODE));
    desk.send(json!({"kind": "revealSecret", "operation": created.operation}))
        .await
        .expect("revealed");
    let shown = desk.adapters.secrets.shown();
    assert_eq!(shown.len(), 1);
    assert!(shown[0].1.starts_with(ONE_TIME) && shown[0].0.contains("New key"));
    desk.send(json!({"kind": "forgetSecret", "operation": created.operation}))
        .await
        .expect("forgotten");
    assert_eq!(
        desk.snap().changes.results[0].secret,
        SecretState::Forgotten
    );
    assert_eq!(
        desk.send(json!({"kind": "revealSecret", "operation": created.operation}))
            .await
            .err(),
        Some(Refusal::CapabilityMissing)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rotation_deletion_and_their_readback() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let rotate = json!({"kind": "apiKeys", "action": "rotate", "target": "key-1",
        "body": json!({"rotation_time_interval": 90, "rotation_time_unit": "days", "api_key_name": "renamed"}).to_string()});
    let review = desk.review(&rotate).await;
    assert!(
        review
            .after
            .as_deref()
            .expect("after")
            .contains("Determined by server on rotation")
    );
    desk.request(&review, &rotate, "Support key")
        .await
        .expect("requested");
    let rotated = desk.settled().await;
    assert_eq!(
        (rotated.outcome, rotated.secret),
        (Some(Outcome::Completed), SecretState::Held)
    );
    let sent = body(&desk.writes()[0]);
    assert_eq!(
        sent,
        json!({"rotation_time_interval": 90, "rotation_time_unit": "days", "updated_by": CLIENT_ID})
    );
    // Deleting the application reviews every affected key and reads back absent.
    let delete =
        json!({"kind": "applications", "action": "delete", "target": "app-1", "body": "{}"});
    let review = desk.review(&delete).await;
    let before: Value =
        serde_json::from_str(review.before.as_deref().expect("before")).expect("json");
    assert_eq!(before["affected_keys"].as_array().map(Vec::len), Some(1));
    assert!(review.impact.contains("ALL associated API keys"));
    desk.request(&review, &delete, "Support bot")
        .await
        .expect("requested");
    let deleted = desk.settled().await;
    assert_eq!(
        (deleted.outcome, deleted.readback),
        (Some(Outcome::Completed), Some(Readback::Absent))
    );
    let last = desk.writes().pop().expect("delete");
    assert!(
        last.request_line
            .starts_with("DELETE /aisec/v1/mgmt/customerapp?")
    );
    assert!(last.request_line.contains("app_name=Support"));
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_drafts_fail_review_without_any_request() {
    let mut desk = desk().await;
    let before = desk.server.seen().len();
    for draft in [
        json!({"kind": "applications", "action": "create", "target": "", "body": "{}"}),
        json!({"kind": "apiKeys", "action": "rotate", "target": "key-1",
               "body": json!({"rotation_time_interval": 45, "rotation_time_unit": "days"}).to_string()}),
        json!({"kind": "topics", "action": "update", "target": TOPIC_ID, "body": "[not an object]"}),
    ] {
        let review = desk.review(&draft).await;
        assert_eq!(review.state, ChangeReviewState::Failed);
        assert!(matches!(
            review.failure,
            Some(ChangeFailure::Problem { .. })
        ));
    }
    assert_eq!(desk.server.seen().len(), before, "nothing read or sent");
    // A scan-only tenant cannot review changes at all.
    desk.select("Tenant B").await;
    let context = desk.context();
    assert_eq!(
        desk.send(json!({"kind": "reviewChange", "context": context, "draft": topic_update("x")}))
            .await
            .err(),
        Some(Refusal::CapabilityMissing)
    );
    let _: Option<OperationId> = None;
}

#[tokio::test(flavor = "multi_thread")]
async fn offline_reviews_and_applies_fail_without_queueing_and_retry_only_when_asked() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    // Unreachable while reviewing: the review fails, nothing is queued.
    for _ in 0..3 {
        desk.service.lock().expect("service").overrides.push((
            "GET".into(),
            "/aisec/v1/mgmt/topics".into(),
            Reply::Close,
        ));
    }
    let draft = topic_update("Offline");
    let failed = desk.review(&draft).await;
    assert_eq!(failed.state, ChangeReviewState::Failed);
    assert!(matches!(
        failed.failure,
        Some(ChangeFailure::Service { .. })
    ));
    // Reachable again: nothing happens by itself.
    desk.service.lock().expect("service").overrides.clear();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(desk.writes().is_empty());
    assert_eq!(
        desk.snap().changes.review.expect("review").state,
        ChangeReviewState::Failed
    );
    // An explicit retry reviews again; the connection drops before apply's
    // re-read: not sent, no write, and the review is consumed (a new one is
    // needed, never a replay).
    let review = desk.review(&draft).await;
    assert_eq!(review.state, ChangeReviewState::Ready);
    for _ in 0..3 {
        desk.service.lock().expect("service").overrides.push((
            "GET".into(),
            "/aisec/v1/mgmt/topics".into(),
            Reply::Close,
        ));
    }
    desk.request(&review, &draft, "Finance")
        .await
        .expect("requested");
    let result = desk.settled().await;
    assert_eq!(result.outcome, Some(Outcome::NotSent));
    assert!(matches!(
        result.failure,
        Some(ChangeFailure::Service { .. })
    ));
    assert!(desk.writes().is_empty());
    assert_eq!(
        desk.request(&review, &draft, "Finance").await.err(),
        Some(Refusal::ReviewConsumed)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_about_a_change_or_its_secret_is_persisted_or_replayed_after_restart() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(Some(true));
    let draft = key_create("New app", "New key");
    let review = desk.review(&draft).await;
    // A second review is left open across the restart.
    desk.request(&review, &draft, "CREATE")
        .await
        .expect("requested");
    let created = desk.settled().await;
    assert_eq!(created.secret, SecretState::Held);
    let pending = desk.review(&topic_update("Left open")).await;
    assert_eq!(pending.state, ChangeReviewState::Ready);
    let saved = desk.adapters.store.saved().expect("saved");
    let persisted = format!("{saved:?}");
    let review_id = serde_json::to_value(&pending.review).expect("id");
    for leaked in [
        ONE_TIME,
        AUTH_CODE,
        review_id.as_str().expect("id"),
        "Left open",
    ] {
        assert!(!persisted.contains(leaked), "{leaked} persisted");
    }
    let writes = desk.writes().len();
    // A new core from the saved state: no review, no results, no replay.
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
