//! C31/I6: native page teardown owns review and one-time disclosure retirement.
//! Uses the real Host lifecycle callback and production IPC/authority against a
//! loopback-only synthetic management plane. No renderer cleanup is simulated.

#[allow(dead_code)]
#[path = "../../../../crates/desktop-core/tests/context_credentials/support.rs"]
mod support;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::ManagementEndpoints;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{Core, CoreConfig, Observation, SecretState, ShellSnapshot, ViewScope};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use support::{Reply, Seen, TestCa, TlsServer, eventually};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow};

const TSG: &str = "1234567890";
const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";

struct Plane {
    keys: Vec<Value>,
    writes: usize,
    delay_issuance: bool,
}

fn raw(value: &Value) -> Reply {
    Reply::ok(&serde_json::to_vec(value).expect("synthetic JSON"))
}

impl Plane {
    fn route(&mut self, seen: &Seen) -> Reply {
        let method = seen.request_line.split_whitespace().next().unwrap_or("");
        let target = seen.request_line.split_whitespace().nth(1).unwrap_or("");
        let path = target.split('?').next().unwrap_or("");
        if path == "/oauth2/access_token" {
            return raw(
                &json!({"access_token":"synthetic-token", "token_type":"Bearer", "expires_in":3600}),
            );
        }
        if method == "GET" && path.starts_with("/aisec/v1/mgmt/apikeys/") {
            return raw(&json!({"api_keys":self.keys}));
        }
        if method == "GET" && path.starts_with("/aisec/v1/mgmt/customerapp/") {
            return raw(
                &json!({"customer_apps":[{"customer_appId":"app-1", "app_name":"Support bot", "cloud_provider":"aws", "environment":"production", "tsg_id":TSG, "api_keys_dp_info":[{"api_key_name":"support", "auth_code":"OTHER-CODE", "dp_name":"dp-2"}]}]}),
            );
        }
        if method == "GET" && path == "/aisec/v1/mgmt/deploymentprofiles" {
            return raw(
                &json!({"status":"success", "deployment_profiles":[{"dp_name":"dp-1", "auth_code":"SYNTHETIC-DP", "status":"active"}]}),
            );
        }
        if method == "POST" && path == "/aisec/v1/mgmt/apikey" {
            self.writes += 1;
            let body: Value = serde_json::from_slice(&seen.body).expect("posted JSON");
            let mut key = body;
            key["api_key_id"] = json!(format!("issued-{}", self.writes));
            key["api_key_last8"] = json!("12345678");
            key["expiration"] = json!("2027-06-01T00:00:00Z");
            self.keys.push(key.clone());
            key["api_key"] = json!("SYNTHETIC-ONE-TIME-12345678");
            let reply = raw(&key);
            return if self.delay_issuance {
                Reply::Delayed {
                    millis: 1500,
                    then: Box::new(reply),
                }
            } else {
                reply
            };
        }
        raw(&json!({"error":"unexpected synthetic route", "path":path}))
    }
}

struct Desk {
    core: Core,
    app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    adapters: Handles,
    plane: Arc<Mutex<Plane>>,
    _server: TlsServer,
    _ca: TestCa,
    session: String,
    sequence: u64,
}

