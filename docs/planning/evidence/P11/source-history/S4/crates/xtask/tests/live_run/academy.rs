//! L11–L14 Academy against a synthetic Academy and identity provider: the
//! operator stand-in approves the device sign-in (the runner only waits for
//! the core) and later withdraws the learner's course access.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
    progress_units: usize,
    reset_ignored: bool,
    certificates_lost: bool,
    exam_started: bool,
    shift_stage: usize,
    empty_certificates: bool,
    wrong_exam_id: bool,
    wrong_settled_id: bool,
    empty_debrief: bool,
    shift_outcome: Option<&'static str>,
    daily_refused: bool,
    inline_correct: bool,
    start_invalid: bool,
    lost_start: bool,
    serve_unexpected: bool,
    dropped_procedure: Option<&'static str>,
    hung_procedure: Option<&'static str>,
    avatar_presign: Option<&'static str>,
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
                let rpc: Value = query
                    .get("input")
                    .and_then(|s| serde_json::from_str(s).ok())
                    .or_else(|| serde_json::from_slice(&seen.body).ok())
                    .unwrap_or(Value::Null);
                let input = &rpc["json"];
                if self.dropped_procedure == Some(procedure)
                    || self.hung_procedure == Some(procedure)
                {
                    // Apply first, then lose or withhold the reply: the client
                    // cannot treat an unobserved mutation as a known rejection.
                    match procedure {
                        "exam.start" => self.exam_started = true,
                        "progress.resetCourse" => self.progress_units = 0,
                        "shift.start" => self.shift_stage = 1,
                        "shift.serve" => self.shift_stage = 2,
                        _ => {}
                    }
                    return if self.hung_procedure == Some(procedure) {
                        Reply::Hang
                    } else {
                        Reply::Close
                    };
                }
                match procedure {
                    "courses.list" => {
                        data(json!([{"id":COURSE, "slug":COURSE, "title":"Runtime Security"}]))
                    }
                    "units.get" if input["unitId"] == "unit-1" => {
                        data(json!({"id":"unit-1", "kind":"content", "title":"Scanning"}))
                    }
                    "exam.eligibility" => data(json!({"eligible":true})),
                    "settings.get" => data(json!({"theme":"system"})),
                    "settings.removeAvatar" => data(json!({"removed":true})),
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
                    "settings.avatarUploadUrl" if self.avatar_presign == Some("rejected") => raw(
                        403,
                        &json!({"error":{"json":{"message":"avatar upload refused","code":-32003,"data":{"code":"FORBIDDEN","httpStatus":403}}}}),
                    ),
                    "settings.avatarUploadUrl" if self.avatar_presign == Some("notSent") => {
                        // A same-origin URL without its required signed query
                        // is refused locally, before any PUT can be sent.
                        data(json!({"uploadUrl":format!("{}/upload/avatar.jpg",self.origin)}))
                    }
                    "settings.avatarUploadUrl" => data(json!({"uploadUrl":
                        format!("{}/upload/avatars/{account}.jpg?sig=s", self.origin), "key": "avatars/x.jpg"})),
                    "settings.confirmAvatar" => data(json!({"account": {}})),
                    "progress.checkInline"
                        if input["lessonId"] == "lesson-1" && input["questionId"] == "q-1" =>
                    {
                        data(json!({"correct": self.inline_correct, "explanation": "synthetic"}))
                    }
                    "certificate.mine" => data(
                        if self.empty_certificates
                            || (self.certificates_lost && self.progress_units == 0)
                        {
                            json!([])
                        } else {
                            json!([{"id":"protected-certificate-id", "name":"Private Learner"}])
                        },
                    ),
                    "progress.courseSummary" if input["courseId"] == COURSE => {
                        data(json!({"hasProgress": self.progress_units > 0,
                        "units":self.progress_units, "quizzes":0, "labs":0, "objectives":0}))
                    }
                    "progress.resetCourse" if input["courseId"] == COURSE => {
                        let cleared = self.progress_units;
                        if !self.reset_ignored {
                            self.progress_units = 0;
                        }
                        data(
                            json!({"cleared":{"units":cleared, "quizzes":0, "labs":0, "objectives":0, "exams":0}}),
                        )
                    }
                    "exam.start" => {
                        self.exam_started = true;
                        data(
                            json!({"attemptId":ATTEMPT, "deadline":in_secs(600), "timeLimitSeconds":600, "questions":[]}),
                        )
                    }
                    "exam.submissionState" if input["attemptId"] == ATTEMPT => data(
                        json!({"attemptId":if self.wrong_exam_id {"different-attempt"} else {ATTEMPT},
                        "status":if self.exam_started { "in-progress" } else { "not-started" }}),
                    ),
                    "shift.start" if self.daily_refused => raw(
                        409,
                        &json!({"error":{"json":{"message":"daily attempt already used","code":-32009,"data":{"code":"CONFLICT","httpStatus":409}}}}),
                    ),
                    "shift.start" => {
                        self.shift_stage = 1;
                        if self.lost_start {
                            return Reply::Close;
                        }
                        data(
                            json!({"shiftId":ATTEMPT, "alertsAnswered":0, "atBoundary":self.start_invalid}),
                        )
                    }
                    "shift.serve" if self.shift_stage == 1 && input["shiftId"] == ATTEMPT => {
                        self.shift_stage = 2;
                        data(
                            json!({"shift":{"shiftId":ATTEMPT, "alertOrder":if self.serve_unexpected {1} else {0}}, "breachResolved":false}),
                        )
                    }
                    "shift.answer"
                        if self.shift_stage == 2
                            && input["shiftId"] == ATTEMPT
                            && input["order"] == 0 =>
                    {
                        let outcome = self.shift_outcome.unwrap_or("incorrect");
                        let boundary = !["ungraded", "breached"].contains(&outcome);
                        self.shift_stage = if boundary { 3 } else { 2 };
                        data(
                            json!({"outcome":outcome, "next":if boundary {"boundary"} else {"alert"}}),
                        )
                    }
                    "shift.decide"
                        if self.shift_stage == 3
                            && input["shiftId"] == ATTEMPT
                            && input["choice"] == "hand_off" =>
                    {
                        self.shift_stage = 4;
                        data(
                            json!({"shiftId":if self.wrong_settled_id {"different-shift"} else {ATTEMPT}, "status":"handed_off"}),
                        )
                    }
                    "shift.debrief" => {
                        let themes = if self.shift_stage == 4
                            && input["shiftId"] == ATTEMPT
                            && !self.empty_debrief
                        {
                            json!([{"theme":"synthetic missed objective"}])
                        } else {
                            json!([])
                        };
                        data(json!({"themes":themes, "links":[], "fallback":false}))
                    }
                    "shift.decide" => raw(
                        400,
                        &json!({"error":{"json":{"message":"stage is not complete","code":-32600,"data":{"code":"BAD_REQUEST","httpStatus":400}}}}),
                    ),
                    _ => raw(
                        404,
                        &json!({"error":{"message":"procedure or input not modeled"}}),
                    ),
                }
            }
            _ => raw(404, &json!({})),
        }
    }
}

