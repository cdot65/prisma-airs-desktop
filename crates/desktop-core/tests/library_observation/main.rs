//! C12 case Library and Runtime Home/Inbox observations through the
//! production authority interface: five-product authoring with v1's limits,
//! sealed per-tenant storage that survives a restart and goes with its
//! tenant or a reset, explicit import/export and catalog imports (bundled
//! and a public snapshot server), copies between tenants, and the Runtime
//! overview: chart and key sources summarized without fabricated zeros,
//! Inbox items, gaps, freshness, attention holds and refresh failures.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::library::CaseProblem;
use desktop_core::management::{ManagementEndpoints, ManagementFailure};
use desktop_core::observation::Tone;
use desktop_core::store::PersistedState;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AttentionFace, AuthorityHandle, Core, CoreConfig, Delivery, ExportState, Intent, LibraryNotice,
    LibraryState, Observation, Receipt, Refusal, ScanState, ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c12";
const TSG: &str = "1234567890";
const CATALOG_PATH: &str = "/cdot65/prisma-airs-test-cases/main/catalog.json";
const PROMPT: &str = "SYNTHETIC-CASE-PROMPT-c12";

/// What the synthetic endpoints answer.
struct Script {
    chart: Option<Value>,
    chart_status: u16,
    keys: Vec<Value>,
    keys_status: u16,
    catalog: Option<Value>,
    down: bool,
}

