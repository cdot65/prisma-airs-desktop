//! C22: workspace MCP servers for the Harness against the real pinned engine
//! (9331a58) and a synthetic Gateway MCP plane (`fixture.rs`): the catalog
//! read through the management client, the desktop's own OAuth sign-in
//! (protected-resource and server metadata, registration, PKCE with
//! `resource`, refresh and revocation), the conversation's fixed scope
//! bootstrapped with memory-only bearers, approval bound to the engine's
//! running call, an expired sign-in keeping inference, MCP-only token
//! rotation restarting and restoring without asking again for changes,
//! and every scope change retiring the conversation.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

#[allow(dead_code, unused_imports)]
#[path = "../harness_process/desk.rs"]
mod desk;

mod fixture;

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::desk::*;
use crate::fixture::{Mcp, RESULT, Shared};

const TOOLS: &str = "srv-tools-0001";
const TOOLS_B: &str = "srv-tools-0002";
const DOCS: &str = "srv-docs-0001";

async fn desk_mcp() -> (Desk, Shared) {
    let desk = desk_with(Some(real_engine()), None).await;
    let origin = format!("https://localhost:{}", desk.server.port);
    let mcp = Arc::new(Mutex::new(Mcp::new(&origin)));
    {
        let mut m = mcp.lock().expect("mcp");
        m.publish(TOOLS, "tools", WS_A);
        m.publish(DOCS, "docs", WS_A);
        // The same server URL published in another workspace.
        m.publish(TOOLS_B, "tools", WS_B);
    }
    let routed = Arc::clone(&mcp);
    desk.gateway.lock().expect("gateway").hook = Some(Arc::new(move |seen| {
        routed.lock().expect("mcp").route(seen)
    }));
    (desk, mcp)
}

fn view(desk: &Desk) -> Value {
    desk.json()["harness"]["mcp"].clone()
}

fn server<'a>(v: &'a Value, id: &str) -> &'a Value {
    v["servers"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["id"] == id))
        .unwrap_or(&Value::Null)
}

async fn list(desk: &mut Desk, reference: &str) -> Value {
    let context = desk.context();
    desk.send(json!({"kind": "listMcpServers", "context": context, "workspace": reference}))
        .await
        .expect("listed");
    assert!(
        settle_for(20, || view(desk)["state"] != "loading").await,
        "{}",
        view(desk)
    );
    view(desk)
}

async fn enable(desk: &mut Desk, reference: &str, id: &str, enabled: bool) {
    let context = desk.context();
    desk.send(
        json!({"kind": "enableMcpServer", "context": context, "workspace": reference,
        "server": id, "enabled": enabled}),
    )
    .await
    .expect("enabled");
}

/// One HTTP/1.1 GET to the app's loopback callback; the status code.
fn callback(url: &str) -> u16 {
    let url = url::Url::parse(url).expect("callback url");
    let port = url.port().expect("port");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let target = format!("{}?{}", url.path(), url.query().unwrap_or_default());
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    reply
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Signs in to `id` through the browser stand-in; the server's view.
async fn sign_in(desk: &mut Desk, mcp: &Shared, reference: &str, id: &str) -> Value {
    let opened = desk.adapters.opener.opened().len();
    let context = desk.context();
    desk.send(json!({"kind": "signInMcpServer", "context": context, "workspace": reference, "server": id}))
        .await
        .expect("sign-in admitted");
    assert!(
        settle_for(20, || {
            desk.adapters.opener.opened().len() > opened
                || !server(&view(desk), id)["failure"].is_null()
        })
        .await,
        "{}",
        view(desk)
    );
    if let Some(url) = desk.adapters.opener.opened().get(opened) {
        let url = url::Url::parse(url).expect("authorize url");
        let back = mcp.lock().expect("mcp").authorize(&url);
        // Another state is answered and ignored.
        assert_eq!(callback(&back.replace("state=", "state=x")), 400);
        assert_eq!(callback(&back), 200);
    }
    assert!(
        settle_for(20, || {
            let s = server(&view(desk), id).clone();
            s["step"].is_null() && (s["signedIn"] == true || !s["failure"].is_null())
        })
        .await,
        "{}",
        view(desk)
    );
    server(&view(desk), id).clone()
}

/// WS_A chosen with the key stored and the catalog read.
async fn prepared(desk: &mut Desk) -> String {
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    let v = list(desk, &reference).await;
    assert_eq!(v["state"], "ready", "{v}");
    reference
}

async fn ready(desk: &mut Desk, reference: &str) -> String {
    let id = desk.start(reference).await.expect("started");
    assert!(
        settle_for(30, || desk.conversation()["phase"] == "ready").await,
        "{}",
        desk.conversation()
    );
    id
}

/// Waits for the scope server `name` to settle its startup.
async fn started(desk: &Desk, name: &str) -> Value {
    assert!(
        settle_for(40, || {
            desk.conversation()["mcp"].as_array().is_some_and(|s| {
                s.iter()
                    .any(|s| s["name"] == name && s["status"] != "starting")
            })
        })
        .await,
        "{}",
        desk.conversation()
    );
    desk.conversation()["mcp"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["name"] == name).cloned())
        .unwrap_or_default()
}

