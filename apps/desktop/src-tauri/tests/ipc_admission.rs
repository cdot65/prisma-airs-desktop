//! Production host admission through Tauri's IPC path (mock runtime, real
//! generated context and capabilities). Literal request/response fixtures are
//! shared with the TypeScript facade tests.

use std::path::PathBuf;

use serde_json::{Value, json};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

fn fixture(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../packages/desktop-contracts/fixtures/wire")
        .join(name);
    serde_json::from_slice(&std::fs::read(&path).expect("fixture")).expect("fixture json")
}

struct Harness {
    app: App<MockRuntime>,
    main: WebviewWindow<MockRuntime>,
    adapters: desktop_core::fixtures::Handles,
}

fn harness() -> Harness {
    let (core, adapters) = desktop_host::fixture_core();
    let app = desktop_host::install(mock_builder(), &core)
        .build(tauri::generate_context!())
        .expect("app builds with the production context");
    let main = desktop_host::main_window(&app).expect("main window");
    Harness {
        app,
        main,
        adapters,
    }
}

fn request(cmd: &str, body: InvokeBody, url: &str) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: CallbackFn(0),
        error: CallbackFn(1),
        url: url.parse().expect("url"),
        body,
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

const ORIGIN: &str = "tauri://localhost";

fn raw(value: &Value) -> InvokeBody {
    InvokeBody::Raw(serde_json::to_vec(value).expect("bytes"))
}

fn call(window: &WebviewWindow<MockRuntime>, cmd: &str, body: InvokeBody) -> Result<Value, Value> {
    call_from(window, cmd, body, ORIGIN)
}

fn call_from(
    window: &WebviewWindow<MockRuntime>,
    cmd: &str,
    body: InvokeBody,
    url: &str,
) -> Result<Value, Value> {
    get_ipc_response(window, request(cmd, body, url))
        .map(|b| b.deserialize::<Value>().expect("json response"))
}

fn register(h: &Harness) -> String {
    let response = call(
        &h.main,
        "desktop_observe",
        raw(&fixture("observe-session-request.json")),
    )
    .expect("session");
    let template = fixture("observe-session-response.json");
    assert_eq!(response["kind"], template["kind"]);
    assert_eq!(response["nextSequence"], template["nextSequence"]);
    let session = response["session"].as_str().expect("session").to_owned();
    assert!(
        session.starts_with("ds-")
            && session.len() == 35
            && session[3..].bytes().all(|b| b.is_ascii_hexdigit())
    );
    session
}

fn dispatch(h: &Harness, session: &str, sequence: &str, theme: &str) -> Result<Value, Value> {
    let mut body = fixture("dispatch-request.json");
    body["session"] = json!(session);
    body["sequence"] = json!(sequence);
    body["intent"]["theme"] = json!(theme);
    call(&h.main, "desktop_dispatch", raw(&body))
}

#[test]
fn literal_dispatch_round_trip_matches_fixtures() {
    let h = harness();
    let session = register(&h);
    assert_eq!(
        dispatch(&h, &session, "1", "dark"),
        Ok(fixture("dispatch-receipt.json"))
    );
}

#[test]
fn shell_observation_matches_the_literal_snapshot() {
    let h = harness();
    let response = call(
        &h.main,
        "desktop_observe",
        raw(&fixture("observe-shell-request.json")),
    )
    .expect("observe");
    // The learner reference is salted per launch: check its shape, then
    // compare everything else literally.
    let mut response = response;
    let learner = response["snapshot"]["learner"]
        .as_str()
        .expect("learner")
        .to_owned();
    assert!(
        learner.len() == 27
            && learner.starts_with("lx-")
            && learner[3..].bytes().all(|b| b.is_ascii_hexdigit()),
        "{learner}"
    );
    response["snapshot"]["learner"] =
        fixture("observe-shell-response.json")["snapshot"]["learner"].clone();
    // How many diagnostics this run has kept varies with its requests.
    assert!(
        response["snapshot"]["diagnostics"]["events"]
            .as_u64()
            .is_some()
    );
    response["snapshot"]["diagnostics"]["events"] =
        fixture("observe-shell-response.json")["snapshot"]["diagnostics"]["events"].clone();
    assert_eq!(response, fixture("observe-shell-response.json"));
    let text = response.to_string();
    for forbidden in ["secret", "token", "password", "apiKey", "credential"] {
        assert!(
            !text.to_lowercase().contains(&forbidden.to_lowercase()),
            "snapshot leaks {forbidden}"
        );
    }
}

