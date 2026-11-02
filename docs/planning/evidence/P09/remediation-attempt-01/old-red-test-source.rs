//! P09 integrated API Client mutation review lifecycle: production Host/IPC,
//! real authority/HTTPS dispatch and the same queued presentation permits
//! used by AppKit. Synthetic data/services only; no renderer cleanup timer.
#[allow(dead_code)]
#[path = "../../../../crates/desktop-core/tests/context_credentials/support.rs"]
mod support;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::ManagementEndpoints;
use desktop_core::runtime::OperationId;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{Core, CoreConfig, Observation, ShellSnapshot, ViewScope};
use desktop_host::native_confirmation::{
    ConfirmationPermit, ConfirmationPresentation, Registry, outcome,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use support::{Reply, TestCa, TlsServer, eventually};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow};

#[derive(Default)]
struct QueuedConfirmations {
    registry: Registry,
    queued: Mutex<Vec<ConfirmationPresentation>>,
    shown: Mutex<BTreeMap<u64, ConfirmationPermit>>,
}
impl QueuedConfirmations {
    fn drain(&self) {
        for task in std::mem::take(&mut *self.queued.lock().expect("queued")) {
            task.run(&self.registry, |nonce, _request, permit| {
                self.shown.lock().expect("shown").insert(nonce, permit);
            });
        }
    }
    fn answer(&self, accepted: bool) {
        for (_, permit) in std::mem::take(&mut *self.shown.lock().expect("shown")) {
            permit.complete(&self.registry, accepted);
        }
    }
    fn queued(&self) -> usize {
        self.queued.lock().expect("queue").len()
    }
    fn shown(&self) -> usize {
        self.shown.lock().expect("shown").len()
    }
}
impl desktop_core::OperatorConfirmation for QueuedConfirmations {
    fn confirm(
        &self,
        nonce: u64,
        request: desktop_core::ConfirmationRequest,
    ) -> futures_util::future::BoxFuture<'static, bool> {
        // Matches production unscoped confirmation: a fresh live lease.
        self.confirm_scoped(nonce, request, desktop_core::PresentationLease::default())
    }
    fn confirm_scoped(
        &self,
        nonce: u64,
        request: desktop_core::ConfirmationRequest,
        lease: desktop_core::PresentationLease,
    ) -> futures_util::future::BoxFuture<'static, bool> {
        if !lease.is_current() {
            return Box::pin(async { false });
        }
        let receiver = self.registry.open(nonce);
        self.queued
            .lock()
            .expect("queue")
            .push(ConfirmationPresentation::new(nonce, request, lease));
        Box::pin(outcome(receiver))
    }
    fn dismiss(&self, nonce: u64) {
        self.registry.complete(nonce, false);
        self.shown.lock().expect("shown").remove(&nonce);
    }
}

