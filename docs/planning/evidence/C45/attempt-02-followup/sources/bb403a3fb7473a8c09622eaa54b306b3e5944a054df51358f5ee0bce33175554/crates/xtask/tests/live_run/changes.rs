//! L03 Runtime changes, L04 DLP and L08 Gateway changes against the
//! synthetic cloud: review, the exact typed confirmation, the armed native
//! confirmation, one-time secrets forgotten unseen, readback, and every
//! created resource deleted again.

use serde_json::{Value, json};

use crate::cloud::{Cloud, ONE_TIME};
use crate::common::{
    Browser, CLIENT_SECRET, assert_no_secret, designate, management_family, management_secrets,
    manifest, outcomes, receipt,
};
use crate::reads::served;

const PREFIX: &str = "airs-v2-c25-";

fn mutating(extra: Value) -> Value {
    let mut family = management_family(extra);
    family["disposable_prefix"] = json!(PREFIX);
    family["cleanup"] = json!([format!(
        "delete every {PREFIX} resource and read back its absence"
    )]);
    family["permitted_operations"] = json!(["create", "update", "delete", "rotate"]);
    family
}

fn scope() -> Value {
    let mut value = manifest();
    let mut l03 = mutating(
        json!({"nonsecret": {"deployment_profile": "dp-1", "profile_template": "Profile 0"}}),
    );
    l03["budget"] = json!({"writes": 20});
    designate(&mut value, "L03", l03);
    designate(
        &mut value,
        "L04",
        mutating(json!({"nonsecret": {
        "data_profile_id": "dprof-1", "filtering_profile_id": "filter-1"}})),
    );
    designate(
        &mut value,
        "L08",
        mutating(json!({"nonsecret": {"route_provider": "@openai-prod"}})),
    );
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_dlp_and_gateway_changes_apply_once_and_clean_up() {
    let s = served(Cloud::new()).await;
    let (repo, record) = s
        .fixture
        .run(
            scope(),
            &["L03", "L04", "L08"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    for family in ["L03", "L04", "L08"] {
        let checks = outcomes(&record, family);
        assert!(
            checks.iter().all(|c| c.ends_with(": passed")),
            "{family}: {checks:?}"
        );
    }
    assert!(record.passed(), "{record:?}");
    let cloud = s.cloud.lock().expect("cloud");
    // Nothing the run created is left behind.
    assert!(!cloud.topics.iter().any(|t| {
        t["topic_name"]
            .as_str()
            .is_some_and(|n| n.starts_with(PREFIX))
    }));
    assert!(!cloud.profiles.iter().any(|t| {
        t["profile_name"]
            .as_str()
            .is_some_and(|n| n.starts_with(PREFIX))
    }));
    assert!(!cloud.keys.iter().any(|t| {
        t["api_key_name"]
            .as_str()
            .is_some_and(|n| n.starts_with(PREFIX))
    }));
    assert!(!cloud.apps.iter().any(|t| {
        t["app_name"]
            .as_str()
            .is_some_and(|n| n.starts_with(PREFIX))
    }));
    // A deleted pattern is archived by the service (status `deleted`).
    assert!(
        cloud.dlp["data-patterns"]
            .iter()
            .all(|p| p["status"] == "deleted")
    );
    assert!(cloud.dlp["dictionaries"].is_empty());
    assert!(
        cloud.items["configs"]
            .iter()
            .all(|c| !c["name"].as_str().unwrap_or("").starts_with(PREFIX))
    );
    assert!(
        cloud
            .workspaces
            .iter()
            .filter(|w| w["name"].as_str().unwrap_or("").starts_with(PREFIX))
            .all(|w| w["status"] == "archived")
    );
    // The designated profiles were written back unchanged.
    assert_eq!(cloud.dlp["data-filtering-profiles"][0]["name"], "Default");
    drop(cloud);

    let key = receipt(&repo, "L03", "L03-key-create");
    assert_eq!(
        key.observed["one_time_secret"],
        json!("held, then forgotten unseen")
    );
    assert!(
        key.observed["confirmation_title"]
            .as_str()
            .is_some_and(|t| !t.is_empty())
    );
    assert_eq!(
        receipt(&repo, "L03", "L03-cleanup-topic").readback,
        "absent"
    );
    assert_eq!(
        receipt(&repo, "L08", "L08-provision-workspace").observed["provisioning"]["scope"],
        json!("completed")
    );
    assert_eq!(
        receipt(&repo, "L08", "L08-service-key-rotate").observed["one_time_secret"],
        json!("held, then forgotten unseen")
    );
    assert_no_secret(&repo, &[CLIENT_SECRET, ONE_TIME, "gw-ONE-TIME"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resource_this_run_did_not_create_is_never_updated_or_deleted() {
    let s = served(Cloud::new()).await;
    // The service refuses the pattern create: nothing labelled "pattern"
    // exists, so its replace and cleanup are never sent.
    let server = {
        let cloud = std::sync::Arc::clone(&s.cloud);
        s.fixture
            .serve(crate::support::Reply::Route(std::sync::Arc::new(
                move |seen| {
                    if seen
                        .request_line
                        .starts_with("POST /dlp/v2/api/data-patterns ")
                    {
                        return crate::common::raw(400, &json!({"error": "refused"}));
                    }
                    cloud.lock().expect("cloud").route(seen)
                },
            )))
            .await
    };
    let fixture = crate::common::Fixture {
        management: Some(format!("https://localhost:{}", server.port)),
        ..crate::common::Fixture {
            ca: s.fixture.ca,
            management: None,
            academy: None,
        }
    };
    let (repo, record) = fixture
        .run(scope(), &["L04"], &management_secrets(&[]), Browser::idle())
        .await;
    let record = record.expect("run");
    assert_eq!(
        receipt(&repo, "L04", "L04-pattern-create").outcome,
        "failed"
    );
    let replace = receipt(&repo, "L04", "L04-pattern-replace");
    assert_eq!(replace.outcome, "blocked");
    assert!(replace.detail.contains("nothing labelled pattern"));
    assert_eq!(
        receipt(&repo, "L04", "L04-cleanup-pattern").outcome,
        "blocked"
    );
    let writes = s.cloud.lock().expect("cloud").writes.clone();
    assert!(
        !writes.iter().any(|w| w.contains("/data-patterns/")),
        "{writes:?}"
    );
    assert!(!record.passed());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_write_stops_the_run_and_leaves_its_resources_for_reconciliation() {
    let s = served(Cloud::new()).await;
    // The topic create is answered, then the profile create's reply is lost.
    let lose = std::sync::Arc::new(std::sync::Mutex::new(true));
    let server = {
        let cloud = std::sync::Arc::clone(&s.cloud);
        let lose = std::sync::Arc::clone(&lose);
        s.fixture
            .serve(crate::support::Reply::Route(std::sync::Arc::new(
                move |seen| {
                    let reply = cloud.lock().expect("cloud").route(seen);
                    if seen
                        .request_line
                        .starts_with("POST /aisec/v1/mgmt/profile ")
                        && std::mem::replace(&mut *lose.lock().expect("lose"), false)
                    {
                        return crate::support::Reply::Close;
                    }
                    reply
                },
            )))
            .await
    };
    let mut fixture = crate::common::Fixture::new();
    // Use the lossy server's origin (same CA as the first).
    fixture.management = Some(format!("https://localhost:{}", server.port));
    let fixture = crate::common::Fixture {
        ca: s.fixture.ca,
        ..fixture
    };
    let (repo, record) = fixture
        .run(
            scope(),
            &["L03", "L04"],
            &management_secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let stop = record.stopped.expect("stopped");
    assert!(stop.reconcile, "{stop:?}");
    assert_eq!(stop.check, "L03-profile-create");
    assert_eq!(
        receipt(&repo, "L03", "L03-profile-create").outcome,
        "needs-reconciliation"
    );
    let left = receipt(&repo, "L03", "L03-cleanup-topic");
    assert_eq!(left.outcome, "blocked");
    assert!(
        left.observed["left_for_reconciliation"]
            .to_string()
            .contains("topics")
    );
    // L04 never ran.
    assert_eq!(record.families[1].status, "blocked");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_blocked_write_back_is_recorded_with_its_reason_and_the_rest_still_runs() {
    let s = served(Cloud::new()).await;
    let mut value = scope();
    value["families"]["L04"]["nonsecret"]["data_profile_blocked"] =
        json!("the service refuses data profiles by ID");
    let (repo, record) = s
        .fixture
        .run(value, &["L04"], &management_secrets(&[]), Browser::idle())
        .await;
    record.expect("run");
    let data = receipt(&repo, "L04", "L04-data-profile-replace");
    assert_eq!(data.outcome, "blocked");
    assert_eq!(data.detail, "the service refuses data profiles by ID");
    assert_eq!(
        receipt(&repo, "L04", "L04-filtering-profile-replace").outcome,
        "passed"
    );
    let writes = s.cloud.lock().expect("cloud").writes.clone();
    assert!(
        !writes.iter().any(|w| w.contains("/data-profiles/")),
        "{writes:?}"
    );
}
