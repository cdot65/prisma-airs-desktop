//! C06 Academy graded workflows through the production authority interface:
//! exam, Shift and desktop lab attempts against a synthetic Academy on this
//! Mac. Academy owns timing, grading, awards and idempotency; the desktop
//! forwards the learner's literal requests once, reports unknown outcomes
//! without resending, keeps slow optional reads from blocking timed work,
//! never lets a stale reply touch a newer account, and refuses assessor,
//! grading and award procedures before any request. Native host actions
//! (print sheet, avatar upload and readback) are observed like learner calls
//! and report what is known of their effect.

include!("../academy_identity/harness.rs");

fn trpc_requests(desk: &Desk, procedure: &str) -> Vec<Seen> {
    desk.requests(&format!("/api/trpc/{procedure}"))
}

fn data(value: Value) -> Value {
    json!({"result": {"data": {"json": value}}})
}

fn error(status: u16, code: &str, message: &str) -> Value {
    json!({"error": {"json": {"message": message, "code": -32000,
        "data": {"code": code, "httpStatus": status}}}})
}

async fn admit(
    desk: &mut Desk,
    procedure: &str,
    kind: &str,
    input: Option<&str>,
) -> desktop_core::runtime::OperationId {
    let learner = desk.current_learner();
    let receipt = desk
        .send(
            json!({"kind": "academyCall", "learner": learner, "procedure": procedure,
                     "procedureKind": kind, "input": input}),
        )
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!("no operation")
    };
    operation
}

