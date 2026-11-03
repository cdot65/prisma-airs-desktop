//! C38 (D059): the Harness contract the ported board draws from, on the
//! real pinned engine against the synthetic Gateway that plays the model:
//! the conversation view the band, rail and transcript show (its title from
//! the opening message, operator and assistant entries in order, the ready
//! phase with its last turn), the decision the aside holds for a desktop
//! read (kind, tool and read-only effect, decided in the page and run once),
//! and the workspace MCP catalog the aside's summary lists (every server
//! with its standing: off and not signed in until the operator acts).

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

#[allow(dead_code, unused_imports)]
#[path = "../harness_process/desk.rs"]
mod desk;

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::desk::*;

const OPENING: &str = "Why were 14 prompts blocked on claims-assistant overnight? Is it one actor?";

/// The Harness desk whose Gateway catalog publishes two MCP servers.
async fn desk() -> Desk {
    let desk = desk_with(Some(real_engine()), None).await;
    desk.gateway.lock().expect("gateway").hook = Some(Arc::new(|seen| {
        let target = seen.request_line.split(' ').nth(1)?;
        if !target.starts_with(&format!("{DATA}/mcp-servers")) {
            return None;
        }
        Some(json_reply(
            200,
            &json!({"object": "list", "total": 2, "data": [
                {"id": "mcp-inventory", "name": "cloud-inventory", "slug": "cloud-inventory",
                 "url": "https://mcp.example/cloud-inventory/mcp", "status": "active",
                 "workspace_id": WS_A, "last_updated_at": "2026-10-28T00:00:00Z", "auth_type": "oauth",
                 "object": "mcp_server"},
                {"id": "mcp-ticketing", "name": "ticketing", "slug": "ticketing",
                 "url": "https://mcp.example/ticketing/mcp", "status": "active",
                 "workspace_id": WS_A, "last_updated_at": "2026-10-28T00:00:00Z", "auth_type": "oauth",
                 "object": "mcp_server"}]}),
        ))
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

fn roles(c: &Value) -> Vec<String> {
    c["entries"]
        .as_array()
        .map(|e| {
            e.iter()
                .filter_map(|e| e["role"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// C38: the band, the rail row and the transcript draw from one conversation
/// view: the opening message is kept whole as the first operator entry, the
/// answer follows it as an assistant entry, and the phase settles to ready
/// with a completed last turn (the rail's "Ready." and the band's End).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_board_draws_the_conversation_from_one_view() {
    let mut desk = desk().await;
    let id = desk.ready(None).await;
    desk.say(&id, OPENING).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["id"], id);
    assert_eq!(c["phase"], "ready");
    assert_eq!(c["lastTurn"], "completed");
    assert_eq!(
        roles(&c).iter().map(String::as_str).collect::<Vec<_>>(),
        ["operator", "assistant"]
    );
    assert_eq!(c["entries"][0]["text"], OPENING);
    assert!(
        c["opening"].is_null() || c["opening"] == OPENING,
        "{}",
        c["opening"]
    );
    assert!(
        c["approvals"].as_array().is_some_and(Vec::is_empty),
        "nothing to decide"
    );
    desk.end(&id).await;
}

/// C38: a desktop read is the decision the aside holds: a read-only
/// `airs_desktop.mcpServers` call with its arguments, nothing read before the
/// decision, and the read made once when it is allowed in the page, after
/// which the answer follows and the aside is empty again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_aside_holds_a_read_only_decision_until_it_is_allowed() {
    let mut desk = desk().await;
    let id = desk.ready(None).await;
    tool(&desk, Some("airs_desktop"), "mcpServers", json!({}));
    desk.say(&id, "Which MCP servers does this workspace have?")
        .await
        .expect("sent");
    assert!(
        settle_for(40, || desk.conversation()["approvals"]
            .as_array()
            .is_some_and(|a| a.len() == 1))
        .await,
        "{}",
        desk.conversation()
    );
    let c = desk.conversation();
    let read = c["approvals"][0].clone();
    assert_eq!(read["kind"], "desktopRead");
    assert_eq!(read["server"], "airs_desktop");
    assert_eq!(read["tool"], "mcpServers");
    assert_eq!(read["effect"], "readOnly");
    assert!(read["arguments"].is_string(), "{read}");
    assert!(
        desk.requests(&format!("{DATA}/mcp-servers")).is_empty(),
        "nothing read before the decision"
    );
    desk.send(json!({"kind": "decideHarnessApproval", "conversation": id, "approval": read["id"], "allow": true}))
        .await
        .expect("allowed");
    assert!(
        settle_for(40, || {
            let c = desk.conversation();
            c["phase"] == "ready"
                && c["lastTurn"].is_string()
                && c["approvals"].as_array().is_some_and(Vec::is_empty)
        })
        .await,
        "{}",
        desk.conversation()
    );
    assert_eq!(
        desk.requests(&format!("{DATA}/mcp-servers")).len(),
        1,
        "read once"
    );
    let c = desk.conversation();
    let last = c["entries"]
        .as_array()
        .expect("entries")
        .last()
        .expect("entry");
    assert_eq!(last["role"], "assistant", "{c}");
    desk.end(&id).await;
}

/// C38: the aside's summary lists the chosen workspace's MCP catalog as the
/// core reads it: both servers, each off and not signed in (their standing
/// "Not signed in"), the view bound to that workspace and complete.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_summary_lists_the_workspace_catalog_with_each_standing() {
    let mut desk = desk().await;
    let reference = desk.choose(WS_A).await;
    let context = desk.context();
    desk.send(json!({"kind": "listMcpServers", "context": context, "workspace": reference}))
        .await
        .expect("listed");
    assert!(
        settle_for(20, || desk.json()["harness"]["mcp"]["state"] == "ready").await,
        "{}",
        desk.json()["harness"]["mcp"]
    );
    let mcp = desk.json()["harness"]["mcp"].clone();
    assert_eq!(mcp["workspace"], WS_A);
    assert_eq!(mcp["complete"], true);
    assert!(mcp["failure"].is_null());
    let servers = mcp["servers"].as_array().expect("servers");
    let names: Vec<&str> = servers.iter().filter_map(|s| s["name"].as_str()).collect();
    assert_eq!(names, ["cloud-inventory", "ticketing"]);
    for s in servers {
        assert_eq!(s["supported"], true, "{s}");
        assert_eq!(s["enabled"], false, "{s}");
        assert_eq!(s["signedIn"], false, "{s}");
        assert_eq!(s["stale"], false, "{s}");
        assert!(
            s["expired"].is_null() && s["step"].is_null() && s["failure"].is_null(),
            "{s}"
        );
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
}
