//! C42: the Academy as the R2App board draws it, through the production
//! authority interface against a synthetic Academy (docs/planning/chunks/
//! C42.md). What the chrome draws from: the snapshot's `academy`,
//! `learner`, `academyOrigin` and `academyOffline` as a guest and after
//! signing in (a new learner generation); the Learning paths row's
//! `paths.list` is a public query the guest generation reads without any
//! credential and the account generation reads with its token. Nothing
//! here signs in or sends by itself: every call is an intent the view would
//! send.

include!("../academy_identity/harness.rs");

fn paths_reply() -> Value {
    json!({"result": {"data": {"json": [{
        "id": "path-runtime", "slug": "runtime-path", "title": "Runtime Security Path",
        "description": "Protect prompts and responses at runtime.", "familyKey": "prisma-airs",
        "iconKey": null, "tone": "info", "courseCount": 1, "estMinutes": 30,
        "unitCount": 4, "unitsDone": 0, "locked": false, "lockReason": null}]}}})
}

fn snapshot_json(desk: &Desk) -> Value {
    serde_json::to_value(desk.snap()).expect("json")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_chrome_draws_from_the_snapshot_as_a_guest_then_as_the_account() {
    let mut desk = desk().await;
    let origin = desk.script.lock().expect("script").origin.clone();
    // A guest: signed out, the guest generation, Academy's origin, offline work signed out and reachable.
    let s = snapshot_json(&desk);
    assert_eq!(s["academy"]["state"], "signedOut", "{s}");
    let guest = s["learner"].as_str().expect("guest learner").to_owned();
    assert_eq!(s["academyOrigin"], origin);
    assert_eq!(s["academyOffline"]["state"], "signedOut");
    assert_eq!(s["academyOffline"]["reachable"], true);
    assert_eq!(s["academyOffline"]["downloads"], json!([]));
    // The Learning paths row: a public query, sent without any credential.
    desk.script.lock().expect("script").trpc = vec![("paths.list".into(), 200, paths_reply())];
    let reply = desk
        .call_as(&guest, "paths.list", "query", None)
        .await
        .expect("admitted");
    assert_eq!(data_of(reply)["json"][0]["title"], "Runtime Security Path");
    let seen = &trpc(&desk, "paths.list")[0];
    assert_eq!(seen.header("authorization"), None);
    assert_eq!(seen.request_line.split(' ').next(), Some("GET"));
    // Signed in from Settings: the account's name, a new generation, offline work for the account.
    desk.sign_in().await;
    let s = snapshot_json(&desk);
    assert_eq!(s["academy"]["state"], "signedIn", "{s}");
    assert_eq!(s["academy"]["displayName"], "Ada Lovelace");
    let account = s["learner"].as_str().expect("account learner").to_owned();
    assert_ne!(account, guest, "a new generation");
    assert_eq!(s["academy"]["learner"], account);
    assert_ne!(s["academyOffline"]["state"], "signedOut");
    // The row's query as the account carries its token.
    desk.script.lock().expect("script").trpc = vec![("paths.list".into(), 200, paths_reply())];
    desk.call("paths.list", "query", None)
        .await
        .expect("admitted");
    let calls = trpc(&desk, "paths.list");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].header("authorization"), Some("Bearer access-1"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_guest_generation_cannot_read_account_pages_the_frame_offers_only_when_signed_in() {
    let mut desk = desk().await;
    let guest = desk.current_learner();
    // The frame offers Home, Program, Downloads, Notifications and Settings only when signed in:
    // the guest generation is refused their reads before anything is sent.
    for procedure in ["progress.get", "notifications.list", "settings.get"] {
        let refused = desk.call_as(&guest, procedure, "query", None).await.err();
        assert_eq!(refused, Some(Refusal::CapabilityMissing), "{procedure}");
    }
    assert!(
        desk.server
            .seen()
            .iter()
            .all(|s| !s.request_line.contains("/api/trpc/progress"))
    );
}

fn data_of(result: AcademyCallResult) -> Value {
    let AcademyCallResult::Data { data } = result else {
        panic!("{result:?}")
    };
    serde_json::from_str(&data).expect("json")
}

fn trpc(desk: &Desk, procedure: &str) -> Vec<Seen> {
    desk.requests(&format!("/api/trpc/{procedure}"))
}
