//! C05 learner transport for Academy's hosted learner pages, through the
//! production authority interface (dispatch/observe) and the app-owned HTTP
//! client against a synthetic Academy on this Mac: the guest generation and
//! public catalog, which calls carry the account credential, account
//! generations, literal lesson and knowledge-check fixtures shared with the
//! TypeScript link suite, and truth screening.

include!("../academy_identity/harness.rs");

fn shared(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/desktop-contracts/fixtures/academy")
        .join(name);
    std::fs::read_to_string(path)
        .expect("fixture")
        .trim()
        .to_owned()
}

fn envelope(name: &str) -> Value {
    serde_json::from_str(&shared(name)).expect("json")
}

fn data_of(result: AcademyCallResult) -> Value {
    let AcademyCallResult::Data { data } = result else {
        panic!("{result:?}")
    };
    serde_json::from_str(&data).expect("json")
}

/// Every request the synthetic server saw for a procedure.
fn trpc(desk: &Desk, procedure: &str) -> Vec<Seen> {
    desk.requests(&format!("/api/trpc/{procedure}"))
}

fn query_input(seen: &Seen) -> Option<String> {
    let target = seen.request_line.split(' ').nth(1)?;
    let (_, query) = target.split_once('?')?;
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(k, _)| k == "input")
        .map(|(_, v)| v.into_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_reads_the_public_catalog_without_any_credential() {
    let mut desk = desk().await;
    assert!(matches!(desk.academy(), AcademyView::SignedOut));
    let guest = desk.current_learner();
    let courses = envelope("courses-list.result.json");
    desk.script.lock().expect("script").trpc = vec![
        ("courses.list".into(), 200, courses.clone()),
        ("units.get".into(), 200, envelope("units-get.result.json")),
    ];
    let reply = desk
        .call_as(&guest, "courses.list", "query", None)
        .await
        .expect("admitted");
    assert_eq!(data_of(reply), courses["result"]["data"]);
    let input = shared("units-get.input.json");
    let reply = desk
        .call_as(&guest, "units.get", "query", Some(&input))
        .await
        .expect("admitted");
    assert_eq!(
        data_of(reply),
        envelope("units-get.result.json")["result"]["data"]
    );

    let list = &trpc(&desk, "courses.list")[0];
    assert_eq!(list.request_line.split(' ').next(), Some("GET"));
    assert_eq!(query_input(list), None, "no input is sent for none");
    assert_eq!(query_input(&trpc(&desk, "units.get")[0]), Some(input));
    // No credential, no identity-provider traffic, no sign-in discovery.
    for seen in desk.server.seen() {
        assert!(
            seen.header("authorization").is_none(),
            "{}",
            seen.request_line
        );
        assert!(
            seen.request_line.contains("/api/trpc/"),
            "{}",
            seen.request_line
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_is_refused_everything_outside_the_public_queries_before_sending() {
    let mut desk = desk().await;
    let guest = desk.current_learner();
    for (procedure, kind) in [
        ("progress.get", "query"),
        ("units.complete", "mutation"),
        ("units.startKnowledgeCheck", "mutation"),
        ("units.submitKnowledgeCheck", "mutation"),
        ("desktop.me", "query"),
        ("settings.get", "query"),
        ("courses.list", "mutation"),
        ("modules.intro", "mutation"),
        ("admin.users.list", "query"),
        ("labs.submitLab", "mutation"),
        ("telemetry.track", "mutation"),
    ] {
        assert_eq!(
            desk.call_as(&guest, procedure, kind, None).await.err(),
            Some(Refusal::CapabilityMissing),
            "{procedure} as {kind}"
        );
    }
    assert_eq!(
        desk.call_as(&guest, "courses.list", "query", Some("{not json"))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    assert!(desk.server.seen().is_empty(), "refusals send nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signed_in_learner_sends_the_credential_only_on_the_account_allowlist() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let ok = json!({"result": {"data": {"json": []}}});
    desk.script.lock().expect("script").trpc = vec![
        ("courses.list".into(), 200, ok.clone()),
        ("modules.intro".into(), 200, ok.clone()),
        ("progress.get".into(), 200, ok.clone()),
    ];
    for (procedure, kind) in [
        ("courses.list", "query"),
        ("modules.intro", "query"),
        ("progress.get", "query"),
    ] {
        desk.call(procedure, kind, None).await.expect("admitted");
    }
    // Allowlisted procedures carry the account token; a public procedure
    // outside the account allowlist is sent without any credential.
    assert_eq!(
        trpc(&desk, "courses.list")[0].header("authorization"),
        Some("Bearer access-1")
    );
    assert_eq!(
        trpc(&desk, "progress.get")[0].header("authorization"),
        Some("Bearer access-1")
    );
    assert_eq!(
        trpc(&desk, "modules.intro")[0].header("authorization"),
        None
    );
    // Signed in, a public procedure is still query-only.
    let learner = desk.learner();
    assert_eq!(
        desk.call_as(&learner, "modules.intro", "mutation", None)
            .await
            .err(),
        Some(Refusal::CapabilityMissing)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_guest_call_is_never_renewed_or_resent() {
    let mut desk = desk().await;
    let guest = desk.current_learner();
    let unauthorized = json!({"error": {"json": {"message": "UNAUTHORIZED", "code": -32001,
        "data": {"code": "UNAUTHORIZED", "httpStatus": 401, "path": "paths.list"}}}});
    desk.script.lock().expect("script").trpc =
        vec![("paths.list".into(), 401, unauthorized.clone())];
    let reply = desk
        .call_as(&guest, "paths.list", "query", None)
        .await
        .expect("admitted");
    let AcademyCallResult::Error {
        error, http_status, ..
    } = reply
    else {
        panic!("{reply:?}")
    };
    assert_eq!(http_status, 401);
    assert_eq!(
        serde_json::from_str::<Value>(&error).expect("json"),
        unauthorized["error"]
    );
    assert_eq!(trpc(&desk, "paths.list").len(), 1, "sent once");
    assert!(
        desk.requests("/oauth/").is_empty(),
        "no renewal for a guest"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_account_change_issues_a_new_generation_and_retires_the_old_one() {
    let mut script = Script::new();
    script.polls = vec![
        (400, json!({"error": "authorization_pending"})),
        (200, bearer("access-1", "refresh-1", 3600)),
    ];
    let mut desk = desk_with(script, None).await;
    let first_guest = desk.current_learner();
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("start");
    assert!(eventually(|| matches!(desk.academy(), AcademyView::AwaitingApproval { .. })).await);
    // The guest generation current while approval is pending.
    let guest = desk.current_learner();
    assert!(
        eventually(|| matches!(desk.academy(), AcademyView::SignedIn { .. })).await,
        "{:?}",
        desk.academy()
    );
    let account = desk.learner();
    assert_eq!(desk.current_learner(), account);
    assert!(account != guest && account != first_guest);
    for stale in [&first_guest, &guest] {
        assert_eq!(
            desk.call_as(stale, "courses.list", "query", None)
                .await
                .err(),
            Some(Refusal::StaleContext),
            "a guest generation ends at sign-in: {stale}"
        );
    }
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("sign out");
    assert!(
        eventually(|| matches!(desk.academy(), AcademyView::SignedOut)).await,
        "{:?}",
        desk.academy()
    );
    let next_guest = desk.current_learner();
    assert!(next_guest != guest && next_guest != account);
    for stale in [&guest, &account] {
        assert_eq!(
            desk.call_as(stale, "courses.list", "query", None)
                .await
                .err(),
            Some(Refusal::StaleContext),
            "{stale}"
        );
    }
    // (Sign-out revokes the grant in the background; count catalog calls.)
    let before = trpc(&desk, "courses.list").len();
    desk.script.lock().expect("script").trpc = vec![(
        "courses.list".into(),
        200,
        envelope("courses-list.result.json"),
    )];
    desk.call_as(&next_guest, "courses.list", "query", None)
        .await
        .expect("the new guest reads the catalog");
    let after = trpc(&desk, "courses.list");
    assert_eq!(after.len(), before + 1);
    assert!(
        after
            .last()
            .expect("seen")
            .header("authorization")
            .is_none()
    );
}

/// The same literal fixtures drive the TypeScript link suite
/// (apps/desktop/src/features/academy/trpc-link.test.ts).
#[tokio::test(flavor = "multi_thread")]
async fn a_knowledge_check_passes_literal_inputs_and_grades_through_unchanged() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let start = envelope("units-startKnowledgeCheck.result.json");
    let grade = envelope("units-submitKnowledgeCheck.result.json");
    desk.script.lock().expect("script").trpc = vec![
        ("units.startKnowledgeCheck".into(), 200, start.clone()),
        ("units.submitKnowledgeCheck".into(), 200, grade.clone()),
    ];
    let start_input = shared("units-startKnowledgeCheck.input.json");
    let submit_input = shared("units-submitKnowledgeCheck.input.json");
    let reply = desk
        .call("units.startKnowledgeCheck", "mutation", Some(&start_input))
        .await
        .expect("admitted");
    assert_eq!(data_of(reply), start["result"]["data"]);
    let reply = desk
        .call(
            "units.submitKnowledgeCheck",
            "mutation",
            Some(&submit_input),
        )
        .await
        .expect("admitted");
    assert_eq!(
        data_of(reply),
        grade["result"]["data"],
        "SuperJSON meta kept"
    );
    for (procedure, body) in [
        ("units.startKnowledgeCheck", &start_input),
        ("units.submitKnowledgeCheck", &submit_input),
    ] {
        let seen = trpc(&desk, procedure);
        assert_eq!(seen.len(), 1, "{procedure} sent once");
        assert_eq!(seen[0].request_line.split(' ').next(), Some("POST"));
        assert_eq!(seen[0].body, body.as_bytes(), "{procedure} byte for byte");
        assert_eq!(seen[0].header("authorization"), Some("Bearer access-1"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reply_carrying_assessment_truth_never_reaches_the_renderer() {
    let mut desk = desk().await;
    let guest = desk.current_learner();
    let canary = envelope("units-get.truth-canary.result.json");
    assert!(canary.to_string().contains("AIRS-C05-TRUTH-CANARY"));
    desk.script.lock().expect("script").trpc = vec![("units.get".into(), 200, canary.clone())];
    let reply = desk
        .call_as(
            &guest,
            "units.get",
            "query",
            Some(r#"{"json":{"unitId":"x"}}"#),
        )
        .await
        .expect("admitted");
    assert_eq!(
        reply,
        AcademyCallResult::Failed {
            failure: desktop_core::academy::CallFailure::InvalidResponse,
            outcome: Outcome::Unknown,
        }
    );
    desk.sign_in().await;
    desk.script.lock().expect("script").trpc = vec![("units.get".into(), 200, canary)];
    let reply = desk
        .call("units.get", "query", Some(r#"{"json":{"unitId":"x"}}"#))
        .await
        .expect("admitted");
    assert!(
        matches!(
            reply,
            AcademyCallResult::Failed {
                failure: desktop_core::academy::CallFailure::InvalidResponse,
                ..
            }
        ),
        "{reply:?}"
    );
    assert!(!format!("{reply:?}").contains("CANARY"));
    assert!(!format!("{:?}", desk.snap()).contains("CANARY"));
}