#[test]
fn delivery_sequence_rules_hold_through_ipc() {
    let h = harness();
    let session = register(&h);
    let first = dispatch(&h, &session, "1", "dark").expect("first");
    assert_eq!(
        dispatch(&h, &session, "1", "dark").expect("duplicate"),
        first,
        "identical duplicate returns the same result"
    );
    assert_eq!(
        dispatch(&h, &session, "1", "light").expect("changed duplicate"),
        json!({"outcome": "refusal", "refusal": "invalidInput"}),
        "changed input under a consumed sequence is refused"
    );
    assert_eq!(
        dispatch(&h, &session, "3", "light").expect("gap"),
        fixture("dispatch-refusal-sequence-gap.json")
    );
    assert_eq!(
        dispatch(&h, &session, "2", "light").expect("next")["outcome"],
        json!("receipt"),
        "gap did not advance"
    );
    let renewed = register(&h);
    assert_ne!(renewed, session);
    assert_eq!(
        dispatch(&h, &session, "3", "dark").expect("retired"),
        json!({"outcome": "refusal", "refusal": "staleContext"}),
        "a retired session cannot deliver after re-registration"
    );
    assert_eq!(
        dispatch(&h, &renewed, "1", "dark").expect("renewed")["outcome"],
        json!("receipt")
    );
}

#[test]
fn malformed_requests_are_refused_before_authority() {
    let h = harness();
    let session = register(&h);
    let cases = fixture("rejected-requests.json");
    for case in cases["dispatch"].as_array().expect("dispatch cases") {
        let mut body = case["body"].clone();
        if body.get("session").and_then(Value::as_str) == Some("ds-x") {
            body["session"] = json!(session);
        }
        assert_eq!(
            call(&h.main, "desktop_dispatch", raw(&body)),
            Err(json!("invalidInput")),
            "{}",
            case["case"]
        );
    }
    for case in cases["observe"].as_array().expect("observe cases") {
        assert_eq!(
            call(&h.main, "desktop_observe", raw(&case["body"])),
            Err(json!("invalidInput")),
            "{}",
            case["case"]
        );
    }
    // None of the refused deliveries consumed a sequence.
    assert_eq!(
        dispatch(&h, &session, "1", "dark").expect("first")["outcome"],
        json!("receipt")
    );
}

#[test]
fn json_bodies_and_oversized_payloads_are_refused() {
    let h = harness();
    let body = fixture("dispatch-request.json");
    assert_eq!(
        call(&h.main, "desktop_dispatch", InvokeBody::Json(body)),
        Err(json!("invalidInput")),
        "JSON bodies bypass the size ceiling"
    );
    let mut huge = b"{\"protocol\":1,\"session\":\"ds-x\",\"sequence\":\"1\",\"intent\":{\"kind\":\"setAppearance\",\"theme\":\"dark\"},\"pad\":\"".to_vec();
    huge.resize(desktop_core::wire::MAX_REQUEST_BYTES + 1, b'a');
    assert_eq!(
        call(&h.main, "desktop_dispatch", InvokeBody::Raw(huge)),
        Err(json!("invalidInput"))
    );
}

#[test]
fn other_windows_and_remote_origins_cannot_reach_commands() {
    let h = harness();
    let intruder =
        WebviewWindowBuilder::new(&h.main, "intruder", WebviewUrl::App("index.html".into()))
            .build()
            .expect("second window");
    let session_request = raw(&fixture("observe-session-request.json"));
    let denied = call(&intruder, "desktop_observe", session_request.clone());
    assert!(
        denied.is_err(),
        "non-main window must be denied: {denied:?}"
    );
    for url in [
        "https://evil.example/",
        "http://localhost:1420/",
        "tauri://evil/",
    ] {
        let remote = call_from(&h.main, "desktop_observe", session_request.clone(), url);
        assert!(remote.is_err(), "origin {url} must be denied: {remote:?}");
    }
}

#[test]
fn unregistered_and_plugin_commands_are_denied() {
    let h = harness();
    for cmd in [
        "desktop_fixture_complete",
        "desktop_secret_read",
        "plugin:shell|execute",
        "plugin:fs|read_file",
        "plugin:http|fetch",
        "plugin:event|listen",
        "plugin:webview|create_webview_window",
        "plugin:window|set_title",
    ] {
        let result = call(&h.main, cmd, InvokeBody::Json(json!({})));
        assert!(result.is_err(), "{cmd} must be denied: {result:?}");
    }
}

#[test]
fn navigation_policy_accepts_only_packaged_assets() {
    for url in ["tauri://localhost/index.html", "tauri://localhost/"] {
        assert!(
            desktop_host::is_packaged_origin(&url.parse().expect("url")),
            "{url}"
        );
    }
    for url in [
        "https://localhost/",
        "http://localhost:1420/",
        "tauri://localhost:8080/",
        "tauri://evil/",
        "file:///etc/passwd",
        "https://academy.example/learn",
    ] {
        assert!(
            !desktop_host::is_packaged_origin(&url.parse().expect("url")),
            "{url}"
        );
    }
}

