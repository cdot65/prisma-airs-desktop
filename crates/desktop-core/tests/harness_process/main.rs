//! C19 native Harness lifecycle through the production authority interface.
//!
//! The real pinned engine (extracted from the sealed archive and verified
//! against the compiled pin) runs conversations against a synthetic TLS
//! Gateway that streams Responses. A scripted stand-in
//! (`harness-fixture-sidecar`) produces the failures the engine cannot be
//! made to: malformed, oversized and duplicate frames, a refused or silent
//! bootstrap, a crash mid-answer, ignoring the end of its input, a
//! descendant, an approval request and a flood. Every conversation's
//! process is checked gone and its runtime home removed.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

#[allow(dead_code, unused_imports)]
mod desk;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use desktop_core::fixtures::VaultCall;
use desktop_core::harness::engine::{self, Engine};
use desktop_core::harness::session::ENV_NAMES;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{Observation, Receipt, Refusal, ViewScope};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::desk::*;

// ----------------------------------------------------------- real engine

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_answer_is_named_and_only_server_failures_are_retried_by_the_engine() {
    // The pinned engine's provider retries a 5xx or transport failure four
    // more times within one attempt, then reconnects its stream (up to five
    // times, each with those retries again: 30 requests). The host stops the
    // turn at the engine's first reconnect notice (D046), so a failing
    // Gateway sees at most the five requests of one attempt; never a 4xx.
    let mut desk = desk_with(Some(real_engine()), None).await;
    let id = desk.ready(None).await;
    desk.gateway.lock().expect("fixture").answer = Answer::Status(401);
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "failed", "{c}");
    assert_eq!(c["phase"], "ready");
    assert_eq!(
        desk.requests(RUNTIME).len(),
        1,
        "a refusal is never retried"
    );
    desk.gateway.lock().expect("fixture").answer = Answer::Status(503);
    desk.say(&id, "Again.").await.expect("sent");
    assert!(
        settle_for(60, || desk.conversation()["lastTurn"] == "failed"
            && desk.conversation()["phase"] == "ready")
        .await
    );
    assert_eq!(
        desk.requests(RUNTIME).len(),
        1 + 5,
        "one attempt, retried four times inside it, and no reconnect"
    );
    // No answer text arrived, so the operator's two messages are all there is.
    let roles: Vec<Value> = desk.conversation()["entries"]
        .as_array()
        .expect("fixture")
        .iter()
        .map(|e| e["role"].clone())
        .collect();
    assert_eq!(roles, [json!("operator"), json!("operator")]);
    // The conversation goes on.
    desk.gateway.lock().expect("fixture").answer = Answer::Short;
    desk.say(&id, "Once more.").await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_conversation_streams_through_the_pinned_engine_with_only_the_workspace_key() {
    let engine = real_engine();
    let mut desk = desk_with(Some(engine.clone()), None).await;
    let id = desk.ready(None).await;
    let pid = desk.last_pid().expect("engine running");
    assert!(exists(pid));

    // Launched exactly as the engine, with nothing but its own environment.
    let launches = desk.launcher.launches();
    assert_eq!(launches.len(), 1);
    let spec = &launches[0];
    assert_eq!(spec.executable, engine.executable);
    assert_eq!(spec.args, vec![std::ffi::OsString::from("desktop-server")]);
    let names: Vec<String> = spec
        .env
        .iter()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| ENV_NAMES.contains(&n.as_str())),
        "{names:?}"
    );
    for required in [
        "PATH",
        "LANG",
        "TMPDIR",
        "AIRS_HARNESS_HOME",
        "CODEX_CA_CERTIFICATE",
        "NO_PROXY",
    ] {
        assert!(names.iter().any(|n| n == required), "{required}");
    }
    assert!(
        spec.env
            .iter()
            .all(|(_, v)| !v.to_string_lossy().contains(KEY))
    );
    let path = spec
        .env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.to_string_lossy().into_owned())
        .expect("fixture");
    assert_eq!(path, "/usr/bin:/bin:/usr/sbin:/sbin");
    // No Node or Bun on that path: the engine needs neither.
    for dir in path.split(':') {
        for runtime in ["node", "bun", "npm", "npx"] {
            assert!(!Path::new(dir).join(runtime).exists(), "{dir}/{runtime}");
        }
    }
    let home = PathBuf::from(
        spec.env
            .iter()
            .find(|(k, _)| k == "AIRS_HARNESS_HOME")
            .expect("fixture")
            .1
            .clone(),
    );
    assert_eq!(
        home.parent(),
        Some(desk.runtime_root.canonicalize().expect("fixture").as_path())
    );
    assert_eq!(spec.cwd, home);

    // The process's own environment and arguments carry no credential.
    let ps = std::process::Command::new("/bin/ps")
        .args(["eww", "-o", "command=", "-p", &pid.to_string()])
        .output()
        .expect("fixture");
    let ps = String::from_utf8_lossy(&ps.stdout);
    assert!(ps.contains("desktop-server"));
    assert!(
        !ps.contains(KEY),
        "the key is never in the engine's environment or arguments"
    );

    // One answer, streamed, sent once with the workspace key.
    assert!(matches!(
        desk.say(&id, PROMPT).await,
        Ok(Receipt::Admitted { .. })
    ));
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert_eq!(
        c["entries"][0],
        json!({"id": "u-1", "role": "operator", "text": PROMPT, "state": "complete"})
    );
    assert_eq!(c["entries"][1]["role"], "assistant");
    assert_eq!(c["entries"][1]["text"], ANSWER);
    assert_eq!(c["entries"][1]["state"], "complete");
    assert_eq!(c["usage"]["total"], 16);
    assert_eq!(c["runtimeVersion"], "0.1.7-desktop.2");
    assert_eq!(c["workspace"]["id"], WS_A);
    assert_eq!(c["endpoint"], desk.endpoint(None));
    let sent = desk.requests(RUNTIME);
    assert_eq!(sent.len(), 1, "one Responses request");
    assert!(
        sent[0]
            .request_line
            .starts_with(&format!("POST {RUNTIME}/responses "))
    );
    assert_eq!(sent[0].header("x-portkey-api-key"), Some(KEY));
    assert!(String::from_utf8_lossy(&sent[0].body).contains(PROMPT));
    // Nothing else was fetched: no download, no other route.
    assert!(desk.server.seen().iter().all(|s| {
        let t = s.request_line.split(' ').nth(1).unwrap_or("");
        t.starts_with(&format!("{RUNTIME}/responses"))
            || t.starts_with(DATA)
            || t.starts_with("/oauth2/")
    }));

    // Nothing of the conversation or its key is on disk.
    for file in files_under(&desk.runtime_root) {
        let bytes = std::fs::read(&file).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains(KEY) && !text.contains(PROMPT) && !text.contains(ANSWER),
            "{}",
            file.display()
        );
    }

    // A second turn in the same conversation.
    desk.say(&id, "And again.").await.expect("second turn");
    let c = desk.answered().await;
    assert_eq!(c["entries"].as_array().expect("fixture").len(), 4);
    assert_eq!(desk.requests(RUNTIME).len(), 2);

    // Ending stops and reaps the engine and removes its home.
    desk.end(&id).await;
    let c = desk.conversation();
    assert_eq!(c["phase"], "ended");
    assert_eq!(c["stop"], "ended");
    assert_eq!(c["cleanup"], "graceful");
    assert!(c["failure"].is_null());
    assert!(gone(pid).await);
    assert!(desk.run_dirs().is_empty());
    assert!(!home.exists());
    // An ended conversation takes nothing more.
    assert_eq!(
        desk.say(&id, PROMPT).await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.send(json!({"kind": "endHarness", "conversation": id}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );

    // A new conversation starts a new engine with a new home; the old one's
    // transcript is not carried over.
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    assert!(desk.conversation().is_null());
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let next = desk.start(&reference).await.expect("started again");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    assert_ne!(next, id);
    assert!(
        desk.conversation()["entries"]
            .as_array()
            .expect("fixture")
            .is_empty()
    );
    assert_eq!(desk.launcher.launches().len(), 2);
    let second = desk.last_pid().expect("fixture");
    assert_ne!(second, pid);
    desk.end(&next).await;
    assert!(gone(second).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_an_answer_interrupts_only_that_turn() {
    let mut desk = desk_with(Some(real_engine()), None).await;
    let id = desk.ready(None).await;
    desk.gateway.lock().expect("fixture").answer = Answer::Paced;
    desk.say(&id, PROMPT).await.expect("sent");
    assert!(
        settle_for(20, || desk.conversation()["entries"][1]["text"]
            .as_str()
            .is_some_and(|t| t.len() > 60))
        .await,
        "{}",
        desk.conversation()
    );
    // Only while answering may another prompt wait: it is refused.
    assert_eq!(desk.say(&id, "too soon").await.err(), Some(Refusal::Busy));
    desk.send(json!({"kind": "interruptHarness", "conversation": id}))
        .await
        .expect("interrupt");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "interrupted", "{c}");
    assert_eq!(c["phase"], "ready");
    assert_eq!(c["entries"][1]["state"], "interrupted");
    // The stream was closed by the engine, not left running.
    assert!(settle(|| desk.server.closed_by_client() >= 1).await);
    // The conversation goes on.
    desk.gateway.lock().expect("fixture").answer = Answer::Short;
    desk.say(&id, "Short this time.").await.expect("next turn");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    assert_eq!(c["entries"][3]["text"], ANSWER);
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_changed_key_workspace_or_context_retires_the_conversation_at_once() {
    let mut desk = desk_with(Some(real_engine()), None).await;
    // The key replaced.
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    desk.store_key(&reference, OTHER_KEY, None).await;
    assert!(
        desk.conversation().is_null(),
        "withdrawn in the same snapshot"
    );
    assert!(gone(pid).await);
    assert!(settle(|| desk.run_dirs().is_empty()).await);

    // The key removed.
    let id = desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    let pid = desk.last_pid().expect("fixture");
    let context = desk.context();
    desk.send(json!({"kind": "removeInferenceCredential", "context": context}))
        .await
        .expect("removed");
    assert!(desk.conversation().is_null());
    assert!(gone(pid).await);
    // Refused either way: stale while its task settles, unknown once cleared.
    assert!(matches!(
        desk.say(&id, PROMPT).await.err(),
        Some(Refusal::StaleContext | Refusal::InvalidInput)
    ));

    // Another workspace chosen.
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    let _ = desk.choose(WS_B).await;
    assert!(desk.conversation().is_null());
    assert!(gone(pid).await);

    // The tenant context reissued (a new transport policy).
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    desk.core
        .set_transport_policy(TransportPolicy {
            revision: 3,
            proxy: ProxyMode::Direct,
            extra_roots: vec![desk._ca.pem.as_bytes().to_vec()],
        })
        .expect("policy");
    assert!(desk.conversation().is_null());
    assert!(gone(pid).await);
    assert!(settle(|| desk.run_dirs().is_empty()).await);
    // Nothing was sent by any retired conversation.
    assert!(desk.requests(RUNTIME).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sleep_and_quit_end_the_conversation_and_reap_the_engine() {
    let mut desk = desk_with(Some(real_engine()), None).await;
    let _ = desk.ready(None).await;
    let pid = desk.last_pid().expect("fixture");
    desk.core.system_will_sleep();
    assert!(settle(|| desk.conversation()["phase"] == "ended").await);
    assert_eq!(desk.conversation()["stop"], "sleep");
    assert!(gone(pid).await);

    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let id = desk.json()["harness"]["conversation"]["id"]
        .as_str()
        .expect("fixture")
        .to_owned();
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    desk.start(&reference).await.expect("started");
    assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
    let pid = desk.last_pid().expect("fixture");
    let core = desk.core.clone();
    let reaped =
        tokio::task::spawn_blocking(move || core.end_harness_for_exit(Duration::from_secs(5)))
            .await
            .expect("fixture");
    assert!(reaped);
    assert!(!exists(pid));
    assert_eq!(desk.conversation()["stop"], "quit");
    assert!(desk.run_dirs().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_changed_or_missing_engine_is_never_started_and_no_key_is_read() {
    let real = real_engine();
    let dir = std::env::temp_dir()
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-c19-corrupt-{}", hex::encode(rand_bytes())));
    std::fs::create_dir_all(&dir).expect("fixture");
    let corrupt = dir.join("airs-harness");
    std::fs::copy(&real.executable, &corrupt).expect("fixture");
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(&corrupt)
            .expect("fixture");
        f.seek(SeekFrom::Start(4096)).expect("fixture");
        f.write_all(&[0x5a]).expect("fixture");
    }
    let mut desk = desk_with(
        Some(Engine {
            executable: corrupt,
        }),
        None,
    )
    .await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    let reads_before = desk
        .adapters
        .vault
        .calls()
        .iter()
        .filter(|c| matches!(c, VaultCall::Get(_)))
        .count();
    desk.start(&reference).await.expect("admitted");
    assert!(settle_for(20, || desk.conversation()["phase"] == "failed").await);
    assert_eq!(desk.conversation()["failure"], "engineUnavailable");
    assert!(desk.launcher.launches().is_empty(), "nothing launched");
    let reads_after = desk
        .adapters
        .vault
        .calls()
        .iter()
        .filter(|c| matches!(c, VaultCall::Get(_)))
        .count();
    assert_eq!(reads_before, reads_after, "the key was never read");
    assert!(settle(|| desk.run_dirs().is_empty()).await);
    std::fs::remove_dir_all(&dir).expect("fixture");

    // No engine bundled: no Harness at all.
    let mut desk = desk_with(None, None).await;
    assert_eq!(desk.json()["harness"]["engine"]["available"], false);
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    assert_eq!(
        desk.start(&reference).await.err(),
        Some(Refusal::CapabilityMissing)
    );
}

#[test]
fn the_sealed_engine_ships_without_node_bun_or_a_managed_cli() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let engine = real_engine();
    let root = engine.executable.parent().expect("fixture");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(root.join("MANIFEST.json")).expect("fixture"))
            .expect("fixture");
    let files = manifest["files"].as_object().expect("fixture");
    for name in files.keys() {
        let lower = name.to_ascii_lowercase();
        for banned in [
            "node",
            "bun",
            "npm",
            "package.json",
            "airs-cli",
            "managed-cli",
            ".js",
            ".mjs",
        ] {
            let base = lower.rsplit('/').next().expect("fixture");
            assert!(
                !(base == banned
                    || base.ends_with(banned) && banned.starts_with('.')
                    || lower.contains("node_modules")),
                "{name} looks like a JavaScript runtime or package"
            );
        }
    }
    // Every extracted file is in the inventory with its hash, and the pin
    // names the inventory.
    let mut extracted = 0;
    for file in files_under(root) {
        let rel = file
            .strip_prefix(root)
            .expect("fixture")
            .to_string_lossy()
            .into_owned();
        if rel == "MANIFEST.json" {
            continue;
        }
        extracted += 1;
        assert_eq!(
            files[&rel],
            hex::encode(Sha256::digest(std::fs::read(&file).expect("fixture"))),
            "{rel}"
        );
    }
    assert_eq!(extracted, files.len());
    let digest = hex::encode(Sha256::digest(
        files
            .iter()
            .map(|(n, h)| format!("{n}\0{}\n", h.as_str().expect("fixture")))
            .collect::<String>()
            .as_bytes(),
    ));
    let pin = engine::pin();
    assert_eq!(manifest["pin"]["resourcesSha256"], pin.resources_sha256);
    assert_eq!(digest, pin.resources_sha256);
    assert_eq!(files["airs-harness"], pin.binary_sha256);
    let record: Value = serde_json::from_slice(
        &std::fs::read(repo.join("resources/harness/pin.json")).expect("fixture"),
    )
    .expect("fixture");
    assert_eq!(
        record["engineCommit"],
        "9331a5839bc6a763d94dee7584746f58bbbe1181"
    );
    assert_eq!(
        record["source"]["v1Commit"],
        "74754c02b19d87187b6dca81f64550a669b0f841"
    );
}

// ------------------------------------------------------------- stand-in

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_engine_is_given_nothing_but_its_bootstrap() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("normal")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    let text = c["entries"][1]["text"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let key_hash = hex::encode(&Sha256::digest(KEY.as_bytes())[..8]);
    assert!(text.contains(&format!("key:{key_hash}")), "{text}");
    assert!(text.contains("args:desktop-server "), "{text}");
    assert!(text.contains(&format!("prompt:{}", PROMPT.len())), "{text}");
    let env = text
        .split("env:")
        .nth(1)
        .expect("fixture")
        .split(' ')
        .next()
        .expect("fixture");
    let mut seen: Vec<&str> = env.split(',').collect();
    seen.sort();
    // macOS may add __CF_USER_TEXT_ENCODING for any process; nothing else.
    seen.retain(|n| *n != "__CF_USER_TEXT_ENCODING");
    assert_eq!(
        seen,
        [
            "AIRS_HARNESS_HOME",
            "CODEX_CA_CERTIFICATE",
            "LANG",
            "NO_PROXY",
            "PATH",
            "TMPDIR"
        ]
    );
    // The key never reaches what is shown.
    assert!(!desk.json().to_string().contains(KEY));
    desk.end(&id).await;
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");

    // Engine text that holds the key shows it replaced, even split across
    // deltas, at every snapshot while it streams.
    let id = desk.ready(Some("echo-key")).await;
    let Observation::Shell { mut updates, .. } =
        desk.handle.observe(ViewScope::Shell).expect("observe")
    else {
        unreachable!("shell scope")
    };
    while updates.try_next().is_some() {}
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["entries"][1]["text"], "the key is [redacted].");
    let mut published = 0;
    while let Some(update) = updates.try_next() {
        if let desktop_core::Update::Shell(snapshot) = update {
            published += 1;
            assert!(
                !serde_json::to_string(&*snapshot)
                    .expect("fixture")
                    .contains(KEY),
                "a published snapshot held the key"
            );
        }
    }
    assert!(published > 0);
    desk.end(&id).await;
}

/// Starts a stand-in conversation in `mode` and, for turn modes, sends a
/// prompt; returns the conversation once it has failed and its pid.
async fn failing(desk: &mut Desk, mode: &str, turn: bool) -> (Value, Option<u32>) {
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, Some(mode)).await;
    let id = desk.start(&reference).await.expect("started");
    let mut pid = None;
    if turn {
        assert!(
            settle_for(10, || desk.conversation()["phase"] == "ready").await,
            "{mode}: {}",
            desk.conversation()
        );
        pid = desk.last_pid();
        desk.say(&id, PROMPT).await.expect("sent");
    } else {
        let _ = settle_for(2, || desk.last_pid().is_some()).await;
        pid = pid.or(desk.last_pid());
    }
    assert!(
        settle_for(10, || desk.conversation()["phase"] == "failed").await,
        "{mode}: {}",
        desk.conversation()
    );
    let c = desk.conversation();
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    (c, pid)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_oversized_duplicate_or_foreign_frames_fail_the_conversation_and_reap_it() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    for (mode, turn, failure) in [
        ("reject", false, "bootstrapRejected"),
        ("wrong-receipt", false, "bootstrapRejected"),
        ("receipt-big", false, "bounds"),
        ("silent", false, "timeout"),
        ("exit", false, "exited"),
        ("duplicate", false, "protocol"),
        ("malformed", true, "protocol"),
        ("long-id", true, "protocol"),
        ("foreign", true, "protocol"),
        ("oversized", true, "bounds"),
        ("crash", true, "exited"),
    ] {
        let (c, pid) = failing(&mut desk, mode, turn).await;
        assert_eq!(c["failure"], failure, "{mode}: {c}");
        assert!(!c["cleanup"].is_null(), "{mode}: {c}");
        // Nothing past the contract's bounds was published (entry ids <= 300).
        assert!(
            c["entries"].as_array().is_some_and(|entries| entries
                .iter()
                .all(|e| e["id"].as_str().is_some_and(|i| i.len() <= 300))),
            "{mode}: {c}"
        );
        if let Some(pid) = pid {
            assert!(gone(pid).await, "{mode}: process left");
        }
        assert!(
            settle(|| desk.run_dirs().is_empty()).await,
            "{mode}: home left"
        );
        if mode == "crash" {
            // What arrived is kept, marked as not finished.
            assert_eq!(c["entries"][1]["text"], "partial");
            assert_eq!(c["entries"][1]["state"], "interrupted");
        }
    }
    assert!(!desk.json().to_string().contains(KEY));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_engine_that_ignores_its_input_closing_is_killed_with_its_descendants() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("hang-on-eof")).await;
    let pid = desk.last_pid().expect("fixture");
    let started = Instant::now();
    desk.end(&id).await;
    let took = started.elapsed();
    let c = desk.conversation();
    assert_eq!(c["cleanup"], "forced", "{c}");
    assert!(
        took >= Duration::from_secs(2) && took < Duration::from_secs(5),
        "{took:?}"
    );
    assert!(!exists(pid));
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");

    // A descendant goes with the group.
    let id = desk.ready(Some("descendant")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    let text = c["entries"][1]["text"]
        .as_str()
        .expect("fixture")
        .to_owned();
    let child: u32 = text
        .split("child:")
        .nth(1)
        .expect("fixture")
        .trim()
        .parse()
        .expect("fixture");
    assert!(exists(child));
    desk.end(&id).await;
    assert_eq!(desk.conversation()["cleanup"], "graceful");
    assert!(
        gone(child).await,
        "the descendant was killed with the group"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn approvals_are_declined_and_an_answer_past_its_bound_is_truncated() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("approval")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["entries"][1]["text"], "approval:decline");
    desk.end(&id).await;
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");

    let id = desk.ready(Some("flood")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "truncated", "{}", c["lastTurn"]);
    assert_eq!(c["entries"][1]["state"], "truncated");
    let kept = c["entries"][1]["text"].as_str().expect("fixture").len();
    assert!(kept <= desktop_core::harness::MAX_ANSWER_BYTES, "{kept}");
    assert!(kept > 0);
    // Still usable.
    assert_eq!(c["phase"], "ready");
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn completed_answers_and_token_counts_are_held_to_their_bounds() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let bound = desktop_core::harness::MAX_ANSWER_BYTES;
    let clear = |id: String| json!({"kind": "clearHarness", "conversation": id});

    // A completion past the bound after a short delta: cut at the bound and
    // never shown as complete.
    let id = desk.ready(Some("big-complete")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "truncated", "{}", c["lastTurn"]);
    assert_eq!(c["entries"][1]["state"], "truncated");
    assert_eq!(c["entries"][1]["text"].as_str().map(str::len), Some(bound));
    assert_eq!(c["phase"], "ready");
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // Two items of one turn, together past the bound: the second is cut so
    // the turn keeps exactly the bound.
    let id = desk.ready(Some("two-items")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "truncated", "{c}");
    let first = c["entries"][1]["text"].as_str().map_or(0, str::len);
    let second = c["entries"][2]["text"].as_str().map_or(0, str::len);
    assert_eq!(
        (first, c["entries"][1]["state"].as_str()),
        (200 * 1024, Some("complete"))
    );
    assert_eq!(c["entries"][2]["state"], "truncated");
    assert_eq!(first + second, bound);
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // Whole answers with no delta until the transcript has no room: the
    // fourth is cut to what is left, and the transcript stays bounded.
    let id = desk.ready(Some("fill")).await;
    for turn in 0..4 {
        desk.say(&id, PROMPT).await.expect("sent");
        let c = desk.answered().await;
        let want = if turn < 3 { "completed" } else { "truncated" };
        assert_eq!(c["lastTurn"], want, "turn {turn}");
    }
    let c = desk.conversation();
    let total: usize = c["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["text"].as_str().map_or(0, str::len))
        .sum();
    assert_eq!(total, desktop_core::harness::MAX_TRANSCRIPT_BYTES);
    assert_eq!(c["entries"][7]["state"], "truncated");
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // A whole answer with no start and no delta is kept, complete.
    let id = desk.ready(Some("complete-only")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    // (The stand-in starts its usual item first; this one it never started.)
    let entry = c["entries"]
        .as_array()
        .and_then(|e| e.iter().find(|e| e["id"] == "u-1/unstarted"))
        .expect("the unstarted item's entry");
    assert_eq!(entry["text"], "Only completed.");
    assert_eq!(entry["state"], "complete");
    desk.end(&id).await;
    desk.send(clear(id)).await.expect("cleared");

    // Token counts past JavaScript's safe integers are never published: the
    // answer is shown and every number in the snapshot stays exact there.
    let id = desk.ready(Some("huge-usage")).await;
    desk.say(&id, PROMPT).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed");
    assert_eq!(c["entries"][1]["text"], "small");
    assert!(c["usage"].is_null(), "{}", c["usage"]);
    fn safe(v: &Value) -> bool {
        match v {
            Value::Number(n) => n.as_u64().is_none_or(|n| n < (1 << 53)),
            Value::Array(items) => items.iter().all(safe),
            Value::Object(map) => map.values().all(safe),
            _ => true,
        }
    }
    assert!(safe(&desk.json()));
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_one_conversation_and_only_bounded_prompts_are_admitted() {
    let (engine, pin) = fixture_engine();
    let mut desk = desk_with(Some(engine), Some(pin)).await;
    let id = desk.ready(Some("normal")).await;
    let reference = desk.json()["gateway"]["selected"]["reference"]
        .as_str()
        .expect("fixture")
        .to_owned();
    assert_eq!(desk.start(&reference).await.err(), Some(Refusal::Busy));
    assert_eq!(
        desk.say(&id, "   ").await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.say(
            &id,
            &"x".repeat(desktop_core::harness::protocol::MAX_PROMPT_BYTES + 1)
        )
        .await
        .err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.say("op-999999", PROMPT).await.err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.send(json!({"kind": "clearHarness", "conversation": id}))
            .await
            .err(),
        Some(Refusal::InvalidInput),
        "a running conversation is not cleared"
    );
    // A stale context is refused.
    let context = desk.context();
    desk.core
        .set_transport_policy(TransportPolicy {
            revision: 4,
            proxy: ProxyMode::Direct,
            extra_roots: vec![desk._ca.pem.as_bytes().to_vec()],
        })
        .expect("policy");
    assert_eq!(
        desk.send(json!({"kind": "startHarness", "context": context, "workspace": reference}))
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
}