fn ask_tool(desk: &Desk, server: &str) {
    desk.gateway.lock().expect("gateway").answer = Answer::McpTool {
        server: server.to_owned(),
        tool: "lookup".to_owned(),
        arguments: json!({"query": "record-1"}).to_string(),
    };
}

async fn turn_ended(desk: &mut Desk, entries: usize) -> Value {
    assert!(
        settle_for(40, || {
            let c = desk.conversation();
            c["phase"] == "ready"
                && c["approvals"].as_array().is_some_and(Vec::is_empty)
                && c["entries"].as_array().is_some_and(|e| e.len() >= entries)
        })
        .await,
        "{}",
        desk.conversation()
    );
    desk.conversation()
}

async fn approval(desk: &Desk) -> Value {
    assert!(
        settle_for(40, || desk.conversation()["approvals"]
            .as_array()
            .is_some_and(|a| !a.is_empty()))
        .await,
        "{}",
        desk.conversation()
    );
    desk.conversation()["approvals"][0].clone()
}

fn records_with(desk: &Desk, needle: &str) -> usize {
    desk.adapters
        .vault
        .records()
        .into_iter()
        .filter(|r| {
            desk.adapters
                .vault
                .secret(r)
                .is_some_and(|s| String::from_utf8_lossy(&s).contains(needle))
        })
        .count()
}