fn bound(mut value: Value) -> Value {
    let hash = hex::encode(Sha256::digest(b"0b7c9d2e-4f1a-4c3b-9e8d-1a2b3c4d5e6f"));
    for family in ["L12", "L13", "L14"] {
        value["families"][family]["nonsecret"]["expected_account_sha256"] = json!(hash);
    }
    value
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
        "nonsecret": {"call.01-catalog": "W50 courses.list ?? :nonempty-array",
                      "call.02-unit": "W50 units.get {\"json\":{\"unitId\":\"unit-1\"}} ?? /id=\"unit-1\"",
                      "call.03-check": "W51 progress.checkInline {\"json\":{\"lessonId\":\"lesson-1\",\"questionId\":\"q-1\",\"answer\":\"a\"}} ?? /correct=false"},
        "permitted_operations": ["progress.checkInline"],
        "budget": {"calls": 10}}),
    );
    designate(
        &mut value,
        "L13",
        json!({
        "nonsecret": {"call.00-a-start": "W53 exam.start {\"json\":{\"consent\":true}} ?? /attemptId:nonempty-string && /deadline:nonempty-string && /timeLimitSeconds:positive-number",
                      "call.00-b-state": "W53 exam.submissionState {\"json\":{\"attemptId\":{\"$call\":\"00-a-start\",\"pointer\":\"/attemptId\"}}} ?? /attemptId=@00-a-start/attemptId && /status=\"in-progress\"",
                      "call.01-eligibility": "W53 exam.eligibility {\"json\":{\"certificationId\":\"cert-1\"}} ?? /eligible=true",
                      "call.02-certs": "W53 certificate.mine ?? :array",
                      "call.03-certs-again": "W53 certificate.mine ?? =@02-certs"},
        "permitted_operations": ["exam.start", "settings.removeAvatar"],
        "disposable_prefix": "c25-", "cleanup": ["remove the avatar"],
        "budget": {"calls": 10}}),
    );
    designate(
        &mut value,
        "L14",
        json!({
        "nonsecret": {"course_id": COURSE, "attempt_id": ATTEMPT, "base_version": "2", "access_change": "designated",
                      "call.01-attempt": format!("W56 desktop.attempt.get {{\"json\":{{\"attemptId\":\"{ATTEMPT}\"}}}} ?? /attemptId=\"{ATTEMPT}\"")},
        "budget": {"calls": 5}}),
    );
    bound(value)
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
    // A call's own result is asserted and shown, not only that it answered.
    let check = receipt(&repo, "L12", "L12-call-03-check");
    assert_eq!(
        check.observed["assertions"][0]["actual"]["value"],
        json!(false)
    );
    assert!(check.detail.contains("its result held"), "{}", check.detail);
    assert_eq!(
        receipt(&repo, "L13", "L13-call-03-certs-again").outcome,
        "passed"
    );
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
        json!(7),
        "five calls and the avatar's upload and removal"
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
        "call.01-settings": "W54 settings.get ?? /theme=\"system\"",
        "call.02-settings-again": "W54 settings.get ?? /theme=\"system\""});
    let (repo, record) = fixture
        .run(bound(value), &["L12", "L13"], &secrets(&[]), operator)
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
        receipt(&repo, "L13", "L13-call-01-settings").outcome,
        "passed"
    );
    let refused = receipt(&repo, "L13", "L13-call-02-settings-again");
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
    assert_eq!(calls("settings.get"), 1);
    assert_eq!(calls("exam.eligibility"), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_answer_that_does_not_hold_its_expectation_fails_the_check() {
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
    value["families"]["L12"]["nonsecret"] = json!({
        "call.01-graded": "W51 progress.checkInline {\"json\":{\"lessonId\":\"lesson-1\",\"questionId\":\"q-1\",\"answer\":\"a\"}} ?? /correct=true",
        "call.02-kind": "W50 courses.list ?? /missing:array"});
    let (repo, record) = fixture
        .run(bound(value), &["L12"], &secrets(&[]), operator)
        .await;
    assert!(!record.expect("run").passed());
    let graded = receipt(&repo, "L12", "L12-call-01-graded");
    assert_eq!(graded.outcome, "failed");
    assert!(graded.detail.contains("did not hold"), "{}", graded.detail);
    assert_eq!(
        graded.observed["assertions"][0]["actual"]["value"],
        json!(false)
    );
    assert_eq!(receipt(&repo, "L12", "L12-call-02-kind").outcome, "failed");
}

fn reset_program() -> Value {
    json!({
        "call.01-certificates": "W66 certificate.mine ?? :nonempty-array",
        "call.02-before": "W66 progress.courseSummary {\"json\":{\"courseId\":\"runtime-security\"}} ?? /hasProgress=true && /units:positive-number && /quizzes:number && /labs:number && /objectives:number",
        "call.03-reset": "W66 progress.resetCourse {\"json\":{\"courseId\":\"runtime-security\"}} ?? /cleared/units=@02-before/units && /cleared/quizzes=@02-before/quizzes && /cleared/labs=@02-before/labs && /cleared/objectives=@02-before/objectives",
        "call.04-after": "W66 progress.courseSummary {\"json\":{\"courseId\":\"runtime-security\"}} ?? /hasProgress=false && /units=0 && /quizzes=0 && /labs=0 && /objectives=0",
        "call.05-certificates-after": "W66 certificate.mine ?? =@01-certificates"
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn unasserted_calls_shape_only_grading_and_forward_references_are_refused_unsent() {
    for (entry, reason) in [
        ("W50 courses.list", "explicit result expectation"),
        (
            "W51 progress.checkInline ?? /correct:bool",
            "inline grading requires",
        ),
        ("W50 courses.list ?? =@02-future", "earlier call"),
        (
            "W50 units.get {\"json\":{\"unitId\":{\"$call\":\"02-future\",\"pointer\":\"/id\"}}} ?? :object",
            "earlier call",
        ),
    ] {
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
        value["families"]["L12"]["nonsecret"] = json!({"call.01-invalid":entry});
        let (repo, record) = fixture
            .run(bound(value), &["L12"], &secrets(&[]), operator)
            .await;
        assert!(!record.expect("run").passed(), "{entry}");
        assert_eq!(receipt(&repo, "L12", "L12-calls").outcome, "blocked");
        assert!(
            receipt(&repo, "L12", "L12-calls").detail.contains(reason),
            "{entry}"
        );
        assert!(
            !server
                .seen()
                .iter()
                .any(|r| ["courses.list", "progress.checkInline", "units.get"]
                    .iter()
                    .any(|p| r.request_line.contains(&format!("/api/trpc/{p}")))),
            "{entry}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_qualification_rejects_unchanged_progress_lost_certificates_and_an_empty_baseline() {
    for (units, ignored, lost, empty_certificates, failed_check) in [
        (3, false, false, false, None),
        (3, true, false, false, Some("L13-call-04-after")),
        (
            3,
            false,
            true,
            false,
            Some("L13-call-05-certificates-after"),
        ),
        (0, false, false, false, Some("L13-call-02-before")),
        (3, false, false, true, Some("L13-call-01-certificates")),
    ] {
        let mut fixture = Fixture::new();
        let academy = Arc::new(Mutex::new(Academy {
            progress_units: units,
            reset_ignored: ignored,
            certificates_lost: lost,
            empty_certificates,
            ..Academy::default()
        }));
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
        value["families"]["L13"]["nonsecret"] = reset_program();
        value["families"]["L13"]["permitted_operations"] =
            json!(["progress.resetCourse", "settings.removeAvatar"]);
        let (repo, record) = fixture
            .run(bound(value), &["L13"], &secrets(&[]), operator)
            .await;
        assert_eq!(record.expect("run").passed(), failed_check.is_none());
        let workflow = receipt(&repo, "L13", "L13-workflow-W66");
        assert_eq!(
            workflow.outcome,
            if failed_check.is_none() {
                "passed"
            } else {
                "failed"
            }
        );
        if let Some(check) = failed_check {
            assert_eq!(receipt(&repo, "L13", check).outcome, "failed");
        }
        if units == 0 || empty_certificates {
            assert!(
                !server
                    .seen()
                    .iter()
                    .any(|r| r.request_line.contains("/api/trpc/progress.resetCourse"))
            );
            assert_eq!(
                receipt(&repo, "L13", "L13-call-03-reset").outcome,
                "blocked"
            );
        }
        // Even short certificate data is fingerprinted, never copied to disk.
        assert_no_secret(
            &repo,
            &[
                "Private Learner",
                "protected-certificate-id",
                "access-1",
                "refresh-1",
            ],
        );
    }
}

fn daily_program() -> Value {
    json!({
        "call.01-start": "W65 shift.start {\"json\":{\"daily\":true}} ?? /shiftId:nonempty-string && /alertsAnswered=0 && /atBoundary=false",
        "call.02-serve": "W65 shift.serve {\"json\":{\"shiftId\":{\"$call\":\"01-start\",\"pointer\":\"/shiftId\"}}} ?? /shift/shiftId=@01-start/shiftId && /shift/alertOrder=0 && /breachResolved=false",
        "call.03-answer": "W65 shift.answer {\"json\":{\"shiftId\":{\"$call\":\"01-start\",\"pointer\":\"/shiftId\"},\"order\":{\"$call\":\"02-serve\",\"pointer\":\"/shift/alertOrder\"},\"values\":[\"0\"]}} ?? /outcome in [\"correct\",\"incorrect\"] && /next=\"boundary\"",
        "call.04-decide": "W65 shift.decide {\"json\":{\"shiftId\":{\"$call\":\"01-start\",\"pointer\":\"/shiftId\"},\"choice\":\"hand_off\"}} ?? /shiftId=@01-start/shiftId && /status=\"handed_off\"",
        "call.05-debrief": "W65 shift.debrief {\"json\":{\"shiftId\":{\"$call\":\"01-start\",\"pointer\":\"/shiftId\"}}} ?? /themes:nonempty-array && /links:array && /fallback=false"
    })
}

async fn exercise_program(
    initial: Academy,
    family: &str,
    program: Value,
    permitted: &[&str],
) -> (xtask::Repo, xtask::live_run::RunRecord, Vec<Seen>) {
    exercise_program_budget(initial, family, program, permitted, 10).await
}

async fn exercise_program_budget(
    initial: Academy,
    family: &str,
    program: Value,
    permitted: &[&str],
    calls_budget: u64,
) -> (xtask::Repo, xtask::live_run::RunRecord, Vec<Seen>) {
    let mut fixture = Fixture::new();
    let short_settle = initial.hung_procedure.is_some();
    let academy = Arc::new(Mutex::new(initial));
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
    value["families"][family]["nonsecret"] = program;
    value["families"][family]["permitted_operations"] = json!(permitted);
    value["families"][family]["budget"] = json!({"calls":calls_budget});
    let mut evidence = fixture.evidence();
    if short_settle && let xtask::live_run::Evidence::Fixture(targets) = &mut evidence {
        targets.settle = Some(std::time::Duration::from_secs(3));
    }
    let repo = xtask::Repo::new(crate::common::scratch());
    let manifest = serde_json::from_value(bound(value)).expect("manifest");
    let record = xtask::live_run::run(
        &repo,
        &manifest,
        &"f".repeat(64),
        &secrets(&[]),
        operator,
        evidence,
        &[family.to_owned()],
    )
    .await;
    (repo, record.expect("run"), server.seen())
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_exam_identity_fails_and_the_state_read_uses_the_started_attempt() {
    let program = scope("https://localhost")["families"]["L13"]["nonsecret"].clone();
    let (repo, record, requests) = exercise_program(
        Academy {
            wrong_exam_id: true,
            ..Academy::default()
        },
        "L13",
        program,
        &["exam.start", "settings.removeAvatar"],
    )
    .await;
    assert!(!record.passed());
    assert_eq!(
        receipt(&repo, "L13", "L13-call-00-b-state").outcome,
        "failed"
    );
    let sent = requests
        .iter()
        .find(|r| r.request_line.contains("/api/trpc/exam.submissionState"))
        .expect("state read");
    let (_, _, query) = crate::common::parts(sent);
    let input: Value = serde_json::from_str(&query["input"]).expect("input");
    assert_eq!(input["json"]["attemptId"], ATTEMPT);
    assert_no_secret(&repo, &[ATTEMPT]);
}

#[tokio::test(flavor = "multi_thread")]
async fn wrong_shift_settlement_unknown_debrief_ungraded_breached_and_duplicate_daily_fail() {
    for (initial, failed_check) in [
        (
            Academy {
                wrong_settled_id: true,
                ..Academy::default()
            },
            "L12-call-04-decide",
        ),
        (
            Academy {
                empty_debrief: true,
                ..Academy::default()
            },
            "L12-call-05-debrief",
        ),
        (
            Academy {
                shift_outcome: Some("ungraded"),
                ..Academy::default()
            },
            "L12-call-03-answer",
        ),
        (
            Academy {
                shift_outcome: Some("breached"),
                ..Academy::default()
            },
            "L12-call-03-answer",
        ),
        (
            Academy {
                daily_refused: true,
                ..Academy::default()
            },
            "L12-call-01-start",
        ),
    ] {
        let (repo, record, requests) = exercise_program(
            initial,
            "L12",
            daily_program(),
            &["shift.start", "shift.serve", "shift.answer", "shift.decide"],
        )
        .await;
        assert!(!record.passed());
        assert_eq!(
            receipt(&repo, "L12", failed_check).outcome,
            if failed_check == "L12-call-04-decide" {
                "needs-reconciliation"
            } else {
                "failed"
            }
        );
        assert_eq!(receipt(&repo, "L12", "L12-workflow-W65").outcome, "failed");
        if failed_check == "L12-call-03-answer" {
            assert!(
                requests
                    .iter()
                    .any(|r| r.request_line.contains("/api/trpc/shift.decide")),
                "a designated hand-off is attempted without assuming a boundary"
            );
        }
        if failed_check == "L12-call-03-answer" {
            let cleanup = receipt(&repo, "L12", "L12-call-04-decide");
            assert_eq!(cleanup.outcome, "needs-reconciliation");
            assert!(cleanup.cleanup.contains("stale sweep"));
            assert!(record.stopped.as_ref().is_some_and(|stop| stop.reconcile));
        }
        if failed_check == "L12-call-01-start" {
            assert!(
                !requests
                    .iter()
                    .any(|r| r.request_line.contains("/api/trpc/shift.serve"))
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn weak_interactive_assertions_are_refused_before_any_program_call() {
    let mut exam = scope("https://localhost")["families"]["L13"]["nonsecret"].clone();
    exam["call.00-b-state"] = json!("W53 exam.submissionState ?? :object");
    let mut reset = reset_program();
    reset["call.04-after"] = json!(
        "W66 progress.courseSummary {\"json\":{\"courseId\":\"runtime-security\"}} ?? :object"
    );
    let mut shift = daily_program();
    shift["call.05-debrief"] = json!(
        "W65 shift.debrief {\"json\":{\"shiftId\":{\"$call\":\"01-start\",\"pointer\":\"/shiftId\"}}} ?? /themes:array"
    );
    for (family, program) in [("L13", exam), ("L13", reset), ("L12", shift)] {
        let (repo, record, requests) = exercise_program(
            Academy::default(),
            family,
            program,
            &[
                "exam.start",
                "progress.resetCourse",
                "shift.start",
                "shift.serve",
                "shift.answer",
                "shift.decide",
                "settings.removeAvatar",
            ],
        )
        .await;
        assert!(!record.passed());
        let check = receipt(&repo, family, &format!("{family}-calls"));
        assert_eq!(check.outcome, "blocked");
        assert!(
            check.detail.contains("required outcome assertions"),
            "{}",
            check.detail
        );
        assert!(!requests.iter().any(|r| {
            ["exam.start", "progress.resetCourse", "shift.start"]
                .iter()
                .any(|p| r.request_line.contains(&format!("/api/trpc/{p}")))
        }));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn correct_grading_passes_and_a_failed_query_dependency_is_never_sent() {
    let (repo,record,_) = exercise_program(Academy { inline_correct:true, ..Academy::default() },"L12",
        json!({"call.01-grade":"W51 progress.checkInline {\"json\":{\"lessonId\":\"lesson-1\",\"questionId\":\"q-1\",\"answer\":\"SYNTHETIC-PRIVATE-ANSWER\"}} ?? /correct=true"}), &["progress.checkInline"]).await;
    assert!(record.passed());
    assert_no_secret(&repo, &["SYNTHETIC-PRIVATE-ANSWER"]);
    assert_eq!(receipt(&repo, "L12", "L12-call-01-grade").outcome, "passed");
    let (repo,record,requests)=exercise_program(Academy::default(),"L12",json!({
        "call.01-wrong":"W50 units.get {\"json\":{\"unitId\":\"unit-1\"}} ?? /id=\"different-unit\"",
        "call.02-never":"W50 units.get {\"json\":{\"unitId\":{\"$call\":\"01-wrong\",\"pointer\":\"/id\"}}} ?? /id:nonempty-string"
    }),&["read"]).await;
    assert!(!record.passed());
    assert_eq!(
        receipt(&repo, "L12", "L12-call-02-never").outcome,
        "blocked"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.request_line.contains("/api/trpc/units.get"))
            .count(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn diagnostic_observations_are_read_only_and_never_claim_a_workflow() {
    let (repo, record, _) = exercise_program(
        Academy::default(),
        "L12",
        json!({
            "call.01-observe":"observe certificate.mine ?? :array"
        }),
        &["read"],
    )
    .await;
    assert!(record.passed());
    assert!(
        receipt(&repo, "L12", "L12-call-01-observe")
            .workflows
            .is_empty()
    );
    let (repo,record,requests)=exercise_program(Academy::default(),"L12",json!({
        "call.01-observe":"observe progress.resetCourse {\"json\":{\"courseId\":\"runtime-security\"}} ?? :object"
    }),&["progress.resetCourse"]).await;
    assert!(!record.passed());
    assert!(
        receipt(&repo, "L12", "L12-calls")
            .detail
            .contains("observe is read-only")
    );
    assert!(
        !requests
            .iter()
            .any(|r| r.request_line.contains("/api/trpc/progress.resetCourse"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_in_and_failed_receipts_never_echo_private_expectation_literals() {
    let (repo,record,_)=exercise_program(Academy::default(),"L12",json!({
        "call.01-membership":"observe settings.get ?? /theme in [\"system\",\"PRIVATE-PREFERENCE\"]",
        "call.02-equality":"observe settings.get ?? /theme=\"PRIVATE-EQUALITY\""
    }),&["read"]).await;
    assert!(!record.passed());
    assert_no_secret(&repo, &["PRIVATE-PREFERENCE", "PRIVATE-EQUALITY"]);
    let sign_in = receipt(&repo, "L12", "L11-device-sign-in");
    assert!(!sign_in.inputs.keys().any(|k| k.starts_with("call.")));
}

#[tokio::test(flavor = "multi_thread")]
async fn call_labels_cannot_select_receipt_paths() {
    for label in [
        "01/nested".to_owned(),
        "..".to_owned(),
        "".to_owned(),
        "a".repeat(97),
    ] {
        let key = format!("call.{label}");
        let (repo, record, requests) = exercise_program(
            Academy::default(),
            "L12",
            json!({key:"W50 courses.list ?? :nonempty-array"}),
            &["read"],
        )
        .await;
        assert!(!record.passed());
        let blocked = receipt(&repo, "L12", "L12-calls");
        assert_eq!(blocked.outcome, "blocked");
        assert!(blocked.detail.contains("1-96 ASCII"));
        assert!(
            !requests
                .iter()
                .any(|r| r.request_line.contains("/api/trpc/courses.list"))
        );
    }
    let (repo, record, requests) = exercise_program(
        Academy::default(),
        "L12",
        json!({
            "call.Start":"W50 courses.list ?? :nonempty-array",
            "call.start":"W50 courses.list ?? :nonempty-array"
        }),
        &["read"],
    )
    .await;
    assert!(!record.passed());
    assert!(
        receipt(&repo, "L12", "L12-calls")
            .detail
            .contains("unique ignoring case")
    );
    assert!(
        !requests
            .iter()
            .any(|r| r.request_line.contains("/api/trpc/courses.list"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_read_timeout_stops_without_claiming_a_remote_mutation() {
    let (repo, record, requests) = exercise_program(
        Academy {
            hung_procedure: Some("courses.list"),
            ..Academy::default()
        },
        "L13",
        json!({"call.01-catalog":"observe courses.list ?? :nonempty-array"}),
        &["settings.removeAvatar"],
    )
    .await;
    assert!(!record.passed());
    assert!(record.stopped.as_ref().is_some_and(|stop| !stop.reconcile));
    let read = receipt(&repo, "L13", "L13-call-01-catalog");
    assert_eq!(read.outcome, "failed");
    assert!(read.detail.contains("did not settle"));
    assert_eq!(read.cleanup, "nothing created");
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.request_line.contains("/api/trpc/courses.list"))
            .count(),
        1
    );
    assert!(!requests.iter().any(|r| {
        r.request_line
            .contains("/api/trpc/settings.avatarUploadUrl")
            || r.request_line.contains("/api/trpc/settings.removeAvatar")
    }));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_known_avatar_upload_refusal_never_removes_the_existing_avatar() {
    for outcome in ["rejected", "notSent"] {
        let (repo, record, requests) = exercise_program(
            Academy {
                avatar_presign: Some(outcome),
                ..Academy::default()
            },
            "L13",
            json!({"call.01-observe":"observe settings.get ?? /theme=\"system\""}),
            &["settings.removeAvatar"],
        )
        .await;
        assert!(!record.passed());
        assert!(record.stopped.is_none(), "{outcome}");
        let avatar = receipt(&repo, "L13", "L13-avatar");
        assert_eq!(avatar.outcome, "failed", "{outcome}");
        assert_eq!(avatar.observed["outcome"], outcome);
        assert_eq!(
            requests
                .iter()
                .filter(|r| r
                    .request_line
                    .contains("/api/trpc/settings.avatarUploadUrl"))
                .count(),
            1
        );
        assert!(
            !requests.iter().any(|r| {
                r.request_line.starts_with("PUT ")
                    || r.request_line.contains("/api/trpc/settings.confirmAvatar")
                    || r.request_line.contains("/api/trpc/settings.removeAvatar")
            }),
            "{outcome}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_unobserved_mutation_stops_for_reconciliation_before_later_changes() {
    for (procedure, hang) in [
        ("exam.start", false),
        ("progress.resetCourse", false),
        ("settings.confirmAvatar", false),
        ("settings.removeAvatar", false),
        ("exam.start", true),
        ("shift.serve", true),
    ] {
        let mut initial = Academy {
            progress_units: 3,
            ..Academy::default()
        };
        if hang {
            initial.hung_procedure = Some(procedure);
        } else {
            initial.dropped_procedure = Some(procedure);
        }
        let (family, program, permitted, check) = match procedure {
            "exam.start" => (
                "L13",
                scope("https://localhost")["families"]["L13"]["nonsecret"].clone(),
                vec!["exam.start", "settings.removeAvatar"],
                "L13-call-00-a-start",
            ),
            "progress.resetCourse" => (
                "L13",
                reset_program(),
                vec!["progress.resetCourse", "settings.removeAvatar"],
                "L13-call-03-reset",
            ),
            "shift.serve" => (
                "L12",
                daily_program(),
                vec!["shift.start", "shift.serve", "shift.answer", "shift.decide"],
                "L12-call-02-serve",
            ),
            _ => (
                "L13",
                json!({"call.01-observe":"observe settings.get ?? /theme=\"system\""}),
                vec!["settings.removeAvatar"],
                "L13-avatar",
            ),
        };
        let (repo, record, requests) = exercise_program(initial, family, program, &permitted).await;
        assert!(!record.passed());
        assert!(record.stopped.as_ref().is_some_and(|stop| stop.reconcile));
        let mutation = receipt(&repo, family, check);
        assert_eq!(mutation.outcome, "needs-reconciliation", "{procedure}");
        assert!(mutation.cleanup.contains("inspect") || mutation.cleanup.contains("remote"));
        if hang {
            assert!(mutation.detail.contains("did not settle"));
        } else if check != "L13-avatar" {
            assert_eq!(mutation.observed["outcome"], "unknown");
        }
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.request_line.contains(&format!("/api/trpc/{procedure}")))
                .count(),
            1,
            "never replay {procedure}"
        );
        if procedure == "progress.resetCourse" {
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| r.request_line.contains("/api/trpc/progress.courseSummary"))
                    .count(),
                1
            );
        }
        if ["exam.start", "progress.resetCourse"].contains(&procedure) {
            assert!(!requests.iter().any(|r| {
                r.request_line
                    .contains("/api/trpc/settings.avatarUploadUrl")
            }));
        }
        if procedure != "settings.removeAvatar" {
            assert!(
                !requests
                    .iter()
                    .any(|r| r.request_line.contains("/api/trpc/settings.removeAvatar"))
            );
        }
        if procedure == "shift.serve" {
            assert!(
                !requests
                    .iter()
                    .any(|r| r.request_line.contains("/api/trpc/shift.answer"))
            );
            assert!(
                !requests
                    .iter()
                    .any(|r| r.request_line.contains("/api/trpc/shift.decide"))
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unsettled_daily_is_reconciled_when_budget_or_cleanup_dependencies_stop_the_run() {
    let permitted = ["shift.start", "shift.serve", "shift.answer", "shift.decide"];
    let (repo, record, requests) =
        exercise_program_budget(Academy::default(), "L12", daily_program(), &permitted, 1).await;
    assert!(!record.passed());
    assert!(record.stopped.as_ref().is_some_and(|stop| stop.reconcile));
    let refused = receipt(&repo, "L12", "L12-call-02-serve");
    assert_eq!(refused.outcome, "needs-reconciliation");
    assert_eq!(refused.consumption["calls"]["used"], 1);
    assert!(refused.cleanup.contains("no extra call"));
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.request_line.contains("/api/trpc/shift.start"))
            .count(),
        1
    );
    for name in [
        "shift.serve",
        "shift.answer",
        "shift.decide",
        "shift.debrief",
    ] {
        assert!(
            !requests
                .iter()
                .any(|r| r.request_line.contains(&format!("/api/trpc/{name}")))
        );
    }

    let mut program = daily_program();
    program["call.04-decide"] = json!(format!(
        "{} && /shiftId=@02-serve/shift/shiftId",
        program["call.04-decide"].as_str().expect("call")
    ));
    for initial in [
        Academy {
            serve_unexpected: true,
            ..Academy::default()
        },
        Academy {
            start_invalid: true,
            ..Academy::default()
        },
        Academy {
            lost_start: true,
            ..Academy::default()
        },
    ] {
        let (repo, record, requests) =
            exercise_program_budget(initial, "L12", program.clone(), &permitted, 5).await;
        assert!(!record.passed());
        assert!(record.stopped.as_ref().is_some_and(|stop| stop.reconcile));
        let stop = record.stopped.as_ref().expect("stop");
        let cleanup = receipt(&repo, "L12", &stop.check);
        assert_eq!(cleanup.outcome, "needs-reconciliation");
        if stop.check == "L12-call-04-decide" {
            assert!(cleanup.detail.contains("call not sent"));
            assert!(cleanup.cleanup.contains("dependency unavailable"));
        } else {
            assert_eq!(stop.check, "L12-call-01-start");
            assert_eq!(cleanup.observed["outcome"], "unknown");
        }
        assert!(
            !requests
                .iter()
                .any(|r| r.request_line.contains("/api/trpc/shift.decide"))
        );
        assert!(
            !requests
                .iter()
                .any(|r| r.request_line.contains("/api/trpc/shift.answer"))
        );
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.request_line.contains("/api/trpc/shift.start"))
                .count(),
            1
        );
    }
    // A later budget refusal must fail the workflow, but a confirmed
    // hand-off must not be reported as an active, unreconciled Daily run.
    let mut program = daily_program();
    program["call.06-history"] = json!("W65 shift.history ?? :array");
    let (repo, record, requests) =
        exercise_program_budget(Academy::default(), "L12", program, &permitted, 5).await;
    assert!(!record.passed());
    assert!(record.stopped.as_ref().is_some_and(|stop| !stop.reconcile));
    assert_eq!(
        receipt(&repo, "L12", "L12-call-04-decide").outcome,
        "passed"
    );
    assert_eq!(receipt(&repo, "L12", "L12-workflow-W65").outcome, "failed");
    assert!(
        !requests
            .iter()
            .any(|r| r.request_line.contains("/api/trpc/shift.history"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn destructive_procedures_cannot_bypass_contracts_by_changing_workflow_tags() {
    for (family, workflow, procedure, input) in [
        (
            "L12",
            "W51",
            "progress.resetCourse",
            json!({"json":{"courseId":COURSE}}),
        ),
        (
            "L13",
            "W54",
            "progress.resetCourse",
            json!({"json":{"courseId":COURSE}}),
        ),
        ("L13", "W54", "exam.start", json!({"json":{"consent":true}})),
        ("L12", "W52", "shift.start", json!({"json":{"daily":true}})),
        (
            "L13",
            "W54",
            "shift.serve",
            json!({"json":{"shiftId":ATTEMPT}}),
        ),
        (
            "L13",
            "W54",
            "shift.answer",
            json!({"json":{"shiftId":ATTEMPT,"order":0,"values":["0"]}}),
        ),
        (
            "L13",
            "W54",
            "shift.decide",
            json!({"json":{"shiftId":ATTEMPT,"choice":"hand_off"}}),
        ),
    ] {
        let (repo, record, requests) = exercise_program(
            Academy::default(),
            family,
            json!({"call.01-bypass":format!("{workflow} {procedure} {input} ?? :object")}),
            &[procedure],
        )
        .await;
        assert!(!record.passed());
        let check = receipt(&repo, family, &format!("{family}-calls"));
        assert!(
            check
                .detail
                .contains("requires its interactive workflow contract"),
            "{}",
            check.detail
        );
        assert!(
            !requests
                .iter()
                .any(|r| r.request_line.contains(&format!("/api/trpc/{procedure}")))
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn daily_incident_uses_the_started_run_and_served_order_for_its_interactive_checkpoints() {
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
    value["families"]["L12"]["permitted_operations"] =
        json!(["shift.start", "shift.serve", "shift.answer", "shift.decide"]);
    let mut private_program = daily_program();
    private_program["call.03-answer"] = json!(
        private_program["call.03-answer"]
            .as_str()
            .expect("call")
            .replace("[\"0\"]", "[\"SYNTHETIC-PRIVATE-CHOICE\"]")
    );
    value["families"]["L12"]["nonsecret"] = private_program;
    let (repo, record) = fixture
        .run(bound(value), &["L12"], &secrets(&[]), operator)
        .await;
    assert!(record.expect("run").passed());
    assert_eq!(receipt(&repo, "L12", "L12-workflow-W65").outcome, "passed");
    let request = server
        .seen()
        .into_iter()
        .find(|r| r.request_line.contains("/api/trpc/shift.answer"))
        .expect("answer sent");
    let sent: Value = serde_json::from_slice(&request.body).expect("JSON input");
    assert_eq!(sent["json"]["shiftId"], ATTEMPT);
    assert_eq!(sent["json"]["order"], 0);
    assert_eq!(sent["json"]["values"][0], "SYNTHETIC-PRIVATE-CHOICE");
    assert_no_secret(&repo, &["SYNTHETIC-PRIVATE-CHOICE"]);
    assert_no_secret(&repo, &[ATTEMPT, "access-1", "refresh-1"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_do_not_close_interactive_rows_and_a_failed_dependency_is_never_sent() {
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
    value["families"]["L13"]["nonsecret"] = json!({
        "call.01-read": "W53 exam.eligibility ?? /eligible=true",
        "call.02-failed": "W53 certificate.mine ?? =[]",
        "call.03-never": "W53 exam.submissionState {\"json\":{\"attemptId\":{\"$call\":\"02-failed\",\"pointer\":\"/0/id\"}}} ?? /status=\"in-progress\""
    });
    let (repo, record) = fixture
        .run(bound(value), &["L13"], &secrets(&[]), operator)
        .await;
    assert!(!record.expect("run").passed());
    let blocked = receipt(&repo, "L13", "L13-calls");
    assert_eq!(blocked.outcome, "blocked");
    assert!(blocked.detail.contains("ordered interactive checkpoints"));
    assert!(
        !server
            .seen()
            .iter()
            .any(|r| r.request_line.contains("/api/trpc/exam.submissionState"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unexpected_or_undesignated_account_cannot_run_a_mutating_program() {
    for mismatch in [true, false] {
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
        if mismatch {
            value["families"]["L12"]["nonsecret"]["expected_account_sha256"] =
                json!("a".repeat(64));
        } else {
            value["families"]["L12"]["nonsecret"]
                .as_object_mut()
                .expect("fields")
                .remove("expected_account_sha256");
        }
        let (repo, record) = fixture.run(value, &["L12"], &secrets(&[]), operator).await;
        let record = record.expect("run");
        assert!(!record.passed());
        let check = if mismatch {
            "L12-account-binding"
        } else {
            "L12-calls"
        };
        assert_eq!(receipt(&repo, "L12", check).outcome, "blocked");
        assert!(!server.seen().iter().any(|r| {
            ["courses.list", "units.get", "progress.checkInline"]
                .iter()
                .any(|p| r.request_line.contains(&format!("/api/trpc/{p}")))
        }));
        assert!(crate::common::within(|| academy.lock().expect("academy").revoked >= 1).await);
    }
}
