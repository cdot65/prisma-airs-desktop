//! A scripted stand-in for the pinned engine's `desktop-server` (C19
//! fixture builds only). It speaks `airs-desktop/1` like the engine and,
//! chosen by the first path segment of the bootstrap's gateway endpoint
//! (`https://host/<mode>/v1`), fails the ways the real engine cannot be
//! made to: refusing or never answering the bootstrap, a wrong or
//! oversized receipt, malformed, oversized or duplicate frames, crashing
//! mid-answer, ignoring the end of its input, leaving a descendant, asking
//! for an approval, flooding an answer, or echoing the key in an answer. It
//! never opens a connection.
//!
//! An answer in `normal` mode reports what the sidecar was given, so tests
//! can check it: its environment's names, its arguments, a short hash of
//! the key it received and the prompt's length; never the key itself.

use std::io::{BufRead, Write};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn send(value: &Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

fn raw(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

fn forever() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn note(method: &str, params: Value) {
    send(&json!({"method": method, "params": params}));
}

fn main() {
    let stdin = std::io::stdin();
    let mut first = String::new();
    if stdin.lock().read_line(&mut first).unwrap_or(0) == 0 {
        return;
    }
    let boot: Value = serde_json::from_str(&first).unwrap_or(Value::Null);
    let p = &boot["params"];
    let endpoint = p["gatewayEndpoint"].as_str().unwrap_or_default();
    let mode = url_mode(endpoint);
    let key = p["apiKey"].as_str().unwrap_or_default();
    let key_hash = hex::encode(&Sha256::digest(key.as_bytes())[..8]);
    let mut env: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    env.sort();
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut receipt = json!({"v": 1, "protocol": "airs-desktop/1", "runtimeVersion": "0.1.7-desktop.2",
        "tenantId": p["tenantId"], "workspaceId": p["workspaceId"], "credentialId": p["credentialId"],
        "credentialEpoch": p["credentialEpoch"], "mcpServers": []});
    match mode.as_str() {
        "reject" => {
            send(
                &json!({"id": 0, "error": {"code": -32602, "message": "Desktop bootstrap rejected"}}),
            );
            std::process::exit(1);
        }
        "silent" => forever(),
        "receipt-big" => {
            raw(&format!(
                r#"{{"id":0,"result":{{"pad":"{}"}}}}"#,
                "x".repeat(70 * 1024)
            ));
            forever();
        }
        "wrong-receipt" => receipt["tenantId"] = json!("00000000-0000-8000-8000-000000000001"),
        "exit" => std::process::exit(0),
        _ => {}
    }
    send(&json!({"id": 0, "result": receipt}));

    // Requests arrive on a thread so a paced answer can notice them.
    let (tx, rx) = mpsc::channel::<Option<Value>>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(serde_json::from_str(&line).ok()).is_err() {
                return;
            }
        }
        let _ = tx.send(None);
    });

    let mut turns = 0u32;
    let thread = "thread-fixture";
    let mut child: Option<std::process::Child> = None;
    while let Ok(message) = rx.recv() {
        let Some(message) = message else {
            // End of input.
            if mode == "hang-on-eof" {
                forever();
            }
            break;
        };
        let id = message["id"].clone();
        match message["method"].as_str().unwrap_or_default() {
            "initialize" => {
                // Every start takes 2 s (P06 review R3-F1).
                if mode == "slow-start" {
                    std::thread::sleep(Duration::from_secs(2));
                }
                send(&json!({"id": id, "result": {"userAgent": "fixture"}}));
                if mode == "duplicate" {
                    send(&json!({"id": id, "result": {"userAgent": "fixture"}}));
                }
            }
            "initialized" => {}
            "thread/start" => {
                if mode == "descendant" {
                    child = std::process::Command::new("/bin/sleep")
                        .arg("600")
                        .spawn()
                        .ok();
                }
                note("thread/started", json!({"thread": {"id": thread}}));
                send(&json!({"id": id, "result": {"thread": {"id": thread}}}));
            }
            "turn/start" => {
                turns += 1;
                let turn = format!("turn-{turns}");
                let item = format!("msg-{turns}");
                let prompt_len: usize = message["params"]["input"]
                    .as_array()
                    .map(|items| {
                        items
                            .iter()
                            .map(|i| i["text"].as_str().map_or(0, str::len))
                            .sum()
                    })
                    .unwrap_or(0);
                if mode == "foreign" {
                    // An answer to a request the host never made.
                    send(&json!({"id": 9_999, "result": {}}));
                }
                send(&json!({"id": id, "result": {"turn": {"id": turn, "status": "inProgress"}}}));
                note(
                    "turn/started",
                    json!({"threadId": thread, "turn": {"id": turn}}),
                );
                note(
                    "item/started",
                    json!({"threadId": thread, "turnId": turn,
                    "item": {"type": "agentMessage", "id": item, "text": ""}}),
                );
                let delta = |text: &str| {
                    note(
                        "item/agentMessage/delta",
                        json!({"threadId": thread, "turnId": turn, "itemId": item, "delta": text}),
                    );
                };
                let complete = |text: &str, status: &str| {
                    note(
                        "item/completed",
                        json!({"threadId": thread, "turnId": turn,
                        "item": {"type": "agentMessage", "id": item, "text": text}}),
                    );
                    note(
                        "thread/tokenUsage/updated",
                        json!({"threadId": thread, "turnId": turn,
                        "tokenUsage": {"total": {"inputTokens": 10, "outputTokens": 6, "totalTokens": 16}}}),
                    );
                    note(
                        "turn/completed",
                        json!({"threadId": thread, "turn": {"id": turn, "status": status}}),
                    );
                };
                match mode.as_str() {
                    "crash" => {
                        delta("partial");
                        std::process::exit(3);
                    }
                    "history-refuse-stall" if turns > 1 => {
                        delta("partial");
                        forever();
                    }
                    "oversized" => {
                        raw(&format!(
                            r#"{{"method":"item/agentMessage/delta","params":{{"delta":"{}"}}}}"#,
                            "x".repeat(4 * 1024 * 1024 + 1)
                        ));
                    }
                    "malformed" => raw("not json"),
                    // A short delta, then a completion past the answer bound.
                    "big-complete" => {
                        delta("partial");
                        complete(&"z".repeat(256 * 1024 + 1), "completed");
                    }
                    // Each turn completes a whole 256 KiB answer with no
                    // delta, so the fourth outgrows the transcript.
                    "fill" => complete(&"f".repeat(256 * 1024), "completed"),
                    // Two answer items in one turn, together past the bound.
                    "two-items" => {
                        let second = format!("{item}-b");
                        note(
                            "item/completed",
                            json!({"threadId": thread, "turnId": turn,
                            "item": {"type": "agentMessage", "id": item, "text": "a".repeat(200 * 1024)}}),
                        );
                        note(
                            "item/started",
                            json!({"threadId": thread, "turnId": turn,
                            "item": {"type": "agentMessage", "id": second, "text": ""}}),
                        );
                        note(
                            "item/completed",
                            json!({"threadId": thread, "turnId": turn,
                            "item": {"type": "agentMessage", "id": second, "text": "b".repeat(100 * 1024)}}),
                        );
                        note(
                            "turn/completed",
                            json!({"threadId": thread, "turn": {"id": turn, "status": "completed"}}),
                        );
                    }
                    // A completion for an item never started, with an id
                    // past the bound.
                    "long-id" => {
                        note(
                            "item/completed",
                            json!({"threadId": thread, "turnId": turn,
                            "item": {"type": "agentMessage", "id": "i".repeat(301), "text": "OK"}}),
                        );
                        note(
                            "turn/completed",
                            json!({"threadId": thread, "turn": {"id": turn, "status": "completed"}}),
                        );
                    }
                    // A whole answer with no item/started and no delta.
                    "complete-only" => {
                        note(
                            "item/completed",
                            json!({"threadId": thread, "turnId": turn,
                            "item": {"type": "agentMessage", "id": "unstarted", "text": "Only completed."}}),
                        );
                        note(
                            "turn/completed",
                            json!({"threadId": thread, "turn": {"id": turn, "status": "completed"}}),
                        );
                    }
                    // Token counts past what the presentation can hold.
                    "huge-usage" => {
                        delta("small");
                        note(
                            "thread/tokenUsage/updated",
                            json!({"threadId": thread, "turnId": turn,
                            "tokenUsage": {"total": {"inputTokens": 9_007_199_254_740_992u64,
                                "outputTokens": u64::MAX, "totalTokens": 1}}}),
                        );
                        note(
                            "item/completed",
                            json!({"threadId": thread, "turnId": turn,
                            "item": {"type": "agentMessage", "id": item, "text": "small"}}),
                        );
                        note(
                            "turn/completed",
                            json!({"threadId": thread, "turn": {"id": turn, "status": "completed"}}),
                        );
                    }
                    "echo-key" => {
                        // Model text that happens to contain the key, split
                        // across two deltas.
                        let text = format!("the key is {key}.");
                        let (a, b) = text.split_at(text.len() - key.len() / 2 - 1);
                        delta(a);
                        delta(b);
                        // Long enough for a snapshot of the joined text.
                        std::thread::sleep(Duration::from_millis(300));
                        complete(&text, "completed");
                    }
                    "approval" => {
                        send(
                            &json!({"id": "approval-1", "method": "item/commandExecution/requestApproval",
                            "params": {"threadId": thread, "turnId": turn, "itemId": "cmd-1", "command": "ls"}}),
                        );
                        let decision = loop {
                            match rx.recv() {
                                Ok(Some(m)) if m["id"] == "approval-1" => {
                                    break m["result"]["decision"]
                                        .as_str()
                                        .unwrap_or("none")
                                        .to_owned();
                                }
                                Ok(Some(_)) => {}
                                _ => std::process::exit(0),
                            }
                        };
                        let text = format!("approval:{decision}");
                        delta(&text);
                        complete(&text, "completed");
                    }
                    "paced" | "flood" => {
                        let piece = if mode == "flood" {
                            "y".repeat(64 * 1024)
                        } else {
                            "word ".into()
                        };
                        let mut sent = String::new();
                        let interrupted = loop {
                            if sent.len() > 40 * 1024 * 1024 {
                                break false;
                            }
                            delta(&piece);
                            sent.push_str(&piece);
                            match rx.recv_timeout(Duration::from_millis(if mode == "flood" {
                                1
                            } else {
                                50
                            })) {
                                Ok(Some(m)) if m["method"] == "turn/interrupt" => {
                                    send(&json!({"id": m["id"], "result": {}}));
                                    break true;
                                }
                                Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                                    std::process::exit(0)
                                }
                                _ => {}
                            }
                        };
                        complete(
                            &sent,
                            if interrupted {
                                "interrupted"
                            } else {
                                "completed"
                            },
                        );
                    }
                    _ => {
                        let mut text = format!(
                            "env:{} args:{} key:{key_hash} prompt:{prompt_len} turn:{turns}",
                            env.join(","),
                            args.join(",")
                        );
                        if let Some(c) = &child {
                            text.push_str(&format!(" child:{}", c.id()));
                        }
                        let (a, b) = text.split_at(text.len() / 2);
                        delta(a);
                        delta(b);
                        complete(&text, "completed");
                    }
                }
            }
            "turn/interrupt" => send(&json!({"id": id, "result": {}})),
            // C20: an export of the idle thread (a stand-in replay naming
            // its turns), and a restore of one.
            "desktop/history/read" => match mode.as_str() {
                "history-silent" => {}
                // Only the first turn's export is answered (P06 review F1).
                "history-silent-later" if turns > 1 => {}
                // Answered after 2 s with a refusal, a malformed result or
                // the export (on later turns only with `-later`): inside a
                // stop's grace (P06 review R2, R3).
                m if m.starts_with("history-slow-") && (!m.ends_with("-later") || turns > 1) => {
                    std::thread::sleep(Duration::from_secs(2));
                    if m.starts_with("history-slow-ok") {
                        send(&json!({"id": id, "result": {
                            "context": {"fixture": true, "credentialId": p["credentialId"]},
                            "replay": {"schemaVersion": 1, "turns": turns}}}));
                    } else if m.contains("malformed") {
                        send(&json!({"id": id, "result": {}}));
                    } else {
                        send(&json!({"id": id, "error": {"code": -32600, "message": "busy"}}));
                    }
                }
                "history-refuse" => {
                    send(&json!({"id": id, "error": {"code": -32600, "message": "busy"}}))
                }
                // Never exported, and the second turn stalls (C22 review
                // R2-F1).
                "history-refuse-stall" => {
                    send(&json!({"id": id, "error": {"code": -32600, "message": "busy"}}))
                }
                // Only the first turn is exported (C20 review F1).
                "history-refuse-later" if turns > 1 => {
                    send(&json!({"id": id, "error": {"code": -32600, "message": "busy"}}))
                }
                _ => send(&json!({"id": id, "result": {
                    "context": {"fixture": true, "credentialId": p["credentialId"]},
                    "replay": {"schemaVersion": 1, "turns": turns}}})),
            },
            "desktop/history/restore" => {
                let params = &message["params"];
                if mode == "restore-refuse"
                    || params["context"]["credentialId"] != p["credentialId"]
                {
                    send(&json!({"id": id, "error": {"code": -32600, "message": "context"}}));
                } else {
                    turns = params["replay"]["turns"].as_u64().unwrap_or(0) as u32;
                    send(&json!({"id": id, "result": {"thread": {"id": thread, "turns": []}}}));
                }
            }
            _ => {
                if !id.is_null() {
                    send(&json!({"id": id, "error": {"code": -32601, "message": "unknown"}}));
                }
            }
        }
    }
}

/// The first path segment of the endpoint (`normal` when absent).
fn url_mode(endpoint: &str) -> String {
    endpoint
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split('/').nth(1))
        .filter(|s| !s.is_empty() && *s != "v1")
        .unwrap_or("normal")
        .to_owned()
}
