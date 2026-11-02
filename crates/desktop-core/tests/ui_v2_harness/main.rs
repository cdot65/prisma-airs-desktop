//! C32: the Harness's decisions, proposals and confirmations in the core,
//! on the real pinned engine against the synthetic Gateway that plays the
//! model (screens/harness.md; provenance P45, P47–P49): desktop reads are
//! decided in the page, changes need the native confirmation, the key
//! rotation hand-off and the workspace guardrail are staged for review only,
//! the MCP catalog is read once allowed, and a confirmation that outlives
//! its conversation runs nothing.

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

/// The Harness desk with the management inventories the proposals review.
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
                 "cust_app": "Support bot", "expiration": "2027-01-01T00:00:00Z", "revoked": false,
                 "status": "ACTIVE"}]}),
            _ => return None,
        };
        Some(json_reply(200, &body))
    }));
    desk
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

fn management_reads(desk: &Desk) -> usize {
    desk.requests("/aisec/v1/mgmt/").len()
}

/// A disposable project folder in the build's own scratch space.
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

fn entry<'a>(c: &'a Value, role: &str) -> Option<&'a Value> {
    c["entries"]
        .as_array()?
        .iter()
        .rev()
        .find(|e| e["role"] == role)
}

/// C32: the keys lifecycle hands off a rotation as an API key version and
/// the mcp lifecycle proposes a guardrail of the conversation's own
/// workspace. Each is staged, never applied (nothing is written), and opens
/// as the operator's review; the guardrail's review is bound to that
/// workspace's reference.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rotation_hand_off_and_a_workspace_guardrail_are_staged_never_applied() {
    let mut desk = desk().await;
    let id = ready(&mut desk).await;
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("ref")
        .to_owned();
    tool(
        &desk,
        Some("airs_desktop"),
        "proposeChange",
        json!({"kind": "apiKeys", "action": "version", "target": "key-1",
            "body": {"cust_app": "Support bot", "cust_env": "production", "cust_cloud_provider": "aws",
                     "api_key_name": "Runtime 2", "dp_name": "dp-0", "revoked": false,
                     "rotation_time_interval": 30, "rotation_time_unit": "days"},
            "summary": "Rotation is a write: start it in Administration"}),
    );
    desk.say(&id, "Rotate the Support bot key.")
        .await
        .expect("sent");
    let c = answered_with(&mut desk, |c| c["lastTurn"] == "completed").await;
    assert_eq!(c["proposals"][0]["state"], "staged", "{c}");
    assert_eq!(c["proposals"][0]["draft"]["kind"], "apiKeys");
    assert_eq!(c["proposals"][0]["draft"]["action"], "version");
    tool(
        &desk,
        Some("airs_desktop"),
        "proposeChange",
        json!({"kind": "gatewayGuardrails", "action": "create",
            "body": {"name": "mcp-allow-list", "checks": [{"id": "pii-check", "parameters": {"scan_scope": "all_messages"}}],
                     "actions": {"deny": true}},
            "summary": "Deny MCP servers that are not on the allow list"}),
    );
    desk.say(&id, "Propose a guardrail.").await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["proposals"].as_array().map(Vec::len) == Some(2) && c["phase"] == "ready"
    })
    .await;
    assert_eq!(c["proposals"][1]["state"], "staged", "{c}");
    assert_eq!(
        c["proposals"][1]["draft"]["workspace"], reference,
        "bound to the conversation's own workspace"
    );
    // Nothing was written by the Harness.
    assert!(
        desk.requests("/aisec/v1/mgmt/apikey")
            .iter()
            .all(|s| s.request_line.starts_with("GET"))
    );
    assert!(
        desk.requests(&format!("{DATA}/guardrails"))
            .iter()
            .all(|s| s.request_line.starts_with("GET"))
    );
    // Each opens as the operator's review.
    for proposal in ["P-1", "P-2"] {
        let opened = desk
            .send(
                json!({"kind": "reviewHarnessProposal", "conversation": id, "proposal": proposal}),
            )
            .await;
        assert!(opened.is_ok(), "{proposal}: {opened:?}");
        assert!(settle(|| !desk.json()["changes"]["review"].is_null()).await);
        let review = desk.json()["changes"]["review"]["review"].clone();
        desk.send(json!({"kind": "discardChange", "review": review}))
            .await
            .expect("discarded");
    }
    // A Gateway delete or a key rotation in place is never staged.
    for (kind, action) in [("gatewayGuardrails", "delete"), ("apiKeys", "rotate")] {
        tool(
            &desk,
            Some("airs_desktop"),
            "proposeChange",
            json!({"kind": kind, "action": action, "target": "x-1", "summary": "s"}),
        );
        desk.say(&id, "Try it.").await.expect("sent");
        answered_with(&mut desk, |c| {
            c["phase"] == "ready" && c["lastTurn"].is_string()
        })
        .await;
        let last = outputs(&desk).last().cloned().unwrap_or_default();
        assert!(
            last.contains("This tool or its arguments are unavailable."),
            "{last}"
        );
    }
    assert_eq!(
        desk.conversation()["proposals"].as_array().map(Vec::len),
        Some(2)
    );
    desk.end(&id).await;
}

