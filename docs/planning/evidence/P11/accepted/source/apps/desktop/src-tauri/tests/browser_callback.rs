//! Academy sign-in's browser hand-off and learner results through Tauri's
//! IPC path (mock runtime, real generated context and capabilities), against
//! a synthetic Academy + Clerk endpoint on this Mac.
//!
//! The renderer can ask the native core to open the current sign-in's
//! verification page, but it cannot name a URL: the core opens only the
//! HTTPS page the Academy issuer returned for the flow in progress.
//!
//! C20: Harness company sign-in through the same IPC path. The renderer
//! names no URL or redirect; the core opens only the configured issuer's
//! authorization page, and only the loopback callback carrying that
//! attempt's state completes it.

#[allow(dead_code)]
#[path = "../../../../crates/desktop-core/tests/context_credentials/support.rs"]
mod support;

#[allow(dead_code)]
#[path = "../../../../crates/desktop-core/tests/harness_identity_history/idp.rs"]
mod idp;

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
        .build(context())
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

/// The production context (generated once: its symbols are global).
fn context() -> tauri::Context<MockRuntime> {
    tauri::generate_context!()
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

// ------------------------------------------------------------ C20 sign-in

const WORKSPACE: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";

fn sso_route(origin: &str, seen: &Seen, idp: &std::sync::Mutex<idp::Idp>) -> Reply {
    if let Some(reply) = idp.lock().expect("idp").route(seen) {
        return reply;
    }
    let path = seen.request_line.split(' ').nth(1).unwrap_or("");
    match path.split('?').next().unwrap_or("") {
        "/oauth2/access_token" => {
            reply(json!({"access_token": "mgmt", "token_type": "Bearer", "expires_in": 3600}))
        }
        "/ai_gw/v2/workspaces" => reply(
            json!({"object": "list", "total": 1, "success": true, "has_more": false,
            "data": [{"id": WORKSPACE, "slug": "team-a", "name": "Team A", "icon": null, "description": null,
                "created_at": "2026-10-01T00:00:00Z", "last_updated_at": "2026-10-02T00:00:00Z", "is_default": 0,
                "status": "active", "scope_name": "scope-team-a", "object": "workspace", "defaults": null,
                "usage_limits": null, "rate_limits": null}]}),
        ),
        _ => {
            let _ = origin;
            reply(json!({}))
        }
    }
}

fn shell(h: &Harness) -> Value {
    match h
        .core
        .operator()
        .observe(ViewScope::Shell)
        .expect("observe")
    {
        Observation::Shell { snapshot, .. } => serde_json::to_value(snapshot).expect("json"),
        _ => unreachable!(),
    }
}

fn wait_shell(h: &Harness, check: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..1000 {
        let v = shell(h);
        if check(&v) {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    shell(h)
}

fn get(url: &str) -> u16 {
    use std::io::{Read, Write};
    let url = url::Url::parse(url).expect("url");
    let port = url.port().expect("port");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .expect("timeout");
    write!(
        stream,
        "GET {}?{} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n",
        url.path(),
        url.query().unwrap_or("")
    )
    .expect("write");
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    reply
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[test]
fn harness_sign_in_opens_only_the_issuers_page_and_completes_only_on_its_loopback() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let ca = TestCa::new("airs-v2 c20 host ca");
    let origin_cell: Arc<std::sync::OnceLock<String>> = Arc::default();
    let idp_cell: Arc<std::sync::OnceLock<std::sync::Mutex<idp::Idp>>> = Arc::default();
    let (routed, provider) = (Arc::clone(&origin_cell), Arc::clone(&idp_cell));
    let server = runtime.block_on(TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            let origin = routed.get().cloned().unwrap_or_default();
            sso_route(&origin, seen, provider.get().expect("idp"))
        })),
    ));
    let origin = format!("https://localhost:{}", server.port);
    origin_cell.set(origin.clone()).expect("origin");
    let _ = idp_cell.set(std::sync::Mutex::new(idp::Idp::new(&origin, "team-a")));
    let (core, adapters) = desktop_host::fixture_core_with(CoreConfig {
        management: desktop_core::management::ManagementEndpoints::at_origin(&origin),
        ..CoreConfig::default()
    });
    core.set_transport_policy(TransportPolicy {
        revision: 2,
        proxy: ProxyMode::Direct,
        extra_roots: vec![ca.pem.as_bytes().to_vec()],
    })
    .expect("policy");
    let app = desktop_host::install(mock_builder(), &core)
        .build(context())
        .expect("app builds with the production context");
    let main = desktop_host::main_window(&app).expect("main window");
    let mut h = Harness {
        _app: app,
        main,
        core,
        adapters,
        _server: server,
        origin: origin.clone(),
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

    // A tenant with its management client, and its Gateway workspace chosen.
    let created = dispatch(
        &mut h,
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": "1234567890",
        "destination": {"kind": "custom", "origin": origin}}}),
    );
    assert_eq!(created["outcome"], "receipt", "{created}");
    let context = shell(&h)["active"]["context"]
        .as_str()
        .expect("context")
        .to_owned();
    dispatch(
        &mut h,
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": "synthetic@1234567890.iam.panserviceaccount.com", "secret": "synthetic-secret"}),
    );
    wait_shell(&h, |v| v["tenants"][0]["management"]["state"] == "ready");
    // A stored management client issues a new context.
    let context = shell(&h)["active"]["context"]
        .as_str()
        .expect("context")
        .to_owned();
    let read = dispatch(
        &mut h,
        json!({"kind": "management", "context": context, "request": {"kind": "gateway",
        "workspace": null, "read": {"kind": "inventory", "resource": "workspaces"}}}),
    );
    assert_eq!(read["outcome"], "receipt", "{read}");
    wait_shell(&h, |v| v["gateway"]["workspaces"].is_array());
    dispatch(
        &mut h,
        json!({"kind": "selectGatewayWorkspace", "context": context, "workspace": WORKSPACE}),
    );
    let reference = wait_shell(&h, |v| v["gateway"]["selected"]["reference"].is_string())["gateway"]["selected"]["reference"].clone();

    // An http issuer is refused; a valid configuration is kept.
    let refused = dispatch(
        &mut h,
        json!({"kind": "configureHarnessSignIn", "context": context, "workspace": reference,
        "endpoint": format!("{origin}/rt/v1"), "issuer": "http://localhost/idp", "clientId": idp::CLIENT, "audience": idp::AUDIENCE}),
    );
    assert_eq!(refusal(&refused), Some("invalidInput"));
    let issuer = format!("{origin}/idp");
    let configured = dispatch(
        &mut h,
        json!({"kind": "configureHarnessSignIn", "context": context, "workspace": reference,
        "endpoint": format!("{origin}/rt/v1"), "issuer": issuer, "clientId": idp::CLIENT, "audience": idp::AUDIENCE}),
    );
    assert_eq!(configured["outcome"], "receipt", "{configured}");

    // The renderer cannot name the page or the redirect: an intent carrying
    // either is refused at the boundary, and only the issuer's
    // authorization page is ever opened.
    let forged = call(
        &h,
        "desktop_dispatch",
        &json!({"protocol": 1, "session": h.session,
        "sequence": (h.sequence + 1).to_string(), "intent": {"kind": "signInHarness", "context": context,
        "url": "https://evil.example/authorize", "redirectUri": "http://evil.example/callback"}}),
    );
    assert_eq!(
        forged.err().as_ref().and_then(Value::as_str),
        Some("invalidInput")
    );
    assert!(h.adapters.opener.opened().is_empty());
    let start = dispatch(&mut h, json!({"kind": "signInHarness", "context": context}));
    assert_eq!(start["outcome"], "receipt", "{start}");
    for _ in 0..500 {
        if !h.adapters.opener.opened().is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let opened = h.adapters.opener.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    let url = url::Url::parse(&opened[0]).expect("url");
    assert!(
        opened[0].starts_with(&format!("{issuer}/authorize?")),
        "{opened:?}"
    );
    let redirect = url
        .query_pairs()
        .find(|(k, _)| k == "redirect_uri")
        .map(|(_, v)| v.into_owned())
        .expect("redirect");
    assert!(
        redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"),
        "{redirect}"
    );
    assert_eq!(
        wait_shell(&h, |v| v["harness"]["signIn"]["step"] == "browser")["harness"]["signIn"]["step"],
        "browser"
    );

    let (_, back) = idp_cell
        .get()
        .expect("idp")
        .lock()
        .expect("idp")
        .authorize(&url);
    // A wrong state, path or method is answered and ignored.
    assert_eq!(get(&back.replace("state=", "state=x")), 400);
    assert_eq!(get(&back.replace("/callback", "/other")), 404);
    assert_eq!(get(&back), 200);
    let signed = wait_shell(&h, |v| !v["harness"]["signIn"]["account"].is_null());
    assert_eq!(
        signed["harness"]["signIn"]["account"],
        format!("{}@example.com", idp::SUBJECT),
        "{}",
        signed["harness"]
    );
    assert!(signed["harness"]["signIn"]["failure"].is_null());
    // The listener is gone with its attempt.
    let port = url::Url::parse(&redirect)
        .expect("redirect")
        .port()
        .expect("port");
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
    // Nothing secret crossed IPC.
    let text = signed.to_string();
    for secret in ["refresh-", "eyJ", "code-1"] {
        assert!(!text.contains(secret), "{secret}");
    }
    let out = dispatch(
        &mut h,
        json!({"kind": "signOutHarness", "context": context}),
    );
    assert_eq!(out["outcome"], "receipt");
    assert!(wait_shell(&h, |v| v["harness"]["signIn"]["account"].is_null())["harness"]["signIn"]["account"].is_null());
}