#[tokio::test(flavor = "multi_thread")]
async fn exam_requests_pass_literally_and_academy_timing_comes_back_untouched() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let start = json!({"attemptId": "00000000-0000-4000-8000-0000000000e1",
        "deadline": "2026-10-17T13:00:00.000Z", "questions": [], "terminalItems": []});
    desk.script.lock().expect("script").trpc = vec![
        ("exam.start".into(), 200, data(start.clone())),
        ("exam.answer".into(), 200, data(json!({"saved": true}))),
    ];
    let start_input = r#"{"json":{"certificationId":"ai-app-security"}}"#;
    let reply = desk
        .call("exam.start", "mutation", Some(start_input))
        .await
        .expect("admitted");
    let AcademyCallResult::Data { data: returned } = reply else {
        panic!("{reply:?}")
    };
    // Academy's deadline arrives exactly as sent; the desktop adds no clock.
    assert_eq!(
        serde_json::from_str::<Value>(&returned).expect("json"),
        json!({"json": start})
    );
    let answer_input = r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000e1","questionId":"q1","value":0}}"#;
    desk.call("exam.answer", "mutation", Some(answer_input))
        .await
        .expect("admitted");
    for (procedure, body) in [("exam.start", start_input), ("exam.answer", answer_input)] {
        let seen = trpc_requests(&desk, procedure);
        assert_eq!(seen.len(), 1, "{procedure}");
        assert_eq!(seen[0].request_line.split(' ').next(), Some("POST"));
        assert_eq!(seen[0].body, body.as_bytes(), "{procedure} byte for byte");
        assert_eq!(seen[0].header("authorization"), Some("Bearer access-1"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_submission_reports_an_unknown_outcome_once_and_reconciles_by_query() {
    let mut desk = desk().await;
    desk.sign_in().await;
    desk.script.lock().expect("script").trpc = vec![
        (
            "exam.submit".into(),
            503,
            error(503, "INTERNAL_SERVER_ERROR", "upstream"),
        ),
        (
            "exam.submissionState".into(),
            200,
            data(
                json!({"attemptId": "00000000-0000-4000-8000-0000000000e1", "status": "submitted"}),
            ),
        ),
    ];
    let submit = r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000e1","answers":[]}}"#;
    let reply = desk
        .call("exam.submit", "mutation", Some(submit))
        .await
        .expect("admitted");
    let AcademyCallResult::Error {
        outcome,
        http_status,
        ..
    } = reply
    else {
        panic!("{reply:?}")
    };
    assert_eq!((outcome, http_status), (Outcome::Unknown, 503));
    // The mutation was sent once: unknown is not permission to resend it.
    assert_eq!(trpc_requests(&desk, "exam.submit").len(), 1);
    let state = desk
        .call(
            "exam.submissionState",
            "query",
            Some(r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000e1"}}"#),
        )
        .await
        .expect("admitted");
    assert!(matches!(state, AcademyCallResult::Data { .. }), "{state:?}");
    assert_eq!(trpc_requests(&desk, "exam.submit").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn desktop_lab_idempotency_keys_and_versions_reach_academy_unchanged() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let conflict = error(
        409,
        "CONFLICT",
        "The draft changed. Review both copies again.",
    );
    let graded =
        data(json!({"passed": false, "feedback": "Check every step before submitting the lab."}));
    desk.script.lock().expect("script").trpc = vec![
        (
            "desktop.attempt.start".into(),
            200,
            data(json!({"attemptId": "00000000-0000-4000-8000-0000000000a1"})),
        ),
        (
            "desktop.draft.save".into(),
            200,
            data(json!({"status": "saved", "draftVersion": 1})),
        ),
        ("desktop.attempt.submit".into(), 200, graded.clone()),
        ("desktop.attempt.submit".into(), 200, graded),
        ("desktop.draft.resolve".into(), 409, conflict),
    ];
    let start = r#"{"json":{"labId":"lab-1","requestId":"00000000-0000-4000-8000-0000000000b1"}}"#;
    let save = r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000a1","baseVersion":0,"clientMutationId":"00000000-0000-4000-8000-0000000000b2","draft":{"commands":[{"op":"runtime.profiles.list","params":{}}]}}}"#;
    let submit = r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000a1","draftVersion":1,"submissionId":"00000000-0000-4000-8000-0000000000b3"}}"#;
    desk.call("desktop.attempt.start", "mutation", Some(start))
        .await
        .expect("start");
    desk.call("desktop.draft.save", "mutation", Some(save))
        .await
        .expect("save");
    // The learner submits the same work twice with the same id: both are
    // sent as asked (Academy answers the second from its receipt).
    for _ in 0..2 {
        desk.call("desktop.attempt.submit", "mutation", Some(submit))
            .await
            .expect("submit");
    }
    let resolve = r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000a1","conflictId":"00000000-0000-4000-8000-0000000000c1","baseVersion":3,"choice":"incoming"}}"#;
    let reply = desk
        .call("desktop.draft.resolve", "mutation", Some(resolve))
        .await
        .expect("resolve");
    let AcademyCallResult::Error {
        outcome,
        http_status,
        ..
    } = reply
    else {
        panic!("{reply:?}")
    };
    assert_eq!(
        (outcome, http_status),
        (Outcome::Rejected, 409),
        "a stale resolve is Academy's conflict"
    );
    for (procedure, body, count) in [
        ("desktop.attempt.start", start, 1),
        ("desktop.draft.save", save, 1),
        ("desktop.attempt.submit", submit, 2),
        ("desktop.draft.resolve", resolve, 1),
    ] {
        let seen = trpc_requests(&desk, procedure);
        assert_eq!(seen.len(), count, "{procedure}");
        assert!(
            seen.iter().all(|s| s.body == body.as_bytes()),
            "{procedure} byte for byte"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_optional_briefing_never_blocks_a_timed_shift_answer() {
    let mut desk = desk().await;
    desk.sign_in().await;
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc = vec![
            (
                "shift.briefing".into(),
                200,
                data(json!({"briefing": "optional context"})),
            ),
            ("shift.answer".into(), 200, data(json!({"recorded": true}))),
            ("shift.serve".into(), 200, data(json!({"served": true}))),
        ];
        script.trpc_delays = vec![("shift.briefing".into(), 3000)];
    }
    let briefing = admit(&mut desk, "shift.briefing", "query", None).await;
    let started = std::time::Instant::now();
    let answer = admit(
        &mut desk,
        "shift.answer",
        "mutation",
        Some(r#"{"json":{"runId":"00000000-0000-4000-8000-0000000000d1","alertId":"a1","choice":"block"}}"#),
    )
    .await;
    let reply = desk.result(&answer).await;
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    let serve = admit(
        &mut desk,
        "shift.serve",
        "mutation",
        Some(r#"{"json":{"runId":"00000000-0000-4000-8000-0000000000d1"}}"#),
    )
    .await;
    assert!(matches!(
        desk.result(&serve).await,
        AcademyCallResult::Data { .. }
    ));
    assert!(
        started.elapsed() < Duration::from_millis(2500),
        "the timed answer and serve did not wait for the briefing: {:?}",
        started.elapsed()
    );
    assert!(matches!(
        desk.result(&briefing).await,
        AcademyCallResult::Data { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_late_unauthorized_reply_for_an_earlier_generation_never_retires_the_newer_account() {
    let mut script = Script::new();
    script.polls = vec![
        (200, bearer("access-A", "refresh-A", 3600)),
        (200, bearer("access-B", "refresh-B", 3600)),
    ];
    let mut desk = desk_with(script, None).await;
    desk.sign_in().await;
    let first = desk.learner();
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc = vec![(
            "shift.briefing".into(),
            401,
            error(401, "UNAUTHORIZED", "UNAUTHORIZED"),
        )];
        script.trpc_delays = vec![("shift.briefing".into(), 1500)];
    }
    let late = admit(&mut desk, "shift.briefing", "query", None).await;
    let Ok(Observation::AcademyCall(late)) = desk.handle.observe(ViewScope::AcademyCall(late))
    else {
        panic!("call observation")
    };
    assert!(eventually(|| !trpc_requests(&desk, "shift.briefing").is_empty()).await);
    // The learner signs out and in again (account B) while A's briefing is
    // outstanding.
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("sign out");
    assert!(eventually(|| matches!(desk.academy(), AcademyView::SignedOut)).await);
    desk.sign_in().await;
    let second = desk.learner();
    assert_ne!(first, second);
    // A's call ends for A's generation; nothing from it is delivered.
    let ended = tokio::time::timeout(Duration::from_secs(5), late.result())
        .await
        .expect("A's call ends");
    assert!(
        matches!(
            ended,
            AcademyCallResult::Failed {
                failure: desktop_core::academy::CallFailure::Cancelled,
                ..
            }
        ),
        "{ended:?}"
    );
    // After A's 401 would have landed, B is still signed in and still works.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert!(
        matches!(desk.academy(), AcademyView::SignedIn { ref learner, .. } if learner.as_str() == second),
        "{:?}",
        desk.academy()
    );
    assert!(
        desk.requests("/oauth/token")
            .iter()
            .all(|s| !form(s).contains(&("grant_type".into(), "refresh_token".into())))
    );
    desk.script.lock().expect("script").trpc =
        vec![("progress.get".into(), 200, data(json!({"done": 1})))];
    let reply = desk
        .call("progress.get", "query", None)
        .await
        .expect("newer account works");
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    assert_eq!(
        trpc_requests(&desk, "progress.get")[0].header("authorization"),
        Some("Bearer access-B")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn assessor_grading_and_award_procedures_are_refused_before_any_request() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let learner = desk.learner();
    for (procedure, kind) in [
        ("identityPractical.queue", "query"),
        ("identityPractical.review", "mutation"),
        ("exam.grade", "mutation"),
        ("exam.override", "mutation"),
        ("admin.users.list", "query"),
        ("labs.submitLab", "mutation"),
        ("gamification.award", "mutation"),
        ("certificate.issue", "mutation"),
        ("exam.submit", "query"),
    ] {
        assert_eq!(
            desk.call_as(&learner, procedure, kind, None).await.err(),
            Some(Refusal::CapabilityMissing),
            "{procedure} as {kind}"
        );
    }
    assert!(
        desk.requests("/api/trpc/")
            .iter()
            .all(|s| s.request_line.contains("desktop.me"))
    );
}

/// Dispatches a native host action and waits for its learner-call result.
async fn host(desk: &mut Desk, intent: Value) -> Result<AcademyCallResult, Refusal> {
    let receipt = desk.send(intent).await?;
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!("no operation: {receipt:?}")
    };
    Ok(desk.result(&operation).await)
}

/// A real, decodable source photo (larger than the prepared avatar).
fn photo() -> Vec<u8> {
    desktop_core::avatar::samples::photo(900, 600, image::ImageFormat::Png)
}

fn origin(desk: &Desk) -> String {
    desk.script.lock().expect("script").origin.clone()
}

fn failed(failure: CallFailure, outcome: Outcome) -> AcademyCallResult {
    AcademyCallResult::Failed { failure, outcome }
}

fn presigned(url: String) -> Value {
    data(json!({"uploadUrl": url, "key": "avatars/user_2fixture.jpg"}))
}

/// The prepared avatar a PUT carried: a 512-pixel square JPEG.
fn assert_prepared(body: &[u8]) {
    assert_eq!(&body[..3], &[0xFF, 0xD8, 0xFF], "a JPEG");
    assert!(body.len() <= desktop_core::avatar::MAX_AVATAR_BYTES);
    assert_eq!(
        desktop_core::avatar::samples::dimensions(body),
        (
            desktop_core::avatar::AVATAR_SIZE,
            desktop_core::avatar::AVATAR_SIZE
        )
    );
}

use desktop_core::academy::CallFailure;

#[tokio::test(flavor = "multi_thread")]
async fn print_opens_the_native_sheet_once_and_changes_no_state() {
    let mut desk = desk().await;
    let before = desk.snap();
    let learner = desk.current_learner();
    let reply = host(&mut desk, json!({"kind": "printPage", "learner": learner}))
        .await
        .expect("admitted");
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    assert_eq!(desk.adapters.printer.printed(), 1);
    let after = desk.snap();
    assert_eq!(
        (after.academy, after.tenants, after.storage),
        (before.academy, before.tenants, before.storage)
    );
    assert!(desk.server.seen().is_empty(), "printing contacts nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_print_sheet_that_cannot_open_says_so_and_changes_nothing() {
    let mut desk = desk().await;
    desk.adapters.printer.set_unavailable(true);
    let before = desk.snap();
    let learner = desk.current_learner();
    let reply = host(&mut desk, json!({"kind": "printPage", "learner": learner}))
        .await
        .expect("admitted");
    assert_eq!(
        reply,
        failed(CallFailure::HostUnavailable, Outcome::NotSent)
    );
    assert_eq!(desk.adapters.printer.printed(), 0);
    let after = desk.snap();
    assert_eq!(
        (after.academy, after.tenants, after.storage),
        (before.academy, before.tenants, before.storage)
    );
    // A stale generation cannot ask for a sheet at all.
    assert_eq!(
        desk.send(json!({"kind": "printPage", "learner": "lx-stale"}))
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chosen_photo_is_prepared_stored_at_the_presigned_destination_and_confirmed() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let origin = origin(&desk);
    desk.adapters
        .images
        .reply(Some(Ok(("me.png".into(), photo()))));
    let confirmed = json!({"displayName": "Ada Lovelace",
        "account": {"avatarUrl": format!("{origin}/api/avatars/user_2fixture.jpg?v=7")}});
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc = vec![
            (
                "settings.avatarUploadUrl".into(),
                200,
                presigned(format!(
                    "{origin}/upload/avatars/user_2fixture.jpg?X-Amz-Signature=s"
                )),
            ),
            (
                "settings.confirmAvatar".into(),
                200,
                data(confirmed.clone()),
            ),
        ];
        script.files = vec![(
            "/upload/avatars/user_2fixture.jpg".into(),
            200,
            "text/plain".into(),
            Vec::new(),
        )];
    }
    let learner = desk.learner();
    let reply = host(
        &mut desk,
        json!({"kind": "uploadAvatar", "learner": learner}),
    )
    .await
    .expect("admitted");
    let AcademyCallResult::Data { data: returned } = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&returned).expect("json"),
        json!({"json": confirmed})
    );
    let stored = desk.requests("/upload/avatars/user_2fixture.jpg");
    assert_eq!(stored.len(), 1);
    assert!(stored[0].request_line.starts_with("PUT "));
    assert_eq!(stored[0].header("content-type"), Some("image/jpeg"));
    assert_eq!(
        stored[0].header("authorization"),
        None,
        "the signature is in the URL"
    );
    assert_prepared(&stored[0].body);
    let presign = trpc_requests(&desk, "settings.avatarUploadUrl");
    assert_eq!(presign.len(), 1);
    assert_eq!(
        presign[0].body,
        format!(
            r#"{{"json":{{"contentType":"image/jpeg","size":{}}}}}"#,
            stored[0].body.len()
        )
        .as_bytes()
    );
    assert_eq!(presign[0].header("authorization"), Some("Bearer access-1"));
    let confirm = trpc_requests(&desk, "settings.confirmAvatar");
    assert_eq!(confirm.len(), 1);
    assert_eq!(confirm[0].body, br#"{"json":{"ext":"jpg"}}"#);
}

#[tokio::test(flavor = "multi_thread")]
async fn extreme_landscape_and_portrait_photos_are_prepared_from_their_centre_and_uploaded() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let origin = origin(&desk);
    let learner = desk.learner();
    for (width, height) in [(8192, 1), (1, 8192)] {
        desk.adapters.images.reply(Some(Ok((
            "strip.png".into(),
            desktop_core::avatar::samples::photo(width, height, image::ImageFormat::Png),
        ))));
        {
            let mut script = desk.script.lock().expect("script");
            script.trpc = vec![
                (
                    "settings.avatarUploadUrl".into(),
                    200,
                    presigned(format!(
                        "{origin}/upload/avatars/user_2fixture.jpg?sig={width}"
                    )),
                ),
                (
                    "settings.confirmAvatar".into(),
                    200,
                    data(json!({"account": {}})),
                ),
            ];
            script.files = vec![(
                "/upload/avatars/user_2fixture.jpg".into(),
                200,
                "text/plain".into(),
                Vec::new(),
            )];
        }
        let reply = host(
            &mut desk,
            json!({"kind": "uploadAvatar", "learner": learner}),
        )
        .await
        .expect("admitted");
        assert!(
            matches!(reply, AcademyCallResult::Data { .. }),
            "{width}x{height}: {reply:?}"
        );
    }
    let stored = desk.requests("/upload/avatars/user_2fixture.jpg");
    assert_eq!(stored.len(), 2);
    for put in &stored {
        assert_prepared(&put.body);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_avatar_upload_sends_nothing_when_cancelled_or_the_photo_does_not_decode() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let learner = desk.learner();
    let png = photo();
    let mut oversize = png.clone();
    oversize.resize(desktop_core::avatar::MAX_SOURCE_BYTES + 1, 0);
    for (pick, expected) in [
        (None, failed(CallFailure::Cancelled, Outcome::NotSent)),
        (
            Some(Ok((
                "fake.png".into(),
                b"\x89PNG\r\n\x1a\nsynthetic text, not pixels".to_vec(),
            ))),
            failed(CallFailure::UnsupportedImage, Outcome::NotSent),
        ),
        (
            Some(Ok(("cut.png".into(), png[..png.len() / 2].to_vec()))),
            failed(CallFailure::UnsupportedImage, Outcome::NotSent),
        ),
        (
            Some(Ok(("a.gif".into(), b"GIF89a\x01\0\x01\0".to_vec()))),
            failed(CallFailure::UnsupportedImage, Outcome::NotSent),
        ),
        (
            Some(Ok(("b.svg".into(), b"<svg onload=x/>".to_vec()))),
            failed(CallFailure::UnsupportedImage, Outcome::NotSent),
        ),
        (
            Some(Ok(("big.png".into(), oversize))),
            failed(CallFailure::UnsupportedImage, Outcome::NotSent),
        ),
        // Within 8192 pixels a side, but its decoded pixels exceed the budget.
        (
            Some(Ok((
                "7000x7000.png".into(),
                desktop_core::avatar::samples::over_budget_png().to_vec(),
            ))),
            failed(CallFailure::UnsupportedImage, Outcome::NotSent),
        ),
    ] {
        desk.adapters.images.reply(pick);
        let reply = host(
            &mut desk,
            json!({"kind": "uploadAvatar", "learner": learner}),
        )
        .await
        .expect("admitted");
        assert_eq!(reply, expected);
    }
    assert!(trpc_requests(&desk, "settings.avatarUploadUrl").is_empty());
    assert!(trpc_requests(&desk, "settings.confirmAvatar").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_avatar_never_goes_to_an_unreviewed_destination_or_past_a_refused_step() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let origin = origin(&desk);
    let learner = desk.learner();
    // Academy names a destination outside the reviewed set: nothing is stored.
    desk.adapters
        .images
        .reply(Some(Ok(("me.png".into(), photo()))));
    desk.script.lock().expect("script").trpc = vec![(
        "settings.avatarUploadUrl".into(),
        200,
        presigned("https://uploads.evil.example/u.jpg?sig=1".into()),
    )];
    let reply = host(
        &mut desk,
        json!({"kind": "uploadAvatar", "learner": learner}),
    )
    .await
    .expect("admitted");
    assert_eq!(
        reply,
        failed(CallFailure::DestinationRefused, Outcome::NotSent)
    );
    // Academy denies the presign: no upload, Academy's refusal shown.
    desk.adapters
        .images
        .reply(Some(Ok(("me.png".into(), photo()))));
    desk.script.lock().expect("script").trpc = vec![(
        "settings.avatarUploadUrl".into(),
        412,
        error(
            412,
            "PRECONDITION_FAILED",
            "avatar storage is not configured",
        ),
    )];
    let reply = host(
        &mut desk,
        json!({"kind": "uploadAvatar", "learner": learner}),
    )
    .await
    .expect("admitted");
    assert!(
        matches!(
            reply,
            AcademyCallResult::Error {
                http_status: 412,
                outcome: Outcome::Rejected,
                ..
            }
        ),
        "{reply:?}"
    );
    // The store refuses the bytes (4xx): nothing was stored, no confirm.
    desk.adapters
        .images
        .reply(Some(Ok(("me.png".into(), photo()))));
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc = vec![(
            "settings.avatarUploadUrl".into(),
            200,
            presigned(format!("{origin}/upload/u.jpg?sig=1")),
        )];
        script.files = vec![(
            "/upload/u.jpg".into(),
            403,
            "text/plain".into(),
            b"denied".to_vec(),
        )];
    }
    let reply = host(
        &mut desk,
        json!({"kind": "uploadAvatar", "learner": learner}),
    )
    .await
    .expect("admitted");
    assert_eq!(reply, failed(CallFailure::UploadFailed, Outcome::Rejected));
    assert_eq!(
        desk.requests("/upload/").len(),
        1,
        "only the reviewed destination"
    );
    assert!(trpc_requests(&desk, "settings.confirmAvatar").is_empty());
}

/// Academy keys an avatar by user, so an upload that reaches the store can
/// replace the photo already served before (or without) confirmation; the
/// desktop then says it may have changed, never that it is unchanged, and
/// resends nothing.
#[tokio::test(flavor = "multi_thread")]
async fn an_upload_that_may_have_replaced_the_photo_is_reported_as_possibly_applied() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let origin = origin(&desk);
    let learner = desk.learner();
    // The store takes the bytes but its reply is lost, or fails after them.
    for status in [0, 500] {
        desk.adapters
            .images
            .reply(Some(Ok(("me.png".into(), photo()))));
        {
            let mut script = desk.script.lock().expect("script");
            script.trpc = vec![(
                "settings.avatarUploadUrl".into(),
                200,
                presigned(format!(
                    "{origin}/upload/avatars/user_2fixture.jpg?sig={status}"
                )),
            )];
            script.files = vec![(
                "/upload/avatars/user_2fixture.jpg".into(),
                status,
                "text/plain".into(),
                Vec::new(),
            )];
        }
        let reply = host(
            &mut desk,
            json!({"kind": "uploadAvatar", "learner": learner}),
        )
        .await
        .expect("admitted");
        assert_eq!(
            reply,
            failed(CallFailure::UploadFailed, Outcome::Unknown),
            "store reply {status}"
        );
    }
    assert_eq!(
        desk.requests("/upload/avatars/user_2fixture.jpg").len(),
        2,
        "one PUT each, none resent"
    );
    assert!(trpc_requests(&desk, "settings.confirmAvatar").is_empty());
    // Stored, then Academy denies or fails the confirmation: the photo under
    // the existing address may already be the new one.
    for (status, code) in [(404, "NOT_FOUND"), (503, "INTERNAL_SERVER_ERROR")] {
        desk.adapters
            .images
            .reply(Some(Ok(("me.png".into(), photo()))));
        {
            let mut script = desk.script.lock().expect("script");
            script.trpc = vec![
                (
                    "settings.avatarUploadUrl".into(),
                    200,
                    presigned(format!(
                        "{origin}/upload/avatars/user_2fixture.jpg?sig=c{status}"
                    )),
                ),
                (
                    "settings.confirmAvatar".into(),
                    status,
                    error(status, code, "not confirmed"),
                ),
            ];
            script.files = vec![(
                "/upload/avatars/user_2fixture.jpg".into(),
                200,
                "text/plain".into(),
                Vec::new(),
            )];
        }
        let reply = host(
            &mut desk,
            json!({"kind": "uploadAvatar", "learner": learner}),
        )
        .await
        .expect("admitted");
        assert!(
            matches!(reply, AcademyCallResult::Error { http_status, outcome: Outcome::Unknown, .. } if http_status == status),
            "confirm {status}: {reply:?}"
        );
    }
    assert_eq!(
        trpc_requests(&desk, "settings.confirmAvatar").len(),
        2,
        "never resent"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_upload_cancelled_after_its_photo_was_sent_is_not_reported_as_unchanged() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let origin = origin(&desk);
    let learner = desk.learner();
    desk.adapters
        .images
        .reply(Some(Ok(("me.png".into(), photo()))));
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc = vec![(
            "settings.avatarUploadUrl".into(),
            200,
            presigned(format!("{origin}/upload/avatars/user_2fixture.jpg?sig=h")),
        )];
        script.files = vec![(
            "/upload/avatars/user_2fixture.jpg".into(),
            1,
            "text/plain".into(),
            Vec::new(),
        )];
    }
    let receipt = desk
        .send(json!({"kind": "uploadAvatar", "learner": learner}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!("{receipt:?}")
    };
    let Ok(Observation::AcademyCall(pending)) =
        desk.handle.observe(ViewScope::AcademyCall(operation))
    else {
        panic!("call observation")
    };
    assert!(eventually(|| desk.requests("/upload/avatars/user_2fixture.jpg").len() == 1).await);
    // The learner signs out while the photo is with the store.
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("sign out");
    let ended = tokio::time::timeout(Duration::from_secs(10), pending.result())
        .await
        .expect("ends");
    assert_eq!(ended, failed(CallFailure::Cancelled, Outcome::Unknown));
    assert!(trpc_requests(&desk, "settings.confirmAvatar").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn only_a_signed_in_current_learner_may_upload_an_avatar() {
    let mut desk = desk().await;
    let guest = desk.current_learner();
    assert_eq!(
        desk.send(json!({"kind": "uploadAvatar", "learner": guest}))
            .await
            .err(),
        Some(Refusal::CapabilityMissing)
    );
    desk.sign_in().await;
    assert_eq!(
        desk.send(json!({"kind": "uploadAvatar", "learner": guest}))
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
    assert_eq!(desk.adapters.images.opened(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_avatar_is_read_back_from_academys_origin_and_re_encoded_only() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let origin = origin(&desk);
    let learner = desk.learner();
    let served = desktop_core::avatar::samples::photo(512, 512, image::ImageFormat::Jpeg);
    desk.script.lock().expect("script").files = vec![
        (
            "/api/avatars/user_2fixture.jpg".into(),
            200,
            "image/jpeg".into(),
            served,
        ),
        (
            "/api/avatars/user_2other.jpg".into(),
            200,
            "image/jpeg".into(),
            b"\xFF\xD8\xFF\xE0 not pixels".to_vec(),
        ),
        (
            "/api/avatars/user_2third.jpg".into(),
            200,
            "text/html".into(),
            b"<script>x</script>".to_vec(),
        ),
        (
            "/api/avatars/user_2strip.png".into(),
            200,
            "image/png".into(),
            desktop_core::avatar::samples::photo(1, 8192, image::ImageFormat::Png),
        ),
        (
            "/api/avatars/user_2huge.png".into(),
            200,
            "image/png".into(),
            desktop_core::avatar::samples::over_budget_png().to_vec(),
        ),
    ];
    let reply = host(
        &mut desk,
        json!({"kind": "loadAcademyImage", "learner": learner, "url": format!("{origin}/api/avatars/user_2fixture.jpg?v=7")}),
    )
    .await
    .expect("admitted");
    let AcademyCallResult::Data { data: returned } = reply else {
        panic!("{reply:?}")
    };
    let src = serde_json::from_str::<Value>(&returned).expect("json")["json"]["src"]
        .as_str()
        .expect("src")
        .to_owned();
    assert!(
        src.starts_with("data:image/jpeg;base64,/9j/"),
        "{}",
        &src[..40]
    );
    let read = desk.requests("/api/avatars/user_2fixture.jpg");
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].header("authorization"), None, "avatars are public");
    // An extreme portrait is read back from its centre, within bounds.
    let reply = host(
        &mut desk,
        json!({"kind": "loadAcademyImage", "learner": learner, "url": format!("{origin}/api/avatars/user_2strip.png")}),
    )
    .await
    .expect("admitted");
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    // A reply that does not decode to the image it claims is not shown.
    for file in ["user_2other.jpg", "user_2third.jpg", "user_2huge.png"] {
        let reply = host(
            &mut desk,
            json!({"kind": "loadAcademyImage", "learner": learner, "url": format!("{origin}/api/avatars/{file}")}),
        )
        .await
        .expect("admitted");
        assert_eq!(
            reply,
            failed(CallFailure::InvalidResponse, Outcome::Rejected),
            "{file}"
        );
    }
    // Any other address is refused before a request.
    for url in [
        "https://cdn.example/api/avatars/user_2fixture.jpg".to_owned(),
        format!("{origin}/api/trpc/settings.get"),
        format!("{origin}/api/avatars/user_2fixture.svg"),
    ] {
        assert_eq!(
            desk.send(json!({"kind": "loadAcademyImage", "learner": learner, "url": url}))
                .await
                .err(),
            Some(Refusal::InvalidInput),
            "{url}"
        );
    }
    assert_eq!(desk.requests("/api/avatars/").len(), 5);
    assert!(trpc_requests(&desk, "settings.get").is_empty());
}
