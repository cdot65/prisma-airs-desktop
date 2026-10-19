//! C07 Academy offline work through the production authority interface:
//! downloads kept sealed per account under Academy's lease and revision,
//! offline reads only while Academy is unreachable and only from a readable
//! download or cached attempt, lab work kept on this Mac and never sent by
//! itself, revalidation on reconnect, locking on sign-out, isolation between
//! accounts, Keychain denial and corruption without plaintext or loss, crash
//! and restart safety, no action replayed after reconnect, and reset.

include!("../academy_identity/harness.rs");

use desktop_core::academy::CallFailure;
use desktop_core::{DownloadStatus, OfflineState};

const COURSE: &str = "runtime-security";
const UNIT: &str = "unit-scan-1";
const MARKER: &str = "SYNTHETIC-OFFLINE-LESSON-BODY";

fn data(value: Value) -> Value {
    json!({"result": {"data": {"json": value}}})
}

fn error(status: u16, code: &str) -> Value {
    json!({"error": {"json": {"message": code, "code": -32000,
        "data": {"code": code, "httpStatus": status}}}})
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

fn unit() -> Value {
    json!({"id": UNIT, "moduleId": "m1", "order": 1, "kind": "content", "title": "Scanning",
           "bodyHtml": format!("<p>{MARKER}</p>"), "status": "not_started", "siblings": [],
           "pathContext": null})
}

fn bundle(account: &str, revision: &str, lease_secs: i64) -> Value {
    json!({"schemaVersion": 1, "courseId": COURSE, "title": "Runtime Security Foundations",
           "description": "d", "revision": revision, "hash": revision, "accountId": account,
           "modules": [{"moduleId": "m1", "order": 1}], "units": [unit()], "labs": [],
           "labModes": {}, "manifest": false,
           "lease": {"issuedAt": in_secs(0), "expiresAt": in_secs(lease_secs), "access": "restricted"}})
}

const ATTEMPT: &str = "00000000-0000-4000-8000-0000000000a1";
const AUTHORED: &str = "SYNTHETIC-AUTHORED-STEP";

/// Academy's `desktop.attempt.get` for an unlocked attempt: its lease and
/// the authored definition (here, a marked step) with the learner's work.
fn lab_attempt(lease_secs: i64, revision: &str) -> Value {
    json!({"attemptId": ATTEMPT, "accountId": "user_2fixture", "labId": "lab-1", "courseId": COURSE,
           "courseTitle": "Runtime Security Foundations", "revision": revision, "status": "in_progress",
           "draftVersion": 2, "draft": {"debrief": "LEARNER-WORK"}, "result": {}, "conflicts": [],
           "locked": false, "lockReason": null, "createdAt": in_secs(-60), "mode": "triage",
           "steps": [{"id": "s1", "prompt": AUTHORED}],
           "lease": {"access": "restricted", "issuedAt": in_secs(0), "expiresAt": in_secs(lease_secs)}})
}

/// Academy's reply for an attempt it locked: no definition, no lease.
fn locked_lab_attempt(reason: &str) -> Value {
    json!({"attemptId": ATTEMPT, "accountId": "user_2fixture", "labId": "lab-1", "courseId": COURSE,
           "courseTitle": "Runtime Security Foundations", "revision": "rev-1", "status": "in_progress",
           "draftVersion": 2, "draft": {"debrief": "LEARNER-WORK"}, "result": {}, "conflicts": [],
           "locked": true, "lockReason": reason, "createdAt": in_secs(-60)})
}

/// Academy's `desktop.unitBundle`: the whole bundle projection, for one
/// unit (with `body_bytes` of lesson text when non-zero).
fn unit_part(id: &str, revision: &str, body_bytes: usize) -> Value {
    let mut unit = unit();
    unit["id"] = json!(id);
    if body_bytes > 0 {
        unit["bodyHtml"] = json!("x".repeat(body_bytes));
    }
    let mut part = bundle("user_2fixture", revision, 600);
    part["units"] = json!([unit]);
    part["labs"] = json!([]);
    part
}

fn access(account: &str, allowed: bool, lease_secs: i64) -> Value {
    json!({"serverTime": in_secs(0), "accountId": account, "courses": [
        if allowed {
            json!({"courseId": COURSE, "allowed": true, "access": "restricted",
                   "issuedAt": in_secs(0), "expiresAt": in_secs(lease_secs)})
        } else {
            json!({"courseId": COURSE, "allowed": false, "reason": "FORBIDDEN",
                   "issuedAt": in_secs(0), "expiresAt": in_secs(0)})
        }
    ]})
}

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

async fn download(desk: &mut Desk, lease_secs: i64) -> AcademyCallResult {
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, lease_secs)),
        ));
        script.trpc.push((
            "desktop.bundle".into(),
            200,
            data(bundle("user_2fixture", "rev-1", lease_secs)),
        ));
    }
    let learner = desk.learner();
    host(
        desk,
        json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted")
}

async fn read_unit(desk: &mut Desk) -> AcademyCallResult {
    let input = format!(r#"{{"json":{{"unitId":"{UNIT}"}}}}"#);
    desk.call("units.get", "query", Some(&input))
        .await
        .expect("admitted")
}

fn offline_files(desk: &Desk) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![desk.adapters.offline_root.clone()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(path);
                }
            }
        }
    }
    out
}

