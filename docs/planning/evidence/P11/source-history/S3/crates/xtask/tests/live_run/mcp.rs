//! L17 Workspace MCP against the synthetic cloud and the desktop-core MCP
//! plane fixture: the catalog, the desktop's own OAuth sign-in completed by
//! the operator stand-in in the "browser", a scope change the operator
//! makes, and revocation. Tool calls are recorded as needing the app.

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use crate::cloud::{Cloud, RUNTIME, WS_A};
use crate::common::{
    Browser, CLIENT_SECRET, Fixture, assert_no_secret, designate, keychain, management_family,
    management_secrets, manifest, outcomes, receipt,
};
use crate::mcp_fixture::Mcp;
use crate::support::Reply;

const SERVER: &str = "mcp-tools-1";
const INFERENCE_KEY: &str = "synthetic-inference-key-c25";

/// One HTTP GET to the desktop's loopback callback.
fn callback(url: &str) {
    let Ok(url) = url::Url::parse(url) else {
        return;
    };
    let Some(port) = url.port() else { return };
    let Ok(mut stream) = std::net::TcpStream::connect(("127.0.0.1", port)) else {
        return;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let target = format!("{}?{}", url.path(), url.query().unwrap_or_default());
    let _ = write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    );
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_operator_signs_in_changes_scope_and_the_desktop_revokes() {
    let mut fixture = Fixture::new();
    let cloud = Arc::new(Mutex::new(Cloud::new()));
    let mcp: Arc<Mutex<Option<Mcp>>> = Arc::new(Mutex::new(None));
    let (c, m) = (Arc::clone(&cloud), Arc::clone(&mcp));
    let server = fixture
        .serve(Reply::Route(Arc::new(move |seen| {
            if let Some(reply) = m.lock().expect("mcp").as_mut().and_then(|m| m.route(seen)) {
                return reply;
            }
            c.lock().expect("cloud").route(seen)
        })))
        .await;
    let origin = format!("https://localhost:{}", server.port);
    {
        let mut plane = Mcp::new(&origin);
        plane.publish(SERVER, "tools", WS_A);
        *mcp.lock().expect("mcp") = Some(plane);
    }
    fixture.management = Some(origin.clone());
    let acting = Arc::clone(&mcp);
    let operator = Browser::new(move |instruction, url| {
        if let Some(url) = url.filter(|u| u.path().ends_with("/authorize")) {
            let back = acting
                .lock()
                .expect("mcp")
                .as_mut()
                .map(|m| m.authorize(url));
            if let Some(back) = back {
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    callback(&back);
                });
            }
        }
        if instruction.contains("change the designated MCP server's scope")
            && let Some(m) = acting.lock().expect("mcp").as_mut()
        {
            m.servers[0].updated = "2026-10-25T00:00:00Z".into();
        }
    });
    let mut value = manifest();
    designate(
        &mut value,
        "L17",
        management_family(json!({
            "disposable_prefix": "c25-", "cleanup": ["sign out and revoke the MCP grant"],
            "nonsecret": {"workspace_id": WS_A, "mcp_server_id": SERVER, "scope_change": "designated",
                "inference_endpoint": format!("{origin}{RUNTIME}")},
            "credentials": {"inference_key": keychain("inference")}
        })),
    );
    let (repo, record) = fixture
        .run(
            value,
            &["L17"],
            &management_secrets(&[("inference", INFERENCE_KEY)]),
            operator,
        )
        .await;
    let record = record.expect("run");
    let checks = outcomes(&record, "L17");
    let remote: Vec<&String> = checks
        .iter()
        .filter(|c| !c.ends_with("native-app-required"))
        .collect();
    assert!(remote.iter().all(|c| c.ends_with(": passed")), "{checks:?}");
    assert_eq!(record.families[0].status, "needs-native-app");
    let sign_in = receipt(&repo, "L17", "L17-sign-in");
    assert_eq!(
        sign_in.operator_step.expect("step")["completed"],
        json!(true)
    );
    assert!(
        crate::common::within(|| mcp
            .lock()
            .expect("mcp")
            .as_ref()
            .is_some_and(|p| !p.revoked.is_empty()))
        .await,
        "signing out revoked the tokens"
    );
    let plane = mcp.lock().expect("mcp");
    let plane = plane.as_ref().expect("plane");
    assert_eq!(plane.authorized.len(), 1, "the operator authorized once");
    assert_no_secret(
        &repo,
        &[CLIENT_SECRET, INFERENCE_KEY, "mcp-access-", "mcp-refresh-"],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_server_found_by_name_waits_for_the_operator_when_absent() {
    let mut fixture = Fixture::new();
    let cloud = Arc::new(Mutex::new(Cloud::new()));
    let mcp: Arc<Mutex<Option<Mcp>>> = Arc::new(Mutex::new(None));
    let (c, m) = (Arc::clone(&cloud), Arc::clone(&mcp));
    let server = fixture
        .serve(Reply::Route(Arc::new(move |seen| {
            if let Some(reply) = m.lock().expect("mcp").as_mut().and_then(|m| m.route(seen)) {
                return reply;
            }
            c.lock().expect("cloud").route(seen)
        })))
        .await;
    let origin = format!("https://localhost:{}", server.port);
    {
        let mut plane = Mcp::new(&origin);
        plane.publish(SERVER, "tools", WS_A);
        *mcp.lock().expect("mcp") = Some(plane);
    }
    fixture.management = Some(origin.clone());
    let mut value = manifest();
    designate(
        &mut value,
        "L17",
        management_family(json!({
            "disposable_prefix": "c25-", "cleanup": ["sign out and revoke the MCP grant"],
            "nonsecret": {"workspace_id": WS_A, "mcp_server_name": "TOOL",
                "operator_present": "no", "inference_endpoint": format!("{origin}{RUNTIME}")},
            "credentials": {"inference_key": keychain("inference")}
        })),
    );
    let (repo, record) = fixture
        .run(
            value,
            &["L17"],
            &management_secrets(&[("inference", INFERENCE_KEY)]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let discovery = receipt(&repo, "L17", "L17-discovery");
    assert_eq!(discovery.outcome, "passed", "{}", discovery.detail);
    assert_eq!(discovery.ids["mcp_server_id"], SERVER);
    assert_eq!(
        discovery.observed["discovered"]["by"],
        json!("mcp_server_name")
    );
    for check in ["L17-sign-in", "L17-scope-change", "L17-revocation"] {
        let r = receipt(&repo, "L17", check);
        assert_eq!(r.outcome, "blocked", "{check}");
        assert!(r.detail.contains("operator was not present"), "{check}");
    }
    assert!(!record.passed());
    let plane = mcp.lock().expect("mcp");
    assert!(
        plane.as_ref().expect("plane").authorized.is_empty(),
        "no sign-in was attempted"
    );
}
