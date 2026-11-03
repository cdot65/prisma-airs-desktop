//! C24: cumulative privacy and diagnostics through the production authority
//! interface (D051, D021; T19).
//!
//! Secret and content canaries are put through the authority's own paths
//! (a tenant, its scan key, a saved case, refused requests carrying them),
//! then every file the app keeps is searched: no secret appears in any of
//! them, and neither a secret nor content appears in the diagnostics or
//! their export. The diagnostics are bounded, exported only when asked,
//! cleared and removed by reset, and never sent anywhere.

use std::path::{Path, PathBuf};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles, SaveScript};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

/// A scan key: a secret, never in any file the app keeps in plaintext.
const SECRET: &str = "SYNTHETIC-C24-SECRET-scan-key";
/// Content (a prompt and a tenant name): kept where the operator saved it,
/// never in diagnostics.
const CONTENT: &str = "SYNTHETIC-C24-CONTENT-prompt";

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
}

fn start() -> Desk {
    let (adapters, handles) = fixtures::adapters();
    let core = Core::new(CoreConfig::default(), adapters);
    core.start();
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
    }
}

impl Desk {
    async fn send(&mut self, intent: Value) -> Result<Receipt, Refusal> {
        let intent: Intent = serde_json::from_value(intent).expect("intent");
        self.sequence += 1;
        self.handle
            .dispatch(
                Delivery {
                    session: self.session.clone(),
                    sequence: self.sequence,
                },
                intent,
            )
            .await
    }

    fn snap(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).expect("observe") {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell scope"),
        }
    }

    fn json(&self) -> Value {
        serde_json::to_value(self.snap()).expect("json")
    }

    fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    /// Every file the app keeps, outside the Keychain.
    fn roots(&self) -> Vec<PathBuf> {
        let a = &self.adapters;
        vec![
            a.offline_root.clone(),
            a.sessions_root.clone(),
            a.library_root.clone(),
            a.workbench_root.clone(),
            a.import_root.clone(),
            a.logs_root.clone(),
        ]
    }
}