fn status(desk: &Desk) -> Option<DownloadStatus> {
    desk.snap()
        .academy_offline
        .downloads
        .iter()
        .find(|d| d.course_id == COURSE)
        .map(|d| d.status)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_downloaded_course_is_kept_sealed_for_its_account_under_academys_lease() {
    let mut desk = desk().await;
    desk.sign_in().await;
    // Offline work opens after sign-in settles; wait rather than race it
    // (the assertion flaked once under a loaded parallel run).
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Empty).await);
    assert!(
        desk.adapters.learner_vault.records().len() == 1,
        "no offline key before any download"
    );
    let reply = download(&mut desk, 7 * 86_400).await;
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    let offline = desk.snap().academy_offline;
    assert_eq!(offline.state, OfflineState::Ready);
    assert_eq!(offline.downloads.len(), 1);
    assert_eq!(offline.downloads[0].status, DownloadStatus::Current);
    assert_eq!(offline.downloads[0].units, 1);
    assert_eq!(offline.downloads[0].first_unit.as_deref(), Some(UNIT));
    assert_eq!(
        desk.adapters.learner_vault.records().len(),
        2,
        "the account's offline key"
    );
    // The access check and bundle were account requests.
    for path in ["/api/trpc/desktop.access", "/api/trpc/desktop.bundle"] {
        let seen = desk.requests(path);
        assert_eq!(seen.len(), 1, "{path}");
        assert_eq!(seen[0].header("authorization"), Some("Bearer access-1"));
    }
    // Nothing on disk is readable: no lesson text, course id or account id.
    let files = offline_files(&desk);
    assert!(
        files
            .iter()
            .any(|f| f.extension().is_some_and(|x| x == "bin"))
    );
    for file in &files {
        let text = String::from_utf8_lossy(&std::fs::read(file).expect("read")).to_string();
        assert!(
            !text.contains(MARKER) && !text.contains(COURSE) && !text.contains("user_2fixture")
        );
        assert!(!file.to_string_lossy().contains("user_2fixture"));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_course_academy_does_not_give_the_account_is_not_downloaded() {
    let mut desk = desk().await;
    desk.sign_in().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.access".into(),
        200,
        data(access("user_2fixture", false, 0)),
    ));
    let learner = desk.learner();
    let reply = host(
        &mut desk,
        json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert_eq!(
        reply,
        AcademyCallResult::Failed {
            failure: CallFailure::NotDownloadable,
            outcome: Outcome::NotSent
        }
    );
    assert!(desk.requests("/api/trpc/desktop.bundle").is_empty());
    assert!(desk.snap().academy_offline.downloads.is_empty());
    // A bundle for another account is refused too.
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 600)),
        ));
        script.trpc.push((
            "desktop.bundle".into(),
            200,
            data(bundle("user_2someone", "rev-1", 600)),
        ));
    }
    let reply = host(
        &mut desk,
        json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert_eq!(
        reply,
        AcademyCallResult::Failed {
            failure: CallFailure::InvalidResponse,
            outcome: Outcome::NotSent
        }
    );
    assert!(desk.snap().academy_offline.downloads.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_course_past_academys_bundle_cap_is_assembled_unit_by_unit() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let mut manifest = bundle("user_2fixture", "rev-1", 600);
    manifest["manifest"] = json!(true);
    manifest["units"] = json!([{"id": UNIT}, {"id": "unit-scan-2"}]);
    let part = |id: &str, revision: &str| {
        let mut part = unit_part(id, revision, 0);
        part["labs"] = json!([{"id": "lab-shared", "title": "Shared lab"}]);
        data(part)
    };
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 600)),
        ));
        script.trpc.push((
            "desktop.bundle".into(),
            413,
            error(413, "PAYLOAD_TOO_LARGE"),
        ));
        script
            .trpc
            .push(("desktop.manifest".into(), 200, data(manifest.clone())));
        script
            .trpc
            .push(("desktop.unitBundle".into(), 200, part(UNIT, "rev-1")));
        script.trpc.push((
            "desktop.unitBundle".into(),
            200,
            part("unit-scan-2", "rev-1"),
        ));
    }
    let learner = desk.learner();
    let reply = host(
        &mut desk,
        json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    let offline = desk.snap().academy_offline;
    assert_eq!(offline.downloads.len(), 1);
    assert_eq!(offline.downloads[0].units, 2, "both units assembled");
    assert_eq!(desk.requests("/api/trpc/desktop.unitBundle").len(), 2);

    // A unit from another revision makes the whole course unreadable: nothing
    // half-assembled is kept.
    let reply = host(
        &mut desk,
        json!({"kind": "removeDownload", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 600)),
        ));
        script.trpc.push((
            "desktop.bundle".into(),
            413,
            error(413, "PAYLOAD_TOO_LARGE"),
        ));
        script
            .trpc
            .push(("desktop.manifest".into(), 200, data(manifest)));
        script
            .trpc
            .push(("desktop.unitBundle".into(), 200, part(UNIT, "rev-1")));
        script.trpc.push((
            "desktop.unitBundle".into(),
            200,
            part("unit-scan-2", "rev-2"),
        ));
    }
    let reply = host(
        &mut desk,
        json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert!(
        matches!(
            reply,
            AcademyCallResult::Failed {
                failure: CallFailure::InvalidResponse,
                ..
            }
        ),
        "{reply:?}"
    );
    assert!(desk.snap().academy_offline.downloads.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn offline_reads_come_from_the_download_only_while_academy_is_unreachable() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    // Online, Academy answers (here: its NOT_FOUND), not the download.
    let online = read_unit(&mut desk).await;
    assert!(
        matches!(
            online,
            AcademyCallResult::Error {
                http_status: 404,
                ..
            }
        ),
        "{online:?}"
    );
    desk.script.lock().expect("script").unreachable = true;
    let offline = read_unit(&mut desk).await;
    let AcademyCallResult::Data { data: answer } = offline else {
        panic!("{offline:?}")
    };
    assert!(answer.contains(MARKER), "the downloaded unit");
    assert!(!desk.snap().academy_offline.reachable);
    // Nothing else is invented offline.
    let other = desk
        .call(
            "units.get",
            "query",
            Some(r#"{"json":{"unitId":"not-downloaded"}}"#),
        )
        .await
        .expect("admitted");
    assert!(
        matches!(
            other,
            AcademyCallResult::Failed {
                failure: CallFailure::Network,
                ..
            }
        ),
        "{other:?}"
    );
    let progress = desk
        .call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(
        matches!(progress, AcademyCallResult::Failed { .. }),
        "{progress:?}"
    );
    // A mutation offline fails and is never replayed on reconnect.
    let complete = desk
        .call(
            "units.complete",
            "mutation",
            Some(&format!(r#"{{"json":{{"unitId":"{UNIT}"}}}}"#)),
        )
        .await
        .expect("admitted");
    assert!(
        matches!(complete, AcademyCallResult::Failed { .. }),
        "{complete:?}"
    );
    let before = desk.requests("/api/trpc/units.complete").len();
    {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 7 * 86_400)),
        ));
        script.trpc.push((
            "desktop.manifest".into(),
            200,
            data(bundle("user_2fixture", "rev-1", 0)),
        ));
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({"done": 0}))));
    }
    let back = desk
        .call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(matches!(back, AcademyCallResult::Data { .. }), "{back:?}");
    assert!(eventually(|| desk.snap().academy_offline.reachable).await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        desk.requests("/api/trpc/units.complete").len(),
        before,
        "nothing replayed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_expired_lease_is_not_read_offline() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 2).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(status(&desk), Some(DownloadStatus::Expired));
    desk.script.lock().expect("script").unreachable = true;
    let offline = read_unit(&mut desk).await;
    assert!(
        matches!(
            offline,
            AcademyCallResult::Failed {
                failure: CallFailure::Network,
                ..
            }
        ),
        "{offline:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reconnecting_withdraws_courses_academy_no_longer_gives_and_marks_new_revisions() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    // Offline, then back: Academy says a newer revision exists.
    desk.script.lock().expect("script").unreachable = true;
    read_unit(&mut desk).await;
    {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 7 * 86_400)),
        ));
        script.trpc.push((
            "desktop.manifest".into(),
            200,
            data(bundle("user_2fixture", "rev-2", 0)),
        ));
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({}))));
    }
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(
        eventually(|| status(&desk) == Some(DownloadStatus::Outdated)).await,
        "{:?}",
        status(&desk)
    );
    desk.script.lock().expect("script").unreachable = true;
    assert!(
        matches!(read_unit(&mut desk).await, AcademyCallResult::Data { .. }),
        "outdated stays readable"
    );
    // Offline, then back: Academy no longer gives this account the course.
    {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", false, 0)),
        ));
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({}))));
    }
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(
        eventually(|| status(&desk) == Some(DownloadStatus::Withdrawn)).await,
        "{:?}",
        status(&desk)
    );
    desk.script.lock().expect("script").unreachable = true;
    assert!(
        matches!(read_unit(&mut desk).await, AcademyCallResult::Failed { .. }),
        "withdrawn is not served"
    );
    // Revalidation only reads: no Academy write was sent.
    assert!(
        desk.requests("/api/trpc/")
            .iter()
            .all(|s| s.request_line.starts_with("GET ") || s.request_line.contains("desktop.me"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn sign_out_locks_offline_work_and_another_account_cannot_see_it() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("out");
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::SignedOut).await);
    assert!(desk.snap().academy_offline.downloads.is_empty());
    let files_after_sign_out = offline_files(&desk).len();
    assert!(files_after_sign_out > 0, "kept on disk, sealed");
    // A guest offline gets nothing from the download.
    desk.script.lock().expect("script").unreachable = true;
    let guest_ref = desk.current_learner();
    let guest = desk
        .call_as(
            &guest_ref,
            "units.get",
            "query",
            Some(&format!(r#"{{"json":{{"unitId":"{UNIT}"}}}}"#)),
        )
        .await
        .expect("admitted");
    assert!(
        matches!(guest, AcademyCallResult::Failed { .. }),
        "{guest:?}"
    );
    desk.script.lock().expect("script").unreachable = false;
    // Another account signs in: its own empty offline work.
    desk.script.lock().expect("script").trpc.push((
        "desktop.me".into(),
        200,
        data(
            json!({"accountId": "user_2second", "displayName": "Grace Hopper",
                    "serverTime": "2026-10-17T00:00:00.000Z"}),
        ),
    ));
    desk.sign_in().await;
    assert!(eventually(|| desk.snap().academy_offline.state != OfflineState::Opening).await);
    assert!(desk.snap().academy_offline.downloads.is_empty());
    desk.script.lock().expect("script").unreachable = true;
    assert!(matches!(
        read_unit(&mut desk).await,
        AcademyCallResult::Failed { .. }
    ));
    desk.script.lock().expect("script").unreachable = false;
    // The first account signs in again: its download is back.
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("out");
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::SignedOut).await);
    desk.sign_in().await;
    assert!(
        eventually(|| desk.snap().academy_offline.downloads.len() == 1).await,
        "{:?}",
        desk.snap().academy_offline
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_denied_keychain_keeps_offline_work_closed_and_writes_nothing() {
    let mut desk = desk().await;
    desk.sign_in().await;
    desk.adapters
        .learner_vault
        .fail_with(Some(VaultError::Unavailable));
    let reply = download(&mut desk, 600).await;
    assert_eq!(
        reply,
        AcademyCallResult::Failed {
            failure: CallFailure::OfflineUnavailable,
            outcome: Outcome::NotSent
        }
    );
    assert!(
        desk.requests("/api/trpc/desktop.bundle").is_empty(),
        "nothing fetched to keep"
    );
    assert!(
        offline_files(&desk)
            .iter()
            .all(|f| !f.extension().is_some_and(|x| x == "bin")),
        "no record written"
    );
    let learner = desk.learner();
    let draft = host(
        &mut desk,
        json!({"kind": "saveLocalDraft", "learner": learner,
        "attemptId": "att-1", "baseVersion": 0, "draft": "{\"debrief\":\"x\"}"}),
    )
    .await
    .expect("admitted");
    assert_eq!(
        draft,
        AcademyCallResult::Failed {
            failure: CallFailure::OfflineUnavailable,
            outcome: Outcome::NotSent
        }
    );
    assert_eq!(desk.snap().academy_offline.state, OfflineState::Unavailable);
}

#[tokio::test(flavor = "multi_thread")]
async fn lab_work_kept_on_this_mac_survives_restart_and_is_never_sent_by_itself() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let learner = desk.learner();
    let draft = r#"{"commands":[],"debrief":"SYNTHETIC-LOCAL-WORK"}"#;
    let saved = host(
        &mut desk,
        json!({"kind": "saveLocalDraft", "learner": learner,
        "attemptId": "att-1", "baseVersion": 3, "draft": draft}),
    )
    .await
    .expect("admitted");
    assert!(matches!(saved, AcademyCallResult::Data { .. }), "{saved:?}");
    assert_eq!(desk.snap().academy_offline.local_drafts.len(), 1);
    assert!(
        desk.requests("/api/trpc/desktop.draft.save").is_empty(),
        "kept here only"
    );
    for file in offline_files(&desk) {
        let text = String::from_utf8_lossy(&std::fs::read(file).expect("read")).to_string();
        assert!(!text.contains("SYNTHETIC-LOCAL-WORK"));
    }
    // Restart: the work is still there and readable for this account.
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(eventually(|| matches!(desk.academy(), AcademyView::SignedIn { .. })).await);
    assert!(
        eventually(|| desk.snap().academy_offline.local_drafts.len() == 1).await,
        "{:?}",
        desk.snap().academy_offline
    );
    let learner = desk.learner();
    let loaded = host(
        &mut desk,
        json!({"kind": "loadLocalDraft", "learner": learner, "attemptId": "att-1"}),
    )
    .await
    .expect("admitted");
    let AcademyCallResult::Data { data: answer } = loaded else {
        panic!("{loaded:?}")
    };
    let value: Value = serde_json::from_str(&answer).expect("json");
    assert_eq!(value["json"]["baseVersion"], 3);
    assert_eq!(value["json"]["draft"]["debrief"], "SYNTHETIC-LOCAL-WORK");
    // Academy is reachable and nothing was sent for it. With no kept
    // attempt holding this work, it is its only offline copy: a clear is
    // refused and the work stays.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(desk.requests("/api/trpc/desktop.draft.save").is_empty());
    assert!(desk.requests("/api/trpc/desktop.attempt.submit").is_empty());
    let cleared = host(
        &mut desk,
        json!({"kind": "clearLocalDraft", "learner": learner, "attemptId": "att-1"}),
    )
    .await
    .expect("admitted");
    assert!(
        matches!(cleared, AcademyCallResult::Failed { .. }),
        "{cleared:?}"
    );
    assert_eq!(desk.snap().academy_offline.local_drafts.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn cached_attempts_open_offline_and_a_submission_is_never_replayed() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let attempt = lab_attempt(600, "rev-1");
    {
        let mut script = desk.script.lock().expect("script");
        script
            .trpc
            .push(("desktop.attempt.get".into(), 200, data(attempt.clone())));
    }
    // A first download opens the account's offline work for caching.
    download(&mut desk, 600).await;
    let input = r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000a1"}}"#;
    let online = desk
        .call("desktop.attempt.get", "query", Some(input))
        .await
        .expect("admitted");
    assert!(
        matches!(online, AcademyCallResult::Data { .. }),
        "{online:?}"
    );
    desk.script.lock().expect("script").unreachable = true;
    let offline = desk
        .call("desktop.attempt.get", "query", Some(input))
        .await
        .expect("admitted");
    let AcademyCallResult::Data { data: answer } = offline else {
        panic!("{offline:?}")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&answer).expect("json")["json"],
        attempt
    );
    // Submitting offline fails; reconnecting does not submit it.
    let submit = r#"{"json":{"attemptId":"00000000-0000-4000-8000-0000000000a1","draftVersion":2,"submissionId":"00000000-0000-4000-8000-0000000000b3"}}"#;
    let failed = desk
        .call("desktop.attempt.submit", "mutation", Some(submit))
        .await
        .expect("admitted");
    assert!(
        matches!(failed, AcademyCallResult::Failed { .. }),
        "{failed:?}"
    );
    let attempted = desk.requests("/api/trpc/desktop.attempt.submit").len();
    {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({}))));
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 600)),
        ));
        script.trpc.push((
            "desktop.manifest".into(),
            200,
            data(bundle("user_2fixture", "rev-1", 0)),
        ));
        script.trpc.push((
            "desktop.attempt.get".into(),
            200,
            data(lab_attempt(600, "rev-1")),
        ));
        script.trpc.push((
            "desktop.attempt.submit".into(),
            200,
            data(json!({"passed": false})),
        ));
    }
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        desk.requests("/api/trpc/desktop.attempt.submit").len(),
        attempted,
        "not replayed"
    );
    // The learner submits explicitly: sent once with the same id.
    let explicit = desk
        .call("desktop.attempt.submit", "mutation", Some(submit))
        .await
        .expect("admitted");
    assert!(
        matches!(explicit, AcademyCallResult::Data { .. }),
        "{explicit:?}"
    );
    let sent = desk.requests("/api/trpc/desktop.attempt.submit");
    assert_eq!(sent.len(), attempted + 1);
    assert_eq!(sent.last().expect("sent").body, submit.as_bytes());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_record_is_kept_aside_and_the_rest_still_opens() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    let learner = desk.learner();
    host(
        &mut desk,
        json!({"kind": "saveLocalDraft", "learner": learner,
        "attemptId": "att-1", "baseVersion": 0, "draft": "{\"debrief\":\"kept\"}"}),
    )
    .await
    .expect("admitted");
    // Corrupt the course record on disk (one flipped byte).
    let course_file = offline_files(&desk)
        .into_iter()
        .find(|f| {
            f.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("course-"))
        })
        .expect("course file");
    let mut bytes = std::fs::read(&course_file).expect("read");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&course_file, bytes).expect("write");
    // Restart: the course is not shown or served, the draft is, and the
    // corrupt file is kept aside rather than deleted.
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    let offline = desk.snap().academy_offline;
    assert!(offline.downloads.is_empty());
    assert_eq!(offline.local_drafts.len(), 1);
    assert!(
        offline_files(&desk)
            .iter()
            .any(|f| f.to_string_lossy().contains(".unreadable-"))
    );
    desk.script.lock().expect("script").unreachable = true;
    assert!(matches!(
        read_unit(&mut desk).await,
        AcademyCallResult::Failed { .. }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_removes_offline_work_and_its_keys() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    assert!(!offline_files(&desk).is_empty());
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(
        eventually(|| !desk.adapters.offline_root.exists()).await,
        "offline root removed"
    );
    assert!(eventually(|| desk.adapters.learner_vault.records().is_empty()).await);
    assert_eq!(desk.snap().academy_offline.state, OfflineState::SignedOut);
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_signed_in_current_learner_keeps_work_offline() {
    let mut desk = desk().await;
    let guest = desk.current_learner();
    for intent in [
        json!({"kind": "downloadCourse", "learner": guest, "courseId": COURSE}),
        json!({"kind": "saveLocalDraft", "learner": guest, "attemptId": "a", "baseVersion": 0, "draft": "{}"}),
    ] {
        assert_eq!(
            desk.send(intent).await.err(),
            Some(Refusal::CapabilityMissing)
        );
    }
    desk.sign_in().await;
    assert_eq!(
        desk.send(json!({"kind": "downloadCourse", "learner": guest, "courseId": COURSE}))
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
    // Ids that name records are plain and bounded.
    let learner = desk.learner();
    let long = "a".repeat(129);
    for intent in [
        json!({"kind": "downloadCourse", "learner": learner, "courseId": "../x"}),
        json!({"kind": "removeDownload", "learner": learner, "courseId": ""}),
        json!({"kind": "loadLocalDraft", "learner": learner, "attemptId": long}),
        json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": "a b", "baseVersion": 0, "draft": "{}"}),
    ] {
        assert_eq!(desk.send(intent).await.err(), Some(Refusal::InvalidInput));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_relaunch_while_academy_is_unreachable_still_opens_downloaded_work() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let reply = download(&mut desk, 7 * 86_400).await;
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    // Academy goes away entirely (no token renewal either), then the app
    // is quit and opened again.
    desk.script.lock().expect("script").unreachable = true;
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(eventually(|| matches!(desk.academy(), AcademyView::SignedIn { .. })).await);
    assert!(
        eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await,
        "{:?}",
        desk.snap().academy_offline
    );
    let read = read_unit(&mut desk).await;
    let AcademyCallResult::Data { data: answer } = read else {
        panic!("{read:?}")
    };
    assert!(answer.contains(MARKER));
    assert!(!desk.snap().academy_offline.reachable);
    assert_eq!(desk.snap().academy_offline.downloads.len(), 1);
}

async fn attempt_read(desk: &mut Desk) -> Value {
    let input = format!(r#"{{"json":{{"attemptId":"{ATTEMPT}"}}}}"#);
    let reply = desk
        .call("desktop.attempt.get", "query", Some(&input))
        .await
        .expect("admitted");
    let AcademyCallResult::Data { data: answer } = reply else {
        panic!("{reply:?}")
    };
    serde_json::from_str::<Value>(&answer).expect("json")["json"].clone()
}

/// Reconnects: one read succeeds, revalidation runs with these replies.
async fn reconnect(desk: &mut Desk, course_allowed: bool, attempt: (u16, Value)) {
    {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({}))));
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", course_allowed, 7 * 86_400)),
        ));
        script.trpc.push((
            "desktop.manifest".into(),
            200,
            data(bundle("user_2fixture", "rev-1", 0)),
        ));
        script
            .trpc
            .push(("desktop.attempt.get".into(), attempt.0, attempt.1));
    }
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(
        eventually(|| desk.requests("/api/trpc/desktop.attempt.get").len() >= 2).await,
        "the cached attempt is read again"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cached_attempts_lock_when_their_lease_ends_access_is_revoked_or_the_revision_is_withdrawn()
{
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    let learner = desk.learner();
    // The learner's own work kept on this Mac is never affected.
    host(
        &mut desk,
        json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": ATTEMPT,
               "baseVersion": 2, "draft": r#"{"debrief":"LOCAL-WORK"}"#}),
    )
    .await
    .expect("admitted");
    let assert_locked = |value: &Value, reason: &str| {
        assert_eq!(value["locked"], true, "{value}");
        assert_eq!(value["lockReason"], reason);
        assert_eq!(
            value["draft"]["debrief"], "LEARNER-WORK",
            "the learner's work stays"
        );
        assert!(
            !value.to_string().contains(AUTHORED),
            "no authored definition: {value}"
        );
        assert!(value["lease"].is_null());
    };

    // 1. A lease that ends locks the cached attempt offline.
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(2, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    desk.script.lock().expect("script").unreachable = true;
    let within = attempt_read(&mut desk).await;
    assert!(
        within.to_string().contains(AUTHORED),
        "readable within its lease"
    );
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert_locked(&attempt_read(&mut desk).await, "lease_expired");

    // 2. Course access revoked: Academy refuses the attempt on reconnect.
    reconnect(
        &mut desk,
        false,
        (
            403,
            json!({"error": {"json": {"message": "FORBIDDEN", "code": -32003,
        "data": {"code": "FORBIDDEN", "httpStatus": 403}}}}),
        ),
    )
    .await;
    desk.script.lock().expect("script").unreachable = true;
    assert_locked(&attempt_read(&mut desk).await, "access_revoked");

    // 3. The pinned revision withdrawn while the course stays allowed:
    // Academy's own lock replaces what was kept, and survives a restart.
    reconnect(
        &mut desk,
        true,
        (200, data(locked_lab_attempt("revision_withdrawn"))),
    )
    .await;
    desk.script.lock().expect("script").unreachable = true;
    assert_locked(&attempt_read(&mut desk).await, "revision_withdrawn");
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    assert_locked(&attempt_read(&mut desk).await, "revision_withdrawn");
    for file in offline_files(&desk) {
        let text = String::from_utf8_lossy(&std::fs::read(file).expect("read")).to_string();
        assert!(
            !text.contains(AUTHORED) && !text.contains("LOCAL-WORK"),
            "sealed"
        );
    }

    // The local draft is still there, and nothing was submitted or graded.
    let learner = desk.learner();
    let loaded = host(
        &mut desk,
        json!({"kind": "loadLocalDraft", "learner": learner, "attemptId": ATTEMPT}),
    )
    .await
    .expect("admitted");
    let AcademyCallResult::Data { data: kept } = loaded else {
        panic!("{loaded:?}")
    };
    assert!(kept.contains("LOCAL-WORK"));
    for path in [
        "/api/trpc/desktop.attempt.submit",
        "/api/trpc/desktop.attempt.checkStep",
        "/api/trpc/desktop.draft.save",
    ] {
        assert!(desk.requests(path).is_empty(), "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_waits_for_admitted_offline_work_and_nothing_survives_it() {
    // A reset that does not wait can deadlock on the held Keychain; fail
    // rather than hang.
    tokio::time::timeout(Duration::from_secs(30), reset_race())
        .await
        .expect("reset completed within 30 s");
}

/// Releases a held Keychain stand-in when dropped, so a failing assertion
/// never leaves a blocked write for the runtime to wait on at teardown.
struct ReleaseOnDrop(Arc<desktop_core::fixtures::ScriptedVault>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

async fn reset_race() {
    use desktop_core::StorageHealth;
    // 1. The first offline key write is held while reset is requested.
    let mut desk = desk().await;
    desk.sign_in().await;
    let learner = desk.learner();
    desk.adapters.learner_vault.hold_writes();
    let _release = ReleaseOnDrop(desk.adapters.learner_vault.clone());
    let receipt = desk
        .send(
            json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": ATTEMPT,
                     "baseVersion": 0, "draft": r#"{"debrief":"HELD"}"#}),
        )
        .await
        .expect("admitted");
    assert!(matches!(receipt, Receipt::Admitted { .. }));
    assert!(
        eventually(|| desk.adapters.learner_vault.waiting() == 1).await,
        "the key write is held"
    );
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        desk.snap().storage,
        StorageHealth::Resetting,
        "reset waits for it"
    );
    desk.adapters.learner_vault.release();
    assert!(eventually(|| desk.snap().storage == StorageHealth::Ready).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !desk.adapters.offline_root.exists(),
        "no offline root after reset"
    );
    assert!(
        desk.adapters.learner_vault.records().is_empty(),
        "no key after reset"
    );

    // 2. Opening kept work at launch is held on the Keychain read while
    // reset is requested; after release nothing is recreated.
    let desk = desk_with_download().await;
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    adapters.learner_vault.hold_reads();
    let _release_reads = ReleaseOnDrop(adapters.learner_vault.clone());
    let signed_in = state.clone();
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(eventually(|| desk.adapters.learner_vault.waiting() >= 1).await);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    tokio::time::sleep(Duration::from_millis(400)).await;
    desk.adapters.learner_vault.release();
    assert!(eventually(|| desk.snap().storage == StorageHealth::Ready).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!desk.adapters.offline_root.exists(), "nothing recreated");
    assert!(desk.adapters.learner_vault.records().is_empty());
    // And after another restart, even from a document still naming the
    // signed-in learner, nothing is recreated.
    let state = signed_in;
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    let desk = start(ca, server, script, Some((adapters, state))).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!desk.adapters.offline_root.exists());
    assert!(desk.adapters.learner_vault.records().is_empty());
}

async fn desk_with_download() -> Desk {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    desk
}

#[tokio::test(flavor = "multi_thread")]
async fn assembling_stops_once_a_course_cannot_fit_one_record() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    let before = offline_files(&desk);
    // Six unit bundles each within Academy's 7 MiB cap; four of them already
    // pass the 16 MiB record limit.
    let ids: Vec<String> = (1..=6).map(|n| format!("big-unit-{n}")).collect();
    let mut manifest = bundle("user_2fixture", "big-1", 600);
    manifest["manifest"] = json!(true);
    manifest["units"] = json!(ids.iter().map(|id| json!({"id": id})).collect::<Vec<_>>());
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 600)),
        ));
        script.trpc.push((
            "desktop.bundle".into(),
            413,
            error(413, "PAYLOAD_TOO_LARGE"),
        ));
        script
            .trpc
            .push(("desktop.manifest".into(), 200, data(manifest)));
        for id in &ids {
            script.trpc.push((
                "desktop.unitBundle".into(),
                200,
                data(unit_part(id, "big-1", 5 * 1024 * 1024)),
            ));
        }
    }
    let learner = desk.learner();
    let reply = host(
        &mut desk,
        json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert!(
        matches!(
            reply,
            AcademyCallResult::Failed {
                failure: CallFailure::OfflineFull,
                ..
            }
        ),
        "{reply:?}"
    );
    assert_eq!(
        desk.requests("/api/trpc/desktop.unitBundle").len(),
        4,
        "no unit is fetched once the course cannot fit"
    );
    // The download already kept is unchanged.
    assert_eq!(offline_files(&desk), before);
    assert_eq!(desk.snap().academy_offline.downloads.len(), 1);
    assert_eq!(desk.snap().academy_offline.downloads[0].revision, "rev-1");
}