impl Script {
    fn route(&mut self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        if self.down {
            return Reply::Close;
        }
        if path == "/oauth2/access_token" {
            return raw(
                200,
                &json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        if path == "/aisec/v1/mgmt/dashboard/v2/sessions/sessionschart" {
            return match &self.chart {
                Some(chart) => raw(self.chart_status, chart),
                None => raw(self.chart_status, &json!(null)),
            };
        }
        if path == format!("/aisec/v1/mgmt/apikeys/tsg/{TSG}") {
            // The SDK's key model needs these fields, as the service sends.
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
        if path == CATALOG_PATH {
            return match &self.catalog {
                Some(catalog) => raw(200, catalog),
                None => raw(404, &json!({"error": "not found"})),
            };
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

fn chart(buckets: Value) -> Value {
    json!({"bucket_size_seconds": 3600, "buckets": buckets})
}

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
    ca: TestCa,
}

async fn desk() -> Desk {
    let ca = TestCa::new("airs-v2 c12 ca");
    let script = Arc::new(Mutex::new(Script {
        chart: Some(chart(json!([
            {"time": "2026-10-19T00:00:00Z", "total": 10, "violating": 3,
             "detection_type_violation_breakdown": [{"detection_type": "pi", "violation_breakdown": {"total": 3}}]},
            {"time": "2026-10-19T01:00:00Z", "total": 5, "violating": 0,
             "detection_type_violation_breakdown": []}]))),
        chart_status: 200,
        keys: vec![],
        keys_status: 200,
        catalog: None,
        down: false,
    }));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("script").route(seen)
        })),
    )
    .await;
    let mut desk = start(ca, server, script, None).await;
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    desk
}

async fn start(
    ca: TestCa,
    server: TlsServer,
    script: Arc<Mutex<Script>>,
    restart: Option<(Handles, PersistedState)>,
) -> Desk {
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = match restart {
        None => fixtures::adapters(),
        Some((old, state)) => {
            let (mut adapters, mut handles) =
                fixtures::adapters_with(fixtures::ScriptedStore::with_state(state));
            adapters.keychain = old.vault.clone();
            handles.vault = old.vault;
            adapters.library_root = Some(old.library_root.clone());
            handles.library_root = old.library_root;
            (adapters, handles)
        }
    };
    let config = CoreConfig {
        management: ManagementEndpoints::at_origin(&origin),
        case_catalog: url::Url::parse(&format!("{origin}{CATALOG_PATH}")).expect("url"),
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
    Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        script,
        server,
        ca,
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

    async fn open_library(&mut self) {
        let context = self.context();
        self.send(json!({"kind": "openLibrary", "context": context}))
            .await
            .expect("open");
        assert!(eventually(|| self.snap().library.state == LibraryState::Ready).await);
    }

    async fn save(&mut self, draft: Value) -> Result<Receipt, Refusal> {
        let context = self.context();
        self.send(json!({"kind": "saveCase", "context": context, "draft": draft}))
            .await
    }

    async fn management_key(&mut self) {
        let context = self.context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        assert!(eventually(|| self.snap().tenants[0].management.state == ScanState::Ready).await);
    }

    async fn refresh(&mut self) -> desktop_core::OverviewView {
        let context = self.context();
        let before = self.snap().overview.read.map(|r| r.read_at);
        let receipt = self
            .send(
                json!({"kind": "management", "context": context, "request": {"kind": "overview"}}),
            )
            .await
            .expect("admitted");
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = receipt
        else {
            panic!("no operation");
        };
        let Ok(Observation::Management(handle)) =
            self.handle.observe(ViewScope::Management(operation))
        else {
            panic!("observation");
        };
        tokio::time::timeout(Duration::from_secs(20), handle.result())
            .await
            .expect("result");
        assert!(eventually(|| !self.snap().overview.refreshing).await);
        let _ = before;
        self.snap().overview
    }

    fn library_files(&self) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(&self.adapters.library_root)
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default()
    }

    async fn restart(self) -> Desk {
        let state = self.adapters.store.saved().expect("saved");
        let Desk {
            adapters,
            script,
            server,
            ca,
            core,
            ..
        } = self;
        core.shutdown();
        drop(core);
        start(ca, server, script, Some((adapters, state))).await
    }
}

fn runtime(name: &str) -> Value {
    json!({"product": "runtime-security", "name": name, "profile": "prod", "prompt": PROMPT,
           "response": "", "appName": "Desktop examples", "expected": "A test goal", "payload": ""})
}

fn spec(product: &str, name: &str, payload: &str) -> Value {
    json!({"product": product, "name": name, "profile": "ignored", "prompt": "A specification",
           "response": "", "appName": "ignored", "expected": "", "payload": payload})
}

#[tokio::test(flavor = "multi_thread")]
async fn cases_are_authored_sealed_and_survive_a_restart() {
    let mut desk = desk().await;
    desk.open_library().await;
    desk.save(runtime("Benign question"))
        .await
        .expect("runtime");
    desk.save(spec("model-security", "Provenance", "{\"artifact\":\"m\"}"))
        .await
        .expect("spec");
    let cases = desk.snap().library.cases;
    assert_eq!(cases.len(), 2);
    assert_eq!(cases[1].profile, "", "specifications carry no profile");
    assert_eq!(cases[1].app_name, "");
    assert!(
        cases[1]
            .payload
            .as_deref()
            .is_some_and(|p| p.contains("artifact"))
    );
    assert!(cases[0].payload.is_none(), "runtime cases carry no payload");
    // Sealed, owner-only, with no plaintext; one derived Keychain key.
    assert!(eventually(|| desk.library_files().len() == 1).await);
    let file = &desk.library_files()[0];
    let bytes = std::fs::read(file).expect("file");
    assert!(!String::from_utf8_lossy(&bytes).contains(PROMPT));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(file).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert_eq!(desk.adapters.vault.records().len(), 1);
    let id = cases[0].id.clone();
    let context = desk.context();
    desk.send(json!({"kind": "deleteCase", "context": context, "id": id}))
        .await
        .expect("deleted");
    assert_eq!(desk.snap().library.cases.len(), 1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut desk = desk.restart().await;
    desk.open_library().await;
    let cases = desk.snap().library.cases;
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].name, "Provenance");
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_payloads_and_unknown_types_are_refused() {
    let mut desk = desk().await;
    desk.open_library().await;
    assert_eq!(
        desk.save(spec("ai-gateway", "Bad payload", "[1,2]")).await,
        Err(Refusal::InvalidInput)
    );
    assert_eq!(desk.snap().library.problem, Some(CaseProblem::Payload));
    let mut nameless = runtime("x");
    nameless["name"] = json!("  ");
    assert_eq!(desk.save(nameless).await, Err(Refusal::InvalidInput));
    assert_eq!(desk.snap().library.problem, Some(CaseProblem::Fields));
    let mut no_profile = runtime("x");
    no_profile["profile"] = json!("");
    assert_eq!(desk.save(no_profile).await, Err(Refusal::InvalidInput));
    // An unknown product is not an intent the desktop accepts.
    let context = desk.context();
    let unknown = json!({"kind": "saveCase", "context": context,
        "draft": spec("prisma-access", "Unknown", "")});
    assert!(serde_json::from_value::<Intent>(unknown).is_err());
    for n in 0..32 {
        desk.save(runtime(&format!("Case {n}")))
            .await
            .expect("saved");
    }
    assert_eq!(
        desk.save(runtime("One too many")).await,
        Err(Refusal::InvalidInput)
    );
    assert_eq!(desk.snap().library.problem, Some(CaseProblem::Full));
    assert_eq!(desk.snap().library.cases.len(), 32);
}

#[tokio::test(flavor = "multi_thread")]
async fn exports_and_imports_are_explicit_bounded_and_tenant_bound() {
    let mut desk = desk().await;
    desk.open_library().await;
    desk.save(runtime("Exported")).await.expect("saved");
    let context = desk.context();
    desk.send(json!({"kind": "exportCases", "context": context}))
        .await
        .expect("export");
    assert!(
        eventually(|| desk
            .snap()
            .library
            .export
            .is_some_and(|e| e.state == ExportState::Saved))
        .await
    );
    assert!(
        desk.snap().sessions.export.is_none(),
        "kept apart from Sessions"
    );
    let (name, bytes) = desk.adapters.saver.saved().pop().expect("saved");
    assert_eq!(name, "prisma-airs-test-cases.json");
    let file: Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(file["version"], 2);
    let id = desk.snap().library.cases[0].id.clone();
    desk.send(json!({"kind": "deleteCase", "context": context, "id": id}))
        .await
        .expect("deleted");
    // Importing the export restores it, once.
    for _ in 0..2 {
        desk.adapters
            .case_files
            .reply(Some(Ok(("cases.json".into(), bytes.clone()))));
        desk.send(json!({"kind": "importCases", "context": context}))
            .await
            .expect("import");
        assert!(eventually(|| !desk.snap().library.choosing).await);
    }
    assert_eq!(desk.snap().library.cases.len(), 1);
    assert!(
        eventually(|| desk.snap().library.notice == Some(LibraryNotice::Imported { added: 0 }))
            .await
    );
    // Another tenant's export is refused.
    let mut foreign = file.clone();
    foreign["tenant"] = json!("tn-ffffffffffffffff");
    desk.adapters.case_files.reply(Some(Ok((
        "foreign.json".into(),
        serde_json::to_vec(&foreign).expect("json"),
    ))));
    desk.send(json!({"kind": "importCases", "context": context}))
        .await
        .expect("import");
    assert!(eventually(|| desk.snap().library.problem == Some(CaseProblem::Format)).await);
    // A cancelled sheet changes nothing.
    desk.adapters.case_files.reply(None);
    desk.send(json!({"kind": "importCases", "context": context}))
        .await
        .expect("import");
    assert!(eventually(|| !desk.snap().library.choosing).await);
    assert_eq!(desk.snap().library.cases.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn catalogs_import_examples_once_and_report_unusable_catalogs() {
    let mut desk = desk().await;
    desk.open_library().await;
    let context = desk.context();
    desk.send(json!({"kind": "importExamples", "context": context, "source": "bundled"}))
        .await
        .expect("bundled");
    assert_eq!(desk.snap().library.cases.len(), 11);
    assert!(
        eventually(
            || desk.snap().library.notice == Some(LibraryNotice::ExamplesAdded { added: 11 })
        )
        .await
    );
    assert_eq!(
        desk.send(json!({"kind": "importExamples", "context": context, "source": "bundled"}))
            .await,
        Err(Refusal::InvalidInput)
    );
    assert_eq!(desk.snap().library.problem, Some(CaseProblem::AllPresent));
    // The public catalog (a local snapshot server here).
    desk.script().catalog = Some(
        json!({"version": 1, "kind": "prisma-airs-test-case-examples",
        "examples": [{"product": "skill-scanning", "name": "New public example", "prompt": "Spec",
                      "expected": "A goal"}]}),
    );
    desk.send(json!({"kind": "importExamples", "context": context, "source": "public"}))
        .await
        .expect("public");
    assert!(eventually(|| !desk.snap().library.fetching).await);
    assert_eq!(desk.snap().library.cases.len(), 12);
    // A malformed catalog leaves the library unchanged.
    desk.script().catalog = Some(
        json!({"version": 1, "kind": "prisma-airs-test-case-examples",
        "examples": [], "api_key": "x"}),
    );
    desk.send(json!({"kind": "importExamples", "context": context, "source": "public"}))
        .await
        .expect("public");
    assert!(eventually(|| desk.snap().library.problem == Some(CaseProblem::Catalog)).await);
    assert_eq!(desk.snap().library.cases.len(), 12);
    // An unreachable catalog is reported, not retried.
    desk.script().down = true;
    desk.send(json!({"kind": "importExamples", "context": context, "source": "public"}))
        .await
        .expect("public");
    assert!(
        eventually(|| desk.snap().library.notice == Some(LibraryNotice::CatalogUnavailable)).await
    );
    let seen = desk.server.seen().len();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(desk.server.seen().len(), seen, "no retry");
    // Catalog reads never carry a credential.
    assert!(
        desk.server
            .seen()
            .iter()
            .filter(|s| s.request_line.contains(CATALOG_PATH))
            .all(|s| s.header("authorization").is_none() && s.header("x-pan-token").is_none())
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_case_copied_to_another_tenant_is_a_new_case_there() {
    let mut desk = desk().await;
    desk.open_library().await;
    desk.save(runtime("Shared")).await.expect("saved");
    let id = desk.snap().library.cases[0].id.clone();
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant b");
    let b = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("b")
        .id
        .clone();
    let a = desk.snap().active.expect("active").tenant;
    let context = desk.context();
    desk.send(json!({"kind": "copyCase", "context": context, "id": id, "tenant": b}))
        .await
        .expect("copied");
    assert!(
        eventually(|| matches!(
            desk.snap().library.notice,
            Some(LibraryNotice::Copied { .. })
        ))
        .await
    );
    // Copying into the same tenant is refused.
    assert_eq!(
        desk.send(json!({"kind": "copyCase", "context": context, "id": id, "tenant": a}))
            .await,
        Err(Refusal::InvalidInput)
    );
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("select");
    desk.open_library().await;
    let cases = desk.snap().library.cases;
    assert_eq!(cases.len(), 1);
    assert_eq!(cases[0].name, "Shared");
    assert_ne!(cases[0].id, id, "a new identifier");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_deleted_tenant_or_reset_takes_its_sealed_library_and_key() {
    let mut desk = desk().await;
    desk.open_library().await;
    desk.save(runtime("Doomed")).await.expect("saved");
    assert!(eventually(|| desk.library_files().len() == 1).await);
    let tenant = desk.snap().tenants[0].id.clone();
    desk.send(json!({"kind": "deleteTenant", "tenant": tenant}))
        .await
        .expect("deleted");
    assert!(eventually(|| desk.library_files().is_empty()).await);
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
    // Reset removes the whole library root.
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant C", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant c");
    let c = desk.snap().tenants[0].id.clone();
    desk.send(json!({"kind": "selectTenant", "tenant": c}))
        .await
        .expect("select");
    desk.open_library().await;
    desk.save(runtime("Reset me")).await.expect("saved");
    assert!(eventually(|| desk.library_files().len() == 1).await);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| !desk.adapters.library_root.exists()).await);
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_locked_keychain_or_unreadable_library_is_reported_and_kept() {
    let mut desk = desk().await;
    desk.open_library().await;
    desk.save(runtime("Kept")).await.expect("saved");
    assert!(eventually(|| desk.library_files().len() == 1).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.adapters
        .vault
        .fail_with(Some(desktop_core::vault::VaultError::Locked));
    let mut desk = desk.restart().await;
    let context = desk.context();
    desk.send(json!({"kind": "openLibrary", "context": context}))
        .await
        .expect("open");
    assert!(eventually(|| desk.snap().library.state == LibraryState::Failed).await);
    assert_eq!(
        desk.snap().library.failure,
        Some(desktop_core::library::LibraryFailure::Locked)
    );
    assert_eq!(desk.library_files().len(), 1, "nothing removed");
    // An explicit retry once unlocked opens it.
    desk.adapters.vault.fail_with(None);
    desk.send(json!({"kind": "openLibrary", "context": context}))
        .await
        .expect("open");
    assert!(eventually(|| desk.snap().library.state == LibraryState::Ready).await);
    assert_eq!(desk.snap().library.cases[0].name, "Kept");
    // A library that no longer opens is set aside and reported.
    let file = desk.library_files()[0].clone();
    std::fs::write(&file, b"not sealed").expect("corrupt");
    let mut desk = desk.restart().await;
    let context = desk.context();
    desk.send(json!({"kind": "openLibrary", "context": context}))
        .await
        .expect("open");
    assert!(eventually(|| desk.snap().library.state == LibraryState::Failed).await);
    assert_eq!(
        desk.snap().library.failure,
        Some(desktop_core::library::LibraryFailure::Unreadable)
    );
    assert!(file.with_extension("unreadable").exists(), "kept aside");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_overview_summarizes_sources_without_fabricating_zeros_or_completeness() {
    let mut desk = desk().await;
    desk.management_key().await;
    let expiring = desktop_core::sessions::rfc3339(now() + 5 * 86_400 + 60);
    desk.script().keys = vec![
        json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring,
               "api_key": "SYNTHETIC-KEY-NEVER-SHOWN"}),
    ];
    assert_eq!(desk.snap().overview.face, AttentionFace::FirstLaunch);
    let view = desk.refresh().await;
    let read = view.read.clone().expect("read");
    assert_eq!(read.summary.sessions, Some(15));
    assert_eq!(read.summary.violating, Some(3));
    assert_eq!(read.summary.benign, Some(12));
    assert_eq!(read.summary.detection_events, Some(3));
    assert!(
        view.fresh && read.keys_complete && read.gaps.is_empty(),
        "{read:?} fresh={}",
        view.fresh
    );
    let ids: Vec<&str> = read.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["runtime-activity", "runtime-key:k1"]);
    assert_eq!(read.items[1].title, "Runtime expires in 5 days");
    assert!(
        !serde_json::to_string(&read)
            .expect("json")
            .contains("SYNTHETIC-KEY-NEVER-SHOWN")
    );
    assert_eq!(view.face, AttentionFace::Items);
    assert_eq!(view.attention.len(), 2);
    // A failed chart is a gap with its failure; the keys still answer.
    desk.script().chart_status = 503;
    let view = desk.refresh().await;
    let read = view.read.clone().expect("read");
    assert_eq!(read.chart, None);
    assert_eq!(read.chart_failure, Some(ManagementFailure::Unavailable));
    assert_eq!(read.summary.sessions, None, "unknown, not zero");
    assert!(
        read.gaps
            .contains(&"Runtime activity unavailable".to_owned())
    );
    assert_eq!(read.items.len(), 1);
    // A healthy empty day: zeros are reported because the service said so.
    desk.script().chart_status = 200;
    desk.script().chart = Some(chart(json!([])));
    desk.script().keys = vec![];
    let view = desk.refresh().await;
    let read = view.read.clone().expect("read");
    assert_eq!(read.summary.sessions, Some(0));
    assert_eq!(view.face, AttentionFace::AllClear);
    // A breakdown missing while violating makes detector counts unknown.
    desk.script().chart = Some(chart(json!([{"time": "2026-10-19T00:00:00Z", "total": 2,
        "violating": 1, "detection_type_violation_breakdown": []}])));
    let read = desk.refresh().await.read.expect("read");
    assert_eq!(read.summary.detection_events, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_refresh_keeps_the_last_read_and_holds_follow_the_evidence() {
    let mut desk = desk().await;
    desk.management_key().await;
    let expiring = desktop_core::sessions::rfc3339(now() + 3 * 86_400 + 60);
    desk.script().keys =
        vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring})];
    let first = desk.refresh().await.read.expect("read");
    let context = desk.context();
    desk.send(json!({"kind": "holdInsight", "context": context, "id": "runtime-key:k1", "hold": "acknowledge"}))
        .await
        .expect("acknowledged");
    desk.send(json!({"kind": "holdInsight", "context": context, "id": "runtime-activity", "hold": "snooze"}))
        .await
        .expect("snoozed");
    let view = desk.snap().overview;
    assert!(view.attention.is_empty());
    assert_eq!(view.held.len(), 2);
    // Everything set aside and every source fresh and complete.
    assert_eq!(view.face, AttentionFace::AllClear);
    // The key expires: its level rises above the acknowledgement.
    let expired = desktop_core::sessions::rfc3339(now() - 2 * 86_400);
    desk.script().keys =
        vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expired})];
    let view = desk.refresh().await;
    assert_eq!(view.attention.len(), 1);
    assert_eq!(view.attention[0].tone, Tone::Bad);
    desk.send(json!({"kind": "restoreInsight", "context": context, "id": "runtime-activity"}))
        .await
        .expect("restored");
    assert_eq!(desk.snap().overview.attention.len(), 2);
    // An unreachable service: the last read stays, marked with the failure.
    desk.script().down = true;
    let view = desk.refresh().await;
    assert!(view.failure.is_some() && view.failed_at.is_some());
    let kept = view.read.as_ref().expect("the last read is kept");
    assert!(kept.chart.is_some(), "the last good chart is still shown");
    assert_ne!(view.face, AttentionFace::AllClear);
    let seen = desk.server.seen().len();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(desk.server.seen().len(), seen, "no automatic replay");
    // Only reads reached the service.
    assert!(
        desk.server
            .seen()
            .iter()
            .all(|s| s.request_line.starts_with("GET ")
                || s.request_line.contains("/oauth2/access_token"))
    );
    let _ = first;
    // Holding an item the read does not report is refused.
    assert_eq!(
        desk.send(json!({"kind": "holdInsight", "context": context, "id": "runtime-key:nope", "hold": "snooze"}))
            .await,
        Err(Refusal::InvalidInput)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn imported_runtime_cases_with_a_payload_and_inconsistent_charts_stay_in_contract() {
    let mut desk = desk().await;
    desk.open_library().await;
    let context = desk.context();
    let tenant = desk.snap().tenants[0].id.clone();
    let file = json!({"version": 2, "tenant": tenant, "cases": [
        {"id": "11111111-2222-4333-8444-555555555555", "tenant": tenant, "product": "runtime-security",
         "name": "R", "profile": "p", "prompt": "x", "response": "", "app_name": "a", "payload": {"k": 1}}]});
    desk.adapters.case_files.reply(Some(Ok((
        "cases.json".into(),
        serde_json::to_vec(&file).expect("json"),
    ))));
    desk.send(json!({"kind": "importCases", "context": context}))
        .await
        .expect("import");
    assert!(eventually(|| desk.snap().library.problem == Some(CaseProblem::Payload)).await);
    assert!(desk.snap().library.cases.is_empty(), "nothing stored");
    // An inconsistent chart: its share is unknown and a gap says why.
    desk.management_key().await;
    desk.script().chart = Some(chart(json!([{"time": "2026-10-19T00:00:00Z", "total": 1,
        "violating": 2, "detection_type_violation_breakdown": [
            {"detection_type": "pi", "violation_breakdown": {"total": 2}}]}])));
    let read = desk.refresh().await.read.expect("read");
    assert_eq!(read.summary.benign, None);
    assert!(
        read.items
            .iter()
            .all(|i| i.per_mille.is_none_or(|n| n <= 1000))
    );
    assert!(read.gaps.iter().any(|g| g.contains("inconsistent totals")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_library_cleanup_stays_ledgered_and_finishes_later() {
    let mut desk = desk().await;
    desk.open_library().await;
    desk.save(runtime("Keep me out")).await.expect("saved");
    assert!(eventually(|| desk.library_files().len() == 1).await);
    let file = desk.library_files()[0].clone();
    // Neither the key nor the file can be removed: the Keychain denies and
    // a directory stands where the sealed file was.
    desk.adapters
        .vault
        .fail_with(Some(desktop_core::vault::VaultError::Denied));
    std::fs::remove_file(&file).expect("remove");
    std::fs::create_dir_all(file.join("blocker")).expect("blocker");
    let tenant = desk.snap().tenants[0].id.clone();
    desk.send(json!({"kind": "deleteTenant", "tenant": tenant}))
        .await
        .expect("deleted");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let key = desktop_core::library::key_record(tenant.as_str());
    let ledgered = |desk: &Desk| {
        desk.adapters
            .store
            .saved()
            .expect("saved")
            .pending_cleanup
            .contains(&key)
    };
    assert!(ledgered(&desk));
    assert!(
        desk.snap().tenants.is_empty(),
        "the tenant does not come back"
    );
    // Storage access returns but the file still cannot go: still ledgered.
    desk.adapters.vault.fail_with(None);
    let desk = desk.restart().await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(file.exists(), "kept while it cannot be removed");
    assert!(ledgered(&desk));
    std::fs::remove_dir_all(&file).expect("unblock");
    std::fs::write(&file, b"sealed bytes").expect("file back");
    let desk = desk.restart().await;
    assert!(eventually(|| !file.exists()).await);
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
    assert!(
        eventually(|| !desk
            .adapters
            .store
            .saved()
            .is_some_and(|s| s.pending_cleanup.contains(&key)))
        .await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn freshness_and_snooze_deadlines_are_published_without_any_event() {
    let mut desk = desk().await;
    desk.management_key().await;
    desk.script().chart = Some(chart(json!([])));
    let expiring = desktop_core::sessions::rfc3339(now() + 3 * 86_400 + 60);
    desk.script().keys =
        vec![json!({"api_key_id": "k1", "api_key_name": "Runtime", "expiration": expiring})];
    let view = desk.refresh().await;
    assert!(view.fresh);
    let context = desk.context();
    desk.send(json!({"kind": "holdInsight", "context": context, "id": "runtime-key:k1", "hold": "snooze"}))
        .await
        .expect("snoozed");
    assert_eq!(desk.snap().overview.face, AttentionFace::AllClear);
    let Ok(Observation::Shell { mut updates, .. }) = desk.handle.observe(ViewScope::Shell) else {
        panic!("shell");
    };
    // Ten minutes pass with no event: the stale state is published.
    desk.core.advance_clock(601);
    let stale = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(desktop_core::Update::Shell(snapshot)) = updates.next().await
                && !snapshot.overview.fresh
            {
                return snapshot;
            }
        }
    })
    .await
    .expect("published");
    assert_eq!(stale.overview.face, AttentionFace::Gap);
    // A day later the snooze ends and the item is back, published too.
    desk.core.advance_clock(86_400);
    let back = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(desktop_core::Update::Shell(snapshot)) = updates.next().await
                && !snapshot.overview.attention.is_empty()
            {
                return snapshot;
            }
        }
    })
    .await
    .expect("published");
    assert_eq!(back.overview.attention[0].id, "runtime-key:k1");
}