struct Desk {
    core: Core,
    app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    adapters: Handles,
    native: Arc<QueuedConfirmations>,
    writes: Arc<AtomicUsize>,
    _server: TlsServer,
    _ca: TestCa,
    root: PathBuf,
    session: String,
    sequence: u64,
}
impl Desk {
    async fn new(delay: u64) -> Self {
        let ca = TestCa::new("P09 Workbench page fixture");
        let writes = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&writes);
        let server = TlsServer::start(
            ca.leaf(&["localhost"]),
            Reply::Route(Arc::new(move |seen| {
                if seen
                    .request_line
                    .starts_with("DELETE /iam/v1/access_policies/")
                {
                    count.fetch_add(1, Ordering::AcqRel);
                    let reply = Reply::ok(b"{\"deleted\":true}");
                    if delay > 0 {
                        Reply::Delayed {
                            millis: delay,
                            then: Box::new(reply),
                        }
                    } else {
                        reply
                    }
                } else {
                    Reply::ok(b"{}")
                }
            })),
        )
        .await;
        let origin = format!("https://localhost:{}", server.port);
        let (mut adapters, handles) = fixtures::adapters();
        let native = Arc::new(QueuedConfirmations::default());
        adapters.confirmer = native.clone();
        let core = Core::new(
            CoreConfig {
                management: ManagementEndpoints::at_origin(&origin),
                workbench_api_origins: vec![origin.clone()],
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
        .expect("transport");
        let app = desktop_host::install(mock_builder(), &core)
            .build(tauri::generate_context!())
            .expect("production host");
        let window = desktop_host::main_window(&app).expect("main window");
        let root = handles.workbench_root.join("page-review-repository");
        std::fs::create_dir_all(root.join(".git")).expect("fixture repo");
        std::fs::write(
            root.join("collection.toml"),
            "format_version = 1\nid = \"page-lifecycle\"\nname = \"Page lifecycle\"\n",
        )
        .expect("collection");
        std::fs::write(root.join("change.request.toml"),format!("format_version = 1\nid = \"change\"\nmethod = \"DELETE\"\nurl = \"{origin}/iam/v1/access_policies/synthetic\"\n")).expect("request");
        let mut desk = Self {
            core,
            app,
            window,
            adapters: handles,
            native,
            writes,
            _server: server,
            _ca: ca,
            root,
            session: String::new(),
            sequence: 0,
        };
        desk.register();
        desk.send(json!({"kind":"createTenant","profile":{"label":"Tenant A","tsgId":"1234567890","destination":{"kind":"custom","origin":origin}}})).expect("tenant");
        desk
    }
    fn call(&self, cmd: &str, body: Value) -> Result<Value, Value> {
        get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().expect("origin"),
                body: InvokeBody::Raw(serde_json::to_vec(&body).expect("JSON")),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_owned(),
            },
        )
        .map(|r| r.deserialize::<Value>().expect("reply"))
    }
    fn register(&mut self) {
        let r = self
            .call(
                "desktop_observe",
                json!({"protocol":1,"scope":{"kind":"deliverySession"}}),
            )
            .expect("session");
        self.session = r["session"].as_str().expect("nonce").to_owned();
        self.sequence = 0;
    }
    fn send(&mut self, intent: Value) -> Result<Value, Value> {
        self.sequence += 1;
        self.call("desktop_dispatch",json!({"protocol":1,"session":self.session,"sequence":self.sequence.to_string(),"intent":intent}))
    }
    fn start(&mut self, request: Value) -> OperationId {
        let r = self
            .send(json!({"kind":"workbench","request":request}))
            .expect("IPC admission");
        serde_json::from_value(r["operation"].clone()).expect("operation")
    }
    async fn wait(&self, id: OperationId) -> Value {
        let Observation::Workbench(task) = self
            .core
            .operator()
            .observe(ViewScope::Workbench(id))
            .expect("operation observation")
        else {
            panic!("workbench observation")
        };
        serde_json::to_value(task.result().await).expect("result")
    }
    async fn wb(&mut self, request: Value) -> Value {
        let op = self.start(request);
        self.wait(op).await
    }
    fn snap(&self) -> ShellSnapshot {
        let Observation::Shell { snapshot, .. } = self
            .core
            .operator()
            .observe(ViewScope::Shell)
            .expect("snapshot")
        else {
            panic!("shell")
        };
        snapshot
    }
    async fn review(&mut self) -> (Value, Value) {
        if self.snap().workbench.collection.is_none() {
            self.adapters.folders.reply(Some(self.root.clone()));
            assert_eq!(
                self.wb(json!({"kind":"openFolder"})).await["kind"],
                "opened"
            );
        }
        let doc = self
            .wb(json!({"kind":"openRequest","relative":"change.request.toml"}))
            .await["document"]
            .clone();
        let context = self.snap().active.map(|a| a.context);
        let r=self.wb(json!({"kind":"send","id":doc["id"],"revision":doc["revision"],"fields":doc["fields"],"context":context})).await;
        assert_eq!(r["kind"], "reviewRequired", "{r}");
        (doc, r["review"].clone())
    }
    fn apply(&mut self, doc: &Value, review: &Value) -> OperationId {
        let context = self.snap().active.map(|a| a.context);
        self.start(json!({"kind":"apply","review":review["review"],"id":doc["id"],"revision":doc["revision"],"fields":doc["fields"],"context":context}))
    }
    fn reload(&mut self) {
        self.app
            .state::<desktop_host::Host>()
            .retire_webview("main");
        self.register();
    }
}

async fn retired_page(shown: bool) {
    let mut desk = Desk::new(0).await;
    let (doc, review) = desk.review().await;
    let op = desk.apply(&doc, &review);
    assert!(
        eventually(|| desk.native.queued() == 1).await,
        "native effect registered"
    );
    if shown {
        desk.native.drain();
        assert_eq!(desk.native.shown(), 1);
    }
    desk.reload();
    desk.native.drain();
    let stale_shown = desk.native.shown();
    desk.native.answer(true);
    let result = desk.wait(op).await;
    assert_eq!(
        desk.writes.load(Ordering::Acquire),
        0,
        "retired page must never authorize a mutation: {result}"
    );
    assert_eq!(
        stale_shown, 0,
        "retired review must not retain/show its native sheet"
    );
    assert!(
        desk.snap().workbench.review.is_none(),
        "retired review gone"
    );
    let (doc, review) = desk.review().await;
    let fresh = desk.apply(&doc, &review);
    assert!(eventually(|| desk.native.queued() > 0).await);
    desk.native.drain();
    assert_eq!(desk.native.shown(), 1, "explicit fresh-page review works");
    desk.native.answer(true);
    let result = desk.wait(fresh).await;
    assert_eq!(result["kind"], "sent", "{result}");
    assert_eq!(
        desk.writes.load(Ordering::Acquire),
        1,
        "fresh request sent exactly once"
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_mutation_confirmation_is_revoked_by_main_page_reload() {
    retired_page(false).await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shown_mutation_confirmation_is_revoked_by_main_page_reload() {
    retired_page(true).await;
}