#[tokio::test(flavor = "multi_thread")]
async fn revalidation_never_overwrites_a_newer_download_or_brings_back_a_removed_one() {
    let mut desk = desk().await;
    desk.sign_in().await;
    download(&mut desk, 7 * 86_400).await;
    let learner = desk.learner();
    // Academy unreachable, then back: revalidation starts and its manifest
    // reply (a newer revision) is slow.
    let revalidate = |desk: &mut Desk, manifest_revision: &str| {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script.trpc_delays.push(("desktop.manifest".into(), 800));
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({}))));
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 7 * 86_400)),
        ));
        script.trpc.push((
            "desktop.manifest".into(),
            200,
            data(bundle("user_2fixture", manifest_revision, 0)),
        ));
    };
    let go_offline = |desk: &mut Desk| desk.script.lock().expect("script").unreachable = true;

    // 1. Removed while revalidation waits: it is not brought back.
    go_offline(&mut desk);
    let _ = read_unit(&mut desk).await;
    revalidate(&mut desk, "rev-9");
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(eventually(|| !desk.requests("/api/trpc/desktop.manifest").is_empty()).await);
    let removed = host(
        &mut desk,
        json!({"kind": "removeDownload", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert!(
        matches!(removed, AcademyCallResult::Data { .. }),
        "{removed:?}"
    );
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(
        desk.snap().academy_offline.downloads.is_empty(),
        "not resurrected"
    );
    assert!(
        offline_files(&desk)
            .iter()
            .all(|f| !f.to_string_lossy().contains("course-")),
        "no course record"
    );

    // 2. Downloaded again (rev-2) while revalidation of rev-1 waits: its
    // verdict for rev-1 is not applied to rev-2.
    download(&mut desk, 7 * 86_400).await;
    go_offline(&mut desk);
    let _ = read_unit(&mut desk).await;
    revalidate(&mut desk, "rev-3");
    let before = desk.requests("/api/trpc/desktop.manifest").len();
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(eventually(|| desk.requests("/api/trpc/desktop.manifest").len() > before).await);
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 7 * 86_400)),
        ));
        script.trpc.push((
            "desktop.bundle".into(),
            200,
            data(bundle("user_2fixture", "rev-2", 7 * 86_400)),
        ));
    }
    let again = host(
        &mut desk,
        json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    assert!(matches!(again, AcademyCallResult::Data { .. }), "{again:?}");
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    let offline = desk.snap().academy_offline;
    assert_eq!(offline.downloads.len(), 1);
    assert_eq!(offline.downloads[0].revision, "rev-2");
    assert_eq!(
        offline.downloads[0].status,
        DownloadStatus::Current,
        "rev-1's verdict not applied"
    );
}