impl Desk {
    async fn new(delay_issuance: bool) -> Self {
        Self::with_presenter(delay_issuance, None).await
    }
    async fn with_presenter(delay_issuance: bool, queued: Option<Arc<QueuedNative>>) -> Self {
        let ca = TestCa::new("C31 renderer lifetime fixture");
        let plane = Arc::new(Mutex::new(Plane {
            keys: vec![
                json!({"api_key_id":"key-1", "api_key_name":"Support key", "cust_app":"Support bot", "api_key_last8":"abcd1234", "expiration":"2027-01-01T00:00:00Z", "revoked":false, "auth_code":"STORED-CODE"}),
            ],
            writes: 0,
            delay_issuance,
        }));
        let routed = Arc::clone(&plane);
        let server = TlsServer::start(
            ca.leaf(&["localhost"]),
            Reply::Route(Arc::new(move |seen| {
                routed.lock().expect("plane").route(seen)
            })),
        )
        .await;
        let origin = format!("https://localhost:{}", server.port);
        let (mut adapters, handles) = fixtures::adapters();
        if let Some(queued) = queued {
            adapters.confirmer = queued.clone();
            adapters.secrets = queued;
        }
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
        .expect("fixture policy");
        let app = desktop_host::install(mock_builder(), &core)
            .build(tauri::generate_context!())
            .expect("production host");
        let window = desktop_host::main_window(&app).expect("main webview");
        let mut desk = Self {
            core,
            app,
            window,
            adapters: handles,
            plane,
            _server: server,
            _ca: ca,
            session: String::new(),
            sequence: 0,
        };
        desk.register();
        desk.send(json!({"kind":"createTenant", "profile":{"label":"Tenant A", "tsgId":TSG, "destination":{"kind":"custom", "origin":origin}}})).expect("tenant");
        let tenant = desk.snap().tenants[0].id.clone();
        desk.send(json!({"kind":"selectTenant", "tenant":tenant}))
            .expect("selected");
        let context = desk.snap().active.expect("active").context;
        desk.send(json!({"kind":"configureManagementCredential", "context":context, "mode":"session", "clientId":CLIENT_ID, "secret":"synthetic-client-secret"})).expect("management credential");
        assert!(eventually(|| desk.snap().tenants[0].management.state == desktop_core::ScanState::Ready).await);
        desk
    }

