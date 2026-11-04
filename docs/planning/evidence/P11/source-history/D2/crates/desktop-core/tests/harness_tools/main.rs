//! C21 Harness tools, approvals, proposals and project access through the
//! production authority interface.
//!
//! The real pinned engine runs against the synthetic TLS Gateway, which
//! answers a turn with the tool call a test names (a typed desktop tool or
//! the engine's own `exec_command`) and the request carrying its output
//! with a short answer; the management reads the tools make go to the same
//! synthetic tenant. Commands run in the engine's own sandbox, in a
//! disposable project folder.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

#[allow(dead_code, unused_imports)]
#[path = "../harness_process/desk.rs"]
mod desk;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::desk::*;

const API_KEY_VALUE: &str = "SYNTHETIC-C21-ISSUED-KEY-VALUE";
const AUTH_CODE: &str = "SYNTHETIC-C21-AUTH-CODE";

/// A desk whose tenant has topics and API keys, with the real engine.
async fn desk() -> Desk {
    let desk = desk_with(Some(real_engine()), None).await;
    desk.gateway.lock().expect("gateway").hook = Some(Arc::new(|seen| {
        let target = seen.request_line.split(' ').nth(1)?;
        let path = target.split_once('?').map_or(target, |(p, _)| p);
        let body = match path {
            p if p.starts_with("/aisec/v1/mgmt/topics/tsg/") => json!({"custom_topics": [
                {"topic_id": "11111111-1111-4111-8111-111111111111", "topic_name": "Finance",
                 "revision": 1, "description": "Money talk"}]}),
            p if p.starts_with("/aisec/v1/mgmt/apikeys/tsg/") => json!({"api_keys": [
                {"api_key_id": "key-1", "api_key_name": "Runtime", "api_key_last8": "abcd1234",
                 "api_key": API_KEY_VALUE, "auth_code": AUTH_CODE, "cust_app": "Support bot",
                 // Unknown fields the management client keeps (C21 review F2).
                 "metadata": {"Authorization": "Bearer NESTED-C21-CANARY",
                     "X-Api-Key": "MIXED-C21-CANARY", "note": "safe-c21-metadata",
                     // Encoded five times over (C21 review R2-F2).
                     "wrapped": five_deep()},
                 "expiration": "2027-01-01T00:00:00Z", "revoked": false, "status": "ACTIVE"}]}),
            _ => return None,
        };
        Some(json_reply(200, &body))
    }));
    desk
}

/// A credential inside JSON carried as text, five layers deep.
fn five_deep() -> String {
    let mut deep = json!({"Authorization": "Bearer FIVE-DEEP-C21-CANARY"}).to_string();
    for _ in 0..5 {
        deep = json!({"wrapped": deep}).to_string();
    }
    deep
}

fn tool(desk: &Desk, namespace: Option<&str>, name: &str, arguments: Value) {
    desk.gateway.lock().expect("gateway").answer = Answer::Tool {
        namespace: namespace.map(str::to_owned),
        name: name.to_owned(),
        arguments: arguments.to_string(),
    };
}

/// The bodies of the Gateway requests that carried a tool's output.
fn outputs(desk: &Desk) -> Vec<String> {
    desk.requests(&format!("{RUNTIME}/responses"))
        .into_iter()
        .filter_map(|s| {
            let body: Value = serde_json::from_slice(&s.body).ok()?;
            let last = body["input"].as_array()?.last()?.clone();
            (last["type"] == "function_call_output").then(|| last["output"].to_string())
        })
        .collect()
}

/// Management requests other than reads.
fn management_writes(desk: &Desk) -> usize {
    desk.requests("/aisec/")
        .iter()
        .filter(|s| !s.request_line.starts_with("GET"))
        .count()
}

fn management_reads(desk: &Desk) -> usize {
    desk.requests("/aisec/v1/mgmt/").len()
}

/// A disposable project folder in the build's own scratch space (the
/// engine's sandbox never lets it write in the system's temporary folders).
fn project() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-c21-project-{}", hex::encode(rand_bytes())));
    std::fs::create_dir_all(&dir).expect("fixture");
    for n in 0..3 {
        std::fs::write(dir.join(format!("file-{n}.txt")), format!("content {n}\n"))
            .expect("fixture");
    }
    dir
}

async fn choose_project(desk: &mut Desk, dir: &Path) {
    desk.adapters.folders.reply(Some(dir.to_path_buf()));
    let context = desk.context();
    desk.send(json!({"kind": "chooseHarnessProject", "context": context}))
        .await
        .expect("choose");
    let want = dir.to_str().expect("utf8").to_owned();
    assert!(
        settle(|| desk.json()["harness"]["project"]["path"] == want.as_str()).await,
        "{}",
        desk.json()["harness"]["project"]
    );
}

/// A ready conversation with the workspace key (and the project if chosen).
async fn ready(desk: &mut Desk) -> String {
    desk.ready(None).await
}