/// Restarts the core over the same adapters and Academy (optionally with a
/// hook run on the adapters first).
async fn restart(desk: Desk, before: impl FnOnce(&desktop_core::fixtures::Handles)) -> Desk {
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    before(&adapters);
    start(ca, server, script, Some((adapters, state))).await
}

fn offline_key_record() -> desktop_core::vault::RecordId {
    let digest = desktop_core::offline::digest("academy-offline-key:user_2fixture");
    desktop_core::vault::RecordId::parse(&format!("cr-{}", &digest[..16])).expect("record")
}

fn sealed_files(desk: &Desk) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = offline_files(desk)
        .into_iter()
        .filter(|f| !f.to_string_lossy().ends_with(".lock"))
        .map(|f| {
            let bytes = std::fs::read(&f).expect("read");
            (f, bytes)
        })
        .collect();
    files.sort();
    files
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_offline_key_opens_nothing_and_loses_nothing() {
    let mut desk = desk_with_download().await;
    let learner = desk.learner();
    for n in 0..10 {
        host(
            &mut desk,
            json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": format!("att-{n}"),
                   "baseVersion": 0, "draft": format!(r#"{{"debrief":"WORK-{n}"}}"#)}),
        )
        .await
        .expect("admitted");
    }
    assert_eq!(desk.snap().academy_offline.local_drafts.len(), 10);
    let record = offline_key_record();
    let original = desk.adapters.learner_vault.secret(&record).expect("key");
    let before = sealed_files(&desk);
    // Another 32-byte key under the account's record.
    let desk = restart(desk, |a| a.learner_vault.plant(&record, &[9u8; 32])).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Unavailable).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        sealed_files(&desk),
        before,
        "nothing read aside, deleted or rewritten"
    );
    assert!(desk.snap().academy_offline.local_drafts.is_empty());
    // The original key recovers every record.
    let desk = restart(desk, |a| a.learner_vault.plant(&record, &original)).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    assert_eq!(desk.snap().academy_offline.local_drafts.len(), 10);
    assert_eq!(desk.snap().academy_offline.downloads.len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_draft_opens_offline_without_the_attempt_being_read_again() {
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    let cached = attempt_read(&mut desk).await;
    let lease = cached["lease"]["expiresAt"].clone();
    // Saved, then a conflict resolved, online; the attempt is never read again.
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.draft.save".into(),
            200,
            data(
                json!({"status": "saved", "draftVersion": 3, "draft": {"debrief": "CONFIRMED-3"}}),
            ),
        ));
        script.trpc.push((
            "desktop.draft.resolve".into(),
            200,
            data(json!({"status": "saved", "draftVersion": 4, "draft": {"debrief": "RESOLVED-4"}})),
        ));
    }
    let save = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","baseVersion":2,"clientMutationId":"00000000-0000-4000-8000-0000000000c1","draft":{{"debrief":"CONFIRMED-3"}}}}}}"#
    );
    let saved = desk
        .call("desktop.draft.save", "mutation", Some(&save))
        .await
        .expect("admitted");
    assert!(matches!(saved, AcademyCallResult::Data { .. }), "{saved:?}");
    desk.script.lock().expect("script").unreachable = true;
    let mut desk = restart(desk, |_| {}).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    let offline = attempt_read(&mut desk).await;
    assert_eq!(offline["draftVersion"], 3);
    assert_eq!(offline["draft"]["debrief"], "CONFIRMED-3");
    assert_eq!(
        offline["lease"]["expiresAt"], lease,
        "the lease is not extended"
    );
    // A resolved conflict too (online again for the mutation).
    desk.script.lock().expect("script").unreachable = false;
    let resolve = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","conflictId":"00000000-0000-4000-8000-0000000000d1","baseVersion":3,"choice":"incoming"}}}}"#
    );
    let resolved = desk
        .call("desktop.draft.resolve", "mutation", Some(&resolve))
        .await
        .expect("admitted");
    assert!(
        matches!(resolved, AcademyCallResult::Data { .. }),
        "{resolved:?}"
    );
    desk.script.lock().expect("script").unreachable = true;
    let offline = attempt_read(&mut desk).await;
    assert_eq!(offline["draftVersion"], 4);
    assert_eq!(offline["draft"]["debrief"], "RESOLVED-4");
    assert_eq!(offline["conflicts"], json!([]));
    assert!(desk.requests("/api/trpc/desktop.attempt.submit").is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_draft_asked_for_while_kept_work_is_opening_waits_for_it() {
    let mut desk = desk_with_download().await;
    let learner = desk.learner();
    host(
        &mut desk,
        json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": ATTEMPT,
               "baseVersion": 2, "draft": r#"{"debrief":"KEPT-WHILE-OPENING"}"#}),
    )
    .await
    .expect("admitted");
    let mut desk = restart(desk, |a| a.learner_vault.hold_reads()).await;
    let _release = ReleaseOnDrop(desk.adapters.learner_vault.clone());
    assert!(
        eventually(|| desk.adapters.learner_vault.waiting() >= 1).await,
        "opening is held"
    );
    let learner = desk.learner();
    let receipt = desk
        .send(json!({"kind": "loadLocalDraft", "learner": learner, "attemptId": ATTEMPT}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!("{receipt:?}")
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.adapters.learner_vault.release();
    let AcademyCallResult::Data { data: loaded } = desk.result(&operation).await else {
        panic!("not loaded")
    };
    assert!(loaded.contains("KEPT-WHILE-OPENING"), "{loaded}");
}

#[tokio::test(flavor = "multi_thread")]
async fn revalidation_results_never_undo_newer_state() {
    let mut desk = desk_with_download().await;
    let learner = desk.learner();
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    let go_offline = |desk: &mut Desk| desk.script.lock().expect("script").unreachable = true;

    // 1. A same-revision re-download while revalidation's newer-revision
    //    verdict is pending keeps its own status.
    go_offline(&mut desk);
    let _ = read_unit(&mut desk).await;
    {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({}))));
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 7 * 86_400)),
        ));
        script
            .trpc_once_delays
            .push(("desktop.manifest".into(), 1_000));
        script.trpc.push((
            "desktop.manifest".into(),
            200,
            data(bundle("user_2fixture", "rev-9", 0)),
        ));
        // Revalidation also reads the kept attempt again: slow and stale.
        script
            .trpc_once_delays
            .push(("desktop.attempt.get".into(), 2_500));
        let mut stale = lab_attempt(7 * 86_400, "rev-1");
        stale["draftVersion"] = json!(1);
        stale["draft"] = json!({"debrief": "STALE"});
        script
            .trpc
            .push(("desktop.attempt.get".into(), 200, data(stale)));
    }
    let manifests = desk.requests("/api/trpc/desktop.manifest").len();
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(eventually(|| desk.requests("/api/trpc/desktop.manifest").len() > manifests).await);
    host(
        &mut desk,
        json!({"kind": "removeDownload", "learner": learner, "courseId": COURSE}),
    )
    .await
    .expect("admitted");
    download(&mut desk, 7 * 86_400).await;
    // 2. While the stale attempt read is pending: Academy's newer lock and a
    //    newer confirmed draft arrive in the foreground.
    assert!(eventually(|| desk.requests("/api/trpc/desktop.attempt.get").len() >= 2).await);
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.attempt.get".into(),
            200,
            data(locked_lab_attempt("revision_withdrawn")),
        ));
        script.trpc.push((
            "desktop.draft.save".into(),
            200,
            data(json!({"status": "saved", "draftVersion": 5, "draft": {"debrief": "NEWEST"}})),
        ));
    }
    let locked = attempt_read(&mut desk).await;
    assert_eq!(locked["lockReason"], "revision_withdrawn");
    let save = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","baseVersion":2,"clientMutationId":"00000000-0000-4000-8000-0000000000c2","draft":{{"debrief":"NEWEST"}}}}}}"#
    );
    desk.call("desktop.draft.save", "mutation", Some(&save))
        .await
        .expect("admitted");
    // The stale results land now.
    tokio::time::sleep(Duration::from_millis(3_000)).await;
    let offline = desk.snap().academy_offline;
    assert_eq!(offline.downloads.len(), 1);
    assert_eq!(
        offline.downloads[0].status,
        DownloadStatus::Current,
        "rev-9's verdict is older than the re-download"
    );
    go_offline(&mut desk);
    let mut desk = restart(desk, |_| {}).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    assert_eq!(
        desk.snap().academy_offline.downloads[0].status,
        DownloadStatus::Current
    );
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["locked"], true, "the newer lock stays: {attempt}");
    assert_eq!(attempt["lockReason"], "revision_withdrawn");
    assert_eq!(attempt["draftVersion"], 5, "the newer draft stays");
    assert!(!attempt.to_string().contains(AUTHORED));
    let unit = read_unit(&mut desk).await;
    assert!(matches!(unit, AcademyCallResult::Data { .. }), "{unit:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_withdrawal_applies_at_once_while_other_checks_are_pending() {
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    desk.script.lock().expect("script").unreachable = true;
    let _ = read_unit(&mut desk).await;
    {
        let mut script = desk.script.lock().expect("script");
        script.unreachable = false;
        script
            .trpc
            .push(("progress.get".into(), 200, data(json!({}))));
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", false, 0)),
        ));
        // The attempt re-read is held well past the checks below.
        script
            .trpc_once_delays
            .push(("desktop.attempt.get".into(), 3_000));
        script.trpc.push((
            "desktop.attempt.get".into(),
            200,
            data(lab_attempt(7 * 86_400, "rev-1")),
        ));
    }
    desk.call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(eventually(|| desk.requests("/api/trpc/desktop.attempt.get").len() >= 2).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    // Academy drops away again while the attempt re-read is pending.
    desk.script.lock().expect("script").unreachable = true;
    let unit = read_unit(&mut desk).await;
    assert!(
        matches!(unit, AcademyCallResult::Failed { .. }),
        "withdrawn content is not served: {unit:?}"
    );
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["locked"], true, "{attempt}");
    assert_eq!(attempt["lockReason"], "access_revoked");
    assert!(!attempt.to_string().contains(AUTHORED));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_unit_bundle_for_another_account_or_course_is_refused() {
    let mut desk = desk().await;
    desk.sign_in().await;
    for (field, value) in [("accountId", "user_2someone"), ("courseId", "other-course")] {
        let mut manifest = bundle("user_2fixture", "rev-1", 600);
        manifest["manifest"] = json!(true);
        manifest["units"] = json!([{"id": UNIT}]);
        let mut part = unit_part(UNIT, "rev-1", 0);
        part[field] = json!(value);
        {
            let mut script = desk.script.lock().expect("script");
            script.trpc.push((
                "desktop.access".into(),
                200,
                data(access("user_2fixture", true, 600)),
            ));
            script.trpc.push((
                "desktop.bundle".into(),
                413,
                error(413, "PAYLOAD_TOO_LARGE"),
            ));
            script
                .trpc
                .push(("desktop.manifest".into(), 200, data(manifest)));
            script
                .trpc
                .push(("desktop.unitBundle".into(), 200, data(part)));
        }
        let learner = desk.learner();
        let reply = host(
            &mut desk,
            json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}),
        )
        .await
        .expect("admitted");
        assert!(
            matches!(
                reply,
                AcademyCallResult::Failed {
                    failure: CallFailure::InvalidResponse,
                    ..
                }
            ),
            "{field}: {reply:?}"
        );
        assert!(desk.snap().academy_offline.downloads.is_empty());
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attempt_academy_no_longer_returns_is_kept_locked() {
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    desk.script.lock().expect("script").unreachable = true;
    let _ = read_unit(&mut desk).await;
    // The course stays allowed, but Academy no longer returns the attempt.
    reconnect(
        &mut desk,
        true,
        (
            404,
            json!({"error": {"json": {"message": "NOT_FOUND", "code": -32004,
                "data": {"code": "NOT_FOUND", "httpStatus": 404}}}}),
        ),
    )
    .await;
    desk.script.lock().expect("script").unreachable = true;
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["locked"], true, "{attempt}");
    assert_eq!(attempt["lockReason"], "access_revoked");
    assert!(!attempt.to_string().contains(AUTHORED));
    assert_eq!(attempt["draft"]["debrief"], "LEARNER-WORK");
    assert_eq!(
        desk.snap().academy_offline.downloads[0].status,
        DownloadStatus::Current
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attempt_read_that_predates_a_confirmed_save_keeps_the_newer_draft() {
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    // An attempt read starts (its reply, draft version 2, is slow), then a
    // save is confirmed at version 3 before that reply lands.
    {
        let mut script = desk.script.lock().expect("script");
        script
            .trpc_once_delays
            .push(("desktop.attempt.get".into(), 1_500));
        script.trpc.push((
            "desktop.attempt.get".into(),
            200,
            data(lab_attempt(7 * 86_400, "rev-1")),
        ));
        script.trpc.push((
            "desktop.draft.save".into(),
            200,
            data(
                json!({"status": "saved", "draftVersion": 3, "draft": {"debrief": "CONFIRMED-3"}}),
            ),
        ));
    }
    let learner = desk.learner();
    let input = format!(r#"{{"json":{{"attemptId":"{ATTEMPT}"}}}}"#);
    let receipt = desk
        .send(
            json!({"kind": "academyCall", "learner": learner, "procedure": "desktop.attempt.get",
                     "procedureKind": "query", "input": input}),
        )
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(slow),
        ..
    } = receipt
    else {
        panic!("{receipt:?}")
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let save = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","baseVersion":2,"clientMutationId":"00000000-0000-4000-8000-0000000000c3","draft":{{"debrief":"CONFIRMED-3"}}}}}}"#
    );
    let saved = desk
        .call("desktop.draft.save", "mutation", Some(&save))
        .await
        .expect("admitted");
    assert!(matches!(saved, AcademyCallResult::Data { .. }), "{saved:?}");
    let _ = desk.result(&slow).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.script.lock().expect("script").unreachable = true;
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["draftVersion"], 3, "{attempt}");
    assert_eq!(attempt["draft"]["debrief"], "CONFIRMED-3");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_download_answered_after_a_newer_withdrawal_does_not_bring_the_course_back() {
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    // A download is authorized, then its bundle is slow.
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.access".into(),
            200,
            data(access("user_2fixture", true, 7 * 86_400)),
        ));
        script
            .trpc_once_delays
            .push(("desktop.bundle".into(), 2_500));
        script.trpc.push((
            "desktop.bundle".into(),
            200,
            data(bundle("user_2fixture", "rev-1", 7 * 86_400)),
        ));
    }
    let learner = desk.learner();
    let receipt = desk
        .send(json!({"kind": "downloadCourse", "learner": learner, "courseId": COURSE}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(slow),
        ..
    } = receipt
    else {
        panic!("{receipt:?}")
    };
    assert!(eventually(|| desk.requests("/api/trpc/desktop.bundle").len() >= 2).await);
    // Meanwhile Academy withdraws the course (a newer observation).
    desk.script.lock().expect("script").unreachable = true;
    let _ = attempt_read(&mut desk).await;
    reconnect(
        &mut desk,
        false,
        (200, data(locked_lab_attempt("access_revoked"))),
    )
    .await;
    assert!(
        eventually(|| desk
            .snap()
            .academy_offline
            .downloads
            .first()
            .is_some_and(|d| d.status == DownloadStatus::Withdrawn))
        .await
    );
    // The older download answers now: it does not bring the course back.
    let late = desk.result(&slow).await;
    assert!(
        matches!(
            late,
            AcademyCallResult::Failed {
                failure: CallFailure::NotDownloadable,
                ..
            }
        ),
        "{late:?}"
    );
    assert_eq!(
        desk.snap().academy_offline.downloads[0].status,
        DownloadStatus::Withdrawn
    );
    desk.script.lock().expect("script").unreachable = true;
    let unit = read_unit(&mut desk).await;
    assert!(matches!(unit, AcademyCallResult::Failed { .. }), "{unit:?}");
    let mut desk = restart(desk, |_| {}).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    assert_eq!(
        desk.snap().academy_offline.downloads[0].status,
        DownloadStatus::Withdrawn
    );
    let unit = read_unit(&mut desk).await;
    assert!(matches!(unit, AcademyCallResult::Failed { .. }), "{unit:?}");
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["locked"], true, "{attempt}");
    assert!(!attempt.to_string().contains(AUTHORED));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirmation_the_kept_attempt_cannot_hold_leaves_the_local_copy() {
    let mut desk = desk_with_download().await;
    // A kept attempt close to the record limit: growing it by the confirmed
    // draft does not fit.
    let mut big = lab_attempt(7 * 86_400, "rev-1");
    big["steps"] = json!([{"id": "s1", "prompt": format!("{AUTHORED}{}", "x".repeat(15 * 1024 * 1024 + 300 * 1024))}]);
    desk.script
        .lock()
        .expect("script")
        .trpc
        .push(("desktop.attempt.get".into(), 200, data(big)));
    let cached = attempt_read(&mut desk).await;
    let lease = cached["lease"]["expiresAt"].clone();
    let work = "w".repeat(900 * 1024);
    let learner = desk.learner();
    let kept = host(
        &mut desk,
        json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": ATTEMPT,
               "baseVersion": 2, "draft": format!(r#"{{"debrief":"{work}"}}"#)}),
    )
    .await
    .expect("admitted");
    assert!(matches!(kept, AcademyCallResult::Data { .. }), "{kept:?}");
    desk.script.lock().expect("script").trpc.push((
        "desktop.draft.save".into(),
        200,
        data(json!({"status": "saved", "draftVersion": 3, "draft": {"debrief": work}})),
    ));
    let save = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","baseVersion":2,"clientMutationId":"00000000-0000-4000-8000-0000000000c4","draft":{{"debrief":"{work}"}}}}}}"#
    );
    let saved = desk
        .call("desktop.draft.save", "mutation", Some(&save))
        .await
        .expect("admitted");
    assert!(
        matches!(saved, AcademyCallResult::Data { .. }),
        "Academy confirmed: {saved:?}"
    );
    // The page asks to forget its copy: refused, the kept attempt does not hold it.
    let cleared = host(
        &mut desk,
        json!({"kind": "clearLocalDraft", "learner": learner, "attemptId": ATTEMPT}),
    )
    .await
    .expect("admitted");
    assert!(
        matches!(cleared, AcademyCallResult::Failed { .. }),
        "{cleared:?}"
    );
    // Offline after restart the newest work is still on this Mac.
    desk.script.lock().expect("script").unreachable = true;
    let mut desk = restart(desk, |_| {}).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    let learner = desk.learner();
    let loaded = host(
        &mut desk,
        json!({"kind": "loadLocalDraft", "learner": learner, "attemptId": ATTEMPT}),
    )
    .await
    .expect("admitted");
    let AcademyCallResult::Data { data: loaded } = loaded else {
        panic!("{loaded:?}")
    };
    assert!(loaded.contains(&work[..64]), "the newest work survives");
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["lease"]["expiresAt"], lease, "no lease extension");
    assert_eq!(
        desk.requests("/api/trpc/desktop.draft.save").len(),
        1,
        "nothing replayed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn confirmed_work_marks_earlier_verdicts_stale_and_a_resolved_conflict_stays_resolved() {
    let mut desk = desk_with_download().await;
    let mut graded = lab_attempt(7 * 86_400, "rev-1");
    graded["result"] = json!({"steps": {"s1": {"hash": "h-2", "verdict": {"pass": true, "feedback": "Correct."}}}});
    graded["conflicts"] = json!([{"conflictId": "00000000-0000-4000-8000-0000000000d1",
                                  "incoming": {"debrief": "OTHER"}, "currentVersion": 2}]);
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(graded.clone()),
    ));
    attempt_read(&mut desk).await;
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc.push((
            "desktop.draft.save".into(),
            200,
            data(json!({"status": "saved", "draftVersion": 3, "draft": {"debrief": "EDITED"}})),
        ));
        // A read that starts before the resolve and answers after it.
        script
            .trpc_once_delays
            .push(("desktop.attempt.get".into(), 1_500));
        script
            .trpc
            .push(("desktop.attempt.get".into(), 200, data(graded)));
        script.trpc.push((
            "desktop.draft.resolve".into(),
            200,
            data(json!({"status": "saved", "draftVersion": 4, "draft": {"debrief": "EDITED"}})),
        ));
    }
    let save = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","baseVersion":2,"clientMutationId":"00000000-0000-4000-8000-0000000000c5","draft":{{"debrief":"EDITED"}}}}}}"#
    );
    desk.call("desktop.draft.save", "mutation", Some(&save))
        .await
        .expect("admitted");
    let learner = desk.learner();
    let input = format!(r#"{{"json":{{"attemptId":"{ATTEMPT}"}}}}"#);
    let receipt = desk
        .send(
            json!({"kind": "academyCall", "learner": learner, "procedure": "desktop.attempt.get",
                     "procedureKind": "query", "input": input}),
        )
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(slow),
        ..
    } = receipt
    else {
        panic!("{receipt:?}")
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let resolve = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","conflictId":"00000000-0000-4000-8000-0000000000d1","baseVersion":3,"choice":"current"}}}}"#
    );
    desk.call("desktop.draft.resolve", "mutation", Some(&resolve))
        .await
        .expect("admitted");
    let _ = desk.result(&slow).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.script.lock().expect("script").unreachable = true;
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["draftVersion"], 4, "{attempt}");
    assert_eq!(
        attempt["conflicts"],
        json!([]),
        "the resolved conflict stays resolved"
    );
    let step = &attempt["result"]["steps"]["s1"];
    assert_eq!(step["stale"], true, "{step}");
    assert_eq!(
        step["verdict"]["pass"], false,
        "the old pass is not shown for the new work"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn stamps_continue_after_kept_ones_when_the_clock_is_behind() {
    use desktop_core::offline::{OfflineStore, RecordKind};
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    let key = desk
        .adapters
        .learner_vault
        .secret(&offline_key_record())
        .expect("key");
    let root = desk.adapters.offline_root.clone();
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    // Kept stamps from a clock that was ten years ahead (now corrected).
    {
        let ahead = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos() as u64)
            + 10 * 365 * 86_400 * 1_000_000_000;
        let store = OfflineStore::open(&root, "user_2fixture", &key).expect("open");
        store
            .update(RecordKind::Course, COURSE, |r| {
                let mut r = r?;
                r["observed"] = json!(ahead);
                r["validated"] = json!(ahead);
                Some(r)
            })
            .expect("seed course");
        store
            .update(RecordKind::Attempt, ATTEMPT, |r| {
                let mut r = r?;
                r["observed"] = json!(ahead);
                Some(r)
            })
            .expect("seed attempt");
    }
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    // Fresh denials from Academy apply now, in request order.
    desk.script.lock().expect("script").unreachable = true;
    let _ = read_unit(&mut desk).await;
    reconnect(
        &mut desk,
        false,
        (200, data(locked_lab_attempt("access_revoked"))),
    )
    .await;
    assert!(
        eventually(|| desk
            .snap()
            .academy_offline
            .downloads
            .first()
            .is_some_and(|d| d.status == DownloadStatus::Withdrawn))
        .await,
        "{:?}",
        desk.snap().academy_offline
    );
    desk.script.lock().expect("script").unreachable = true;
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["locked"], true, "{attempt}");
    let unit = read_unit(&mut desk).await;
    assert!(matches!(unit, AcademyCallResult::Failed { .. }), "{unit:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirmation_whose_commit_is_not_known_durable_leaves_the_local_copy() {
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    let cached = attempt_read(&mut desk).await;
    let lease = cached["lease"]["expiresAt"].clone();
    let learner = desk.learner();
    host(
        &mut desk,
        json!({"kind": "saveLocalDraft", "learner": learner, "attemptId": ATTEMPT,
               "baseVersion": 2, "draft": r#"{"debrief":"NEWEST-WORK"}"#}),
    )
    .await
    .expect("admitted");
    // The confirmed draft's commit renames into place, then its directory
    // sync fails (and keeps failing).
    desk.core
        .inject_offline_fault(Some(desktop_core::offline::WriteFault::DirectorySync));
    desk.script.lock().expect("script").trpc.push((
        "desktop.draft.save".into(),
        200,
        data(json!({"status": "saved", "draftVersion": 3, "draft": {"debrief": "NEWEST-WORK"}})),
    ));
    let save = format!(
        r#"{{"json":{{"attemptId":"{ATTEMPT}","baseVersion":2,"clientMutationId":"00000000-0000-4000-8000-0000000000c6","draft":{{"debrief":"NEWEST-WORK"}}}}}}"#
    );
    let saved = desk
        .call("desktop.draft.save", "mutation", Some(&save))
        .await
        .expect("admitted");
    assert!(matches!(saved, AcademyCallResult::Data { .. }), "{saved:?}");
    // The attempt reads the new work, but its commit is not known durable:
    // the copy kept on this Mac is not forgotten, on asking or asking again.
    for _ in 0..2 {
        let cleared = host(
            &mut desk,
            json!({"kind": "clearLocalDraft", "learner": learner, "attemptId": ATTEMPT}),
        )
        .await
        .expect("admitted");
        assert!(
            matches!(cleared, AcademyCallResult::Failed { .. }),
            "{cleared:?}"
        );
    }
    assert_eq!(desk.snap().academy_offline.local_drafts.len(), 1);
    // Across the restart boundary the newest work is still on this Mac;
    // opening syncs the directory, after which the copy may go.
    desk.script.lock().expect("script").unreachable = true;
    let mut desk = restart(desk, |_| {}).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    let learner = desk.learner();
    let loaded = host(
        &mut desk,
        json!({"kind": "loadLocalDraft", "learner": learner, "attemptId": ATTEMPT}),
    )
    .await
    .expect("admitted");
    let AcademyCallResult::Data { data: loaded } = loaded else {
        panic!("{loaded:?}")
    };
    assert!(loaded.contains("NEWEST-WORK"), "{loaded}");
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["draftVersion"], 3);
    assert_eq!(attempt["lease"]["expiresAt"], lease, "no lease extension");
    let cleared = host(
        &mut desk,
        json!({"kind": "clearLocalDraft", "learner": learner, "attemptId": ATTEMPT}),
    )
    .await
    .expect("admitted");
    assert!(
        matches!(cleared, AcademyCallResult::Data { .. }),
        "durable now: {cleared:?}"
    );
    assert_eq!(
        desk.requests("/api/trpc/desktop.draft.save").len(),
        1,
        "nothing replayed"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attempt_read_during_opening_keeps_academys_lock_over_kept_stamps_ahead_of_the_clock() {
    use desktop_core::offline::{OfflineStore, RecordKind};
    let mut desk = desk_with_download().await;
    desk.script.lock().expect("script").trpc.push((
        "desktop.attempt.get".into(),
        200,
        data(lab_attempt(7 * 86_400, "rev-1")),
    ));
    attempt_read(&mut desk).await;
    let key = desk
        .adapters
        .learner_vault
        .secret(&offline_key_record())
        .expect("key");
    let root = desk.adapters.offline_root.clone();
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    core.shutdown();
    drop(core);
    tokio::time::sleep(Duration::from_millis(200)).await;
    {
        let ahead = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos() as u64)
            + 10 * 365 * 86_400 * 1_000_000_000;
        let store = OfflineStore::open(&root, "user_2fixture", &key).expect("open");
        store
            .update(RecordKind::Attempt, ATTEMPT, |r| {
                let mut r = r?;
                r["observed"] = json!(ahead);
                Some(r)
            })
            .expect("seed attempt");
    }
    {
        // Background revalidation is unavailable (Academy's access check
        // fails); the foreground read returns Academy's lock.
        let mut s = script.lock().expect("script");
        s.trpc.push((
            "desktop.access".into(),
            500,
            error(500, "INTERNAL_SERVER_ERROR"),
        ));
        s.trpc.push((
            "desktop.attempt.get".into(),
            200,
            data(locked_lab_attempt("revision_withdrawn")),
        ));
    }
    adapters.learner_vault.hold_reads();
    let _release = ReleaseOnDrop(adapters.learner_vault.clone());
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(
        eventually(|| desk.adapters.learner_vault.waiting() >= 1).await,
        "opening is held"
    );
    let learner = desk.learner();
    let input = format!(r#"{{"json":{{"attemptId":"{ATTEMPT}"}}}}"#);
    let receipt = desk
        .send(
            json!({"kind": "academyCall", "learner": learner, "procedure": "desktop.attempt.get",
                     "procedureKind": "query", "input": input}),
        )
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(read),
        ..
    } = receipt
    else {
        panic!("{receipt:?}")
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.adapters.learner_vault.release();
    let reply = desk.result(&read).await;
    assert!(matches!(reply, AcademyCallResult::Data { .. }), "{reply:?}");
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.script.lock().expect("script").unreachable = true;
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(attempt["lockReason"], "revision_withdrawn", "{attempt}");
    assert!(!attempt.to_string().contains(AUTHORED));
    let mut desk = restart(desk, |_| {}).await;
    assert!(eventually(|| desk.snap().academy_offline.state == OfflineState::Ready).await);
    let attempt = attempt_read(&mut desk).await;
    assert_eq!(
        attempt["lockReason"], "revision_withdrawn",
        "kept after restart: {attempt}"
    );
}