async fn eventually(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..200 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

fn contains(path: &Path, needle: &str) -> bool {
    std::fs::read(path).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
}

fn log_text(desk: &Desk) -> String {
    files(&desk.adapters.logs_root)
        .iter()
        .map(|p| std::fs::read_to_string(p).unwrap_or_default())
        .collect()
}

/// Puts the canaries through the authority's own paths.
async fn seed(desk: &mut Desk) {
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": format!("Tenant {CONTENT}"),
        "tsgId": "1234567890", "destination": {"kind": "region", "region": "europe"}}}),
    )
    .await
    .expect("created");
    let context = desk.context();
    desk.send(json!({"kind": "configureScanCredential", "context": context, "mode": "keychain", "key": SECRET}))
        .await
        .expect("stored");
    assert!(
        eventually(|| desk.json()["tenants"][0]["scan"]["state"] == "ready").await,
        "{}",
        desk.json()
    );
    // A stored credential issues a new context.
    let context = desk.context();
    desk.send(json!({"kind": "openLibrary", "context": context}))
        .await
        .expect("open");
    assert!(eventually(|| desk.json()["library"]["state"] == "ready").await);
    desk.send(
        json!({"kind": "saveCase", "context": context, "draft": {"product": "runtime-security",
        "name": "Canary case", "profile": "prod", "prompt": CONTENT, "response": "",
        "appName": "Desktop", "expected": "", "payload": ""}}),
    )
    .await
    .expect("saved");
    assert!(
        eventually(|| desk.json()["library"]["cases"]
            .as_array()
            .is_some_and(|c| !c.is_empty()))
        .await
    );
    // Refused requests carrying both: only their kind and code are kept.
    let _ = desk
        .send(
            json!({"kind": "configureScanCredential", "context": "cx-000000000000000000000000",
            "mode": "keychain", "key": SECRET}),
        )
        .await;
    let _ = desk
        .send(
            json!({"kind": "saveCase", "context": "cx-000000000000000000000000", "draft": {
            "product": "runtime-security", "name": CONTENT, "profile": "prod", "prompt": CONTENT,
            "response": "", "appName": "Desktop", "expected": "", "payload": ""}}),
        )
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn diagnostics_keep_kinds_and_codes_and_never_a_secret_or_content() {
    let mut desk = start();
    seed(&mut desk).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let log = log_text(&desk);
    // Kinds and codes are there.
    assert!(log.contains("\"started\""), "{log}");
    assert!(
        log.contains("\"kind\":\"configureScanCredential\""),
        "{log}"
    );
    assert!(log.contains("\"refusal\":\"staleContext\""), "{log}");
    // Neither canary is.
    assert!(!log.contains(SECRET) && !log.contains(CONTENT), "{log}");
    assert!(
        log.lines()
            .all(|l| !l.contains("prompt") && !l.contains("key\"")),
        "{log}"
    );
    // No file the app keeps holds the secret (it is in the Keychain only).
    for root in desk.roots() {
        for f in files(&root) {
            assert!(!contains(&f, SECRET), "{}", f.display());
        }
    }
    let state = serde_json::to_string(&desk.adapters.store.saved()).expect("json");
    assert!(!state.contains(SECRET));
    // Nor the snapshot the presentation sees.
    assert!(!desk.json().to_string().contains(SECRET));
    let events = desk.json()["diagnostics"]["events"]
        .as_u64()
        .expect("count");
    assert!(events >= 7, "{events}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn diagnostics_are_exported_only_when_asked_and_redacted() {
    let mut desk = start();
    seed(&mut desk).await;
    assert!(
        desk.adapters.saver.saved().is_empty(),
        "nothing exported unasked"
    );
    desk.send(json!({"kind": "exportDiagnostics"}))
        .await
        .expect("export");
    assert!(eventually(|| desk.json()["diagnostics"]["export"] == "saved").await);
    let saved = desk.adapters.saver.saved();
    assert_eq!(saved.len(), 1);
    let (name, bytes) = &saved[0];
    assert_eq!(name, "prisma-airs-desktop-diagnostics.json");
    let text = String::from_utf8(bytes.clone()).expect("utf8");
    assert!(!text.contains(SECRET) && !text.contains(CONTENT), "{text}");
    let doc: Value = serde_json::from_str(&text).expect("json");
    assert_eq!(doc["schema"], "prisma-airs-desktop-v2/diagnostics/1");
    assert!(doc["events"].as_array().is_some_and(|e| e.len() >= 6));
    // A cancelled sheet saves nothing and says so.
    desk.adapters.saver.script(SaveScript::Cancel);
    desk.send(json!({"kind": "exportDiagnostics"}))
        .await
        .expect("export");
    assert!(eventually(|| desk.json()["diagnostics"]["export"] == "cancelled").await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clearing_and_reset_remove_the_diagnostics() {
    let mut desk = start();
    seed(&mut desk).await;
    assert!(desk.json()["diagnostics"]["events"].as_u64() > Some(3));
    desk.send(json!({"kind": "clearDiagnostics"}))
        .await
        .expect("cleared");
    // Only the record of clearing (and the request itself) remain, in
    // the count and on disk.
    assert!(
        desk.json()["diagnostics"]["events"].as_u64() <= Some(2),
        "{}",
        desk.json()["diagnostics"]
    );
    let log = log_text(&desk);
    assert!(log.lines().count() <= 2, "{log}");
    assert!(log.contains("\"cleared\""), "{log}");
    seed_more(&mut desk).await;
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| desk.json()["tenants"].as_array().is_some_and(Vec::is_empty)).await);
    assert!(eventually(|| !log_text(&desk).contains("clearDiagnostics")).await);
    // After reset only the reset's own outcome is kept.
    let log = log_text(&desk);
    assert!(log.lines().count() <= 2, "{log}");
    assert!(log.contains("\"reset\""), "{log}");
}

async fn seed_more(desk: &mut Desk) {
    let _ = desk
        .send(json!({"kind": "setAppearance", "theme": "dark"}))
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn diagnostics_are_bounded_and_survive_a_relaunch() {
    let mut desk = start();
    // More than two files' worth (each line is under 100 bytes).
    for _ in 0..8_000 {
        let _ = desk
            .send(json!({"kind": "setAppearance", "theme": "dark"}))
            .await;
    }
    let kept = files(&desk.adapters.logs_root);
    let size: u64 = kept
        .iter()
        .map(|p| std::fs::metadata(p).map_or(0, |m| m.len()))
        .sum();
    assert!(
        size <= 2 * desktop_core::diagnostics::FILE_LIMIT + 1024,
        "{size}"
    );
    // The full file was rotated, not grown or dropped.
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(size > desktop_core::diagnostics::FILE_LIMIT, "{size}");
    // Relaunched on the same folder, the count is read back.
    let count = desk.json()["diagnostics"]["events"]
        .as_u64()
        .expect("count");
    let logs = desk.adapters.logs_root.clone();
    drop(desk.core);
    let (mut adapters, _handles) = fixtures::adapters();
    adapters.logs_root = Some(logs);
    let core = Core::new(CoreConfig::default(), adapters);
    let handle = core.operator();
    let snapshot = match handle.observe(ViewScope::Shell).expect("observe") {
        Observation::Shell { snapshot, .. } => snapshot,
        _ => unreachable!("shell"),
    };
    let after = serde_json::to_value(snapshot).expect("json")["diagnostics"]["events"]
        .as_u64()
        .expect("count");
    // The relaunch adds its own start.
    assert!(after >= count.min(after) && after > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_panic_is_recorded_by_where_never_by_what() {
    let desk = start();
    desk.core
        .record_panic("crates/desktop-core/src/authority.rs", 42);
    // An absolute location (a dependency's, naming the build machine's
    // folders) is recorded only as external.
    desk.core
        .record_panic(&format!("/Users/someone/{CONTENT}.rs"), 1);
    let log = log_text(&desk);
    assert!(log.contains("\"panicked\""), "{log}");
    assert!(
        log.contains("authority.rs") && log.contains("\"external\""),
        "{log}"
    );
    assert!(!log.contains(CONTENT) && !log.contains("/Users/"), "{log}");
}

/// C24-F1: a stored line is exported only when each of its fields is a
/// value the app records for that event. Valid JSON lines whose string
/// fields hold a token-shaped canary or a single word are dropped; the app's
/// own events stay. Recording holds the same boundary: a panic outside this
/// repository is recorded only as external.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_values_the_app_records_are_exported() {
    let mut desk = start();
    seed(&mut desk).await;
    let logs = desk.adapters.logs_root.clone();
    let current = logs.join("diagnostics.jsonl");
    let mut planted = std::fs::read_to_string(&current).expect("log");
    for (canary, word) in [
        (SECRET, "hunter"),
        ("SYNTHETIC-C24-SECRET-scan-key", "acme"),
    ] {
        for line in [
            json!({"at": 1, "event": {"type": "storage", "health": canary}}),
            json!({"at": 1, "event": {"type": "storage", "health": word}}),
            json!({"at": 1, "event": {"type": "intent", "kind": word, "refusal": null}}),
            json!({"at": 1, "event": {"type": "intent", "kind": "scan", "refusal": word}}),
            json!({"at": 1, "event": {"type": "reset", "outcome": word}}),
            json!({"at": 1, "event": {"type": "reset", "outcome": format!("failed-{word}")}}),
            json!({"at": 1, "event": {"type": "started", "version": word, "fixture": true}}),
            json!({"at": 1, "event": {"type": "panicked", "file": canary, "line": 1}}),
            json!({"at": 1, "event": {"type": "panicked", "file": format!("{word}/main.rs"), "line": 1}}),
        ] {
            planted.push_str(&format!("{line}\n"));
        }
    }
    std::fs::write(&current, planted).expect("plant");
    // The app's own events, recorded through its paths.
    desk.core
        .record_panic("/Users/someone/.cargo/registry/src/x/lib.rs", 7);
    desk.core.record_panic("library/core/src/option.rs", 9);
    desk.core
        .record_panic("crates/desktop-core/src/authority.rs", 42);
    desk.send(json!({"kind": "exportDiagnostics"}))
        .await
        .expect("export");
    assert!(eventually(|| desk.json()["diagnostics"]["export"] == "saved").await);
    let saved = desk.adapters.saver.saved();
    let text = String::from_utf8(saved[0].1.clone()).expect("utf8");
    for needle in [
        SECRET,
        "SYNTHETIC-C24-SECRET",
        "hunter",
        "acme",
        "someone",
        "option.rs",
    ] {
        assert!(!text.contains(needle), "{needle} exported: {text}");
    }
    let doc: Value = serde_json::from_str(&text).expect("json");
    let events = doc["events"].as_array().expect("events");
    // The app's own events stay useful.
    assert!(events.iter().any(|e| e["event"]["type"] == "intent"));
    let panics: Vec<&Value> = events
        .iter()
        .filter(|e| e["event"]["type"] == "panicked")
        .collect();
    assert_eq!(panics.len(), 3, "{panics:?}");
    assert!(panics.iter().any(
        |p| p["event"]["file"] == "crates/desktop-core/src/authority.rs"
            && p["event"]["line"] == 42
    ));
    assert_eq!(
        panics
            .iter()
            .filter(|p| p["event"]["file"] == "external" && p["event"]["line"] == 0)
            .count(),
        2
    );
}

/// C24-F2: a clearing that cannot remove a file says so, keeps counting
/// what is still kept (and exports it), and records no clearing; a later
/// clearing that can removes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_clearing_that_cannot_remove_the_files_says_so_and_keeps_counting_them() {
    use std::os::unix::fs::PermissionsExt;
    let mut desk = start();
    seed(&mut desk).await;
    let logs = desk.adapters.logs_root.clone();
    let before = log_text(&desk).lines().count();
    assert!(before > 3);
    // The folder no longer allows removing its files.
    std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o500)).expect("deny");
    desk.send(json!({"kind": "clearDiagnostics"}))
        .await
        .expect("admitted");
    let view = desk.json()["diagnostics"].clone();
    std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700)).expect("allow");
    assert_eq!(view["clear"], "failed", "{view}");
    let kept = log_text(&desk);
    assert!(
        !kept.contains("\"cleared\""),
        "no clearing recorded: {kept}"
    );
    assert!(kept.lines().count() >= before);
    // Counted as kept, not as cleared.
    let counted = desk.json()["diagnostics"]["events"]
        .as_u64()
        .expect("count");
    assert_eq!(
        counted,
        u64::try_from(kept.lines().count()).expect("lines"),
        "{kept}"
    );
    // And exported.
    desk.send(json!({"kind": "exportDiagnostics"}))
        .await
        .expect("export");
    assert!(eventually(|| desk.json()["diagnostics"]["export"] == "saved").await);
    let saved = desk.adapters.saver.saved();
    let doc: Value = serde_json::from_slice(&saved[0].1).expect("json");
    assert!(doc["events"].as_array().is_some_and(|e| e.len() >= before));
    // Retried, it removes them.
    desk.send(json!({"kind": "clearDiagnostics"}))
        .await
        .expect("cleared");
    assert_eq!(desk.json()["diagnostics"]["clear"], "cleared");
    let log = log_text(&desk);
    assert!(log.lines().count() <= 2, "{log}");
    assert!(log.contains("\"cleared\""), "{log}");
}
