//! Academy sign-in's browser hand-off and learner results through Tauri's
//! IPC path (mock runtime, real generated context and capabilities), against
//! a synthetic Academy + Clerk endpoint on this Mac.
//!
//! The renderer can ask the native core to open the current sign-in's
//! verification page, but it cannot name a URL: the core opens only the
//! HTTPS page the Academy issuer returned for the flow in progress.

#[allow(dead_code)]
#[path = "../../../../crates/desktop-core/tests/context_credentials/support.rs"]
mod support;

use std::sync::Arc;

use desktop_core::learner::AcademyView;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{CoreConfig, Observation, ViewScope};
use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, WebviewWindow};

use crate::support::{Reply, Seen, TestCa, TlsServer};

const SCOPE: &str = "openid profile email offline_access academy:learn";

fn reply(body: Value) -> Reply {
    Reply::ok(body.to_string().as_bytes())
}

fn route(origin: &str, seen: &Seen, verification: &str) -> Reply {
    let path = seen.request_line.split(' ').nth(1).unwrap_or("");
    let form = String::from_utf8_lossy(&seen.body);
    match path.split('?').next().unwrap_or("") {
        "/api/desktop/config" => {
            reply(json!({"issuer": origin, "clientId": "desktop_public_client", "scope": SCOPE}))
        }
        "/.well-known/oauth-authorization-server" => reply(json!({"issuer": origin,
            "device_authorization_endpoint": format!("{origin}/oauth/device"),
            "token_endpoint": format!("{origin}/oauth/token"),
            "revocation_endpoint": format!("{origin}/oauth/revoke")})),
        "/oauth/device" => reply(json!({"device_code": "dc", "user_code": "ABCD-EFGH",
            "verification_uri": verification, "expires_in": 600, "interval": 1})),
        "/oauth/token" if form.contains("device_code") && verification.contains("pending") => {
            let body = json!({"error": "authorization_pending"}).to_string();
            Reply::Raw {
                head: format!(
                    "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
                    body.len()
                ),
                body: body.into_bytes(),
            }
        }
        "/oauth/token" => reply(
            json!({"token_type": "Bearer", "access_token": "at", "refresh_token": "rt", "expires_in": 3600}),
        ),
        "/api/trpc/desktop.me" => reply(
            json!({"result": {"data": {"json": {"accountId": "user_1", "displayName": "Grace"}}}}),
        ),
        "/api/trpc/progress.get" => reply(json!({"result": {"data": {"json": {"done": 4}}}})),
        _ => reply(json!({})),
    }
}

struct Harness {
    _app: App<MockRuntime>,
    main: WebviewWindow<MockRuntime>,
    core: desktop_core::Core,
    adapters: desktop_core::fixtures::Handles,
    _server: TlsServer,
    origin: String,
    sequence: u64,
    session: String,
}

/// `verification` is a path on the endpoint, or a full URL to return.
fn harness(runtime: &tokio::runtime::Runtime, verification: &str) -> Harness {
    let ca = TestCa::new("airs-v2 c04 host ca");
    let origin_cell: Arc<std::sync::OnceLock<String>> = Arc::default();
    let verification = verification.to_owned();
    let routed = Arc::clone(&origin_cell);
    let server = runtime.block_on(TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            let origin = routed.get().cloned().unwrap_or_default();
            let page = if verification.starts_with('/') {
                format!("{origin}{verification}")
            } else {
                verification.clone()
            };
            route(&origin, seen, &page)
        })),
    ));
    let origin = format!("https://localhost:{}", server.port);
    origin_cell.set(origin.clone()).expect("origin");
    let (core, adapters) = desktop_host::fixture_core_with(CoreConfig {
        academy_origin: origin.clone(),
        ..CoreConfig::default()
    });
    core.set_transport_policy(TransportPolicy {
        revision: 2,
        proxy: ProxyMode::Direct,
        extra_roots: vec![ca.pem.as_bytes().to_vec()],
    })
    .expect("policy");
    let app = desktop_host::install(mock_builder(), &core)
        .build(tauri::generate_context!())
        .expect("app builds with the production context");
    let main = desktop_host::main_window(&app).expect("main window");
    let mut h = Harness {
        _app: app,
        main,
        core,
        adapters,
        _server: server,
        origin,
        sequence: 0,
        session: String::new(),
    };
    let response = call(
        &h,
        "desktop_observe",
        &json!({"protocol": 1, "scope": {"kind": "deliverySession"}}),
    )
    .expect("session");
    h.session = response["session"].as_str().expect("session").to_owned();
    h
}

fn call(h: &Harness, cmd: &str, body: &Value) -> Result<Value, Value> {
    get_ipc_response(
        &h.main,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().expect("url"),
            body: InvokeBody::Raw(serde_json::to_vec(body).expect("bytes")),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|b| b.deserialize::<Value>().expect("json"))
}

fn dispatch(h: &mut Harness, intent: Value) -> Value {
    h.sequence += 1;
    call(
        h,
        "desktop_dispatch",
        &json!({"protocol": 1, "session": h.session, "sequence": h.sequence.to_string(), "intent": intent}),
    )
    .expect("dispatch response")
}

