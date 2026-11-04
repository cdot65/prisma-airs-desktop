//! L01 Runtime scan and L18 scope isolation, plus the runner's own rules
//! (budgets, stops, redaction, refusals).

use std::sync::Arc;

use serde_json::{Value, json};
use xtask::live::{self, LiveManifest};
use xtask::live_run::{self, Guard};

use crate::common::{
    Browser, Fixture, assert_no_secret, designate, keychain, manifest, raw, receipt, secrets,
};
use crate::support::{Reply, Seen, TlsServer};

const KEY_A: &str = "synthetic-live-key-a-c25";
const KEY_DENIED: &str = "synthetic-denied-key-c25";
const KEY_B: &str = "synthetic-live-key-b-c25";
const PROFILE_ID: &str = "0b4f6a3e-2c1d-4e5f-8a9b-7c6d5e4f3a2b";
const PROFILE_B: &str = "6d1c2b3a-4e5f-4a6b-9c8d-0e1f2a3b4c5d";

fn keys() -> crate::common::Secrets {
    secrets(&[
        ("scan-a", KEY_A),
        ("denied-a", KEY_DENIED),
        ("scan-b", KEY_B),
    ])
}

fn verdict(profile_id: &str) -> Reply {
    raw(
        200,
        &json!({"report_id": "R-c25-1", "scan_id": "S-c25-1", "tr_id": "T-c25 1",
               "profile_name": "c25 profile", "profile_id": profile_id,
               "category": "benign", "action": "allow",
               "timeout": false, "error": false, "errors": [],
               "prompt_detected": {"injection": false}, "response_detected": {}}),
    )
}

/// A scan service that completes for `own`, refuses everything else.
fn service(own: &'static str, answer: Reply) -> Reply {
    Reply::Route(Arc::new(move |seen: &Seen| {
        if seen.header("x-pan-token") == Some(own) {
            answer.clone()
        } else {
            raw(401, &json!({}))
        }
    }))
}

/// The second scope's service as AIRS behaves on a shared regional endpoint:
/// its own key completes (for its profile by name or ID); another tenant's
/// key aimed at its profile ID is told the profile does not exist (400);
/// anything else is refused as a credential.
fn scope_b(answer_other: Option<Reply>) -> Reply {
    Reply::Route(Arc::new(move |seen: &Seen| {
        let body: Value = serde_json::from_slice(&seen.body).unwrap_or(Value::Null);
        let aimed = body["ai_profile"]["profile_id"] == PROFILE_B;
        match seen.header("x-pan-token") {
            Some(KEY_B) => verdict(PROFILE_B),
            Some(_) if aimed => answer_other.clone().unwrap_or_else(|| {
                raw(400, &json!({"error": {"message": "AI Profile not found"}}))
            }),
            _ => raw(401, &json!({})),
        }
    }))
}

struct Services {
    fixture: Fixture,
    a: TlsServer,
    b: TlsServer,
}

async fn services(a: Reply) -> Services {
    let fixture = Fixture::new();
    let a = fixture.serve(a).await;
    let b = fixture.serve(scope_b(None)).await;
    Services { fixture, a, b }
}

/// L01 and L18 designated against the two services.
fn scope(s: &Services) -> Value {
    let mut value = manifest();
    designate(
        &mut value,
        "L01",
        json!({
            "credentials": {"scan_key": keychain("scan-a"), "denied_scan_key": keychain("denied-a")},
            "nonsecret": {"origin": format!("https://localhost:{}", s.a.port),
                          "profile_name": "c25 profile", "app_name": "c25 live check",
                          "expected_profile_id": PROFILE_ID},
            "permitted_operations": ["scan"],
            "budget": {"scans": 5}
        }),
    );
    designate(
        &mut value,
        "L18",
        json!({
            "environment": "test-b",
            "credentials": {"scan_key": keychain("scan-b")},
            "nonsecret": {"origin": format!("https://localhost:{}", s.b.port),
                          "profile_name": "c25 profile"},
            "permitted_operations": ["scan"]
        }),
    );
    value
}

async fn run(
    s: &Services,
    value: Value,
    families: &[&str],
) -> (xtask::Repo, xtask::Result<xtask::live_run::RunRecord>) {
    s.fixture
        .run(value, families, &keys(), Browser::idle())
        .await
}