/// C32 (R1): a decision is classified by the core. A desktop read is
/// read-only and decided in the page (declined, nothing is read); a command
/// with changes allowed may change the project, and in-page consent alone
/// never runs it: only the native confirmation does.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reads_are_decided_in_the_page_and_changes_need_the_native_confirmation() {
    let mut desk = desk().await;
    let id = ready(&mut desk).await;
    // A read, declined: nothing is read and the engine is told so.
    tool(
        &desk,
        Some("airs_desktop"),
        "inventory",
        json!({"resource": "apiKeys", "limit": 5}),
    );
    let reads = management_reads(&desk);
    desk.say(&id, "Which API keys exist?").await.expect("sent");
    assert!(
        settle_for(40, || desk.conversation()["approvals"]
            .as_array()
            .is_some_and(|a| a.len() == 1))
        .await
    );
    let read = desk.conversation()["approvals"][0].clone();
    assert_eq!(read["kind"], "desktopRead");
    assert_eq!(read["effect"], "readOnly");
    assert_eq!(read["server"], "airs_desktop");
    assert_eq!(read["tool"], "inventory");
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": read["id"], "allow": false}))
        .await
        .expect("declined");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready" && c["lastTurn"].is_string()
    })
    .await;
    assert_eq!(management_reads(&desk), reads, "nothing was read");
    assert_eq!(entry(&c, "tool").expect("entry")["state"], "declined");
    assert!(
        outputs(&desk)
            .last()
            .is_some_and(|o| o.contains("declined this read"))
    );
    desk.end(&id).await;
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    // A command with changes allowed: the sheet refused, nothing runs.
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let context = desk.context();
    desk.send(json!({"kind": "setHarnessWrite", "context": context, "write": true}))
        .await
        .expect("write");
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("ref")
        .to_owned();
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": "echo c32 > c32.txt"}),
    );
    desk.say(&id, "Write a file.").await.expect("sent");
    assert!(
        settle_for(40, || desk.conversation()["approvals"]
            .as_array()
            .is_some_and(|a| a.len() == 1))
        .await
    );
    let change = desk.conversation()["approvals"][0].clone();
    assert_eq!(change["kind"], "command");
    assert_eq!(change["effect"], "mayChange");
    desk.adapters.confirmer.auto(Some(false));
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": change["id"], "allow": true}))
        .await
        .expect("asked natively");
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        desk.conversation()["approvals"][0]["id"],
        change["id"],
        "still pending: in-page consent alone does not run it"
    );
    assert!(!dir.join("c32.txt").exists(), "nothing ran");
    // Accepted in the native sheet: it runs once.
    desk.adapters.confirmer.auto(Some(true));
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": change["id"], "allow": true}))
        .await
        .expect("asked natively");
    answered_with(&mut desk, |c| {
        c["phase"] == "ready"
            && c["approvals"].as_array().is_some_and(Vec::is_empty)
            && c["lastTurn"].is_string()
    })
    .await;
    assert!(
        settle(|| dir.join("c32.txt").exists()).await,
        "it ran after the native confirmation"
    );
    desk.end(&id).await;
}