    fn call(&self, command: &str, body: Value) -> Result<Value, Value> {
        Self::call_window(&self.window, command, body)
    }
    fn call_window(
        window: &WebviewWindow<MockRuntime>,
        command: &str,
        body: Value,
    ) -> Result<Value, Value> {
        get_ipc_response(
            window,
            InvokeRequest {
                cmd: command.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().expect("origin"),
                body: InvokeBody::Raw(serde_json::to_vec(&body).expect("request JSON")),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|response| response.deserialize::<Value>().expect("response JSON"))
    }
    fn send_thread(&mut self, intent: Value) -> std::thread::JoinHandle<Result<Value, Value>> {
        self.sequence += 1;
        let window = self.window.clone();
        let body = json!({"protocol":1,"session":self.session,"sequence":self.sequence.to_string(),"intent":intent});
        std::thread::spawn(move || Self::call_window(&window, "desktop_dispatch", body))
    }
    fn register(&mut self) {
        let value = self
            .call(
                "desktop_observe",
                json!({"protocol":1,"scope":{"kind":"deliverySession"}}),
            )
            .expect("fresh session");
        self.session = value["session"].as_str().expect("session nonce").to_owned();
        self.sequence = 0;
    }
    fn send(&mut self, intent: Value) -> Result<Value, Value> {
        self.sequence += 1;
        let response = self.call("desktop_dispatch", json!({"protocol":1,"session":self.session,"sequence":self.sequence.to_string(),"intent":intent}))?;
        if response["outcome"] == "refusal" {
            Err(response["refusal"].clone())
        } else {
            Ok(response)
        }
    }
    fn snap(&self) -> ShellSnapshot {
        match self
            .core
            .operator()
            .observe(ViewScope::Shell)
            .expect("shell")
        {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell observation"),
        }
    }
    async fn review(&mut self) -> desktop_core::ChangeReviewView {
        self.review_draft(draft()).await
    }
    async fn review_draft(&mut self, draft: Value) -> desktop_core::ChangeReviewView {
        let context = self.snap().active.expect("active").context;
        self.send(json!({"kind":"reviewChange","context":context,"draft":draft}))
            .expect("review");
        assert!(
            eventually(|| self
                .snap()
                .changes
                .review
                .as_ref()
                .is_some_and(|review| matches!(
                    review.state,
                    desktop_core::ChangeReviewState::Ready
                        | desktop_core::ChangeReviewState::Failed
                )))
            .await
        );
        let review = self.snap().changes.review.expect("review exists");
        assert_eq!(
            review.state,
            desktop_core::ChangeReviewState::Ready,
            "{:?}",
            review.failure
        );
        review
    }
    fn apply(&mut self, review: &desktop_core::ChangeReviewView) {
        self.apply_draft(review, draft());
    }
    fn apply_draft(&mut self, review: &desktop_core::ChangeReviewView, draft: Value) {
        self.send(json!({"kind":"requestApply","review":review.review,"draft":draft,"confirmation":"CREATE"})).expect("apply requested");
    }
    async fn settled(&self) -> desktop_core::ChangeResultView {
        assert!(
            eventually(|| self
                .snap()
                .changes
                .results
                .first()
                .is_some_and(|result| result.settled))
            .await,
            "original operation settles"
        );
        self.snap().changes.results[0].clone()
    }
    fn retire(&self) {
        self.app
            .state::<desktop_host::Host>()
            .retire_webview("main");
    }
    fn writes(&self) -> usize {
        self.plane.lock().expect("plane").writes
    }
}

fn draft() -> Value {
    draft_named("Support key 2")
}
fn draft_named(name: &str) -> Value {
    json!({"kind":"apiKeys","action":"version","target":"key-1","body":json!({"cust_app":"Support bot","cust_env":"production","cust_cloud_provider":"aws","api_key_name":name,"dp_name":"dp-1","revoked":false,"rotation_time_interval":30,"rotation_time_unit":"days"}).to_string()})
}

#[tokio::test(flavor = "multi_thread")]
async fn renderer_reload_before_reveal_forgets_the_native_held_key() {
    let mut desk = Desk::new(false).await;
    desk.adapters.confirmer.auto(Some(true));
    let review = desk.review().await;
    desk.apply(&review);
    let result = desk.settled().await;
    assert_eq!(result.secret, SecretState::Held);
    let context = desk.snap().active.expect("active").context;
    let dismissals = desk.adapters.secrets.dismissals();
    desk.retire();
    assert_eq!(
        desk.snap().changes.results[0].secret,
        SecretState::Forgotten,
        "native teardown drops held key before any replacement renderer call"
    );
    assert_eq!(
        desk.snap().active.expect("active").context,
        context,
        "business context survives"
    );
    assert!(
        desk.adapters.secrets.dismissals() > dismissals,
        "host disclosure dismissed"
    );
    assert_eq!(
        desk.send(json!({"kind":"revealSecret","operation":result.operation}))
            .expect_err("old page nonce retired"),
        json!("staleContext")
    );
    desk.register();
    assert!(
        desk.send(json!({"kind":"revealSecret","operation":result.operation}))
            .is_err(),
        "new renderer has no old disclosure"
    );
    assert!(desk.adapters.secrets.shown().is_empty());
    assert_eq!(
        desk.snap().changes.results[0].outcome,
        Some(desktop_core::runtime::Outcome::Completed)
    );
    assert_eq!(desk.writes(), 1, "no issuance replay");
}

#[tokio::test(flavor = "multi_thread")]
async fn renderer_reload_during_confirmation_retires_the_native_review() {
    let mut desk = Desk::new(false).await;
    desk.adapters.confirmer.auto(None);
    let review = desk.review().await;
    desk.apply(&review);
    assert!(
        eventually(|| desk.adapters.confirmer.held() == 1).await,
        "native sheet held"
    );
    desk.retire();
    assert!(
        desk.snap().changes.review.is_none(),
        "departed editor owns no review"
    );
    assert_eq!(desk.adapters.confirmer.held(), 0, "native sheet dismissed");
    assert!(
        !desk.adapters.confirmer.complete(true),
        "stale native acceptance cannot run"
    );
    desk.register();
    assert!(desk.send(json!({"kind":"requestApply","review":review.review,"draft":draft(),"confirmation":"CREATE"})).is_err(),"old review refuses in new page");
    assert_eq!(desk.writes(), 0, "retired review sent no mutation");
    desk.adapters.confirmer.auto(Some(true));
    let fresh = desk.review().await;
    desk.apply(&fresh);
    assert_eq!(
        desk.settled().await.outcome,
        Some(desktop_core::runtime::Outcome::Completed)
    );
    assert_eq!(desk.writes(), 1, "only explicit fresh review applies");
}

#[tokio::test(flavor = "multi_thread")]
async fn renderer_reload_during_issuance_drops_the_late_key_without_replaying() {
    let mut desk = Desk::new(true).await;
    desk.adapters.confirmer.auto(Some(true));
    let review = desk.review().await;
    desk.apply(&review);
    assert!(
        eventually(|| desk.writes() == 1).await,
        "original write dispatched"
    );
    let operation = desk.snap().changes.results[0].operation.clone();
    assert!(
        !desk.snap().changes.results[0].settled,
        "reply still pending"
    );
    desk.retire();
    desk.register();
    let result = desk.settled().await;
    assert_eq!(
        result.operation, operation,
        "the original work remains observable"
    );
    assert_eq!(
        result.outcome,
        Some(desktop_core::runtime::Outcome::Completed),
        "remote completion preserved"
    );
    assert_eq!(
        result.secret,
        SecretState::Forgotten,
        "reply after page exit never acquires disclosure authority"
    );
    assert!(
        desk.send(json!({"kind":"revealSecret","operation":operation}))
            .is_err()
    );
    assert!(desk.adapters.secrets.shown().is_empty());
    let rotation = desk
        .snap()
        .rotations
        .into_iter()
        .find(|rotation| rotation.application == "Support bot")
        .expect("rotation facts retained");
    assert_eq!(rotation.stage, desktop_core::RotationStage::Resumed);
    assert_eq!(rotation.issued_by, None);
    assert_eq!(desk.writes(), 1, "original write was never replayed");
}

#[tokio::test(flavor = "multi_thread")]
async fn renderer_delivery_recovery_and_other_webviews_do_not_end_a_live_review() {
    let mut desk = Desk::new(false).await;
    desk.adapters.confirmer.auto(Some(true));
    let review = desk.review().await;
    desk.apply(&review);
    let result = desk.settled().await;
    desk.register(); // delivery recovery is not page teardown
    desk.app
        .state::<desktop_host::Host>()
        .retire_webview("other-window");
    assert_eq!(desk.snap().changes.results[0].secret, SecretState::Held);
    desk.send(json!({"kind":"revealSecret","operation":result.operation}))
        .expect("same live page may reveal");
    assert_eq!(desk.adapters.secrets.shown().len(), 1);
    assert_eq!(desk.writes(), 1);
}

// Both queue adapters use the production callback guards. The unscoped fallback
// deliberately preserves the old public adapter contract: the old core's calls
// have no owner to revoke. This makes the old-red failures behavioral, not a
// missing-new-API compilation failure.
#[derive(Default)]
struct QueuedNative {
    registry: desktop_host::native_confirmation::Registry,
    owner: desktop_host::native_confirmation::SecretOwnership,
    queue_confirms: std::sync::atomic::AtomicBool,
    confirm_entries: std::sync::atomic::AtomicUsize,
    state: Mutex<QueuedState>,
}
#[derive(Default)]
struct QueuedState {
    secrets: Vec<SecretTask>,
    confirmations: Vec<ConfirmationTask>,
    shown_secrets: usize,
    copied_secrets: usize,
    shown_confirmations: usize,
}
enum SecretTask {
    Legacy(String, zeroize::Zeroizing<String>),
    Scoped(desktop_host::native_confirmation::SecretPresentation),
}
enum ConfirmationTask {
    Legacy(u64, desktop_core::ConfirmationRequest),
    Scoped(desktop_host::native_confirmation::ConfirmationPresentation),
}
impl QueuedNative {
    fn secret_count(&self) -> usize {
        self.state.lock().expect("queue").secrets.len()
    }
    fn confirmation_count(&self) -> usize {
        self.state.lock().expect("queue").confirmations.len()
    }
    fn drain(&self) {
        use desktop_host::native_confirmation::SecretPresentation;
        let (secrets, confirmations) = {
            let mut state = self.state.lock().expect("queue");
            (
                std::mem::take(&mut state.secrets),
                std::mem::take(&mut state.confirmations),
            )
        };
        for task in secrets {
            let task = match task {
                SecretTask::Legacy(label, secret) => SecretPresentation::new(
                    label,
                    secret,
                    desktop_core::PresentationLease::default(),
                ),
                SecretTask::Scoped(task) => task,
            };
            task.run(&self.owner, |_, _, permit| {
                let mut state = self.state.lock().expect("queue");
                state.shown_secrets += 1;
                if permit.allowed(&self.owner) {
                    state.copied_secrets += 1;
                }
            });
        }
        for task in confirmations {
            match task {
                ConfirmationTask::Legacy(nonce, _request) => {
                    self.state.lock().expect("queue").shown_confirmations += 1;
                    self.registry.complete(nonce, true);
                }
                ConfirmationTask::Scoped(task) => {
                    task.run(&self.registry, |_, _, permit| {
                        self.state.lock().expect("queue").shown_confirmations += 1;
                        permit.complete(&self.registry, true);
                    });
                }
            }
        }
    }
}
impl desktop_core::SecretPresenter for QueuedNative {
    fn reveal(&self, label: String, secret: zeroize::Zeroizing<String>) {
        self.state
            .lock()
            .expect("queue")
            .secrets
            .push(SecretTask::Legacy(label, secret));
    }
    fn reveal_scoped(
        &self,
        label: String,
        secret: zeroize::Zeroizing<String>,
        lease: desktop_core::PresentationLease,
    ) {
        let task = desktop_host::native_confirmation::SecretPresentation::new(label, secret, lease);
        self.state
            .lock()
            .expect("queue")
            .secrets
            .push(SecretTask::Scoped(task));
    }
    fn dismiss(&self) {
        self.owner.begin();
    }
}
impl desktop_core::OperatorConfirmation for QueuedNative {
    fn confirm(
        &self,
        nonce: u64,
        request: desktop_core::ConfirmationRequest,
    ) -> futures_util::future::BoxFuture<'static, bool> {
        if !self
            .queue_confirms
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.confirm_entries
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            return Box::pin(async { true });
        }
        let receiver = self.registry.open(nonce);
        self.state
            .lock()
            .expect("queue")
            .confirmations
            .push(ConfirmationTask::Legacy(nonce, request));
        self.confirm_entries
            .fetch_add(1, std::sync::atomic::Ordering::Release);
        Box::pin(desktop_host::native_confirmation::outcome(receiver))
    }
    fn confirm_scoped(
        &self,
        nonce: u64,
        request: desktop_core::ConfirmationRequest,
        lease: desktop_core::PresentationLease,
    ) -> futures_util::future::BoxFuture<'static, bool> {
        if !lease.is_current() {
            self.confirm_entries
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            return Box::pin(async { false });
        }
        if !self
            .queue_confirms
            .load(std::sync::atomic::Ordering::Acquire)
        {
            self.confirm_entries
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            return Box::pin(async { true });
        }
        let receiver = self.registry.open(nonce);
        let task =
            desktop_host::native_confirmation::ConfirmationPresentation::new(nonce, request, lease);
        self.state
            .lock()
            .expect("queue")
            .confirmations
            .push(ConfirmationTask::Scoped(task));
        self.confirm_entries
            .fetch_add(1, std::sync::atomic::Ordering::Release);
        Box::pin(desktop_host::native_confirmation::outcome(receiver))
    }
    fn dismiss(&self, nonce: u64) {
        self.registry.complete(nonce, false);
    }
}