#[tokio::test(flavor = "multi_thread")]
async fn queued_saves_and_copies_report_their_real_outcome() {
    let mut desk = desk().await;
    // A save from the scan form before the library opened: held at the
    // Keychain read, so the library is still opening when it is admitted.
    desk.adapters.vault.hold_reads();
    desk.save(runtime("From the form")).await.expect("admitted");
    assert_eq!(desk.snap().library.notice, None, "not saved yet");
    desk.adapters.vault.release();
    assert!(
        eventually(|| desk.snap().library.notice
            == Some(LibraryNotice::Saved {
                name: "From the form".into()
            }))
        .await
    );
    // A copy into a full, unopened destination: the source hears it failed.
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant b");
    let a = desk.snap().active.expect("a").tenant;
    let b = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("b")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("select b");
    desk.open_library().await;
    for n in 0..32 {
        desk.save(runtime(&format!("B {n}"))).await.expect("saved");
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut desk = desk.restart().await;
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("select a");
    desk.open_library().await;
    let id = desk.snap().library.cases[0].id.clone();
    let context = desk.context();
    desk.send(json!({"kind": "copyCase", "context": context, "id": id, "tenant": b}))
        .await
        .expect("admitted");
    assert!(eventually(|| desk.snap().library.problem == Some(CaseProblem::Full)).await);
    assert!(!matches!(
        desk.snap().library.notice,
        Some(LibraryNotice::Copied { .. })
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_library_works_offline_survives_restart_and_retries_a_failed_save() {
    let mut desk = desk().await;
    desk.script().down = true;
    desk.open_library().await;
    desk.save(runtime("Offline case")).await.expect("saved");
    assert!(eventually(|| desk.library_files().len() == 1).await);
    // A protected write fails (a directory stands where the sealed file
    // goes): shown, then retried explicitly.
    let file = desk.library_files()[0].clone();
    std::fs::remove_file(&file).expect("remove");
    std::fs::create_dir_all(file.join("blocker")).expect("blocker");
    desk.save(runtime("Second")).await.expect("admitted");
    assert!(eventually(|| desk.snap().library.unsaved.is_some()).await);
    assert_eq!(desk.snap().library.problem, Some(CaseProblem::NotSaved));
    std::fs::remove_dir_all(&file).expect("unblock");
    let context = desk.context();
    desk.send(json!({"kind": "saveLibraryAgain", "context": context}))
        .await
        .expect("retried");
    assert!(eventually(|| desk.snap().library.unsaved.is_none()).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let mut desk = desk.restart().await;
    desk.open_library().await;
    let names: Vec<String> = desk
        .snap()
        .library
        .cases
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(names, ["Offline case", "Second"]);
    assert!(
        desk.server.seen().is_empty(),
        "the Library never contacts the tenant"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn delayed_library_completions_stay_with_their_tenant() {
    let mut desk = desk().await;
    desk.open_library().await;
    let context = desk.context();
    let a = desk.snap().active.expect("a").tenant;
    // An import whose sheet answers after the tenant switched.
    let file = json!({"version": 2, "tenant": a, "cases": [
        {"id": "11111111-2222-4333-8444-555555555555", "tenant": a, "product": "ai-gateway",
         "name": "From A's sheet", "prompt": "spec", "response": ""}]});
    desk.adapters.case_files.reply(Some(Ok((
        "a.json".into(),
        serde_json::to_vec(&file).expect("json"),
    ))));
    desk.adapters.case_files.hold();
    desk.send(json!({"kind": "importCases", "context": context}))
        .await
        .expect("import");
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant b");
    let b = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("b")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("select b");
    desk.open_library().await;
    desk.adapters.case_files.release();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        desk.snap().library.cases.is_empty(),
        "B's library is untouched"
    );
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("select a");
    desk.open_library().await;
    assert!(
        eventually(|| desk
            .snap()
            .library
            .cases
            .iter()
            .any(|c| c.name == "From A's sheet"))
        .await
    );
}