/// C32 (R7): the mcp lifecycle's read is the desktop's own `mcpServers`
/// tool: it waits for the operator's decision in the page and, allowed,
/// reads the conversation workspace's MCP catalog (`GatewayRead::McpServers`)
/// and gives the engine the servers it lists.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_mcp_catalog_is_read_for_the_engine_only_once_the_operator_allows_it() {
    let mut desk = desk().await;
    let fast = desk
        .gateway
        .lock()
        .expect("gateway")
        .hook
        .clone()
        .expect("hook");
    desk.gateway.lock().expect("gateway").hook = Some(Arc::new(move |seen| {
        let target = seen.request_line.split(' ').nth(1)?;
        if target.starts_with(&format!("{DATA}/mcp-servers")) {
            return Some(json_reply(
                200,
                &json!({"object": "list", "total": 1, "data": [
                {"id": "mcp-1", "name": "scratch-tools", "slug": "scratch-tools",
                 "url": "https://mcp.example/scratch-tools/mcp", "status": "active",
                 "workspace_id": WS_A, "last_updated_at": "2026-10-28T00:00:00Z", "auth_type": "oauth",
                 "object": "mcp_server"}]}),
            ));
        }
        fast(seen)
    }));
    let id = ready(&mut desk).await;
    tool(&desk, Some("airs_desktop"), "mcpServers", json!({}));
    desk.say(&id, "Which MCP servers does this workspace have?")
        .await
        .expect("sent");
    assert!(
        settle_for(40, || desk.conversation()["approvals"]
            .as_array()
            .is_some_and(|a| a.len() == 1))
        .await
    );
    let read = desk.conversation()["approvals"][0].clone();
    assert_eq!(
        (
            read["kind"].as_str(),
            read["tool"].as_str(),
            read["effect"].as_str()
        ),
        (Some("desktopRead"), Some("mcpServers"), Some("readOnly"))
    );
    assert!(
        desk.requests(&format!("{DATA}/mcp-servers")).is_empty(),
        "nothing read before the decision"
    );
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": read["id"], "allow": true}))
        .await
        .expect("allowed");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready" && c["lastTurn"].is_string()
    })
    .await;
    assert_eq!(
        desk.requests(&format!("{DATA}/mcp-servers")).len(),
        1,
        "read once"
    );
    assert_eq!(entry(&c, "tool").expect("entry")["state"], "complete");
    assert!(
        outputs(&desk)
            .last()
            .is_some_and(|o| o.contains("scratch-tools")),
        "{:?}",
        outputs(&desk)
    );
    desk.end(&id).await;
}

