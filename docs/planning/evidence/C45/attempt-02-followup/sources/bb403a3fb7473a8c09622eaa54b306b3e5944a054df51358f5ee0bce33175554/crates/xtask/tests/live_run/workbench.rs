//! L15 API Client against a synthetic HTTPS echo endpoint and the
//! synthetic cloud's IAM access policies (an approved read, and a write
//! held for review, applied once, then removed).

use std::sync::Arc;

use serde_json::{Value, json};

use crate::cloud::Cloud;
use crate::common::{
    Browser, CLIENT_SECRET, assert_no_secret, designate, keychain, management_family,
    management_secrets, manifest, outcomes, raw, receipt,
};
use crate::reads::served;
use crate::support::{Reply, Seen, TlsServer};

const TOKEN: &str = "synthetic-endpoint-token-c25";

/// An httpbin-like echo service.
fn echo() -> Reply {
    Reply::Route(Arc::new(|seen: &Seen| {
        let (_, path, query) = crate::common::parts(seen);
        let headers: serde_json::Map<String, Value> = seen
            .headers
            .iter()
            .map(|(k, v)| (k.clone(), json!(v)))
            .collect();
        match path.as_str() {
            "/headers" => raw(200, &json!({"headers": headers, "args": query})),
            "/cookies/set" => Reply::Raw {
                head: "HTTP/1.1 200 OK\r\nset-cookie: c25=1; Path=/\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close".into(),
                body: b"{}".to_vec(),
            },
            "/cookies" => raw(200, &json!({"cookies": seen.header("cookie")})),
            "/post" => raw(200, &json!({"data": String::from_utf8_lossy(&seen.body)})),
            "/bytes/2048" => Reply::Raw {
                head: "HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: 2048\r\nconnection: close".into(),
                body: vec![7u8; 2048],
            },
            "/bearer" if seen.header("authorization") == Some(&format!("Bearer {TOKEN}")) => {
                raw(200, &json!({"authenticated": true}))
            }
            "/bearer" => raw(401, &json!({"authenticated": false})),
            _ => raw(404, &json!({})),
        }
    }))
}