#[tokio::test(flavor = "multi_thread")]
async fn l01_and_l18_drive_the_production_core_and_record_redacted_fixture_receipts() {
    let s = services(service(KEY_A, verdict(PROFILE_ID))).await;
    let (repo, record) = run(&s, scope(&s), &[]).await;
    let record = record.expect("run");
    assert!(record.passed(), "{record:?}");
    assert_eq!(record.evidence, "fixture");
    let ids: Vec<(&str, &str)> = record
        .families
        .iter()
        .map(|f| (f.id.as_str(), f.status.as_str()))
        .collect();
    assert_eq!(ids, vec![("L01", "passed"), ("L18", "passed")]);

    let scan = receipt(&repo, "L01", "L01-scan");
    assert_eq!(
        (scan.evidence.as_str(), scan.outcome.as_str()),
        ("fixture", "passed")
    );
    assert_eq!(scan.workflows, ["W01", "W02", "W04", "W06", "W22"]);
    assert_eq!(
        scan.inputs["endpoint"],
        json!(format!("https://localhost:{}", s.a.port))
    );
    assert_eq!(scan.inputs["credential"], json!("L01.scan_key"));
    assert_eq!(scan.observed["verdict"], json!("allow"));
    assert_eq!(scan.observed["management_configured"], json!(false));
    assert_eq!(scan.ids["scan_id"], "S-c25-1");
    // A trace ID with a space is not an identifier; it is never copied.
    assert_eq!(scan.ids["tr_id"], "<unrecognized>");
    // Each receipt shows the cap as it stood when the check settled; L18
    // has no budget of its own and is charged against L01's.
    assert_eq!(scan.consumption["scans"], json!({"used": 1, "limit": 5}));

    let denied = receipt(&repo, "L01", "L01-denied");
    assert_eq!(denied.outcome, "passed");
    assert_eq!(denied.observed["failure"], json!("unauthorized"));
    assert!(denied.ids.is_empty());

    assert_eq!(receipt(&repo, "L18", "L18-own-scope").outcome, "passed");
    let own_id = receipt(&repo, "L18", "L18-own-profile-id");
    assert_eq!(own_id.outcome, "passed");
    assert_eq!(own_id.inputs["profile_id"], json!(PROFILE_B));
    let cross = receipt(&repo, "L18", "L18-cross-scope-denied");
    assert_eq!(
        (cross.outcome.as_str(), cross.inputs["credential"].as_str()),
        ("passed", Some("L01.scan_key"))
    );
    assert_eq!(cross.inputs["profile_id"], json!(PROFILE_B));
    assert_eq!(cross.observed["failure"], json!("badRequest"));
    assert_eq!(cross.consumption["scans"], json!({"used": 5, "limit": 5}));

    // Each key went only to its own service, with its own scope.
    let tokens = |server: &TlsServer| -> Vec<Option<String>> {
        server
            .seen()
            .iter()
            .map(|r| r.header("x-pan-token").map(str::to_owned))
            .collect()
    };
    assert_eq!(
        tokens(&s.a),
        vec![Some(KEY_A.to_owned()), Some(KEY_DENIED.to_owned())]
    );
    assert_eq!(
        tokens(&s.b),
        vec![
            Some(KEY_B.to_owned()),
            Some(KEY_B.to_owned()),
            Some(KEY_A.to_owned())
        ]
    );
    let aimed: Value = serde_json::from_slice(&s.b.seen()[2].body).expect("body");
    assert_eq!(aimed["ai_profile"], json!({"profile_id": PROFILE_B}));
    let body: Value = serde_json::from_slice(&s.a.seen()[0].body).expect("body");
    assert_eq!(body["ai_profile"], json!({"profile_name": "c25 profile"}));
    assert_eq!(body["metadata"], json!({"app_name": "c25 live check"}));
    assert_no_secret(&repo, &[KEY_A, KEY_DENIED, KEY_B]);
}

