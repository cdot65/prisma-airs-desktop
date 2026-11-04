//! F01/F02: dedicated setup warning checks over real authority, storage and
//! management transport. Every endpoint and credential is an owned TLS fixture.
#[allow(dead_code)]
#[path = "context_credentials/support.rs"]
mod support;

use desktop_core::fixtures::{self, Handles, ScriptedStore};
use desktop_core::management::ManagementEndpoints;
use desktop_core::store::PersistedState;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, ShellSnapshot,
    ViewScope,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use support::{Reply, Seen, TestCa, TlsServer, eventually};

const TENANT: &str = "tn-0123456789abcdef";
const TSG: &str = "1234567890";
const WS: &str = "099073d7-ced5-456f-b29b-86601b9cc42d";
const LEGACY_MODEL: &str = "2 of 3 model and skill sources readable.";

struct Script {
    model_status: [u16; 3],
    scope_slug: String,
}
fn reply(status: u16, value: Value) -> Reply {
    if status == 0 {
        return Reply::Hang;
    }
    let body = serde_json::to_vec(&value).unwrap();
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}
impl Script {
    fn route(&self, seen: &Seen) -> Reply {
        let path = seen
            .request_line
            .split(' ')
            .nth(1)
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("");
        match path {
            "/oauth2/access_token" => reply(
                200,
                json!({"access_token":"synthetic-access", "token_type":"Bearer", "expires_in":3600}),
            ),
            "/aims/data/v1/scans" => reply(
                self.model_status[0],
                json!({"scans":[],"pagination":{"total_items":0}}),
            ),
            "/aiag/data/v1/stats/scans" => reply(
                self.model_status[1],
                json!({"unique_skills_scanned":{"count":0,"percent_change":null},"total_vulnerabilities_found":{"count":0,"percent_change":null},"top_vulnerability":null}),
            ),
            "/aiag/data/v1/scans" => reply(
                self.model_status[2],
                json!({"scans":[],"pagination":{"total_items":0}}),
            ),
            "/ai_gw/v2/workspaces" => reply(
                200,
                json!({"object":"list","total":1,"success":true,"has_more":false,"data":[{"id":WS,"slug":"goshawks","name":"Goshawks","icon":null,"description":null,"created_at":"2026-10-01T00:00:00Z","last_updated_at":"2026-10-02T00:00:00Z","is_default":0,"status":"active","scope_name":"ws_goshawks_io_2mwlan","object":"workspace"}]}),
            ),
            "/ai_gw/v2/configs"
            | "/ai_gw/v2/providers"
            | "/ai_gw/v2/api-keys/service"
            | "/ai_gw/admin/v2/integrations" => reply(
                200,
                json!({"object":"list","total":0,"success":true,"has_more":false,"data":[]}),
            ),
            "/iam/v1/scopes/ws_goshawks_io_2mwlan" => reply(
                200,
                json!({"name":"ws_goshawks_io_2mwlan","description":"Fixture", "tsg_id":TSG,"id":"scope-fixture","resources":[{"resource_type":"workspace","resource_id":self.scope_slug}]}),
            ),
            _ => reply(404, json!({"error":"fixture path not admitted"})),
        }
    }
}
struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    handles: Handles,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
}
impl Desk {
    fn snap(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).unwrap() {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => panic!("shell"),
        }
    }
    fn context(&self) -> String {
        self.snap().active.unwrap().context.as_str().to_owned()
    }
    fn warning(&self) -> Value {
        serde_json::to_value(&self.snap().tenants[0].setup_warnings[0]).unwrap()
    }
    async fn send(&mut self, intent: Value) -> Receipt {
        self.sequence += 1;
        self.handle
            .dispatch(
                Delivery {
                    session: self.session.clone(),
                    sequence: self.sequence,
                },
                serde_json::from_value::<Intent>(intent).expect("public typed intent"),
            )
            .await
            .expect("admitted")
    }
    async fn recheck(&mut self) {
        let context = self.context();
        self.send(json!({"kind":"recheckSetupWarning","context":context,"warning":"vw-1"}))
            .await;
        assert!(
            eventually(|| self.warning()["recheck"]["state"]
                .as_str()
                .is_some_and(|s| s != "pending"))
            .await,
            "check settled: {}",
            self.warning()
        );
    }
    async fn workspace(&mut self) {
        let context = self.context();
        self.send(json!({"kind":"management","context":context,"request":{"kind":"gateway","workspace":null,"read":{"kind":"inventory","resource":"workspaces"}}})).await;
        assert!(eventually(|| self.snap().gateway.workspaces.is_some()).await);
        self.send(json!({"kind":"selectGatewayWorkspace","context":context,"workspace":WS}))
            .await;
    }
}
async fn desk(product: &str, reason: &str, check: Option<Value>) -> Desk {
    desk_with_tsg(product, reason, check, Some(TSG)).await
}
async fn desk_with_tsg(
    product: &str,
    reason: &str,
    check: Option<Value>,
    tsg: Option<&str>,
) -> Desk {
    let ca = TestCa::new("warning-recheck fixture");
    let script = Arc::new(Mutex::new(Script {
        model_status: [200; 3],
        scope_slug: "goshawks".to_owned(),
    }));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |s| routed.lock().unwrap().route(s))),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    let mut state = serde_json::to_value(PersistedState::default()).unwrap();
    state["tenants"] = json!([{"id":TENANT,"label":"Fixture","tsgId":tsg,"destination":{"kind":"custom","origin":origin},"environment":"production"}]);
    state["activeTenant"] = json!(TENANT);
    state["onboarding"] = json!("done");
    state["tenantProducts"] = json!([{"tenant":TENANT,"products":if product == "runtimeSecurity" { vec![product] } else { vec!["runtimeSecurity", product] }}]);
    let mut warning =
        json!({"id":"vw-1","product":product,"reason":reason,"recordedSecs":1761480000});
    if let Some(mut binding) = check {
        binding["origin"] = json!(origin);
        warning["check"] = binding;
    }
    state["tenantWarnings"] = json!([{"tenant":TENANT,"warnings":[warning]}]);
    let (adapters, handles) = fixtures::adapters_with(ScriptedStore::with_state(
        serde_json::from_value(state).unwrap(),
    ));
    let core = Core::new(
        CoreConfig {
            management: ManagementEndpoints::at_origin(&origin),
            deadline_tick: Duration::from_millis(25),
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
    .unwrap();
    let handle = core.operator();
    let session = handle.register_delivery_session().unwrap().nonce;
    let mut desk = Desk {
        _core: core,
        handle,
        session,
        sequence: 0,
        handles,
        script,
        server,
    };
    if product != "runtimeSecurity" {
        let context = desk.context();
        desk.send(json!({"kind":"configureManagementCredential","context":context,"mode":"keychain","clientId":"synthetic-client@1234567890.iam.panserviceaccount.com","secret":"synthetic-warning-secret"})).await;
        assert!(
        eventually(|| desk.snap().tenants[0].management.state == desktop_core::ScanState::Ready)
            .await
    );
    }
    desk
}
fn known(product: &str) -> Value {
    json!({"version":1,"line":product,"tsgId":TSG,"origin":"replaced-by-owned-origin","workspace":null})
}

#[tokio::test(flavor = "multi_thread")]
async fn legacy_model_warning_requires_dedicated_check_and_preserves_dated_history() {
    let mut d = desk("modelSkill", LEGACY_MODEL, None).await;
    let context = d.context();
    d.send(json!({"kind":"readTenant","context":context})).await;
    assert!(
        eventually(|| {
            let read = d.snap().inbox.read;
            read.state != desktop_core::ReadState::Reading
                && read.products.iter().any(|p| {
                    p.product == desktop_core::features::ProductId::ModelSkill
                        && p.state == desktop_core::ProductReadState::Ok
                })
        })
        .await
    );
    assert!(
        d.warning().get("resolvedSecs").is_none(),
        "ordinary healthy read is not a setup recheck"
    );
    d.recheck().await;
    let warning = d.warning();
    assert_eq!(warning["reason"], LEGACY_MODEL);
    assert_eq!(warning["recordedSecs"], 1761480000u32);
    assert_eq!(warning["recheck"]["state"], "pass");
    assert_eq!(warning["recheck"]["basis"], "legacyTenantSources");
    assert!(warning["resolvedSecs"].as_u64().is_some());
    assert!(!d.snap().inbox.items.iter().any(|i| i.id == "vw-1"));
    assert!(
        serde_json::to_value(d.handles.store.saved().unwrap()).unwrap()["tenantWarnings"][0]["warnings"][0]["resolvedSecs"].as_u64().is_some()
    );
    assert!(
        d.server
            .seen()
            .iter()
            .all(|r| r.request_line.starts_with("GET ")
                || r.request_line.starts_with("POST /oauth2/access_token ")),
        "no product writes or paid scans"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn source_specific_denial_remains_active_then_resolves_after_fresh_complete_check() {
    let mut d = desk("modelSkill", LEGACY_MODEL, Some(known("modelSkill"))).await;
    d.script.lock().unwrap().model_status[1] = 403;
    d.recheck().await;
    assert!(d.warning().get("resolvedSecs").is_none());
    let reason = d.warning()["recheck"]["reason"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        reason.contains("Skill statistics") && reason.contains("not allowed"),
        "{reason}"
    );
    d.script.lock().unwrap().model_status[1] = 200;
    d.recheck().await;
    assert_eq!(d.warning()["recheck"]["basis"], "recordedScope");
    assert!(d.warning()["resolvedSecs"].as_u64().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn unknown_legacy_scope_and_failed_persistence_do_not_clear_warning() {
    let mut d = desk("modelSkill", "A freeform unknown legacy diagnostic", None).await;
    d.recheck().await;
    assert_eq!(d.warning()["recheck"]["basis"], "unknown");
    assert!(d.warning().get("resolvedSecs").is_none());
    let mut d = desk("modelSkill", LEGACY_MODEL, None).await;
    d.handles.store.fail_saves(true);
    d.recheck().await;
    assert!(d.warning().get("resolvedSecs").is_none());
    assert!(
        d.warning()["recheck"]["reason"]
            .as_str()
            .unwrap()
            .contains("could not be saved")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reported_legacy_gateway_scope_is_rechecked_with_slug_not_display_uuid() {
    let reason =
        format!("Workspace {WS}: partial; scope ws_goshawks_io_2mwlan does not list the workspace");
    let mut d = desk("gateway", &reason, None).await;
    d.workspace().await;
    d.script.lock().unwrap().scope_slug = WS.to_owned();
    d.recheck().await;
    assert!(
        d.warning().get("resolvedSecs").is_none(),
        "UUID binding is not the documented slug contract"
    );
    d.script.lock().unwrap().scope_slug = "goshawks".to_owned();
    d.recheck().await;
    assert_eq!(d.warning()["recheck"]["basis"], "legacyWorkspaceScope");
    assert!(d.warning()["resolvedSecs"].as_u64().is_some());
    assert_eq!(d.warning()["reason"], reason);
}

#[tokio::test(flavor = "multi_thread")]
async fn gateway_wrong_workspace_or_unknown_scope_format_remains_unresolved() {
    for reason in [
        "Workspace other-workspace: partial; scope ws_goshawks_io_2mwlan does not list the workspace",
        "scope ws_goshawks_io_2mwlan: arbitrary unknown scope",
    ] {
        let mut d = desk("gateway", reason, None).await;
        d.workspace().await;
        d.recheck().await;
        assert_eq!(d.warning()["recheck"]["basis"], "unknown");
        assert!(d.warning().get("resolvedSecs").is_none());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn replaced_credential_generation_retires_pending_warning_check() {
    let mut d = desk("modelSkill", LEGACY_MODEL, Some(known("modelSkill"))).await;
    d.script.lock().unwrap().model_status[0] = 0;
    let old_context = d.context();
    d.send(json!({"kind":"recheckSetupWarning","context":old_context,"warning":"vw-1"}))
        .await;
    assert!(
        eventually(|| d
            .server
            .seen()
            .iter()
            .any(|s| s.request_line.starts_with("GET /aims/data/v1/scans")))
        .await
    );
    assert_eq!(d.warning()["recheck"]["state"], "pending");
    d.send(json!({"kind":"configureManagementCredential","context":old_context,"mode":"keychain","clientId":"synthetic-client@1234567890.iam.panserviceaccount.com","secret":"synthetic-replaced-warning-secret"})).await;
    assert!(
        eventually(|| d.context() != old_context
            && d.snap().tenants[0].management.state == desktop_core::ScanState::Ready)
        .await
    );
    assert!(eventually(|| d.server.closed_by_client() > 0).await);
    assert!(d.warning().get("resolvedSecs").is_none());
    assert_ne!(d.warning()["recheck"]["state"], "pending");
    d.script.lock().unwrap().model_status[0] = 200;
    d.recheck().await;
    assert!(d.warning()["resolvedSecs"].as_u64().is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn absent_original_gateway_workspace_does_not_mean_any_workspace_matches() {
    let reason =
        format!("Workspace {WS}: partial; scope ws_goshawks_io_2mwlan does not list the workspace");
    let mut d = desk("gateway", &reason, Some(known("gateway"))).await;
    d.workspace().await;
    d.recheck().await;
    assert_eq!(d.warning()["recheck"]["equivalent"], false);
    assert!(d.warning().get("resolvedSecs").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn known_legacy_scan_configuration_is_rechecked_locally_without_claiming_service_validity() {
    let reason = "No scan key saved; scans stay unavailable until one is added in Administration.";
    let mut d = desk_with_tsg("runtimeSecurity", reason, None, None).await;
    assert!(d.snap().tenants[0].tsg_id.is_none());
    let context = d.context();
    d.send(json!({"kind":"configureScanCredential","context":context,"mode":"keychain","key":"synthetic-scan-key"})).await;
    assert!(eventually(|| d.snap().tenants[0].scan.state == desktop_core::ScanState::Ready).await);
    d.recheck().await;
    assert_eq!(d.warning()["reason"], reason);
    assert_eq!(d.warning()["recheck"]["state"], "stored");
    assert_eq!(d.warning()["recheck"]["basis"], "legacyTenantSources");
    assert!(d.warning()["resolvedSecs"].as_u64().is_some());
    assert!(
        d.server.seen().is_empty(),
        "configuration proof never calls a product or OAuth endpoint"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn known_missing_selection_check_is_superseded_only_by_full_selected_prerequisites() {
    let reason =
        "1 Gateway workspace(s) readable; none chosen, so its prerequisites were not checked.";
    let mut d = desk("gateway", reason, Some(known("gateway"))).await;
    d.recheck().await;
    assert!(
        d.warning().get("resolvedSecs").is_none(),
        "a listing is not selected-workspace prerequisites"
    );
    d.workspace().await;
    d.recheck().await;
    assert_eq!(d.warning()["recheck"]["basis"], "recordedScope");
    assert!(d.warning()["resolvedSecs"].as_u64().is_some());
    assert!(
        d.warning()["recheck"]["reason"]
            .as_str()
            .unwrap()
            .contains("original check found no selected workspace")
    );
}