fn scope(echo: &TlsServer, write: bool) -> Value {
    let mut value = manifest();
    let mut nonsecret = json!({"endpoint": format!("https://localhost:{}", echo.port),
        "airs_read_path": "/iam/v1/access_policies"});
    if write {
        nonsecret["airs_write_method"] = json!("POST");
        nonsecret["airs_write_path"] = json!("/iam/v1/access_policies");
        nonsecret["airs_write_body"] = json!("{\"name\": \"airs-v2-c25-policy\"}");
        nonsecret["airs_cleanup_path"] = json!("/iam/v1/access_policies/airs-v2-c25-policy");
    }
    let mut family = management_family(json!({"nonsecret": nonsecret,
        "credentials": {"endpoint_token": keychain("endpoint")}}));
    family["disposable_prefix"] = json!("airs-v2-c25-");
    family["permitted_operations"] = json!(["read", "POST /iam/v1/access_policies"]);
    designate(&mut value, "L15", family);
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn the_api_client_sends_exactly_and_holds_tenant_writes_for_review() {
    let s = served(Cloud::new()).await;
    let echo = s.fixture.serve(echo()).await;
    let (repo, record) = s
        .fixture
        .run(
            scope(&echo, true),
            &["L15"],
            &management_secrets(&[("endpoint", TOKEN)]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let checks = outcomes(&record, "L15");
    assert!(checks.iter().all(|c| c.ends_with(": passed")), "{checks:?}");
    assert_eq!(
        receipt(&repo, "L15", "L15-download").observed["saved_bytes"],
        json!(2048)
    );
    let write = receipt(&repo, "L15", "L15-airs-reviewed-write");
    assert_eq!(write.observed["held_for_review"], json!(true));
    assert_eq!(write.observed["cleanup"], json!("passed"));
    // Every receipt and the run name what produced them.
    for key in ["source_fingerprint", "runner_sha256"] {
        let value = write.implementation.get(key).expect(key);
        assert!(!value.is_empty(), "{key}");
        assert_eq!(record.implementation.get(key), Some(value), "{key}");
    }
    // Written once, then removed.
    let cloud = s.cloud.lock().expect("cloud");
    assert!(cloud.policies.is_empty());
    let writes: Vec<&String> = cloud
        .writes
        .iter()
        .filter(|w| w.contains("access_policies"))
        .collect();
    assert_eq!(writes.len(), 2, "{writes:?}");
    drop(cloud);
    assert_no_secret(&repo, &[CLIENT_SECRET, TOKEN]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_that_does_not_name_the_prefix_is_never_sent() {
    let s = served(Cloud::new()).await;
    let echo = s.fixture.serve(echo()).await;
    let mut value = scope(&echo, true);
    value["families"]["L15"]["nonsecret"]["airs_write_body"] =
        json!("{\"name\": \"production-policy\"}");
    let (repo, record) = s
        .fixture
        .run(
            value,
            &["L15"],
            &management_secrets(&[("endpoint", TOKEN)]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let write = receipt(&repo, "L15", "L15-airs-reviewed-write");
    assert_eq!(write.outcome, "blocked");
    assert!(
        !s.cloud
            .lock()
            .expect("cloud")
            .writes
            .iter()
            .any(|w| w.contains("access_policies"))
    );
    assert!(!record.passed());
}

/// A non-echo endpoint and a topic write cleaned up by the ID its response
/// reports.
fn by_id_scope(echo: &TlsServer, topic_name: &str) -> Value {
    let mut value = scope(echo, false);
    let nonsecret = &mut value["families"]["L15"]["nonsecret"];
    nonsecret["echo"] = json!("no");
    nonsecret["airs_write_method"] = json!("POST");
    nonsecret["airs_write_path"] = json!("/aisec/v1/mgmt/topic");
    nonsecret["airs_write_body"] = json!(format!(
        "{{\"topic_name\": \"{topic_name}\", \"description\": \"airs-v2-c25- workbench probe\", \"examples\": [\"c25\"], \"active\": true}}"
    ));
    nonsecret["airs_cleanup_path"] = json!("/aisec/v1/mgmt/topic/{created_id}");
    nonsecret["airs_created_id_field"] = json!("topic_id");
    nonsecret["airs_created_name_field"] = json!("topic_name");
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn a_non_echo_endpoint_proves_only_the_download_and_a_write_is_removed_by_its_reported_id() {
    let s = served(Cloud::new()).await;
    let echo = s.fixture.serve(echo()).await;
    let (repo, record) = s
        .fixture
        .run(
            by_id_scope(&echo, "airs-v2-c25-wb-topic"),
            &["L15"],
            &management_secrets(&[("endpoint", TOKEN)]),
            Browser::idle(),
        )
        .await;
    record.expect("run");
    for check in [
        "L15-headers-and-query",
        "L15-cookies",
        "L15-multipart-upload",
        "L15-stored-secret",
    ] {
        let r = receipt(&repo, "L15", check);
        assert_eq!(r.outcome, "blocked", "{check}");
        assert!(r.detail.contains("does not echo"), "{check}: {}", r.detail);
    }
    assert_eq!(receipt(&repo, "L15", "L15-download").outcome, "passed");
    assert_eq!(receipt(&repo, "L15", "L15-airs-read").outcome, "passed");
    let write = receipt(&repo, "L15", "L15-airs-reviewed-write");
    assert_eq!(write.outcome, "passed", "{}", write.detail);
    assert_eq!(write.observed["cleanup"], json!("passed"));
    let cloud = s.cloud.lock().expect("cloud");
    assert!(
        !cloud
            .topics
            .iter()
            .any(|t| t["topic_name"] == "airs-v2-c25-wb-topic")
    );
    let deletes: Vec<&String> = cloud
        .writes
        .iter()
        .filter(|w| w.starts_with("DELETE /aisec/v1/mgmt/topic/33333333-"))
        .collect();
    assert_eq!(deletes.len(), 1, "{:?}", cloud.writes);
    drop(cloud);
    assert_no_secret(&repo, &[CLIENT_SECRET, TOKEN]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_created_resource_outside_the_prefix_is_not_deleted_and_stops_the_run() {
    let s = served(Cloud::new()).await;
    let echo = s.fixture.serve(echo()).await;
    let (repo, record) = s
        .fixture
        .run(
            by_id_scope(&echo, "Customer topic"),
            &["L15"],
            &management_secrets(&[("endpoint", TOKEN)]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let write = receipt(&repo, "L15", "L15-airs-reviewed-write");
    assert_eq!(write.outcome, "needs-reconciliation", "{}", write.detail);
    assert!(
        write
            .detail
            .contains("no created resource under the prefix")
    );
    let cloud = s.cloud.lock().expect("cloud");
    assert!(!cloud.writes.iter().any(|w| w.starts_with("DELETE")));
    drop(cloud);
    assert!(!record.passed());
}