/// C32 (attempt-02 optional): a native confirmation still open when its
/// conversation ends is dismissed, and an acceptance that arrives late runs
/// nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_confirmation_that_outlives_its_conversation_runs_nothing() {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let context = desk.context();
    desk.send(json!({"kind": "setHarnessWrite", "context": context, "write": true}))
        .await
        .expect("write");
    let id = ready(&mut desk).await;
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd": "echo late > late.txt"}),
    );
    desk.say(&id, "Write a file.").await.expect("sent");
    assert!(
        settle_for(40, || desk.conversation()["approvals"]
            .as_array()
            .is_some_and(|a| a.len() == 1))
        .await
    );
    let change = desk.conversation()["approvals"][0].clone();
    assert_eq!(change["effect"], "mayChange");
    // The sheet stays open: the operator has not answered yet.
    desk.adapters.confirmer.auto(None);
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": change["id"], "allow": true}))
        .await
        .expect("asked natively");
    assert!(
        settle(|| desk.adapters.confirmer.held() == 1).await,
        "the sheet is shown"
    );
    let nonce = desk
        .adapters
        .confirmer
        .requests()
        .last()
        .expect("a request")
        .0;
    // The conversation ends while the sheet is open: it is dismissed.
    desk.end(&id).await;
    assert!(
        settle(|| desk.adapters.confirmer.dismissed().contains(&nonce)).await,
        "dismissed with its conversation"
    );
    // A late acceptance runs nothing.
    let _ = desk.adapters.confirmer.complete(true);
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(!dir.join("late.txt").exists(), "nothing ran");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_opening_message_is_kept_whole_beside_its_shortened_title() {
    // A contextual restart prefills the opening question, never the title
    // (screens/harness.md; attempt-12 R1).
    let mut desk = desk().await;
    let id = ready(&mut desk).await;
    let opening = format!(
        "Investigate the Finance topic.\nList every key that expires before the audit, {}and explain.",
        "with its owner and its application, ".repeat(4)
    );
    assert!(opening.chars().count() > 160);
    desk.say(&id, &opening).await.expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready" && c["lastTurn"] == "completed"
    })
    .await;
    assert_eq!(c["opening"], opening.as_str(), "{c}");
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = c["historyId"].as_str().expect("id").to_owned();
    desk.say(&id, "And the second question?")
        .await
        .expect("sent");
    let _ = answered_with(&mut desk, |c| {
        c["phase"] == "ready" && c["entries"].as_array().is_some_and(|e| e.len() >= 4)
    })
    .await;
    desk.end(&id).await;
    // Ended, it still shows its opening whole.
    assert_eq!(desk.conversation()["opening"], opening.as_str());
    // Saved: listed by a shortened one-line title, opened with the opening whole.
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(json!({"kind": "listHarnessHistory", "context": context, "workspace": workspace}))
        .await
        .expect("list");
    assert!(settle(|| desk.json()["harness"]["history"]["state"] == "ready").await);
    let listed = desk.json()["harness"]["history"]["conversations"].clone();
    let title = listed
        .as_array()
        .and_then(|l| l.iter().find(|c| c["id"] == history.as_str()))
        .and_then(|c| c["title"].as_str())
        .expect("listed")
        .to_owned();
    assert!(title.ends_with('…') && !title.contains('\n'), "{title}");
    desk.send(
        json!({"kind": "openHarnessHistory", "context": context, "workspace": workspace, "id": history}),
    )
    .await
    .expect("open");
    assert!(settle(|| desk.json()["harness"]["history"]["opened"]["id"] == history.as_str()).await);
    let opened = desk.json()["harness"]["history"]["opened"].clone();
    assert_eq!(opened["opening"], opening.as_str(), "{opened}");
    assert_eq!(opened["title"], title.as_str());
    let first = desk.json()["harness"]["history"]["lastOpen"].clone();
    assert_eq!(first["id"], history.as_str(), "{first}");
    assert!(first["failure"].is_null());
    // A record that cannot be read: each open settles with its own receipt,
    // also when it fails as the one before it did (attempt-13 R1).
    let missing = "00000000-0000-8000-8000-000000000000";
    let mut sequence = first["sequence"].as_u64().expect("sequence");
    for _ in 0..2 {
        desk.send(
            json!({"kind": "openHarnessHistory", "context": context, "workspace": workspace, "id": missing}),
        )
        .await
        .expect("open");
        assert!(
            settle(|| {
                let l = &desk.json()["harness"]["history"]["lastOpen"];
                l["id"] == missing && l["sequence"].as_u64() > Some(sequence)
            })
            .await
        );
        let h = desk.json()["harness"]["history"].clone();
        assert_eq!(h["lastOpen"]["failure"], "missing", "{h}");
        assert_eq!(h["problem"], "missing");
        assert_eq!(h["busy"], false);
        sequence = h["lastOpen"]["sequence"].as_u64().expect("sequence");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_named_send_is_taken_once_and_reported_with_the_message_it_became() {
    // A send whose receipt was lost is settled by the core's word (attempt-16 R1).
    let mut desk = desk().await;
    let id = ready(&mut desk).await;
    let submission = "0123456789abcdef0123456789abcdef";
    let send = json!({"kind": "sendHarness", "conversation": id, "prompt": "Same words", "submission": submission});
    desk.send(send.clone()).await.expect("sent");
    // The same delivery again (its receipt was lost): taken once, nothing sent again.
    desk.send(send).await.expect("taken already");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready" && c["lastTurn"] == "completed"
    })
    .await;
    assert_eq!(
        c["submissions"],
        json!([{"submission": submission, "entry": "u-1", "state": "delivered"}]),
        "{c}"
    );
    let operators = c["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .filter(|e| e["role"] == "operator")
        .count();
    assert_eq!(operators, 1, "{c}");
    // Identical words under another ID are another send, reported as its own.
    let other = "fedcba9876543210fedcba9876543210";
    desk.send(json!({"kind": "sendHarness", "conversation": id, "prompt": "Same words", "submission": other}))
        .await
        .expect("sent");
    let c = answered_with(&mut desk, |c| {
        c["phase"] == "ready" && c["submissions"].as_array().is_some_and(|s| s.len() == 2)
    })
    .await;
    assert_eq!(
        c["submissions"][1],
        json!({"submission": other, "entry": "u-2", "state": "delivered"})
    );
    // An ID not of the presentation's form is refused.
    assert!(
        desk.send(json!({"kind": "sendHarness", "conversation": id, "prompt": "x", "submission": "not-hex"}))
            .await
            .is_err()
    );
    desk.end(&id).await;
}