fn observe_shell(h: &Harness, token: u32) -> Value {
    let body = json!({"protocol": 1, "scope": {"kind": "shell", "updates": format!("__CHANNEL__:{token}")}});
    call(&h.main, "desktop_observe", raw(&body)).expect("observe")
}

fn eventually(mut condition: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if condition() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    condition()
}

#[test]
fn observations_are_native_owned_closable_and_retired_with_the_webview() {
    let h = harness();
    let host = h.app.state::<desktop_host::Host>();
    for token in [7, 8, 9] {
        assert_eq!(observe_shell(&h, token)["kind"], json!("shell"));
    }
    assert_eq!(host.active_observations("main"), 3);
    assert!(eventually(|| host.core_observers() == 3));

    let close = json!({"protocol": 1, "scope": {"kind": "closeShell", "updates": "__CHANNEL__:8"}});
    assert_eq!(
        call(&h.main, "desktop_observe", raw(&close)),
        Ok(json!({"kind": "closed"}))
    );
    assert!(
        eventually(|| host.active_observations("main") == 2 && host.core_observers() == 2),
        "closing retires the native task and the core subscriber without further updates"
    );

    // Re-observing on the same channel token replaces, never duplicates.
    observe_shell(&h, 7);
    assert!(eventually(
        || host.active_observations("main") == 2 && host.core_observers() == 2
    ));

    // Page reload / window teardown retire everything the webview held, but
    // native business state is untouched (delivery still works afterwards).
    host.retire_webview("main");
    assert!(eventually(
        || host.active_observations("main") == 0 && host.core_observers() == 0
    ));
    let session = register(&h);
    assert_eq!(
        dispatch(&h, &session, "1", "dark").expect("dispatch")["outcome"],
        json!("receipt")
    );
}

#[test]
fn observations_per_webview_are_bounded_without_silent_eviction() {
    let h = harness();
    let host = h.app.state::<desktop_host::Host>();
    let cap = desktop_host::MAX_OBSERVATIONS_PER_WEBVIEW as u32;
    for token in 1..=cap {
        assert_eq!(observe_shell(&h, token)["kind"], json!("shell"));
    }
    for token in cap + 1..=cap + 5 {
        let body = json!({"protocol": 1, "scope": {"kind": "shell", "updates": format!("__CHANNEL__:{token}")}});
        assert_eq!(
            call(&h.main, "desktop_observe", raw(&body)),
            Err(json!("busy")),
            "observation beyond the cap is refused"
        );
    }
    assert_eq!(
        host.active_observations("main"),
        desktop_host::MAX_OBSERVATIONS_PER_WEBVIEW,
        "existing observations stay live"
    );
    assert!(
        eventually(|| host.core_observers() == desktop_host::MAX_OBSERVATIONS_PER_WEBVIEW),
        "refused observations leave no core subscriber: {}",
        host.core_observers()
    );
    // Closing one frees a slot for a new observation.
    let close = json!({"protocol": 1, "scope": {"kind": "closeShell", "updates": "__CHANNEL__:1"}});
    call(&h.main, "desktop_observe", raw(&close)).expect("close");
    assert!(eventually(
        || host.active_observations("main") == desktop_host::MAX_OBSERVATIONS_PER_WEBVIEW - 1
    ));
    assert_eq!(observe_shell(&h, 100)["kind"], json!("shell"));
}

fn send(h: &Harness, session: &str, sequence: u64, intent: Value) -> Result<Value, Value> {
    let body = json!({"protocol": 1, "session": session, "sequence": sequence.to_string(), "intent": intent});
    call(&h.main, "desktop_dispatch", raw(&body))
}

fn snapshot(h: &Harness, token: u32) -> Value {
    let observed = observe_shell(h, token);
    let body = json!({"protocol": 1, "scope": {"kind": "closeShell", "updates": format!("__CHANNEL__:{token}")}});
    call(&h.main, "desktop_observe", raw(&body)).expect("close");
    observed["snapshot"].clone()
}

