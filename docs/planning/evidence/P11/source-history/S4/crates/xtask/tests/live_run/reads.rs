//! L02 management reads, L05 sessions, L06 Red Team/Model reads, L07
//! Gateway reads and L10 operational observation against the synthetic
//! cloud.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::cloud::{Cloud, RED_TEAM_SCAN, Restricted, WS_A};
use crate::common::{
    Browser, CLIENT_SECRET, Fixture, RESTRICTED_SECRET, assert_no_secret, designate,
    management_family, management_secrets, manifest, outcomes, receipt, routed,
};
use crate::support::TlsServer;

pub struct Served {
    pub fixture: Fixture,
    pub cloud: Arc<Mutex<Cloud>>,
    pub server: TlsServer,
}

pub async fn served(cloud: Cloud) -> Served {
    let mut fixture = Fixture::new();
    let cloud = Arc::new(Mutex::new(cloud));
    let server = fixture.serve(routed(&cloud, Cloud::route)).await;
    fixture.management = Some(format!("https://localhost:{}", server.port));
    Served {
        fixture,
        cloud,
        server,
    }
}

fn scope() -> Value {
    let mut value = manifest();
    designate(
        &mut value,
        "L02",
        management_family(json!({"nonsecret": {"restricted_denied_resource": "apiKeys"}})),
    );
    designate(
        &mut value,
        "L05",
        management_family(json!({"nonsecret": {
        "session_id": "ses-0", "session_app_id": "app-0", "session_app_name": "Bot 0",
        "scan_id": "scn-ses-0-0", "reveal_content": "synthetic"}})),
    );
    designate(
        &mut value,
        "L06",
        management_family(json!({"nonsecret": {"red_team_scan_id": RED_TEAM_SCAN}})),
    );
    designate(
        &mut value,
        "L07",
        management_family(
            json!({"nonsecret": {"workspace_id": WS_A, "restricted_denied_resource": "workspaces"}}),
        ),
    );
    designate(
        &mut value,
        "L10",
        management_family(json!({"nonsecret": {"workspace_id": WS_A}})),
    );
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn the_read_families_pass_through_the_production_management_intent() {
    let s = served(Cloud::new()).await;
    let (repo, record) = s
        .fixture
        .run(
            scope(),
            &["L02", "L05", "L06", "L07", "L10"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    for family in ["L02", "L05", "L06", "L07", "L10"] {
        let checks = outcomes(&record, family);
        assert!(
            checks.iter().all(|c| c.ends_with(": passed")),
            "{family}: {checks:?}"
        );
    }
    assert!(record.passed(), "{record:?}");

    let inventories = receipt(&repo, "L02", "L02-inventories");
    assert_eq!(inventories.observed["pages"]["profiles"]["count"], json!(3));
    assert_eq!(
        inventories.observed["pages"]["dataPatterns"]["result"],
        json!("page")
    );
    let paging = receipt(&repo, "L02", "L02-pagination");
    assert_eq!(paging.observed["first"]["next_offset"], json!(1));
    assert_eq!(paging.observed["second"]["offset"], json!(1));
    let denied = receipt(&repo, "L02", "L02-least-privilege-denied");
    assert_eq!(denied.observed["read"]["failure"], json!("denied"));
    assert_eq!(
        receipt(&repo, "L02", "L02-verify").observed["scan_key_used"],
        json!(false)
    );
    // Content is revealed for the designated synthetic scan only, and only
    // its length is kept.
    let content = receipt(&repo, "L05", "L05-content-reveal");
    assert_eq!(
        content.observed["lengths"]["prompt_chars"],
        json!("SYNTHETIC-PROMPT-c25".len())
    );
    assert_eq!(
        receipt(&repo, "L07", "L07-least-privilege-denied").outcome,
        "passed"
    );
    assert_eq!(
        receipt(&repo, "L10", "L10-gateway-observation").observed["observations"]["usage"]["result"],
        json!("gateway")
    );
    // Reads only: nothing was written to the cloud.
    assert!(s.cloud.lock().expect("cloud").writes.is_empty());
    assert_no_secret(
        &repo,
        &[
            CLIENT_SECRET,
            RESTRICTED_SECRET,
            "SYNTHETIC-PROMPT-c25",
            "c25-access-main",
        ],
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restricted_principal_that_can_read_stops_the_run_as_unexpected_authority() {
    let s = served(Cloud::new()).await;
    let mut value = scope();
    // The "restricted" principal is the designated one: it reads.
    value["families"]["L02"]["nonsecret"]["restricted_client_id"] = json!(crate::common::CLIENT_ID);
    value["families"]["L02"]["credentials"]["restricted_client_secret"] =
        crate::common::keychain("client");
    let (repo, record) = s
        .fixture
        .run(
            value,
            &["L02", "L05"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let stop = record.stopped.expect("stopped");
    assert_eq!(stop.check, "L02-least-privilege-denied");
    assert!(stop.reason.contains("unexpected authority"));
    assert_eq!(
        receipt(&repo, "L02", "L02-least-privilege-denied").outcome,
        "failed"
    );
    assert_eq!(record.families[1].status, "blocked");
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_designations_are_blocked_never_passed() {
    let s = served(Cloud::new()).await;
    let mut value = scope();
    value["families"]["L05"]["nonsecret"] =
        json!({"client_id": crate::common::CLIENT_ID, "tsg_id": crate::common::TSG});
    let (repo, record) = s
        .fixture
        .run(value, &["L05"], &management_secrets(&[]), Browser::idle())
        .await;
    let record = record.expect("run");
    assert_eq!(record.families[0].status, "blocked");
    assert_eq!(receipt(&repo, "L05", "L05-inventory").outcome, "passed");
    assert_eq!(
        receipt(&repo, "L05", "L05-session-detail").outcome,
        "blocked"
    );
    assert_eq!(
        receipt(&repo, "L05", "L05-content-reveal").outcome,
        "blocked"
    );
}

/// L02 in write-denial mode: a disposable prefix and no read resource.
fn write_denial_scope() -> Value {
    let mut value = manifest();
    designate(
        &mut value,
        "L02",
        management_family(
            json!({"disposable_prefix": "c25-", "cleanup": ["delete the probe topic"]}),
        ),
    );
    value
}

/// L07 with a disposable prefix: its least privilege is a refused write.
fn gateway_write_denial_scope() -> Value {
    let mut value = manifest();
    designate(
        &mut value,
        "L07",
        management_family(json!({"disposable_prefix": "c25-",
            "cleanup": ["delete the probe service key"], "nonsecret": {"workspace_id": WS_A}})),
    );
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_principals_gateway_write_is_refused_and_read_back_as_absent() {
    let mut cloud = Cloud::new();
    cloud.restricted = Restricted::ReadOnly;
    let s = served(cloud).await;
    let (repo, record) = s
        .fixture
        .run(
            gateway_write_denial_scope(),
            &["L07"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    assert!(record.passed(), "{record:?}");
    let denied = receipt(&repo, "L07", "L07-least-privilege-denied");
    assert_eq!(denied.outcome, "passed", "{}", denied.detail);
    assert_eq!(denied.inputs["mode"], json!("write"));
    assert_eq!(denied.inputs["kind"], json!("gatewayServiceKeys"));
    assert_eq!(denied.readback, "absent");
    assert_eq!(denied.observed["failure"]["failure"], json!("denied"));
    let name = denied.observed["probe_name"].as_str().expect("probe name");
    assert!(name.starts_with("c25-lp-"), "{name}");
    let cloud = s.cloud.lock().expect("cloud");
    assert!(cloud.writes.is_empty(), "{:?}", cloud.writes);
    drop(cloud);
    assert_no_secret(&repo, &[CLIENT_SECRET, RESTRICTED_SECRET]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restricted_gateway_write_that_is_applied_is_deleted_and_stops_the_run() {
    let mut cloud = Cloud::new();
    cloud.restricted = Restricted::WritesAllowed;
    let s = served(cloud).await;
    let (repo, record) = s
        .fixture
        .run(
            gateway_write_denial_scope(),
            &["L07"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let stop = record.stopped.clone().expect("stopped");
    assert_eq!(stop.check, "L07-least-privilege-denied");
    assert!(stop.reason.contains("unexpected authority"));
    let denied = receipt(&repo, "L07", "L07-least-privilege-denied");
    assert_eq!(denied.readback, "found");
    let name = denied.observed["probe_name"]
        .as_str()
        .expect("probe name")
        .to_owned();
    let cloud = s.cloud.lock().expect("cloud");
    // The probe was created by the faulty principal, then deleted.
    assert!(
        cloud.items["api-keys/service"]
            .iter()
            .all(|k| k["name"] != name.as_str()),
        "the probe key remains"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_only_principals_write_is_refused_and_read_back_as_absent() {
    let mut cloud = Cloud::new();
    cloud.restricted = Restricted::ReadOnly;
    let s = served(cloud).await;
    let (repo, record) = s
        .fixture
        .run(
            write_denial_scope(),
            &["L02"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    assert!(record.passed(), "{record:?}");
    let denied = receipt(&repo, "L02", "L02-least-privilege-denied");
    assert_eq!(denied.outcome, "passed", "{}", denied.detail);
    assert_eq!(denied.inputs["mode"], json!("write"));
    assert_eq!(denied.readback, "absent");
    assert_eq!(denied.observed["outcome"], json!("rejected"));
    assert_eq!(denied.observed["failure"]["failure"], json!("denied"));
    let name = denied.observed["probe_name"].as_str().expect("probe name");
    assert!(name.starts_with("c25-lp-"), "{name}");
    let cloud = s.cloud.lock().expect("cloud");
    // The refused write never reached the store, and nothing else was written.
    assert!(cloud.writes.is_empty(), "{:?}", cloud.writes);
    assert!(cloud.topics.iter().all(|t| t["topic_name"] != name));
    drop(cloud);
    assert_no_secret(&repo, &[CLIENT_SECRET, RESTRICTED_SECRET]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restricted_write_that_is_applied_is_deleted_and_stops_the_run() {
    let mut cloud = Cloud::new();
    cloud.restricted = Restricted::WritesAllowed;
    let s = served(cloud).await;
    let (repo, record) = s
        .fixture
        .run(
            write_denial_scope(),
            &["L02"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let stop = record.stopped.clone().expect("stopped");
    assert_eq!(stop.check, "L02-least-privilege-denied");
    assert!(stop.reason.contains("unexpected authority"));
    assert!(!stop.reconcile, "the full principal's delete settled");
    let denied = receipt(&repo, "L02", "L02-least-privilege-denied");
    assert_eq!(denied.outcome, "failed");
    assert_eq!(denied.readback, "found");
    assert!(denied.cleanup.starts_with("deleted by the full principal"));
    let name = denied.observed["probe_name"].as_str().expect("probe name");
    let cloud = s.cloud.lock().expect("cloud");
    assert!(cloud.topics.iter().all(|t| t["topic_name"] != name));
    assert_eq!(
        cloud
            .writes
            .iter()
            .filter(|w| w.starts_with("DELETE"))
            .count(),
        1,
        "{:?}",
        cloud.writes
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn without_a_prefix_the_write_denial_is_blocked() {
    let s = served(Cloud::new()).await;
    let mut value = manifest();
    designate(&mut value, "L02", management_family(json!({})));
    let (repo, _) = s
        .fixture
        .run(value, &["L02"], &management_secrets(&[]), Browser::idle())
        .await;
    let denied = receipt(&repo, "L02", "L02-least-privilege-denied");
    assert_eq!(denied.outcome, "blocked");
    assert!(denied.detail.contains("disposable_prefix"));
}

/// L05 without a designated session, and L01 naming the app its synthetic
/// scans carry.
fn discovery_scope(app: &str) -> Value {
    let mut value = manifest();
    designate(
        &mut value,
        "L01",
        json!({
            "credentials": {"scan_key": crate::common::keychain("scan-a")},
            "nonsecret": {"region": "us", "profile_name": "c25 profile", "app_name": app},
            "permitted_operations": ["scan"],
            "budget": {"scans": 1}
        }),
    );
    designate(
        &mut value,
        "L05",
        management_family(json!({"nonsecret": {"reveal_content": "synthetic"}})),
    );
    designate(&mut value, "L06", management_family(json!({})));
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn l05_discovers_only_the_synthetic_apps_newest_session() {
    let s = served(Cloud::new()).await;
    let (repo, record) = s
        .fixture
        .run(
            discovery_scope("Bot 0"),
            &["L05"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    assert!(record.passed(), "{record:?}");
    let detail = receipt(&repo, "L05", "L05-session-detail");
    // ses-0 and ses-2 belong to Bot 0; ses-1 (Bot 1) is never chosen.
    assert_eq!(detail.ids["session_id"], "ses-2");
    assert_eq!(detail.observed["discovered"]["candidates"], json!(2));
    assert_eq!(
        detail.observed["discovered"]["chosen_by"],
        json!("newest last_seen")
    );
    assert_eq!(detail.ids["scan_id"], "scn-ses-2-0");
    let opened: Vec<String> = s
        .server
        .seen()
        .iter()
        .filter(|r| r.request_line.contains("/sessions/session?"))
        .map(|r| r.request_line.clone())
        .collect();
    assert!(!opened.is_empty());
    assert!(
        opened
            .iter()
            .all(|p| p.contains("ses-2") && p.contains("app-0")),
        "{opened:?}"
    );
    assert_eq!(
        receipt(&repo, "L05", "L05-content-reveal").outcome,
        "passed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn l05_with_no_session_of_the_synthetic_app_is_blocked() {
    let s = served(Cloud::new()).await;
    let (repo, _) = s
        .fixture
        .run(
            discovery_scope("Some other app"),
            &["L05"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let detail = receipt(&repo, "L05", "L05-session-detail");
    assert_eq!(detail.outcome, "blocked");
    assert!(detail.detail.contains("Some other app"));
    assert!(
        s.server
            .seen()
            .iter()
            .all(|r| !r.request_line.contains("/sessions/session?"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn l06_discovers_the_newest_completed_scan_or_is_blocked_without_one() {
    let s = served(Cloud::new()).await;
    let (repo, record) = s
        .fixture
        .run(
            discovery_scope("Bot 0"),
            &["L06"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    assert!(record.expect("run").passed());
    let attacks = receipt(&repo, "L06", "L06-attack-report");
    assert_eq!(attacks.ids["red_team_scan_id"], RED_TEAM_SCAN);
    assert!(attacks.observed.contains_key("discovered"));

    let mut cloud = Cloud::new();
    cloud.red_team_scans = false;
    let s = served(cloud).await;
    let (repo, _) = s
        .fixture
        .run(
            discovery_scope("Bot 0"),
            &["L06"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let attacks = receipt(&repo, "L06", "L06-attack-report");
    assert_eq!(attacks.outcome, "blocked");
    assert!(
        s.server
            .seen()
            .iter()
            .all(|r| !r.request_line.contains("list-attacks"))
    );
}