async fn start_one(desk: &mut Desk, reference: &str) -> (String, u32) {
    let id = desk.start(reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    (id, desk.last_pid().expect("pid"))
}

async fn open_saved(desk: &mut Desk, history: &str) -> Value {
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(json!({"kind": "listHarnessHistory", "context": context, "workspace": workspace}))
        .await
        .expect("list");
    assert!(settle(|| desk.json()["harness"]["history"]["state"] == "ready").await);
    let context = desk.context();
    desk.send(json!({"kind": "openHarnessHistory", "context": context, "workspace": workspace, "id": history}))
        .await
        .expect("open");
    assert!(settle(|| desk.json()["harness"]["history"]["opened"]["id"] == history).await);
    desk.json()["harness"]["history"]["opened"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_catalog_is_read_per_workspace_and_signing_in_is_the_desktops_own_oauth() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    let v = view(&desk);
    let ids: Vec<&str> = v["servers"]
        .as_array()
        .expect("servers")
        .iter()
        .filter_map(|s| s["id"].as_str())
        .collect();
    assert_eq!(ids, [TOOLS, DOCS], "only this workspace's servers: {v}");
    assert_eq!(server(&v, TOOLS)["namespace"], "tools");
    assert!(
        !v.to_string().contains("upstream.example"),
        "the upstream URL is never kept"
    );
    // Signing in: metadata, registration, PKCE with resource, the tool
    // scopes only, and the code exchanged by the desktop.
    let s = sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    assert_eq!(s["signedIn"], true, "{s}");
    {
        let m = mcp.lock().expect("mcp");
        assert_eq!(m.registrations, 1);
        let q: std::collections::HashMap<String, String> =
            m.authorized[0].query_pairs().into_owned().collect();
        assert_eq!(q["resource"], m.url("tools"));
        assert!(
            m.authorized[0]
                .as_str()
                .starts_with(&format!("{}/authorize?", m.issuer()))
        );
    }
    // The tokens are a Keychain item; the state document holds none.
    assert_eq!(records_with(&desk, "mcp-access-"), 1);
    let doc = serde_json::to_string(&desk.adapters.store.durable()).expect("doc");
    assert!(
        !doc.contains("mcp-access-") && !doc.contains("mcp-refresh-"),
        "{doc}"
    );
    // The same URL in the other workspace is another server, not signed in.
    let other = desk.choose(WS_B).await;
    let v = list(&mut desk, &other).await;
    assert_eq!(server(&v, TOOLS_B)["signedIn"], false, "{v}");
    assert!(server(&v, TOOLS).is_null());
    // A server outside the catalog cannot be turned on.
    let context = desk.context();
    assert!(
        desk.send(
            json!({"kind": "enableMcpServer", "context": context, "workspace": other,
            "server": TOOLS, "enabled": true})
        )
        .await
        .is_err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_approved_tool_call_runs_once_with_the_memory_only_bearer() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    enable(&mut desk, &reference, TOOLS, true).await;
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    let id = ready(&mut desk, &reference).await;
    let status = started(&desk, "tools").await;
    assert_eq!(status["status"], "ready", "{status}");
    assert_eq!(status["signedOut"], false);
    // Declined: the server never runs it.
    ask_tool(&desk, "tools");
    desk.say(&id, "Look up record 1.").await.expect("sent");
    let a = approval(&desk).await;
    assert_eq!(a["kind"], "mcpTool", "{a}");
    assert_eq!(a["server"], "tools");
    assert_eq!(a["tool"], "lookup");
    assert!(
        a["arguments"]
            .as_str()
            .is_some_and(|s| s.contains("record-1")),
        "{a}"
    );
    let token = a["id"].as_str().expect("id").to_owned();
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": false}))
        .await
        .expect("declined");
    let c = turn_ended(&mut desk, 3).await;
    assert_eq!(mcp.lock().expect("mcp").calls.len(), 0);
    let entry = c["entries"]
        .as_array()
        .and_then(|e| e.iter().find(|e| e["role"] == "mcp"))
        .cloned()
        .expect("mcp entry");
    assert_eq!(entry["state"], "declined", "{entry}");
    // Approved once: it runs once, with the granted bearer.
    desk.say(&id, "Look it up again.").await.expect("sent");
    let token = approval(&desk).await["id"].as_str().expect("id").to_owned();
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
        .await
        .expect("approved");
    let c = turn_ended(&mut desk, 6).await;
    {
        let m = mcp.lock().expect("mcp");
        assert_eq!(m.calls.len(), 1);
        assert!(m.access.contains_key(&m.calls[0].1), "the granted bearer");
    }
    let last = c["entries"]
        .as_array()
        .and_then(|e| e.iter().rev().find(|e| e["role"] == "mcp"))
        .cloned()
        .expect("mcp entry");
    assert_eq!(last["state"], "complete", "{last}");
    assert!(last["text"].as_str().is_some_and(|t| t.contains(RESULT)));
    // A used approval is stale.
    assert!(desk
        .send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
        .await
        .is_err());
    // The bearer never reaches the Gateway's inference requests or the view,
    // and the engine never signs in itself.
    let bearer = mcp.lock().expect("mcp").calls[0].1.clone();
    for seen in desk.requests(&format!("{RUNTIME}/responses")) {
        assert!(!String::from_utf8_lossy(&seen.body).contains(&bearer));
        assert!(seen.header("authorization").is_none());
    }
    assert!(!desk.json().to_string().contains(&bearer));
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_expired_sign_in_keeps_inference_and_its_tools_are_unavailable() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    enable(&mut desk, &reference, TOOLS, true).await;
    enable(&mut desk, &reference, DOCS, true).await;
    // Tools signed in with a token that lapses at once; its refresh is
    // refused. Docs is never signed in.
    mcp.lock().expect("mcp").lifetime = 1;
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    mcp.lock().expect("mcp").refuse_refresh = true;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let id = ready(&mut desk, &reference).await;
    for name in ["tools", "docs"] {
        let status = started(&desk, name).await;
        assert_eq!(status["status"], "failed", "{status}");
        assert_eq!(status["signedOut"], true, "{status}");
    }
    // No tool request carried a real bearer; inference still answers.
    assert!(mcp.lock().expect("mcp").requests.iter().all(|(_, b)| {
        b.as_deref()
            .is_none_or(|b| b == "airs-desktop-mcp-sign-in-expired")
    }));
    desk.say(&id, "Hello.").await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    // The conversation is not retired by an expiry.
    assert_eq!(desk.conversation()["id"], id);
    let v = view(&desk);
    assert_eq!(server(&v, TOOLS)["expired"], "sessionExpired", "{v}");
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mcp_token_rotation_restarts_and_restores_without_asking_for_changes_again() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    // A project with changes allowed: an MCP-only rotation keeps it.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-c22-project-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("fixture");
    desk.adapters.folders.reply(Some(dir.clone()));
    let context = desk.context();
    desk.send(json!({"kind": "chooseHarnessProject", "context": context}))
        .await
        .expect("chosen");
    assert!(settle(|| !desk.json()["harness"]["project"].is_null()).await);
    let context = desk.context();
    desk.send(json!({"kind": "setHarnessWrite", "context": context, "write": true}))
        .await
        .expect("write");
    enable(&mut desk, &reference, TOOLS, true).await;
    // Ten-second tokens are renewed five seconds in.
    mcp.lock().expect("mcp").lifetime = 10;
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    let id = ready(&mut desk, &reference).await;
    assert_eq!(started(&desk, "tools").await["status"], "ready");
    desk.say(&id, "Remember the C22 canary word: heliotrope.")
        .await
        .expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    let first = desk.last_pid().expect("pid");
    let history = desk.conversation()["historyId"].clone();
    tokio::time::sleep(Duration::from_secs(6)).await;
    desk.say(&id, "What word did I give you?")
        .await
        .expect("sent");
    // No write confirmation: the restart restores and sends.
    assert!(
        settle_for(40, || {
            let c = desk.conversation();
            c["phase"] == "ready" && c["entries"].as_array().map(Vec::len) == Some(4)
        })
        .await,
        "{}",
        desk.conversation()
    );
    let c = desk.conversation();
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert_eq!(c["id"], id);
    assert_eq!(c["historyId"], history);
    assert_eq!(c["write"], true);
    assert_ne!(desk.last_pid(), Some(first), "a new engine");
    assert!(gone(first).await);
    // Renewed before the second message (once, unless a loaded host made
    // the first one wait past the margin too).
    assert!((1..=2).contains(&mcp.lock().expect("mcp").refreshes));
    // The restored engine sent the first turn with the second, and the new
    // engine's server started with the renewed bearer.
    let bodies: Vec<String> = desk
        .requests(&format!("{RUNTIME}/responses"))
        .iter()
        .map(|s| String::from_utf8_lossy(&s.body).into_owned())
        .collect();
    assert!(bodies.last().is_some_and(|b| b.contains("heliotrope")));
    assert_eq!(started(&desk, "tools").await["status"], "ready");
    desk.end(&id).await;
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn signing_out_again_or_off_or_a_new_revision_retires_the_conversation() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    enable(&mut desk, &reference, TOOLS, true).await;
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    // Signing out: retired, the item deleted, the tokens revoked.
    let (_, pid) = start_one(&mut desk, &reference).await;
    let context = desk.context();
    desk.send(json!({"kind": "signOutMcpServer", "context": context, "workspace": reference, "server": TOOLS}))
        .await
        .expect("signed out");
    assert!(settle(|| desk.conversation().is_null()).await);
    assert!(gone(pid).await);
    assert!(settle(|| records_with(&desk, "mcp-access-") == 0).await);
    assert!(settle(|| mcp.lock().expect("mcp").revoked.len() == 2).await);
    // Signing in again (to the same URL) is a new grant: retired.
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    let (_, pid) = start_one(&mut desk, &reference).await;
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    assert!(settle(|| desk.conversation().is_null()).await);
    assert!(gone(pid).await);
    // Turning it off: retired.
    let (_, pid) = start_one(&mut desk, &reference).await;
    enable(&mut desk, &reference, TOOLS, false).await;
    assert!(settle(|| desk.conversation().is_null()).await);
    assert!(gone(pid).await);
    // A new revision in the catalog: retired, and the sign-in is stale.
    enable(&mut desk, &reference, TOOLS, true).await;
    let (_, pid) = start_one(&mut desk, &reference).await;
    mcp.lock().expect("mcp").servers[0].updated = "2026-10-02T00:00:00Z".into();
    let v = list(&mut desk, &reference).await;
    assert!(settle(|| desk.conversation().is_null()).await);
    assert!(gone(pid).await);
    assert_eq!(server(&v, TOOLS)["stale"], true, "{v}");
    assert_eq!(server(&v, TOOLS)["signedIn"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_its_catalog_a_scope_cannot_start_or_continue() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    enable(&mut desk, &reference, TOOLS, true).await;
    // The catalog fails: shown as failed; the enabled server cannot be
    // checked, so nothing starts.
    mcp.lock().expect("mcp").catalog_down = true;
    let v = list(&mut desk, &reference).await;
    assert_eq!(v["state"], "ready", "the last catalog is kept: {v}");
    assert!(!v["failure"].is_null(), "{v}");
    // A relaunch-like state: the catalog of another workspace is loaded.
    let other = desk.choose(WS_B).await;
    let _ = list(&mut desk, &other).await;
    let reference = desk.choose(WS_A).await;
    assert!(matches!(
        desk.start(&reference).await,
        Err(desktop_core::Refusal::Busy)
    ));
    mcp.lock().expect("mcp").catalog_down = false;
    assert_eq!(list(&mut desk, &reference).await["state"], "ready");
    let id = ready(&mut desk, &reference).await;
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_approval_for_a_lapsed_sign_in_is_declined_natively() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    enable(&mut desk, &reference, TOOLS, true).await;
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    let id = ready(&mut desk, &reference).await;
    assert_eq!(started(&desk, "tools").await["status"], "ready");
    ask_tool(&desk, "tools");
    desk.say(&id, "Look up record 1.").await.expect("sent");
    let token = approval(&desk).await["id"].as_str().expect("id").to_owned();
    // The grant lapses while the approval waits: the operator's Allow is
    // declined by the desktop, and nothing runs.
    mcp.lock().expect("mcp").refuse_refresh = true;
    desk.core.expire_mcp_grants_for_tests();
    let decided = desk
        .send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": token, "allow": true}))
        .await;
    assert!(decided.is_err(), "{decided:?}");
    turn_ended(&mut desk, 3).await;
    assert_eq!(mcp.lock().expect("mcp").calls.len(), 0);
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_saved_conversation_continues_only_with_its_mcp_scope() {
    let (mut desk, mcp) = desk_mcp().await;
    let reference = prepared(&mut desk).await;
    enable(&mut desk, &reference, TOOLS, true).await;
    sign_in(&mut desk, &mcp, &reference, TOOLS).await;
    let id = ready(&mut desk, &reference).await;
    desk.say(&id, "Remember heliotrope.").await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("id")
        .to_owned();
    desk.end(&id).await;
    // Another scope (one more server on): not continuable.
    enable(&mut desk, &reference, DOCS, true).await;
    let opened = open_saved(&mut desk, &history).await;
    assert_eq!(opened["notContinuable"], "otherMcp", "{opened}");
    // The same scope again: it continues.
    enable(&mut desk, &reference, DOCS, false).await;
    let opened = open_saved(&mut desk, &history).await;
    assert!(opened["notContinuable"].is_null(), "{opened}");
}