async fn answered_with(desk: &mut Desk, predicate: impl Fn(&Value) -> bool) -> Value {
    // A desktop read waits for the operator's decision in the page (C32):
    // these tests allow each one as it is asked.
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    loop {
        let c = desk.conversation();
        if predicate(&c) {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "{c}");
        let reads: Vec<Value> = c["approvals"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|a| a["kind"] == "desktopRead")
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        for read in reads {
            let _ = desk
                .send(
                    json!({"kind": "decideHarnessApproval", "conversation": c["id"],
                    "approval": read["id"], "allow": true}),
                )
                .await;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    desk.conversation()
}

/// Waits for the turn's one approval of `kind`, allows it and waits for the
/// turn to end.
async fn approve(desk: &mut Desk, id: &str, kind: &str) -> Value {
    let turns = desk.conversation()["entries"]
        .as_array()
        .map_or(0, Vec::len);
    let c = answered_with(desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    assert_eq!(c["approvals"][0]["kind"], kind, "{c}");
    let token = c["approvals"][0]["id"]
        .as_str()
        .expect("an approval")
        .to_owned();
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
        .await
        .expect("decided");
    answered_with(desk, |c| {
        c["phase"] == "ready"
            && c["approvals"].as_array().is_some_and(Vec::is_empty)
            && c["entries"].as_array().is_some_and(|e| e.len() > turns + 1)
    })
    .await
}

fn entry<'a>(c: &'a Value, role: &str) -> Option<&'a Value> {
    c["entries"]
        .as_array()?
        .iter()
        .rev()
        .find(|e| e["role"] == role)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_typed_read_answers_from_the_desktop_without_any_secret() {
    let mut desk = desk().await;
    let id = ready(&mut desk).await;
    tool(
        &desk,
        Some("airs_desktop"),
        "inventory",
        json!({"resource": "apiKeys", "limit": 5}),
    );
    desk.say(&id, "Which API keys exist?").await.expect("sent");
    let c = answered_with(&mut desk, |c| c["lastTurn"] == "completed").await;
    let t = entry(&c, "tool").expect("tool entry");
    assert_eq!(t["text"], "Read the API keys inventory");
    assert_eq!(t["state"], "complete");
    let out = outputs(&desk);
    assert_eq!(out.len(), 1);
    assert!(
        out[0].contains("Runtime") && out[0].contains("abcd1234"),
        "{}",
        out[0]
    );
    for secret in [
        API_KEY_VALUE,
        AUTH_CODE,
        KEY,
        "NESTED-C21-CANARY",
        "MIXED-C21-CANARY",
        "FIVE-DEEP-C21-CANARY",
    ] {
        assert!(!out[0].contains(secret), "{secret}");
        assert!(!desk.json().to_string().contains(secret), "{secret}");
    }
    assert!(out[0].contains("safe-c21-metadata"), "{}", out[0]);
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tool_call_under_way_when_its_turn_is_stopped_is_never_answered() {
    // C21 review F1: the management read is slow; the turn is stopped
    // while it runs.
    let mut desk = desk().await;
    let fast = desk
        .gateway
        .lock()
        .expect("gateway")
        .hook
        .clone()
        .expect("hook");
    let slow = Arc::clone(&fast);
    // The read is held until the stop has landed, then released: no timing
    // assumption, however loaded the suite is.
    let (release, gate) = std::sync::mpsc::channel::<()>();
    let gate = std::sync::Mutex::new(gate);
    let started = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read_started = Arc::clone(&started);
    let served = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let read_served = Arc::clone(&served);
    desk.gateway.lock().expect("gateway").hook = Some(Arc::new(move |seen| {
        if !seen.request_line.contains("/apikeys/") {
            return slow(seen);
        }
        read_started.store(true, std::sync::atomic::Ordering::SeqCst);
        // The plane's runtime keeps running while this read is held.
        let _ = tokio::task::block_in_place(|| {
            gate.lock()
                .expect("gate")
                .recv_timeout(Duration::from_secs(60))
        });
        let reply = slow(seen);
        read_served.store(true, std::sync::atomic::Ordering::SeqCst);
        reply
    }));
    let id = ready(&mut desk).await;
    tool(
        &desk,
        Some("airs_desktop"),
        "inventory",
        json!({"resource": "apiKeys", "limit": 5}),
    );
    desk.say(&id, "Which API keys exist?").await.expect("sent");
    answered_with(&mut desk, |c| {
        entry(c, "tool").is_some_and(|t| t["state"] == "streaming")
            && c["approvals"].as_array().is_some_and(Vec::is_empty)
    })
    .await;
    // The read is under way (a request is recorded only once answered).
    assert!(settle(|| started.load(std::sync::atomic::Ordering::SeqCst)).await);
    desk.send(json!({"kind": "interruptHarness", "conversation": id}))
        .await
        .expect("interrupted");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready" && !c["lastTurn"].is_null()
    })
    .await;
    assert_eq!(c["lastTurn"], "interrupted", "{c}");
    // The read finishes after the turn ended: its result is never sent.
    release.send(()).expect("release");
    assert!(settle(|| served.load(std::sync::atomic::Ordering::SeqCst)).await);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(outputs(&desk).is_empty(), "{:?}", outputs(&desk));
    assert_ne!(
        entry(&desk.conversation(), "tool").expect("tool")["state"],
        "complete"
    );
    // The next turn answers normally, and still nothing of the stopped
    // call reaches the model.
    desk.gateway.lock().expect("gateway").hook = Some(fast);
    desk.gateway.lock().expect("gateway").answer = Answer::Short;
    desk.say(&id, "Never mind.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready"
            && c["lastTurn"] == "completed"
            && c["entries"]
                .as_array()
                .is_some_and(|e| e.iter().any(|e| e["id"] == "u-2"))
    })
    .await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert!(outputs(&desk).is_empty());
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tool_work_still_queued_when_its_turn_is_stopped_is_never_sent() {
    // C21 review R2-F1: the runtime's four slots are held by the operator's
    // own slow reads, so the turn's management read and scan wait in the
    // queue; the turn is stopped (or ended) and capacity released.
    let mut desk = desk().await;
    let fast = desk
        .gateway
        .lock()
        .expect("gateway")
        .hook
        .clone()
        .expect("hook");
    let slow = Arc::clone(&fast);
    // Delayed by the server, outside the routing lock: other requests are
    // answered meanwhile.
    desk.gateway.lock().expect("gateway").hook = Some(Arc::new(move |seen| {
        let target = seen.request_line.split(' ').nth(1).unwrap_or_default();
        let reply = slow(seen);
        if target.contains("/mgmt/topics/") {
            return reply.map(|r| crate::support::Reply::Delayed {
                millis: 4_000,
                then: Box::new(r),
            });
        }
        reply
    }));
    let context = desk.context();
    desk.send(
        json!({"kind": "configureScanCredential", "context": context, "mode": "keychain",
        "key": "synthetic-scan-key-c21"}),
    )
    .await
    .expect("scan key");
    let id = ready(&mut desk).await;
    for (call, stop) in [
        (
            ("inventory", json!({"resource": "apiKeys", "limit": 5})),
            "interruptHarness",
        ),
        (
            ("scan", json!({"profile": "Default", "prompt": "hello"})),
            "endHarness",
        ),
    ] {
        let id = if stop == "endHarness" {
            desk.end(&id).await;
            desk.send(json!({"kind": "clearHarness", "conversation": id}))
                .await
                .expect("cleared");
            ready(&mut desk).await
        } else {
            id.clone()
        };
        let context = desk.context();
        // Counted before the reads are sent: one can reach the server
        // before its send returns.
        let profiles = desk.requests("/aisec/").len();
        for _ in 0..4 {
            desk.send(json!({"kind": "management", "context": context,
                "request": {"kind": "page", "resource": "topics", "offset": 0, "limit": 25}}))
                .await
                .expect("operator read");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let before_keys = desk.requests("/aisec/v1/mgmt/apikeys").len();
        let before_scans = desk.requests("/v1/scan").len();
        tool(&desk, Some("airs_desktop"), call.0, call.1);
        desk.say(&id, "Use the desktop.").await.expect("sent");
        answered_with(&mut desk, |c| {
            entry(c, "tool").is_some_and(|t| t["state"] == "streaming")
        })
        .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        desk.send(json!({"kind": stop, "conversation": id}))
            .await
            .expect("stopped");
        // Capacity frees up: the queued work was cancelled, so nothing is
        // sent for it.
        tokio::time::sleep(Duration::from_secs(6)).await;
        assert_eq!(
            desk.requests("/aisec/v1/mgmt/apikeys").len(),
            before_keys,
            "{stop}"
        );
        assert_eq!(desk.requests("/v1/scan").len(), before_scans, "{stop}");
        assert!(outputs(&desk).is_empty(), "{stop}");
        // The operator's four slow reads did run (they held the slots).
        assert!(desk.requests("/aisec/").len() >= profiles + 4, "{stop}");
        if stop == "interruptHarness" {
            assert_eq!(desk.conversation()["lastTurn"], "interrupted");
        }
    }
    desk.gateway.lock().expect("gateway").hook = Some(fast);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn forged_unknown_or_credential_bearing_calls_run_nothing() {
    let mut desk = desk().await;
    let id = ready(&mut desk).await;
    let before = management_reads(&desk);
    let cases = [
        (Some("other"), "inventory", json!({"resource": "topics"})),
        (Some("airs_desktop"), "apply", json!({"review": "rv-1"})),
        (
            Some("airs_desktop"),
            "content",
            json!({"scanId": "1", "subRequest": 0}),
        ),
        (
            Some("airs_desktop"),
            "inventory",
            json!({"resource": "topics", "token": "x"}),
        ),
        (
            Some("airs_desktop"),
            "inventory",
            json!({"resource": "dlpCatalog"}),
        ),
        (
            Some("airs_desktop"),
            "proposeChange",
            json!({"kind": "topics", "action": "update", "summary": "s", "body": {"api_key": "k"}}),
        ),
    ];
    for (n, (namespace, name, arguments)) in cases.into_iter().enumerate() {
        tool(&desk, namespace, name, arguments);
        desk.say(&id, &format!("Try {n}")).await.expect("sent");
        answered_with(&mut desk, |c| {
            c["phase"] == "ready" && c["entries"].as_array().map(Vec::len) == Some(2 * (n + 1))
        })
        .await;
        // Refused by the desktop, or (another namespace) by the engine
        // before the desktop is asked.
        let out = outputs(&desk);
        let last = out.last().expect("an output");
        assert!(
            last.contains("This tool or its arguments are unavailable.")
                || last.contains("unsupported call"),
            "{name}: {last}"
        );
    }
    assert_eq!(management_reads(&desk), before, "nothing was read");
    assert!(
        desk.conversation()["proposals"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_proposal_is_staged_never_applied_and_never_replaces_a_review() {
    let mut desk = desk().await;
    let id = ready(&mut desk).await;
    tool(
        &desk,
        Some("airs_desktop"),
        "proposeChange",
        json!({"kind": "topics", "action": "update", "target": "11111111-1111-4111-8111-111111111111",
            "body": {"topic_name": "Finance", "description": "Money talk, reviewed"},
            "summary": "Clarify the Finance topic"}),
    );
    desk.say(&id, "Propose a clearer Finance topic.")
        .await
        .expect("sent");
    let c = answered_with(&mut desk, |c| c["lastTurn"] == "completed").await;
    assert_eq!(c["proposals"][0]["state"], "staged", "{c}");
    assert_eq!(c["proposals"][0]["summary"], "Clarify the Finance topic");
    let out = outputs(&desk);
    assert!(out[0].contains("has not been applied"), "{}", out[0]);
    // Nothing was written: the tool never reaches the service's writes.
    assert!(
        desk.requests("/aisec/v1/mgmt/topic")
            .iter()
            .all(|s| s.request_line.starts_with("GET"))
    );
    // Opened into the change review, by the operator.
    let review = desk
        .send(json!({"kind": "reviewHarnessProposal", "conversation": id, "proposal": "P-1"}))
        .await;
    assert!(review.is_ok(), "{review:?}");
    assert!(settle(|| desk.conversation()["proposals"][0]["state"] == "inReview").await);
    assert!(
        settle(|| !desk.json()["changes"]["review"].is_null()).await,
        "{}",
        desk.json()["changes"]
    );
    // A second proposal cannot replace the open review.
    desk.say(&id, "Propose it again.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["proposals"].as_array().map(Vec::len) == Some(2) && c["phase"] == "ready"
    })
    .await;
    let again = desk
        .send(json!({"kind": "reviewHarnessProposal", "conversation": id, "proposal": "P-2"}))
        .await;
    assert!(
        matches!(again, Err(desktop_core::Refusal::Busy)),
        "{again:?}"
    );
    // Once the operator discards that review, the proposal reads closed and
    // the next one may be opened.
    let opened = desk.json()["changes"]["review"]["review"].clone();
    desk.send(json!({"kind": "discardChange", "review": opened}))
        .await
        .expect("discarded");
    assert!(settle(|| desk.conversation()["proposals"][0]["state"] == "closed").await);
    assert_eq!(desk.conversation()["proposals"][0]["review"], opened);
    let next = desk
        .send(json!({"kind": "reviewHarnessProposal", "conversation": id, "proposal": "P-2"}))
        .await;
    assert!(next.is_ok(), "{next:?}");
    assert!(settle(|| desk.conversation()["proposals"][1]["state"] == "inReview").await);
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn commands_need_a_project_and_the_operators_approval() {
    let mut desk = desk().await;
    // Without a project, the engine's request is declined at once.
    let id = ready(&mut desk).await;
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["lastTurn"].is_string() && c["phase"] == "ready"
    })
    .await;
    assert!(c["approvals"].as_array().is_some_and(Vec::is_empty));
    desk.end(&id).await;
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");

    // With a project, the operator decides.
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("ref")
        .to_owned();
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    assert_eq!(desk.conversation()["project"], dir.to_str().expect("utf8"));
    desk.say(&id, "List the files.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let approval = c["approvals"][0].clone();
    assert_eq!(approval["kind"], "command", "{approval}");
    assert!(
        approval["command"]
            .as_str()
            .is_some_and(|t| t.contains("ls")),
        "{approval}"
    );
    let token = approval["id"].as_str().expect("id").to_owned();
    // A forged id is refused.
    assert!(desk
        .send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": "forged", "allow": true}))
        .await
        .is_err());
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
        .await
        .expect("decided");
    let c = answered_with(&mut desk, |c| c["lastTurn"] == "completed").await;
    let command = entry(&c, "command").expect("command entry");
    assert!(
        command["text"]
            .as_str()
            .is_some_and(|t| t.contains("file-0.txt")),
        "{command}"
    );
    // The same id is stale once used, and after the turn.
    assert!(desk
        .send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
        .await
        .is_err());
    // Declined: nothing runs.
    desk.say(&id, "List them again.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let token = c["approvals"][0]["id"].as_str().expect("id").to_owned();
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": false}))
        .await
        .expect("decided");
    let c = answered_with(&mut desk, |c| {
        c["lastTurn"].is_string()
            && c["phase"] == "ready"
            && c["entries"].as_array().map(Vec::len).unwrap_or(0) >= 6
    })
    .await;
    let declined = entry(&c, "command").expect("command entry");
    assert_ne!(declined["state"], "complete", "{declined}");
    desk.end(&id).await;
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sandbox_refuses_children_pipelines_and_other_paths() {
    let mut desk = desk().await;
    let dir = project();
    let outside = project();
    std::fs::write(outside.join("secret.txt"), "OUTSIDE-CANARY").expect("fixture");
    let reference = desk.choose(WS_A).await;
    choose_project(&mut desk, &dir).await;
    desk.store_key(&reference, KEY, None).await;
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    let commands = [
        format!("cat {}", outside.join("secret.txt").display()),
        "sh -c 'sleep 3141 & echo started'".to_owned(),
        "ls | wc -l".to_owned(),
    ];
    for (n, cmd) in commands.iter().enumerate() {
        tool(&desk, None, "exec_command", json!({"cmd": cmd}));
        desk.say(&id, &format!("Run {n}")).await.expect("sent");
        // Approved (the operator's approval never widens the sandbox).
        let c = answered_with(&mut desk, |c| {
            c["approvals"].as_array().is_some_and(|a| !a.is_empty())
                || (c["phase"] == "ready" && c["lastTurn"].is_string())
        })
        .await;
        if let Some(token) = c["approvals"][0]["id"].as_str() {
            let token = token.to_owned();
            desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
                .await
                .expect("decided");
        }
        answered_with(&mut desk, |c| {
            c["phase"] == "ready" && c["approvals"].as_array().is_some_and(Vec::is_empty)
        })
        .await;
    }
    // The engine's sandbox refused each, with the operator's approval.
    let out = outputs(&desk);
    assert_eq!(out.len(), 3, "{out:?}");
    assert!(out[0].contains("Operation not permitted"), "{}", out[0]);
    assert!(out[1].contains("fork"), "{}", out[1]);
    assert!(out[2].contains("fork"), "{}", out[2]);
    let text = desk.json().to_string();
    assert!(
        !text.contains("OUTSIDE-CANARY"),
        "a path outside the project was read"
    );
    assert!(!outputs(&desk).iter().any(|o| o.contains("OUTSIDE-CANARY")));
    let pid = desk.last_pid().expect("pid");
    desk.end(&id).await;
    assert!(gone(pid).await);
    // No sleeper outlived the conversation (killed with the engine's
    // process group; given a moment to exit).
    let sleeper = || {
        let left = std::process::Command::new("/usr/bin/pgrep")
            .args(["-xf", "sleep 3141"])
            .output()
            .expect("pgrep");
        String::from_utf8_lossy(&left.stdout).trim().is_empty()
    };
    let listed = || {
        String::from_utf8_lossy(
            &std::process::Command::new("/usr/bin/pgrep")
                .args(["-lxf", "sleep 3141"])
                .output()
                .expect("pgrep")
                .stdout,
        )
        .into_owned()
    };
    assert!(
        settle_for(5, sleeper).await,
        "a descendant survived: {}",
        listed()
    );
    eprintln!("sandbox outputs: {:?}", outputs(&desk));
    std::fs::remove_dir_all(&dir).expect("cleanup");
    std::fs::remove_dir_all(&outside).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_project_and_its_access_are_the_binding_and_unsafe_folders_are_refused() {
    let mut desk = desk().await;
    let dir = project();
    let reference = desk.choose(WS_A).await;
    choose_project(&mut desk, &dir).await;
    desk.store_key(&reference, KEY, None).await;
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    assert_eq!(desk.conversation()["write"], false);
    // Read-only: even an approved command cannot change the project.
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": "echo kept > kept.txt && echo WROTE"}),
    );
    desk.say(&id, "Write a file.").await.expect("sent");
    let c = approve(&mut desk, &id, "command").await;
    assert!(!dir.join("kept.txt").exists(), "{c}");
    assert!(!outputs(&desk).last().expect("output").contains("WROTE"));
    // A file change is declined at once: the operator is never asked.
    let patch = "*** Begin Patch\n*** Add File: added.txt\n+added by the engine\n*** End Patch";
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": format!("apply_patch <<'EOF'\n{patch}\nEOF\n")}),
    );
    desk.say(&id, "Add a file.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready"
            && c["approvals"].as_array().is_some_and(Vec::is_empty)
            && c["entries"].as_array().map(Vec::len).unwrap_or(0) >= 6
    })
    .await;
    assert_eq!(
        entry(&c, "fileChange").expect("shown")["state"],
        "declined",
        "{c}"
    );
    assert!(!dir.join("added.txt").exists());
    let pid = desk.last_pid().expect("pid");
    // Allowing changes is a new binding: the conversation is retired.
    let context = desk.context();
    desk.send(json!({"kind": "setHarnessWrite", "context": context, "write": true}))
        .await
        .expect("write");
    assert!(settle(|| desk.conversation().is_null()).await);
    assert!(gone(pid).await);
    // The retired conversation finishes ending (its engine reaped, its
    // state settled) just after it leaves the view; until then a start is
    // refused as busy.
    let id = start_when_free(&mut desk, &reference).await;
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    assert_eq!(desk.conversation()["write"], true);
    // With it, an approved command changes the project, and only it.
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": "echo kept > kept.txt && echo WROTE"}),
    );
    desk.say(&id, "Write a file.").await.expect("sent");
    approve(&mut desk, &id, "command").await;
    assert_eq!(
        std::fs::read_to_string(dir.join("kept.txt"))
            .ok()
            .as_deref(),
        Some("kept\n")
    );
    // The pinned engine asks before its own patch writer. An approval that
    // names the file is shown whole and, approved, the writer fails in its
    // narrower helper sandbox; one asked before the file is named is
    // declined at once and never shown (C21 review F3). Which comes first
    // varies between runs; neither writes.
    let patch = "*** Begin Patch\n*** Add File: added.txt\n+added by the engine\n*** End Patch";
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": format!("apply_patch <<'EOF'\n{patch}\nEOF\n")}),
    );
    desk.say(&id, "Add a file.").await.expect("sent");
    let asked = answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
            || (c["phase"] == "ready" && entry(c, "fileChange").is_some())
    })
    .await;
    let c = if asked["approvals"].as_array().is_some_and(|a| !a.is_empty()) {
        let named = dir.join("added.txt");
        assert_eq!(
            asked["approvals"][0]["files"],
            json!([named.to_str().expect("utf8")]),
            "{asked}"
        );
        let token = asked["approvals"][0]["id"].clone();
        desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
            .await
            .expect("decided");
        answered_with(&mut desk, |c| {
            c["phase"] == "ready"
                && c["approvals"].as_array().is_some_and(Vec::is_empty)
                && entry(c, "assistant")
                    .is_some_and(|e| e["id"].as_str().is_some_and(|i| i.starts_with("u-2/")))
        })
        .await
    } else {
        asked
    };
    assert_eq!(
        entry(&c, "fileChange").expect("shown")["state"],
        "failed",
        "{c}"
    );
    assert!(!dir.join("added.txt").exists());
    desk.end(&id).await;
    // Unsafe folders are never a project.
    let home = PathBuf::from(std::env::var("HOME").expect("home"));
    for refused in [
        home.clone(),
        PathBuf::from("/"),
        PathBuf::from("/usr"),
        desk.runtime_root.clone(),
    ] {
        if !refused.exists() {
            continue;
        }
        desk.adapters.folders.reply(Some(refused.clone()));
        let context = desk.context();
        desk.send(json!({"kind": "chooseHarnessProject", "context": context}))
            .await
            .expect("choose");
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(
            desk.json()["harness"]["project"]["path"],
            dir.to_str().expect("utf8"),
            "{}",
            refused.display()
        );
        // Named in the view, since the sheet has closed.
        assert_eq!(
            desk.json()["harness"]["projectRefused"],
            refused.display().to_string()
        );
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

async fn history_open(desk: &mut Desk, id: &str) -> Value {
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(json!({"kind": "listHarnessHistory", "context": context, "workspace": workspace}))
        .await
        .expect("list");
    assert!(settle(|| desk.json()["harness"]["history"]["state"] == "ready").await);
    let context = desk.context();
    desk.send(
        json!({"kind": "openHarnessHistory", "context": context, "workspace": workspace, "id": id}),
    )
    .await
    .expect("open");
    assert!(settle(|| desk.json()["harness"]["history"]["opened"]["id"] == id).await);
    desk.json()["harness"]["history"]["opened"].clone()
}

async fn set_write(desk: &mut Desk, write: bool) {
    let context = desk.context();
    desk.send(json!({"kind": "setHarnessWrite", "context": context, "write": write}))
        .await
        .expect("write");
    assert!(settle(|| desk.json()["harness"]["project"]["write"] == write).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_saved_conversation_needs_its_project_binding_and_replays_no_authority() {
    let mut desk = desk().await;
    let dir = project();
    let reference = desk.choose(WS_A).await;
    choose_project(&mut desk, &dir).await;
    set_write(&mut desk, true).await;
    desk.store_key(&reference, KEY, None).await;
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    // An allowed command writes once.
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": "echo once >> once.txt && echo WROTE"}),
    );
    desk.say(&id, "Write once.").await.expect("sent");
    approve(&mut desk, &id, "command").await;
    assert_eq!(
        std::fs::read_to_string(dir.join("once.txt"))
            .ok()
            .as_deref(),
        Some("once\n")
    );
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("id")
        .to_owned();
    desk.end(&id).await;
    // With other access it cannot be continued: a new conversation is needed.
    set_write(&mut desk, false).await;
    let opened = history_open(&mut desk, &history).await;
    assert_eq!(opened["notContinuable"], "otherProject", "{opened}");
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    assert!(
        desk.send(json!({"kind": "resumeHarness", "context": context, "workspace": workspace, "id": history}))
            .await
            .is_err()
    );
    // The same access again: it continues, and restoring runs nothing.
    set_write(&mut desk, true).await;
    let opened = history_open(&mut desk, &history).await;
    assert!(opened["notContinuable"].is_null(), "{opened}");
    let before = desk.requests(&format!("{RUNTIME}/responses")).len();
    let context = desk.context();
    desk.send(
        json!({"kind": "resumeHarness", "context": context, "workspace": workspace, "id": history}),
    )
    .await
    .expect("resumed");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let c = desk.conversation();
    assert!(c["approvals"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert_eq!(c["project"], dir.to_str().expect("utf8"));
    assert_eq!(desk.requests(&format!("{RUNTIME}/responses")).len(), before);
    assert_eq!(
        std::fs::read_to_string(dir.join("once.txt"))
            .ok()
            .as_deref(),
        Some("once\n")
    );
    // The next turn carries the earlier one as context, and re-runs nothing.
    desk.gateway.lock().expect("gateway").answer = Answer::Short;
    let id = c["id"].as_str().expect("id").to_owned();
    desk.say(&id, "And now?").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["phase"] == "ready" && c["lastTurn"].is_string()
    })
    .await;
    let bodies = desk.requests(&format!("{RUNTIME}/responses"));
    let last = String::from_utf8_lossy(&bodies.last().expect("request").body).to_string();
    assert!(
        last.contains("Write once.") && last.contains("And now?"),
        "{last}"
    );
    assert!(
        desk.conversation()["approvals"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("once.txt"))
            .ok()
            .as_deref(),
        Some("once\n")
    );
    desk.end(&id).await;
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_pending_survives_going_offline_and_nothing_is_resubmitted_on_reconnect() {
    // T23 for C21: a staged proposal opened in review and a command waiting
    // for its approval, then the app relaunched offline and reconnected.
    let mut desk = desk().await;
    let dir = project();
    let reference = desk.choose(WS_A).await;
    choose_project(&mut desk, &dir).await;
    set_write(&mut desk, true).await;
    desk.store_key(&reference, KEY, None).await;
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    tool(
        &desk,
        Some("airs_desktop"),
        "proposeChange",
        json!({"kind": "topics", "action": "update", "target": "11111111-1111-4111-8111-111111111111",
            "body": {"topic_name": "Finance", "description": "Money talk, reviewed"},
            "summary": "Clarify the Finance topic"}),
    );
    desk.say(&id, "Propose a clearer Finance topic.")
        .await
        .expect("sent");
    answered_with(&mut desk, |c| c["lastTurn"] == "completed").await;
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("id")
        .to_owned();
    desk.send(json!({"kind": "reviewHarnessProposal", "conversation": id, "proposal": "P-1"}))
        .await
        .expect("review");
    assert!(settle(|| !desk.json()["changes"]["review"].is_null()).await);
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": "echo pending > pending.txt && echo WROTE"}),
    );
    desk.say(&id, "Write a file.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let answers = desk.requests(&format!("{RUNTIME}/responses")).len();
    let launches = desk.launcher.launches().len();

    // Offline: the conversation, its approval, proposal and review are
    // gone; the project and this Mac's history remain readable.
    let mut desk = relaunch(desk, false, true).await;
    let shell = desk.json();
    assert!(shell["harness"]["conversation"].is_null(), "{shell}");
    assert!(shell["changes"]["review"].is_null(), "{}", shell["changes"]);
    assert_eq!(
        shell["harness"]["project"]["path"],
        dir.to_str().expect("utf8")
    );
    assert_eq!(shell["harness"]["project"]["write"], true);
    let context = desk.context();
    desk.send(json!({"kind": "listHarnessHistory", "context": context, "workspace": null}))
        .await
        .expect("listed");
    assert!(settle(|| desk.json()["harness"]["history"]["state"] == "ready").await);
    assert_eq!(
        desk.json()["harness"]["history"]["conversations"][0]["id"],
        history.as_str()
    );
    assert!(!dir.join("pending.txt").exists());

    // Reconnected: nothing is resubmitted; continuing is the operator's
    // explicit choice and restores without running anything.
    let mut desk = relaunch(desk, true, true).await;
    let reference = desk.choose(WS_A).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(desk.json()["harness"]["conversation"].is_null());
    assert_eq!(
        desk.requests(&format!("{RUNTIME}/responses")).len(),
        answers
    );
    assert_eq!(management_writes(&desk), 0);
    assert_eq!(desk.launcher.launches().len(), launches);
    let opened = history_open(&mut desk, &history).await;
    assert!(opened["notContinuable"].is_null(), "{opened}");
    let context = desk.context();
    desk.send(
        json!({"kind": "resumeHarness", "context": context, "workspace": reference, "id": history}),
    )
    .await
    .expect("resumed");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let c = desk.conversation();
    assert!(c["approvals"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert!(c["proposals"].as_array().is_some_and(Vec::is_empty), "{c}");
    assert_eq!(
        desk.requests(&format!("{RUNTIME}/responses")).len(),
        answers
    );
    assert_eq!(management_writes(&desk), 0);
    assert!(!dir.join("pending.txt").exists());
    let id = c["id"].as_str().expect("id").to_owned();
    desk.end(&id).await;
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// Resolve exactly the refusal the production interface just published.
async fn reviewed_switch(desk: &mut Desk, mut intent: Value, kind: &str) -> Value {
    assert_eq!(
        desk.send(intent.clone()).await,
        Err(desktop_core::Refusal::DecisionPending)
    );
    let guard = desk.json()["switchBlocked"].clone();
    assert!(guard["review"].as_str().is_some(), "native reference");
    intent["resolution"] =
        json!({"kind": kind, "context": guard["context"], "review": guard["review"]});
    intent
}

/// A real-engine pending command after a protected transcript was committed.
async fn saved_pending_switch_conversation(
    desk: &mut Desk,
    reference: &str,
) -> (Value, Vec<(PathBuf, Vec<u8>)>) {
    let id = start_when_free(desk, reference).await;
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    desk.gateway.lock().expect("gateway").answer = Answer::Short;
    desk.say(&id, "Say hello.").await.expect("sent");
    desk.answered().await;
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("saved history")
        .to_owned();
    tool(desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.")
        .await
        .expect("pending command");
    let pending = answered_with(desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let protected: Vec<_> = files_under(&desk.history_root)
        .into_iter()
        .filter(|p| p.to_string_lossy().contains(&history))
        .map(|p| {
            let bytes = std::fs::read(&p).expect("protected history");
            (p, bytes)
        })
        .collect();
    assert!(
        !protected.is_empty(),
        "a real protected transcript exists before the stale action"
    );
    (pending, protected)
}

fn pending_switch_history_is_unchanged(
    desk: &Desk,
    before: &Value,
    protected: &[(PathBuf, Vec<u8>)],
) {
    let current = desk.conversation();
    assert_eq!(current["id"], before["id"]);
    assert_eq!(current["phase"], before["phase"]);
    assert_eq!(current["approvals"], before["approvals"]);
    assert!(
        protected
            .iter()
            .all(|(p, bytes)| std::fs::read(p).is_ok_and(|now| now == *bytes)),
        "stale resolution neither removes nor changes protected history"
    );
}

async fn stale_switch_resolution_is_bound(kind: &str) {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    let _ = saved_pending_switch_conversation(&mut desk, &reference).await;
    let a = desk.json()["active"]["tenant"]
        .as_str()
        .expect("A")
        .to_owned();
    let origin = desk.json()["tenants"][0]["origin"].clone();
    desk.send(json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG, "destination": {"kind": "custom", "origin": origin}}})).await.expect("B");
    let b = desk.json()["tenants"]
        .as_array()
        .expect("tenants")
        .iter()
        .find(|t| t["label"] == "Tenant B")
        .expect("B")["id"]
        .as_str()
        .expect("B id")
        .to_owned();
    assert!(
        serde_json::from_value::<desktop_core::Intent>(
            json!({"kind": "selectTenant", "tenant": b, "resolution": kind})
        )
        .is_err(),
        "an unbound legacy resolution is not admitted by the protocol"
    );
    let lost = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": b}),
        kind,
    )
    .await;
    // The native action succeeds, but the renderer never receives its receipt
    // and later retries the old intent with a new delivery sequence.
    desk.send(lost.clone())
        .await
        .expect("first action admitted, receipt lost");
    assert_eq!(desk.json()["active"]["tenant"], b);
    let context = desk.context();
    desk.send(json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain", "clientId": CLIENT_ID, "secret": CLIENT_SECRET})).await.expect("B synthetic management key");
    assert!(
        settle(|| desk.json()["tenants"]
            .as_array()
            .expect("tenants")
            .iter()
            .find(|t| t["id"] == b)
            .expect("B")["management"]["state"]
            == "ready")
        .await
    );
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    assert!(
        settle(|| desk.json()["tenants"]
            .as_array()
            .expect("tenants")
            .iter()
            .find(|t| t["id"] == b)
            .expect("B")["inference"]["state"]
            == "ready")
        .await
    );
    choose_project(&mut desk, &dir).await;
    let (pending, protected) = saved_pending_switch_conversation(&mut desk, &reference).await;
    // Exact exploit: the old A→B resolution must not touch B's new pending
    // conversation, including when B is already the selected tenant.
    assert_eq!(
        desk.send(lost.clone()).await,
        Err(desktop_core::Refusal::StaleContext)
    );
    pending_switch_history_is_unchanged(&desk, &pending, &protected);
    let mut transplanted = lost;
    transplanted["tenant"] = json!(a);
    assert_eq!(
        desk.send(transplanted.clone()).await,
        Err(desktop_core::Refusal::StaleContext)
    );
    let old_guard = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": a}),
        kind,
    )
    .await;
    transplanted["resolution"]["context"] = json!(desk.context());
    assert_eq!(
        desk.send(transplanted).await,
        Err(desktop_core::Refusal::StaleContext),
        "a source-context edit cannot transplant an old native review reference"
    );
    let dismissed_guard = old_guard.clone();
    desk.send(json!({"kind": "dismissSwitchGuard"}))
        .await
        .expect("Stay");
    assert_eq!(
        desk.send(old_guard.clone()).await,
        Err(desktop_core::Refusal::StaleContext),
        "Stay retires this refusal"
    );
    let old_guard = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": a}),
        kind,
    )
    .await;
    assert_ne!(
        dismissed_guard["resolution"]["review"],
        old_guard["resolution"]["review"]
    );
    assert_eq!(
        desk.send(dismissed_guard).await,
        Err(desktop_core::Refusal::StaleContext),
        "republishing the same pending decision does not revive a dismissed refusal"
    );
    pending_switch_history_is_unchanged(&desk, &pending, &protected);
    // A replacement conversation can use the SAME tenant context and target.
    // Its distinct pending decision still must not inherit the prior action.
    desk.end(pending["id"].as_str().expect("conversation"))
        .await;
    let unchanged_context = desk.context();
    let (replacement, protected) = saved_pending_switch_conversation(&mut desk, &reference).await;
    assert_eq!(desk.context(), unchanged_context);
    assert_ne!(replacement["id"], pending["id"]);
    assert_eq!(
        desk.send(old_guard.clone()).await,
        Err(desktop_core::Refusal::StaleContext)
    );
    let fresh = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": a}),
        kind,
    )
    .await;
    assert_eq!(
        desk.send(old_guard).await,
        Err(desktop_core::Refusal::StaleContext),
        "a new refusal never revives an old resolution"
    );
    pending_switch_history_is_unchanged(&desk, &replacement, &protected);
    desk.send(fresh)
        .await
        .expect("fresh resolution remains usable");
    assert_eq!(desk.json()["active"]["tenant"], a);
    desk.core.shutdown();
    std::fs::remove_dir_all(dir).expect("synthetic project cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_archive_resolution_never_ends_another_pending_conversation() {
    stale_switch_resolution_is_bound("archive").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stale_discard_resolution_never_deletes_another_pending_conversation() {
    stale_switch_resolution_is_bound("discard").await;
}

/// C30 (I1): the one tenant-switch path refuses while the operator has a
/// Harness decision pending; the refusal carries the guard, and the
/// operator's resolution ends the conversation (kept or dropped) and switches.
#[tokio::test(flavor = "multi_thread")]
async fn a_pending_decision_refuses_a_tenant_switch_until_it_is_resolved() {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let id = ready(&mut desk).await;
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let token = c["approvals"][0]["id"].as_str().expect("id").to_owned();
    // Another tenant to switch to.
    let a = desk.json()["active"]["tenant"]
        .as_str()
        .expect("A")
        .to_owned();
    let origin = desk.json()["tenants"][0]["origin"]
        .as_str()
        .expect("origin")
        .to_owned();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": "1234567890",
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("B");
    let b = desk.json()["tenants"]
        .as_array()
        .expect("tenants")
        .iter()
        .find(|t| t["label"] == "Tenant B")
        .and_then(|t| t["id"].as_str())
        .expect("B id")
        .to_owned();
    // Without a resolution the switch is refused and the guard is published.
    assert_eq!(
        desk.send(json!({"kind": "selectTenant", "tenant": b}))
            .await,
        Err(desktop_core::Refusal::DecisionPending)
    );
    let s = desk.json();
    assert_eq!(s["active"]["tenant"], a, "still on A");
    assert_eq!(s["switchBlocked"]["to"], b);
    assert_eq!(s["switchBlocked"]["pending"]["conversation"], id);
    assert_eq!(s["switchBlocked"]["pending"]["decision"], token);
    assert!(
        s["harness"]["conversation"]["approvals"]
            .as_array()
            .is_some_and(|x| !x.is_empty()),
        "nothing was decided for the operator"
    );
    // Stay: the guard closes, nothing else changes.
    desk.send(json!({"kind": "dismissSwitchGuard"}))
        .await
        .expect("dismissed");
    assert!(desk.json()["switchBlocked"].is_null());
    assert_eq!(desk.json()["active"]["tenant"], a);
    // Archive and switch: the conversation is ended (kept) and B is active.
    let resolved = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": b}),
        "archive",
    )
    .await;
    desk.send(resolved).await.expect("switched");
    let s = desk.json();
    assert_eq!(s["active"]["tenant"], b);
    assert!(s["switchBlocked"].is_null());
    assert!(
        s["harness"]["conversation"].is_null(),
        "another tenant's conversation is never shown"
    );
    // A plain switch with nothing pending needs no resolution.
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    assert!(
        settle_for(30, || {
            let c = desk.conversation();
            c.is_null() || c["phase"] == "ended" || c["phase"] == "ending"
        })
        .await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn discard_and_switch_drops_the_pending_conversation() {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let id = ready(&mut desk).await;
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let a = desk.json()["active"]["tenant"]
        .as_str()
        .expect("A")
        .to_owned();
    let origin = desk.json()["tenants"][0]["origin"]
        .as_str()
        .expect("origin")
        .to_owned();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": "1234567890",
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("B");
    let b = desk.json()["tenants"]
        .as_array()
        .expect("tenants")
        .iter()
        .find(|t| t["label"] == "Tenant B")
        .and_then(|t| t["id"].as_str())
        .expect("B id")
        .to_owned();
    let resolved = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": b}),
        "discard",
    )
    .await;
    desk.send(resolved).await.expect("switched");
    assert_eq!(desk.json()["active"]["tenant"], b);
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    assert!(
        settle_for(30, || desk.conversation().is_null()).await,
        "dropped once its task returned"
    );
}

/// The saved conversations of the active tenant's last chosen workspace,
/// read from this Mac, by id.
async fn history_ids(desk: &mut Desk) -> Vec<String> {
    let context = desk.context();
    // The chosen workspace's history when one is chosen under this context,
    // else the tenant's last chosen workspace from this Mac.
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(json!({"kind": "listHarnessHistory", "context": context, "workspace": workspace}))
        .await
        .expect("list");
    assert!(
        settle(|| desk.json()["harness"]["history"]["state"] == "ready").await,
        "{}",
        desk.json()["harness"]["history"]
    );
    desk.json()["harness"]["history"]["conversations"]
        .as_array()
        .expect("conversations")
        .iter()
        .filter_map(|c| c["id"].as_str().map(str::to_owned))
        .collect()
}

/// C30 (I1, P35): "Discard and switch" drops the transcript as the dialog
/// promises: the record its earlier turn saved is removed, the turn the
/// stop cuts short is never saved, and the conversation is absent from the
/// workspace's history when the operator comes back; "Archive and switch"
/// keeps it readable.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn discard_removes_the_saved_transcript_and_archive_keeps_it() {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let id = ready(&mut desk).await;
    // A first turn that completes is saved; then a turn waits for approval.
    desk.say(&id, "Say hello.").await.expect("sent");
    desk.answered().await;
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("history id")
        .to_owned();
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let a = desk.json()["active"]["tenant"]
        .as_str()
        .expect("A")
        .to_owned();
    let origin = desk.json()["tenants"][0]["origin"]
        .as_str()
        .expect("origin")
        .to_owned();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": "1234567890",
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("B");
    let b = desk.json()["tenants"]
        .as_array()
        .expect("tenants")
        .iter()
        .find(|t| t["label"] == "Tenant B")
        .and_then(|t| t["id"].as_str())
        .expect("B id")
        .to_owned();
    let resolved = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": b}),
        "discard",
    )
    .await;
    desk.send(resolved).await.expect("switched");
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    assert!(
        settle_for(30, || desk.conversation().is_null()).await,
        "dropped once its task returned (its cut-short save refused)"
    );
    let ids = history_ids(&mut desk).await;
    assert!(
        !ids.contains(&history),
        "the discarded conversation is gone from the saved history: {ids:?}"
    );
    assert!(
        !files_under(&desk.history_root)
            .iter()
            .any(|f| f.to_string_lossy().contains(&history)),
        "and its record file was removed"
    );
    // The same with Archive: the conversation stays readable. (The dropped
    // conversation's engine may still be reaped: a start waits for it.)
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    let id = start_when_free(&mut desk, &reference).await;
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    // (The synthetic model still proposes the command.) The turn the stop
    // cuts short is saved as it stood: that is what Archive keeps.
    desk.say(&id, "List the files.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let kept = desk.conversation()["historyId"]
        .as_str()
        .expect("history id")
        .to_owned();
    let resolved = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": b}),
        "archive",
    )
    .await;
    desk.send(resolved).await.expect("switched");
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    assert!(
        settle_for(30, || {
            let c = desk.conversation();
            c.is_null() || c["phase"] == "ended"
        })
        .await
    );
    // Its cut-short save lands after the stop: listed once it has.
    let mut ids = history_ids(&mut desk).await;
    for _ in 0..60 {
        if ids.contains(&kept) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        ids = history_ids(&mut desk).await;
    }
    assert!(ids.contains(&kept), "archived, it is listed: {ids:?}");
    assert!(!ids.contains(&history), "the discarded one stays gone");
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// C30 (P35): when a discarded conversation's record cannot be removed
/// (here: its folder made read-only), the discard is not implied to have
/// succeeded: the record stays listed, the workspace's history reports the
/// failure after switching back, and the operator's own removal, once
/// possible, is the recovery that clears it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_discard_whose_removal_fails_is_reported_until_the_record_is_removed() {
    use std::os::unix::fs::PermissionsExt;
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let id = ready(&mut desk).await;
    desk.say(&id, "Say hello.").await.expect("sent");
    desk.answered().await;
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("history id")
        .to_owned();
    let record = files_under(&desk.history_root)
        .into_iter()
        .find(|f| f.to_string_lossy().contains(&history))
        .unwrap_or_else(|| {
            panic!(
                "no record for {history} under {:?}: {:?}",
                desk.history_root,
                files_under(&desk.history_root)
            )
        });
    let folder = record.parent().expect("scope folder").to_path_buf();
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let a = desk.json()["active"]["tenant"]
        .as_str()
        .expect("A")
        .to_owned();
    let origin = desk.json()["tenants"][0]["origin"]
        .as_str()
        .expect("origin")
        .to_owned();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": "1234567890",
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("B");
    let b = desk.json()["tenants"]
        .as_array()
        .expect("tenants")
        .iter()
        .find(|t| t["label"] == "Tenant B")
        .and_then(|t| t["id"].as_str())
        .expect("B id")
        .to_owned();
    // Removal cannot happen: the folder refuses it.
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).expect("chmod");
    let resolved = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": b}),
        "discard",
    )
    .await;
    desk.send(resolved).await.expect("switched");
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    assert!(settle_for(30, || desk.conversation().is_null()).await);
    let mut ids = history_ids(&mut desk).await;
    for _ in 0..40 {
        if desk.json()["harness"]["history"]["problem"] == "unavailable" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        ids = history_ids(&mut desk).await;
    }
    assert!(
        ids.contains(&history),
        "the record is still there and listed: {ids:?}"
    );
    assert_eq!(
        desk.json()["harness"]["history"]["problem"],
        "unavailable",
        "the failed removal is reported: {}",
        desk.json()["harness"]["history"]
    );
    assert!(record.exists());
    // Unrelated history work leaves the unresolved failure reported (P35):
    // opening the record and deleting another saved conversation.
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    let other_id = start_when_free(&mut desk, &reference).await;
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    // (The synthetic model still proposes the command: the ended turn's
    // cut-short save is the other record.)
    desk.say(&other_id, "List the files.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let other = desk.conversation()["historyId"]
        .as_str()
        .expect("history id")
        .to_owned();
    desk.end(&other_id).await;
    let mut ids = history_ids(&mut desk).await;
    for _ in 0..60 {
        if ids.contains(&other) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        ids = history_ids(&mut desk).await;
    }
    assert!(ids.contains(&other), "the other record is saved: {ids:?}");
    let opened = history_open(&mut desk, &history).await;
    assert_eq!(opened["id"], history, "readable");
    assert_eq!(
        desk.json()["harness"]["history"]["problem"],
        "unavailable",
        "still reported after an open"
    );
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(json!({"kind": "closeHarnessHistory", "context": context, "workspace": workspace}))
        .await
        .expect("close");
    let mut removed_other = Err(desktop_core::Refusal::Busy);
    for _ in 0..60 {
        let context = desk.context();
        let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
        removed_other = desk
            .send(json!({"kind": "deleteHarnessHistory", "context": context, "workspace": workspace, "id": other}))
            .await;
        if removed_other != Err(desktop_core::Refusal::Busy) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    removed_other.expect("delete other");
    assert!(settle(|| desk.json()["harness"]["history"]["busy"] == false).await);
    assert_eq!(
        desk.json()["harness"]["history"]["problem"],
        "unavailable",
        "still reported after deleting another record"
    );
    // Recovery: the operator removes the failed record itself.
    // (Its engine may still be being reaped: a delete of a live id is busy.)
    let mut deleted = Err(desktop_core::Refusal::Busy);
    for _ in 0..60 {
        let context = desk.context();
        let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
        deleted = desk
            .send(json!({"kind": "deleteHarnessHistory", "context": context, "workspace": workspace, "id": history}))
            .await;
        if deleted != Err(desktop_core::Refusal::Busy) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    deleted.expect("delete");
    assert!(settle(|| desk.json()["harness"]["history"]["busy"] == false).await);
    let ids = history_ids(&mut desk).await;
    assert!(!ids.contains(&history), "removed: {ids:?}");
    assert!(
        desk.json()["harness"]["history"]["problem"].is_null(),
        "{}",
        desk.json()["harness"]["history"]
    );
    assert!(!record.exists());
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// C30 (P35): a save admitted before a discard, held between its
/// admission and its write (the engine ended with the approval pending, its
/// interrupted turn being saved), cannot write the discarded transcript
/// after the discard removed it: the write re-checks the tombstone under the
/// history hold. The record stays absent after the save settles.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
// The gate is held across the awaits on purpose: that is the race.
#[allow(clippy::await_holding_lock)]
async fn a_save_admitted_before_a_discard_never_writes_the_discarded_transcript() {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let id = ready(&mut desk).await;
    desk.say(&id, "Say hello.").await.expect("sent");
    desk.answered().await;
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("history id")
        .to_owned();
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.").await.expect("sent");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let a = desk.json()["active"]["tenant"]
        .as_str()
        .expect("A")
        .to_owned();
    let origin = desk.json()["tenants"][0]["origin"]
        .as_str()
        .expect("origin")
        .to_owned();
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": "1234567890",
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("B");
    let b = desk.json()["tenants"]
        .as_array()
        .expect("tenants")
        .iter()
        .find(|t| t["label"] == "Tenant B")
        .and_then(|t| t["id"].as_str())
        .expect("B id")
        .to_owned();
    // The engine ends on its own with the approval pending: its interrupted
    // turn's save is admitted and then held at the gate, before it writes.
    let gate = Arc::clone(&desk.save_gate);
    let held = gate.lock().expect("gate");
    let pid = desk.last_pid().expect("engine pid");
    let _ = std::process::Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status();
    tokio::time::sleep(Duration::from_secs(2)).await;
    // The guard is still actionable: Discard removes the earlier record and
    // tombstones the conversation while the save waits.
    let resolved = reviewed_switch(
        &mut desk,
        json!({"kind": "selectTenant", "tenant": b}),
        "discard",
    )
    .await;
    desk.send(resolved).await.expect("switched");
    assert!(
        settle_for(20, || !files_under(&desk.history_root)
            .iter()
            .any(|f| f.to_string_lossy().contains(&history)))
        .await,
        "the discard removed the saved record"
    );
    drop(held);
    tokio::time::sleep(Duration::from_secs(2)).await;
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    assert!(settle_for(30, || desk.conversation().is_null()).await);
    let ids = history_ids(&mut desk).await;
    assert!(!ids.contains(&history), "never written again: {ids:?}");
    assert!(
        !files_under(&desk.history_root)
            .iter()
            .any(|f| f.to_string_lossy().contains(&history)),
        "and no record file"
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// C30 (I1): completing an Add-a-tenant draft makes the new tenant active,
/// so it is the same guarded switch: a pending Harness decision refuses it
/// with the guard naming the draft, nothing is committed or retired until
/// the operator resolves the conversation, and the resolution completes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completing_a_tenant_setup_is_the_guarded_switch_path() {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let id = ready(&mut desk).await;
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let token = c["approvals"][0]["id"].as_str().expect("id").to_owned();
    let a = desk.json()["active"]["tenant"]
        .as_str()
        .expect("A")
        .to_owned();
    let origin = desk.json()["tenants"][0]["origin"]
        .as_str()
        .expect("origin")
        .to_owned();
    desk.send(json!({"kind": "beginTenantSetup", "mode": "add", "profile": {"label": "Tenant C", "tsgId": "1234567890",
        "destination": {"kind": "custom", "origin": origin}}}))
        .await
        .expect("draft");
    let setup = desk.json()["setup"].clone();
    let draft = setup["context"].as_str().expect("draft context").to_owned();
    let draft_id = setup["tenant"]["id"].as_str().expect("draft id").to_owned();
    desk.send(json!({"kind": "verifyTenantSetup", "context": draft}))
        .await
        .expect("verify");
    assert!(settle(|| desk.json()["setup"]["verify"]["done"] == true).await);
    assert_eq!(
        desk.send(json!({"kind": "completeTenantSetup", "context": draft, "packs": []}))
            .await,
        Err(desktop_core::Refusal::DecisionPending)
    );
    let s = desk.json();
    assert_eq!(s["active"]["tenant"], a, "still on A");
    assert_eq!(
        s["switchBlocked"]["to"], draft_id,
        "the guard names the draft"
    );
    assert_eq!(s["switchBlocked"]["completion"], true);
    assert_eq!(s["switchBlocked"]["pending"]["conversation"], id);
    assert_eq!(s["switchBlocked"]["pending"]["decision"], token);
    assert_eq!(s["setup"]["context"], draft, "the draft is untouched");
    assert!(
        s["harness"]["conversation"]["approvals"]
            .as_array()
            .is_some_and(|x| !x.is_empty()),
        "nothing was decided for the operator"
    );
    assert!(
        s["tenants"]
            .as_array()
            .expect("tenants")
            .iter()
            .all(|t| t["label"] != "Tenant C"),
        "nothing was committed"
    );
    // Stay: the guard closes; the draft, the tenant and the decision remain.
    desk.send(json!({"kind": "dismissSwitchGuard"}))
        .await
        .expect("dismissed");
    let s = desk.json();
    assert!(s["switchBlocked"].is_null());
    assert_eq!(s["setup"]["context"], draft);
    assert_eq!(s["active"]["tenant"], a);
    // Archive and complete: the conversation ends (kept), the tenant is
    // committed and active, the draft is gone.
    let resolved = reviewed_switch(
        &mut desk,
        json!({"kind": "completeTenantSetup", "context": draft, "packs": []}),
        "archive",
    )
    .await;
    assert_eq!(
        resolved["resolution"]["context"],
        desk.context(),
        "bound to the active source, not the setup draft"
    );
    let mut wrong_source = resolved.clone();
    wrong_source["resolution"]["context"] = json!(draft);
    assert_eq!(
        desk.send(wrong_source).await,
        Err(desktop_core::Refusal::StaleContext)
    );
    assert_eq!(
        desk.conversation()["id"],
        id,
        "forged source does not end the conversation"
    );
    desk.send(resolved).await.expect("completed");
    let s = desk.json();
    assert!(s["setup"].is_null());
    assert!(s["switchBlocked"].is_null());
    let c_id = s["tenants"]
        .as_array()
        .expect("tenants")
        .iter()
        .find(|t| t["label"] == "Tenant C")
        .and_then(|t| t["id"].as_str())
        .expect("committed")
        .to_owned();
    assert_eq!(s["active"]["tenant"], c_id);
    assert!(
        s["harness"]["conversation"].is_null(),
        "another tenant's conversation is never shown"
    );
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("back to A");
    assert!(
        settle_for(30, || {
            let c = desk.conversation();
            c.is_null() || c["phase"] == "ended" || c["phase"] == "ending"
        })
        .await
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

/// C30 attempt11 R1/E1: a refused state save authorizes no destructive
/// resolution. The same native refusal remains actionable after recovery.
async fn guarded_resolution_save_failure(completion: bool, kind: &str) {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let id = ready(&mut desk).await;
    desk.say(&id, "Say hello.").await.expect("sent");
    desk.answered().await;
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("history")
        .to_owned();
    tool(&desk, None, "exec_command", json!({"cmd": "ls"}));
    desk.say(&id, "List the files.")
        .await
        .expect("pending turn");
    answered_with(&mut desk, |c| {
        c["approvals"].as_array().is_some_and(|a| !a.is_empty())
    })
    .await;
    let origin = desk.json()["tenants"][0]["origin"]
        .as_str()
        .expect("origin")
        .to_owned();
    let intent = if completion {
        desk.send(json!({"kind":"beginTenantSetup","mode":"add","profile":{"label":"Tenant B","tsgId":"1234567890","destination":{"kind":"custom","origin":origin}}})).await.expect("draft");
        let draft = desk.json()["setup"]["context"]
            .as_str()
            .expect("draft context")
            .to_owned();
        desk.send(json!({"kind":"verifyTenantSetup","context":draft}))
            .await
            .expect("verify");
        assert!(settle(|| desk.json()["setup"]["verify"]["done"] == true).await);
        json!({"kind":"completeTenantSetup","context":draft,"packs":[]})
    } else {
        desk.send(json!({"kind":"createTenant","profile":{"label":"Tenant B","tsgId":"1234567890","destination":{"kind":"custom","origin":origin}}})).await.expect("B");
        let b = desk.json()["tenants"]
            .as_array()
            .expect("tenants")
            .iter()
            .find(|t| t["label"] == "Tenant B")
            .expect("B")["id"]
            .clone();
        json!({"kind":"selectTenant","tenant":b})
    };
    let resolved = reviewed_switch(&mut desk, intent, kind).await;
    let before = desk.json();
    let stored = serde_json::to_value(desk.adapters.store.saved()).expect("stored document");
    let protected: Vec<_> = files_under(&desk.history_root)
        .into_iter()
        .filter(|f| f.to_string_lossy().contains(&history))
        .map(|f| {
            let bytes = std::fs::read(&f).expect("protected transcript");
            (f, bytes)
        })
        .collect();
    assert!(
        !protected.is_empty(),
        "the completed first turn is protected on disk"
    );
    desk.adapters.store.fail_saves(true);
    assert_eq!(
        desk.send(resolved.clone()).await,
        Err(desktop_core::Refusal::DeniedStore)
    );
    let refused = desk.json();
    assert_eq!(
        refused["active"], before["active"],
        "the source remains active"
    );
    assert_eq!(
        refused["switchBlocked"], before["switchBlocked"],
        "save refusal preserves the exact native guard"
    );
    assert_eq!(
        refused["setup"], before["setup"],
        "the draft remains actionable"
    );
    for field in ["id", "phase", "approvals", "historyId", "savedAt"] {
        assert_eq!(
            refused["harness"]["conversation"][field], before["harness"]["conversation"][field],
            "pending conversation {field} unchanged"
        );
    }
    assert_eq!(
        serde_json::to_value(desk.adapters.store.saved()).expect("saved"),
        stored
    );
    for (path, bytes) in &protected {
        assert_eq!(&std::fs::read(path).expect("still protected"), bytes);
    }
    // Recovery uses the same refusal binding, with a new explicit delivery.
    desk.adapters.store.fail_saves(false);
    desk.send(resolved.clone())
        .await
        .expect("one successful resolution after storage recovery");
    assert_ne!(desk.json()["active"]["tenant"], before["active"]["tenant"]);
    assert!(desk.json()["switchBlocked"].is_null());
    if completion {
        assert!(desk.json()["setup"].is_null());
    }
    assert_eq!(
        desk.send(resolved).await,
        Err(desktop_core::Refusal::StaleContext),
        "a consumed resolution cannot run again"
    );
    desk.send(json!({"kind":"selectTenant","tenant":before["active"]["tenant"]}))
        .await
        .expect("back to source");
    assert!(
        settle_for(30, || {
            let c = desk.conversation();
            c.is_null() || c["phase"] == "ended"
        })
        .await
    );
    let mut listed = history_ids(&mut desk).await;
    for _ in 0..60 {
        if listed.contains(&history) == (kind == "archive") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
        listed = history_ids(&mut desk).await;
    }
    assert_eq!(
        listed.contains(&history),
        kind == "archive",
        "successful recovery honors the chosen history outcome"
    );
    assert_eq!(
        files_under(&desk.history_root)
            .iter()
            .any(|f| f.to_string_lossy().contains(&history)),
        kind == "archive"
    );
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn select_archive_resolution_save_failure_preserves_pending_work() {
    guarded_resolution_save_failure(false, "archive").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn select_discard_resolution_save_failure_preserves_pending_work() {
    guarded_resolution_save_failure(false, "discard").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup_archive_resolution_save_failure_preserves_pending_work() {
    guarded_resolution_save_failure(true, "archive").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup_discard_resolution_save_failure_preserves_pending_work() {
    guarded_resolution_save_failure(true, "discard").await;
}