#[tokio::test(flavor = "multi_thread")]
async fn another_scopes_key_answered_for_its_profile_stops_the_run() {
    let fixture = Fixture::new();
    let a = fixture.serve(service(KEY_A, verdict(PROFILE_ID))).await;
    let b = fixture.serve(scope_b(Some(verdict(PROFILE_B)))).await;
    let s = Services { fixture, a, b };
    let (repo, record) = run(&s, scope(&s), &[]).await;
    let record = record.expect("run");
    let stop = record.stopped.clone().expect("stopped");
    assert_eq!(
        (stop.family.as_str(), stop.check.as_str(), stop.reconcile),
        ("L18", "L18-cross-scope-denied", false)
    );
    assert!(stop.reason.contains("unexpected authority"));
    assert!(!record.passed());
    assert!(
        receipt(&repo, "L18", "L18-cross-scope-denied")
            .detail
            .contains("must be denied completed a scan")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_for_another_profile_stops_the_run() {
    let s = services(service(
        KEY_A,
        verdict("11111111-2222-4333-8444-555555555555"),
    ))
    .await;
    let (repo, record) = run(&s, scope(&s), &[]).await;
    let record = record.expect("run");
    let stop = record.stopped.clone().expect("stopped");
    assert_eq!(
        (stop.family.as_str(), stop.check.as_str(), stop.reconcile),
        ("L01", "L01-scan", false)
    );
    assert!(stop.reason.contains("unexpected tenant or authority"));
    assert!(!record.passed());
    assert_eq!(record.families[0].status, "stopped");
    assert_eq!(record.families[1].status, "blocked");
    // Nothing ran after the stop.
    assert_eq!(s.a.seen().len(), 1);
    assert!(s.b.seen().is_empty());
    assert!(
        receipt(&repo, "L01", "L01-scan")
            .detail
            .contains("unexpected authority")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_request_over_its_budget_is_never_sent() {
    let s = services(service(KEY_A, verdict(PROFILE_ID))).await;
    let mut value = scope(&s);
    value["families"]["L01"]["budget"]["scans"] = json!(1);
    let (repo, record) = run(&s, value, &["L01"]).await;
    let record = record.expect("run");
    let stop = record.stopped.expect("stopped");
    assert_eq!(stop.check, "L01-denied");
    assert!(stop.reason.contains("unexpected cost"));
    assert_eq!(s.a.seen().len(), 1, "the second scan was not sent");
    let denied = receipt(&repo, "L01", "L01-denied");
    assert_eq!(denied.outcome, "blocked");
    assert_eq!(denied.consumption["scans"], json!({"used": 1, "limit": 1}));
}

#[tokio::test(flavor = "multi_thread")]
async fn budgets_bind_every_run_together_and_fixture_runs_never_charge_the_live_ledger() {
    let s = services(service(KEY_A, verdict(PROFILE_ID))).await;
    let mut value = scope(&s);
    value["families"]["L01"]["budget"]["scans"] = json!(5);
    let repo = xtask::Repo::new(crate::common::scratch());
    let secrets = keys();
    let once = || {
        s.fixture
            .run_in(&repo, value.clone(), &["L01"], &secrets, Browser::idle())
    };
    assert!(once().await.expect("first run").passed());
    assert!(once().await.expect("second run").passed());
    // The second run's receipts count the first run's spend.
    assert_eq!(
        receipt(&repo, "L01", "L01-denied").consumption["scans"],
        json!({"used": 4, "limit": 5})
    );
    // The third run reaches the cap after one scan; the next is never sent.
    let third = once().await.expect("third run");
    let stop = third.stopped.expect("stopped");
    assert_eq!(stop.check, "L01-denied");
    assert!(stop.reason.contains("across runs"), "{}", stop.reason);
    assert_eq!(s.a.seen().len(), 5, "2 + 2 + 1 scans reached the service");

    let live = repo.root().join("docs/planning/evidence/C25/live");
    assert!(!live.join(live_run::LIVE_LEDGER).exists());
    let charges = live_run::read_charges(&live.join(live_run::FIXTURE_LEDGER)).expect("ledger");
    assert_eq!(charges.len(), 5);
    assert!(charges.iter().all(
        |c| (c.family.as_str(), c.budget.as_str(), c.n, c.kind.as_str())
            == ("L01", "scans", 1, "spend")
    ));
    assert_eq!(
        charges
            .iter()
            .map(|c| c.run.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
}

#[test]
fn a_ledger_counts_earlier_spend_and_fails_closed_when_it_cannot_be_read() {
    let s: LiveManifest = serde_json::from_value(json!({
        "schema": live::SCHEMA,
        "owner_approval": {"approved_by": "o", "approved_utc": "t", "decision": "d"},
        "environments": {},
        "families": {"L01": {"status": "blocked", "reason": "r", "budget": {"scans": 10}}}
    }))
    .expect("manifest");
    let dir = crate::common::scratch();
    let path = dir.join(live_run::LIVE_LEDGER);
    let note = json!({"schema": live_run::CONSUMPTION_SCHEMA, "recorded_utc": "t",
                      "run": "operator-note", "manifest_sha256": "f", "family": "L01",
                      "budget": "scans", "n": 8, "kind": "note",
                      "reason": "spend outside the runner"});
    std::fs::write(&path, format!("{note}\n")).expect("seed");
    let mut guard = Guard::with_ledger(&s, &path, "run-1", "f").expect("guard");
    assert!(guard.spend("L01", "scans", 2).is_ok());
    assert!(guard.spend("L01", "scans", 1).is_err());
    assert_eq!(
        guard.consumption("L01")["scans"],
        json!({"used": 10, "limit": 10})
    );
    assert_eq!(live_run::read_charges(&path).expect("read").len(), 2);

    std::fs::write(&path, "{not json}\n").expect("damage");
    assert!(Guard::with_ledger(&s, &path, "run-2", "f").is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_reply_needs_reconciliation_and_stops_the_run() {
    let s = services(service(KEY_A, Reply::Close)).await;
    let (repo, record) = run(&s, scope(&s), &[]).await;
    let record = record.expect("run");
    let stop = record.stopped.expect("stopped");
    assert!(stop.reconcile, "{stop:?}");
    assert_eq!(stop.check, "L01-scan");
    assert_eq!(
        receipt(&repo, "L01", "L01-scan").outcome,
        "needs-reconciliation"
    );
    assert_eq!(s.a.seen().len(), 1);
    assert!(s.b.seen().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_receipt_that_would_hold_a_secret_is_refused() {
    let s = services(service(KEY_A, verdict(PROFILE_ID))).await;
    let mut value = scope(&s);
    // A nonsecret input that happens to equal the key (short and simple
    // enough to pass the manifest's own secret heuristics).
    value["families"]["L01"]["nonsecret"]["app_name"] = json!(KEY_A);
    let (repo, record) = run(&s, value, &["L01"]).await;
    let error = record.expect_err("refused").to_string();
    assert!(error.contains("would contain a secret"), "{error}");
    assert!(
        !repo
            .root()
            .join("docs/planning/evidence/C25/live/L01/L01-scan.json")
            .exists()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn blocked_and_unacceptable_scopes_are_refused_before_anything_runs() {
    let s = services(service(KEY_A, verdict(PROFILE_ID))).await;
    let (_, record) = run(&s, scope(&s), &["L09"]).await;
    assert!(
        record
            .expect_err("blocked")
            .to_string()
            .contains("L09 is blocked")
    );
    let mut value = scope(&s);
    value["environments"]["staging-a"]["kind"] = json!("production");
    let (_, record) = run(&s, value, &["L01"]).await;
    assert!(
        record
            .expect_err("rejected")
            .to_string()
            .contains("not acceptable")
    );
    assert!(s.a.seen().is_empty() && s.b.seen().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_harness_family_is_recorded_as_needing_the_desktop_app() {
    let s = services(service(KEY_A, verdict(PROFILE_ID))).await;
    let mut value = scope(&s);
    designate(&mut value, "L16", json!({"budget": {"tokens": 10}}));
    let (repo, record) = run(&s, value, &["L16"]).await;
    let record = record.expect("run");
    assert_eq!(record.families[0].status, "needs-native-app");
    assert!(!record.passed());
    let r = receipt(&repo, "L16", "L16-engine-conversation");
    assert_eq!(r.outcome, "native-app-required");
    assert!(r.detail.contains("desktop host"));
}

#[test]
fn budgets_refuse_before_charging_and_names_stay_under_the_prefix() {
    let s: LiveManifest = serde_json::from_value(json!({
        "schema": live::SCHEMA,
        "owner_approval": {"approved_by": "o", "approved_utc": "t", "decision": "d"},
        "environments": {},
        "families": {"L09": {"status": "blocked", "reason": "r", "budget": {"tokens": 10}}}
    }))
    .expect("manifest");
    let mut guard = Guard::new(&s);
    assert!(guard.spend("L09", "tokens", 6).is_ok());
    assert!(guard.spend("L09", "tokens", 5).is_err());
    assert!(guard.spend("L09", "tokens", 4).is_ok());
    assert!(guard.spend("L09", "tokens", 1).is_err());
    assert!(guard.spend("L09", "requests", 1).is_err());
    assert_eq!(
        guard.consumption("L09")["tokens"],
        json!({"used": 10, "limit": 10})
    );
    // Reported consumption is recorded even when it went over.
    assert!(guard.report("L09", "tokens", 3).is_err());
    assert_eq!(
        guard.consumption("L09")["tokens"],
        json!({"used": 13, "limit": 10})
    );

    assert!(live_run::disposable(Some("airs-v2-c25-"), "airs-v2-c25-topic-1").is_ok());
    assert!(live_run::disposable(Some("airs-v2-c25-"), "airs-v2-c25-").is_err());
    assert!(live_run::disposable(Some("airs-v2-c25-"), "prod-topic").is_err());
    assert!(live_run::disposable(Some(""), "anything").is_err());
    assert!(live_run::disposable(None, "airs-v2-c25-x").is_err());

    assert_eq!(live_run::sanitize_id("S-1:a.b_c"), "S-1:a.b_c");
    assert_eq!(live_run::sanitize_id("a b"), "<unrecognized>");
    assert_eq!(live_run::sanitize_id(&"x".repeat(129)), "<unrecognized>");
    assert_eq!(live_run::sanitize_id(""), "<unrecognized>");
}