fn academy(h: &Harness) -> AcademyView {
    match h
        .core
        .operator()
        .observe(ViewScope::Shell)
        .expect("observe")
    {
        Observation::Shell { snapshot, .. } => snapshot.academy,
        _ => unreachable!(),
    }
}

fn wait(h: &Harness, check: impl Fn(&AcademyView) -> bool) -> AcademyView {
    for _ in 0..400 {
        let view = academy(h);
        if check(&view) {
            return view;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    academy(h)
}

fn refusal(value: &Value) -> Option<&str> {
    (value["outcome"] == "refusal")
        .then(|| value["refusal"].as_str())
        .flatten()
}

#[test]
fn the_renderer_opens_only_the_page_the_issuer_returned_for_the_current_flow() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut h = harness(&runtime, "/activate?pending");
    // Nothing to open while signed out.
    let open = dispatch(&mut h, json!({"kind": "openAcademyVerification"}));
    assert_eq!(refusal(&open), Some("staleContext"));
    // The renderer cannot supply a URL: the intent carries none, and an
    // extra field is ignored rather than used.
    let forged = dispatch(
        &mut h,
        json!({"kind": "openAcademyVerification", "url": "https://evil.example/phish"}),
    );
    assert_eq!(refusal(&forged), Some("staleContext"));
    let start = dispatch(&mut h, json!({"kind": "startAcademySignIn"}));
    assert_eq!(start["outcome"], "receipt", "{start}");
    let view = wait(&h, |v| matches!(v, AcademyView::AwaitingApproval { .. }));
    let AcademyView::AwaitingApproval {
        verification_uri,
        user_code,
        ..
    } = view
    else {
        panic!("awaiting: {view:?}")
    };
    assert_eq!(user_code, "ABCD-EFGH");
    let open = dispatch(
        &mut h,
        json!({"kind": "openAcademyVerification", "url": "https://evil.example/phish"}),
    );
    assert_eq!(open["outcome"], "receipt", "{open}");
    for _ in 0..200 {
        if !h.adapters.opener.opened().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(
        h.adapters.opener.opened(),
        vec![format!("{}/activate?pending", h.origin)]
    );
    assert_eq!(verification_uri, format!("{}/activate?pending", h.origin));

    let cancel = dispatch(&mut h, json!({"kind": "cancelAcademySignIn"}));
    assert_eq!(cancel["outcome"], "receipt");
    let open = dispatch(&mut h, json!({"kind": "openAcademyVerification"}));
    assert_eq!(
        refusal(&open),
        Some("staleContext"),
        "a cancelled flow opens nothing"
    );
    assert_eq!(h.adapters.opener.opened().len(), 1);
}

#[test]
fn a_non_https_verification_page_fails_the_sign_in_and_is_never_opened() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut h = harness(&runtime, "http://localhost/activate");
    dispatch(&mut h, json!({"kind": "startAcademySignIn"}));
    let view = wait(&h, |v| matches!(v, AcademyView::Failed { .. }));
    assert!(
        matches!(
            view,
            AcademyView::Failed {
                failure: desktop_core::academy::AcademyFailure::InvalidResponse,
                ..
            }
        ),
        "{view:?}"
    );
    let open = dispatch(&mut h, json!({"kind": "openAcademyVerification"}));
    assert_eq!(refusal(&open), Some("staleContext"));
    assert!(h.adapters.opener.opened().is_empty());
}

#[test]
fn learner_results_cross_ipc_once_with_no_credential() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut h = harness(&runtime, "/activate");
    dispatch(&mut h, json!({"kind": "startAcademySignIn"}));
    let view = wait(&h, |v| matches!(v, AcademyView::SignedIn { .. }));
    let AcademyView::SignedIn {
        learner,
        display_name,
        ..
    } = view
    else {
        panic!("{view:?}")
    };
    assert_eq!(display_name, "Grace");
    let receipt = dispatch(
        &mut h,
        json!({"kind": "academyCall", "learner": learner, "procedure": "progress.get", "procedureKind": "query", "input": null}),
    );
    let operation = receipt["receipt"]["operation"]
        .as_str()
        .expect("operation")
        .to_owned();
    let result = call(
        &h,
        "desktop_observe",
        &json!({"protocol": 1, "scope": {"kind": "academyCall", "operation": operation}}),
    )
    .expect("result");
    assert_eq!(
        result,
        json!({"kind": "academyCall", "result": {"kind": "data", "data": "{\"json\":{\"done\":4}}"}})
    );
    let again = call(
        &h,
        "desktop_observe",
        &json!({"protocol": 1, "scope": {"kind": "academyCall", "operation": operation}}),
    );
    assert_eq!(
        again.err().as_ref().and_then(Value::as_str),
        Some("invalidInput"),
        "observed once"
    );
    let denied = dispatch(
        &mut h,
        json!({"kind": "academyCall", "learner": learner, "procedure": "admin.users.list", "procedureKind": "query", "input": null}),
    );
    assert_eq!(refusal(&denied), Some("capabilityMissing"));
    let text = result.to_string();
    for secret in ["\"at\"", "\"rt\"", "dc"] {
        assert!(!text.contains(secret), "{secret}");
    }
}