async fn cross_teardown(secret: bool, before_effect: bool) {
    let native = Arc::new(QueuedNative::default());
    let mut desk = Desk::with_presenter(false, Some(Arc::clone(&native))).await;
    let review = desk.review().await;
    let original = if secret {
        desk.apply(&review);
        Some(desk.settled().await)
    } else {
        native
            .queue_confirms
            .store(true, std::sync::atomic::Ordering::Release);
        None
    };
    let confirm_before = native
        .confirm_entries
        .load(std::sync::atomic::Ordering::Acquire);
    let gate = before_effect.then(|| desk.core.operator().hold_next_native_presentation());
    let intent = if let Some(result) = &original {
        json!({"kind":"revealSecret","operation":result.operation})
    } else {
        json!({"kind":"requestApply","review":review.review,"draft":draft(),"confirmation":"CREATE"})
    };
    let dispatch = desk.send_thread(intent);
    if let Some(gate) = &gate {
        assert!(
            eventually(|| gate.entered()).await,
            "real admission completed; effect not executed"
        );
    } else {
        assert!(
            eventually(|| if secret {
                native.secret_count() == 1
            } else {
                native.confirmation_count() == 1
            })
            .await,
            "real native callback queued"
        );
    }
    desk.retire();
    desk.register();
    if let Some(gate) = gate {
        gate.release();
    }
    dispatch
        .join()
        .expect("dispatch thread")
        .expect("original admission receipt");
    if !secret {
        assert!(
            eventually(|| native
                .confirm_entries
                .load(std::sync::atomic::Ordering::Acquire)
                > confirm_before)
            .await,
            "native adapter has made its queue/refusal decision"
        );
    }
    native.drain();
    {
        let state = native.state.lock().expect("queue");
        assert_eq!(
            state.shown_secrets, 0,
            "departed page's admitted/queued key must never be presented"
        );
        assert_eq!(
            state.copied_secrets, 0,
            "departed page's key must never be copied"
        );
        assert_eq!(
            state.shown_confirmations, 0,
            "departed page's confirmation must never be presented"
        );
    }
    assert_eq!(native.registry.open_count(), 0, "no stale pending nonce");
    assert_eq!(desk.writes(), usize::from(secret), "no mutation replay");
    if let Some(original) = original {
        let result = &desk.snap().changes.results[0];
        assert_eq!(result.operation, original.operation);
        assert_eq!(
            result.outcome,
            Some(desktop_core::runtime::Outcome::Completed)
        );
        assert!(
            desk.send(json!({"kind":"revealSecret","operation":original.operation}))
                .is_err()
        );
    }
    native
        .queue_confirms
        .store(false, std::sync::atomic::Ordering::Release);
    let fresh = desk.review_draft(draft_named("Support key 3")).await;
    desk.apply_draft(&fresh, draft_named("Support key 3"));
    let fresh = desk.settled().await;
    assert_eq!(
        fresh.outcome,
        Some(desktop_core::runtime::Outcome::Completed)
    );
    assert_eq!(
        desk.writes(),
        usize::from(secret) + 1,
        "only explicit fresh-page work writes"
    );
    desk.send(json!({"kind":"revealSecret","operation":fresh.operation}))
        .expect("fresh reveal");
    native.drain();
    let state = native.state.lock().expect("queue");
    assert_eq!(state.shown_secrets, 1, "fresh-page presentation works");
    assert_eq!(state.copied_secrets, 1, "fresh-page explicit copy works");
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_secret_admitted_before_teardown_cannot_present_after_effect_release() {
    cross_teardown(true, true).await;
}
#[tokio::test(flavor = "multi_thread")]
async fn scoped_secret_queued_before_teardown_cannot_present_or_copy() {
    cross_teardown(true, false).await;
}
#[tokio::test(flavor = "multi_thread")]
async fn scoped_confirmation_admitted_before_teardown_cannot_present_after_effect_release() {
    cross_teardown(false, true).await;
}
#[tokio::test(flavor = "multi_thread")]
async fn scoped_confirmation_queued_before_teardown_cannot_present() {
    cross_teardown(false, false).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_review_exit_revokes_a_queued_secret_without_retiring_the_page() {
    let native = Arc::new(QueuedNative::default());
    let mut desk = Desk::with_presenter(false, Some(Arc::clone(&native))).await;
    let review = desk.review().await;
    desk.apply(&review);
    let result = desk.settled().await;
    desk.send(json!({"kind":"revealSecret","operation":result.operation}))
        .expect("admitted reveal");
    assert_eq!(native.secret_count(), 1);
    desk.send(json!({"kind":"discardChange","review":review.review}))
        .expect("review exit");
    native.drain();
    let state = native.state.lock().expect("queue");
    assert_eq!(state.shown_secrets, 0);
    assert_eq!(state.copied_secrets, 0);
    assert_eq!(desk.writes(), 1);
    assert_eq!(
        desk.snap().changes.results[0].outcome,
        Some(desktop_core::runtime::Outcome::Completed)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn scoped_old_callbacks_drained_last_cannot_retire_fresh_page_presentation() {
    let native = Arc::new(QueuedNative::default());
    let mut desk = Desk::with_presenter(false, Some(Arc::clone(&native))).await;
    let review = desk.review().await;
    desk.apply(&review);
    let result = desk.settled().await;
    desk.send(json!({"kind":"revealSecret","operation":result.operation}))
        .expect("old admitted reveal");
    desk.retire();
    desk.register();
    let fresh = desk.review_draft(draft_named("Support key 3")).await;
    desk.apply_draft(&fresh, draft_named("Support key 3"));
    let result = desk.settled().await;
    desk.send(json!({"kind":"revealSecret","operation":result.operation}))
        .expect("new admitted reveal");
    native.state.lock().expect("queue").secrets.reverse();
    let prior_generation = native.owner.begin();
    native.drain();
    let state = native.state.lock().expect("queue");
    assert_eq!(state.shown_secrets, 1, "only the fresh callback presents");
    assert_eq!(state.copied_secrets, 1, "only its copy is authorized");
    assert!(
        native.owner.owns(prior_generation + 1),
        "stale callback allocated no new sheet ownership"
    );
    assert_eq!(
        desk.writes(),
        2,
        "one original and one explicit fresh write"
    );
}