/// C32 attempt22 R1: retire the original decision before its effect can
/// register a native confirmation. This uses actual production admission,
/// the real pinned engine and the native adapter interface, not a renderer timer.
async fn late_harness_confirmation(retire_project: bool) {
    let mut desk = desk().await;
    let dir = project();
    choose_project(&mut desk, &dir).await;
    let context = desk.context();
    desk.send(json!({"kind":"setHarnessWrite","context":context,"write":true}))
        .await
        .expect("write");
    let id = ready(&mut desk).await;
    tool(
        &desk,
        None,
        "exec_command",
        json!({"cmd":"echo stale > stale-c32.txt"}),
    );
    desk.say(&id, "Write a synthetic marker.")
        .await
        .expect("sent");
    assert!(
        settle_for(40, || desk.conversation()["approvals"]
            .as_array()
            .is_some_and(|a| a.len() == 1))
        .await
    );
    let approval = desk.conversation()["approvals"][0]["id"].clone();
    desk.adapters.confirmer.auto(None);
    let before = desk.adapters.confirmer.requests().len();
    let gate = desk.handle.hold_next_native_presentation();
    desk.sequence += 1;
    let delivery = desktop_core::Delivery {
        session: desk.session.clone(),
        sequence: desk.sequence,
    };
    let intent = serde_json::from_value(
        json!({"kind":"decideHarnessApproval","conversation":id,"approval":approval,"allow":true}),
    )
    .expect("intent");
    let handle = desk.handle.clone();
    let dispatch = tokio::spawn(async move { handle.dispatch(delivery, intent).await });
    assert!(
        settle(|| gate.entered()).await,
        "actual admission completed before native registration"
    );
    if retire_project {
        let context = desk.context();
        desk.send(json!({"kind":"setHarnessWrite","context":context,"write":false}))
            .await
            .expect("retire project binding");
        assert!(
            settle(|| desk.conversation()["phase"] == "ended" || desk.conversation().is_null())
                .await
        );
    } else {
        desk.end(&id).await;
    }
    gate.release();
    dispatch
        .await
        .expect("dispatch task")
        .expect("admitted decision");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        desk.adapters.confirmer.requests().len(),
        before,
        "retired Harness decision must not register a native sheet"
    );
    assert_eq!(
        desk.adapters.confirmer.held(),
        0,
        "no stale native confirmation remains"
    );
    assert!(!dir.join("stale-c32.txt").exists(), "no command ran");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_harness_confirmation_after_end_never_opens() {
    late_harness_confirmation(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_harness_confirmation_after_project_retirement_never_opens() {
    late_harness_confirmation(true).await;
}