#[test]
fn tenant_and_scan_key_intents_cross_ipc_without_echoing_the_key() {
    let h = harness();
    let session = register(&h);
    let created = send(
        &h,
        &session,
        1,
        json!({"kind": "createTenant", "profile": {
        "label": "Tenant A", "tsgId": "1234567890", "destination": {"kind": "region", "region": "europe"}}}),
    );
    assert_eq!(created.expect("created")["outcome"], "receipt");
    let snap = snapshot(&h, 10);
    let context = snap["active"]["context"]
        .as_str()
        .expect("context")
        .to_owned();
    assert_eq!(
        snap["tenants"][0]["origin"],
        "https://service-de.api.aisecurity.paloaltonetworks.com"
    );
    let key = "synthetic-ipc-scan-key-0042";
    let stored = send(
        &h,
        &session,
        2,
        json!({"kind": "configureScanCredential",
        "context": context, "mode": "session", "key": key}),
    );
    let stored = stored.expect("admitted");
    assert_eq!(stored["outcome"], "receipt");
    assert!(
        !stored.to_string().contains(key),
        "the receipt carries no key"
    );
    assert!(eventually(
        || snapshot(&h, 11)["tenants"][0]["scan"]["state"] == "ready"
    ));
    let snap = snapshot(&h, 12);
    assert_eq!(snap["tenants"][0]["scan"]["mode"], "session");
    assert!(
        !snap.to_string().contains(key),
        "no snapshot carries the key"
    );
    assert!(
        h.adapters.vault.calls().is_empty(),
        "session-only never reaches the Keychain"
    );
    let stale = send(
        &h,
        &session,
        3,
        json!({"kind": "removeScanCredential", "context": context}),
    );
    assert_eq!(
        stale.expect("refusal"),
        json!({"outcome": "refusal", "refusal": "staleContext"})
    );
}

#[test]
#[allow(unsafe_code)]
fn ambient_environment_credentials_are_never_adopted() {
    // SAFETY: these SDK variable names are read by nothing else in this
    // test binary; the desktop never consults them.
    unsafe {
        std::env::set_var("PANW_AI_SEC_API_KEY", "synthetic-ambient-key");
        std::env::set_var("PANW_AI_SEC_API_ENDPOINT", "https://ambient.example.com");
    }
    let h = harness();
    let session = register(&h);
    send(
        &h,
        &session,
        1,
        json!({"kind": "createTenant", "profile": {
        "label": "Tenant A", "destination": {"kind": "region", "region": "unitedStates"}}}),
    )
    .expect("created");
    let snap = snapshot(&h, 20);
    assert_eq!(
        snap["tenants"][0]["scan"]["state"], "missing",
        "no implicit credential"
    );
    assert_eq!(
        snap["tenants"][0]["origin"],
        "https://service.api.aisecurity.paloaltonetworks.com"
    );
    assert!(!snap.to_string().contains("ambient"));
    assert!(h.adapters.vault.calls().is_empty());
    // SAFETY: as above; only this test reads or writes these variables.
    unsafe {
        std::env::remove_var("PANW_AI_SEC_API_KEY");
        std::env::remove_var("PANW_AI_SEC_API_ENDPOINT");
    }
}

/// C28 (D058): the native Go menu follows the presentation even when the
/// shell observation that would have carried the change was replaced by a
/// `Resync` (an overflowing stream): every new shell subscription reconciles
/// the menu with the fresh snapshot, so the client's resubscribe repairs it.
/// The mock runtime installs no native menu; the host records the variant it
/// reconciled with, which is what `menu::rebuild` is then called with.
#[test]
fn the_go_menu_is_reconciled_on_every_shell_subscription() {
    let h = harness();
    let session = register(&h);
    let host = h.app.state::<desktop_host::Host>();
    assert_eq!(
        host.menu_variant(),
        None,
        "nothing reconciled before a shell observation"
    );
    call(
        &h.main,
        "desktop_observe",
        raw(&fixture("observe-shell-request.json")),
    )
    .expect("observe");
    assert_eq!(
        host.menu_variant(),
        Some(desktop_core::features::UiVariant::Legacy)
    );
    // The variant changes; this subscription's update is treated as lost (the
    // client resubscribes as it does after a Resync).
    let mut body = fixture("dispatch-request.json");
    body["session"] = json!(session);
    body["sequence"] = json!("1");
    body["intent"] = json!({"kind": "setUiVariant", "variant": "v2"});
    call(&h.main, "desktop_dispatch", raw(&body)).expect("v2");
    let mut again = fixture("observe-shell-request.json");
    again["scope"]["updates"] = json!("__CHANNEL__:9");
    call(&h.main, "desktop_observe", raw(&again)).expect("resubscribe");
    assert_eq!(
        host.menu_variant(),
        Some(desktop_core::features::UiVariant::V2)
    );
    body["sequence"] = json!("2");
    body["intent"] = json!({"kind": "setUiVariant", "variant": "legacy"});
    call(&h.main, "desktop_dispatch", raw(&body)).expect("legacy");
    again["scope"]["updates"] = json!("__CHANNEL__:10");
    call(&h.main, "desktop_observe", raw(&again)).expect("resubscribe again");
    assert_eq!(
        host.menu_variant(),
        Some(desktop_core::features::UiVariant::Legacy)
    );
    let _ = &h.adapters;
}
