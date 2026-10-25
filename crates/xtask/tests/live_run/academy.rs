//! L11–L14 Academy against a synthetic Academy and identity provider: the
//! operator stand-in approves the device sign-in (the runner only waits for
//! the core) and later withdraws the learner's course access.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::common::{
    Browser, Fixture, assert_no_secret, designate, manifest, outcomes, raw, receipt, secrets,
};
use crate::support::{Reply, Seen};

const COURSE: &str = "runtime-security";
const ATTEMPT: &str = "00000000-0000-4000-8000-0000000000a1";
const CLIENT: &str = "desktop_public_client";
const SCOPE: &str = "openid profile email offline_access academy:learn";

#[derive(Default)]
struct Academy {
    origin: String,
    /// Sign-ins the operator approved (each a new account).
    approved: usize,
    /// Sign-ins started.
    started: usize,
    denied: bool,
    revoked: usize,
    uploads: usize,
}

fn in_secs(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64;
    let t = (now + secs) as u64;
    let days = t / 86_400;
    let rem = t % 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn data(value: Value) -> Reply {
    raw(200, &json!({"result": {"data": {"json": value}}}))
}

impl Academy {
    fn account(seen: &Seen) -> String {
        match seen.header("authorization") {
            // As live Academy: the account is its own user ID, a UUID.
            Some("Bearer access-2") => "5e1f0c2a-9b3d-4e6f-8a7b-2c4d6e8f0a1b".to_owned(),
            _ => "0b7c9d2e-4f1a-4c3b-9e8d-1a2b3c4d5e6f".to_owned(),
        }
    }

    fn route(&mut self, seen: &Seen) -> Reply {
        let (method, path, query) = crate::common::parts(seen);
        let form = String::from_utf8_lossy(&seen.body).to_string();
        match path.as_str() {
            "/api/desktop/config" => raw(
                200,
                &json!({"issuer": self.origin, "clientId": CLIENT, "scope": SCOPE}),
            ),
            "/.well-known/oauth-authorization-server" => raw(
                200,
                &json!({"issuer": self.origin,
                "device_authorization_endpoint": format!("{}/oauth/device", self.origin),
                "token_endpoint": format!("{}/oauth/token", self.origin),
                "revocation_endpoint": format!("{}/oauth/revoke", self.origin)}),
            ),
            "/oauth/device" => {
                self.started += 1;
                raw(
                    200,
                    &json!({"device_code": format!("device-secret-{}", self.started), "user_code": "WDJB-MJHT",
                    "verification_uri": format!("{}/activate", self.origin),
                    "verification_uri_complete": format!("{}/activate?code=WDJB-MJHT", self.origin),
                    "expires_in": 600, "interval": 1}),
                )
            }
            "/oauth/token" if form.contains("device_code") => {
                if self.approved >= self.started {
                    let n = self.started;
                    raw(
                        200,
                        &json!({"token_type": "Bearer", "access_token": format!("access-{n}"),
                        "refresh_token": format!("refresh-{n}"), "expires_in": 3600, "scope": SCOPE}),
                    )
                } else {
                    raw(400, &json!({"error": "authorization_pending"}))
                }
            }
            "/oauth/revoke" => {
                self.revoked += 1;
                raw(200, &json!({}))
            }
            p if p.starts_with("/upload/") && method == "PUT" => {
                self.uploads += 1;
                raw(200, &json!({}))
            }
            p if p.starts_with("/api/trpc/") => {
                let procedure = p.trim_start_matches("/api/trpc/");
                let account = Self::account(seen);
                match procedure {
                    "desktop.me" => data(
                        json!({"accountId": account, "displayName": "Synthetic Learner",
                        "serverTime": in_secs(0)}),
                    ),
                    // As live Academy: the access check needs its course list, in
                    // the SuperJSON envelope.
                    "desktop.access"
                        if !query.get("input").is_some_and(|i| {
                            serde_json::from_str::<Value>(i)
                                .is_ok_and(|v| v["json"]["courseIds"].is_array())
                        }) =>
                    {
                        raw(400, &json!({"error": {"message": "courseIds is required"}}))
                    }
                    "desktop.access" => data(
                        json!({"serverTime": in_secs(0), "accountId": account, "courses": [
                        if self.denied {
                            json!({"courseId": COURSE, "allowed": false, "reason": "FORBIDDEN",
                                   "issuedAt": in_secs(0), "expiresAt": in_secs(0)})
                        } else {
                            json!({"courseId": COURSE, "allowed": true, "access": "restricted",
                                   "issuedAt": in_secs(0), "expiresAt": in_secs(600)})
                        }]}),
                    ),
                    "desktop.bundle" => data(
                        json!({"schemaVersion": 1, "courseId": COURSE, "title": "Runtime Security",
                        "description": "d", "revision": "rev-1", "hash": "rev-1", "accountId": account,
                        "modules": [{"moduleId": "m1", "order": 1}],
                        "units": [{"id": "unit-1", "moduleId": "m1", "order": 1, "kind": "content", "title": "Scanning",
                                   "bodyHtml": "<p>synthetic</p>", "status": "not_started", "siblings": [], "pathContext": null}],
                        "labs": [], "labModes": {}, "manifest": false,
                        "lease": {"issuedAt": in_secs(0), "expiresAt": in_secs(600), "access": "restricted"}}),
                    ),
                    "desktop.attempt.get" => data(
                        json!({"attemptId": ATTEMPT, "accountId": account, "labId": "lab-1",
                        "courseId": COURSE, "courseTitle": "Runtime Security", "revision": "rev-1",
                        "status": "in_progress", "draftVersion": 2, "draft": {}, "result": {}, "conflicts": [],
                        "locked": false, "lockReason": null, "createdAt": in_secs(-60), "mode": "triage",
                        "steps": [{"id": "s1", "prompt": "SYNTHETIC-STEP"}],
                        "lease": {"access": "restricted", "issuedAt": in_secs(0), "expiresAt": in_secs(600)}}),
                    ),
                    "settings.avatarUploadUrl" => data(json!({"uploadUrl":
                        format!("{}/upload/avatars/{account}.jpg?sig=s", self.origin), "key": "avatars/x.jpg"})),
                    "settings.confirmAvatar" => data(json!({"account": {}})),
                    _ => data(json!({"ok": true})),
                }
            }
            _ => raw(404, &json!({})),
        }
    }
}

fn scope(origin: &str) -> Value {
    let mut value = manifest();
    designate(
        &mut value,
        "L11",
        json!({"nonsecret": {"academy_origin": origin, "second_account": "designated"}}),
    );
    designate(
        &mut value,
        "L12",
        json!({
        "nonsecret": {"call.01-catalog": "W50 courses.list",
                      "call.02-unit": "W50 units.get {\"json\":{\"unitId\":\"unit-1\"}}",
                      "call.03-check": "W51 progress.checkInline {\"json\":{\"unitId\":\"unit-1\",\"answer\":\"a\"}}"},
        "permitted_operations": ["progress.checkInline"],
        "budget": {"calls": 10}}),
    );
    designate(
        &mut value,
        "L13",
        json!({
        "nonsecret": {"call.01-eligibility": "W53 exam.eligibility {\"json\":{\"certificationId\":\"cert-1\"}}"},
        "permitted_operations": ["settings.removeAvatar"],
        "disposable_prefix": "c25-", "cleanup": ["remove the avatar"],
        "budget": {"calls": 10}}),
    );
    designate(
        &mut value,
        "L14",
        json!({
        "nonsecret": {"course_id": COURSE, "attempt_id": ATTEMPT, "base_version": "2", "access_change": "designated",
                      "call.01-attempt": format!("W56 desktop.attempt.get {{\"json\":{{\"attemptId\":\"{ATTEMPT}\"}}}}")},
        "budget": {"calls": 5}}),
    );
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn the_academy_families_share_one_operator_sign_in_and_judge_only_the_core() {
    let mut fixture = Fixture::new();
    let academy = Arc::new(Mutex::new(Academy::default()));
    let server = fixture
        .serve(crate::common::routed(&academy, Academy::route))
        .await;
    let origin = format!("https://localhost:{}", server.port);
    academy.lock().expect("academy").origin = origin.clone();
    fixture.academy = Some(origin.clone());
    let acting = Arc::clone(&academy);
    let operator = Browser::new(move |instruction, url| {
        let mut a = acting.lock().expect("academy");
        if url.is_some_and(|u| u.path() == "/activate") {
            a.approved += 1;
        }
        if instruction.contains("remove the designated test learner's access") {
            a.denied = true;
        }
    });
    let (repo, record) = fixture
        .run(
            scope(&origin),
            &["L11", "L12", "L13", "L14"],
            &secrets(&[]),
            operator.clone(),
        )
        .await;
    let record = record.expect("run");
    for family in ["L11", "L12", "L13", "L14"] {
        let checks = outcomes(&record, family);
        assert!(
            checks.iter().all(|c| c.ends_with(": passed")),
            "{family}: {checks:?}"
        );
    }
    let sign_in = receipt(&repo, "L11", "L11-device-sign-in");
    let step = sign_in.operator_step.expect("operator step");
    assert_eq!(step["completed"], json!(true));
    assert!(
        step["judged_by"]
            .as_str()
            .is_some_and(|j| j.contains("never supplies"))
    );
    // The verification page reached the operator from the core.
    assert!(operator.prompts().iter().any(|(_, u)| {
        u.as_deref()
            .is_some_and(|u| u.ends_with("/activate?code=WDJB-MJHT"))
    }));
    let switch = receipt(&repo, "L11", "L11-account-switch");
    assert_ne!(
        switch.observed["account_sha256_prefix"],
        sign_in.observed["account_sha256_prefix"]
    );
    assert_eq!(
        receipt(&repo, "L14", "L14-access-change-denied")
            .operator_step
            .expect("step")["completed"],
        json!(true)
    );
    assert!(
        crate::common::within(|| academy.lock().expect("academy").revoked >= 2).await,
        "each sign-out revoked its refresh token"
    );
    assert_eq!(academy.lock().expect("academy").uploads, 1);
    // Each call counts only for the workflows its entry names, and was
    // charged before it was sent.
    assert_eq!(
        receipt(&repo, "L12", "L12-call-01-catalog").workflows,
        vec!["W50"]
    );
    assert_eq!(
        receipt(&repo, "L12", "L12-call-03-check").workflows,
        vec!["W51"]
    );
    assert_eq!(
        receipt(&repo, "L13", "L13-call-01-eligibility").workflows,
        vec!["W53"]
    );
    assert_eq!(
        receipt(&repo, "L14", "L14-local-draft").workflows,
        vec!["W55"]
    );
    assert_eq!(
        receipt(&repo, "L12", "L12-call-03-check").consumption["calls"]["used"],
        json!(3)
    );
    assert_eq!(
        receipt(&repo, "L13", "L13-avatar").consumption["calls"]["used"],
        json!(3),
        "one call and the avatar's upload and removal"
    );
    assert_no_secret(
        &repo,
        &[
            "access-1",
            "refresh-1",
            "device-secret-1",
            "access-2",
            "refresh-2",
        ],
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_the_operator_never_approves_blocks_every_academy_check() {
    let mut fixture = Fixture::new();
    let academy = Arc::new(Mutex::new(Academy::default()));
    let server = fixture
        .serve(crate::common::routed(&academy, Academy::route))
        .await;
    let origin = format!("https://localhost:{}", server.port);
    academy.lock().expect("academy").origin = origin.clone();
    fixture.academy = Some(origin.clone());
    let (repo, record) = fixture
        .run(
            scope(&origin),
            &["L11", "L12"],
            &secrets(&[]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    assert!(!record.passed());
    let sign_in = receipt(&repo, "L11", "L11-device-sign-in");
    assert_eq!(sign_in.outcome, "failed");
    assert_eq!(
        sign_in.operator_step.expect("step")["completed"],
        json!(false)
    );
    assert_eq!(
        receipt(&repo, "L12", "L12-after-sign-in").outcome,
        "blocked"
    );
    // No learner procedure was called without a signed-in learner.
    assert!(
        !server
            .seen()
            .iter()
            .any(|r| r.request_line.contains("/api/trpc/"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_exhausted_call_budget_sends_nothing_and_an_untagged_call_is_refused() {
    let mut fixture = Fixture::new();
    let academy = Arc::new(Mutex::new(Academy::default()));
    let server = fixture
        .serve(crate::common::routed(&academy, Academy::route))
        .await;
    let origin = format!("https://localhost:{}", server.port);
    academy.lock().expect("academy").origin = origin.clone();
    fixture.academy = Some(origin.clone());
    let acting = Arc::clone(&academy);
    let operator = Browser::new(move |_, url| {
        if url.is_some_and(|u| u.path() == "/activate") {
            acting.lock().expect("academy").approved += 1;
        }
    });
    let mut value = scope(&origin);
    // L12 names a call with no workflow; L13 may make one call of two.
    value["families"]["L12"]["nonsecret"] = json!({"call.01-untagged": "exam.eligibility"});
    value["families"]["L13"]["budget"] = json!({"calls": 1});
    value["families"]["L13"]["nonsecret"] = json!({
        "call.01-catalog": "W53 certifications.list",
        "call.02-eligibility": "W53 exam.eligibility {\"json\":{\"certificationId\":\"cert-1\"}}"});
    let (repo, record) = fixture
        .run(value, &["L12", "L13"], &secrets(&[]), operator)
        .await;
    let record = record.expect("run");
    assert!(!record.passed());
    // A call that names no workflow is refused unsent.
    let untagged = receipt(&repo, "L12", "L12-calls");
    assert_eq!(untagged.outcome, "blocked");
    assert!(
        untagged.detail.contains("names no workflow"),
        "{}",
        untagged.detail
    );
    // L13's second call is refused by its exhausted budget, unsent.
    assert_eq!(
        receipt(&repo, "L13", "L13-call-01-catalog").outcome,
        "passed"
    );
    let refused = receipt(&repo, "L13", "L13-call-02-eligibility");
    assert_eq!(refused.outcome, "blocked");
    assert!(
        refused.detail.contains("budget exhausted"),
        "{}",
        refused.detail
    );
    let calls = |name: &str| {
        server
            .seen()
            .iter()
            .filter(|r| r.request_line.contains(&format!("/api/trpc/{name}")))
            .count()
    };
    assert_eq!(calls("certifications.list"), 1);
    assert_eq!(calls("exam.eligibility"), 0);
}
