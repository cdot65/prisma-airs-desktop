//! C11 sessions investigations and metadata-only FP/FN reports through the
//! production authority interface: the 24-hour inventory in bounded pages,
//! session, transaction and content reads (content only on an explicit
//! reveal), v1's bounded searches, the investigation cache (sealed under a
//! Keychain key for Keychain credentials, memory-only otherwise, removed
//! with its credential, cleared and exported as metadata), and reports built
//! from scan metadata, edited, removed, undone, persisted and exported as
//! CSV, without a single write to the service.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles, SaveScript};
use desktop_core::management::{ManagementEndpoints, ManagementFailure, ManagementResult};
use desktop_core::sessions::{SearchKind, csv_cell};
use desktop_core::store::PersistedState;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, ExportState, Intent, Observation, Receipt,
    Refusal, ReportsState, ScanState, ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c11";
const TSG: &str = "1234567890";
const PROMPT: &str = "SYNTHETIC-PROMPT-CONTENT-c11";
const RESPONSE: &str = "SYNTHETIC-RESPONSE-CONTENT-c11";
const DASH: &str = "/aisec/v1/mgmt/dashboard/v2/sessions/";
const CONTENT: &str = "/aisec/v1/mgmt/reports/scancontent";

/// What the synthetic dashboard answers.
struct Script {
    /// Sessions in the inventory window (generated, newest first).
    sessions: usize,
    /// Every seventh session reports this detector.
    detector_every: usize,
}

fn session_row(n: usize, detector_every: usize) -> Value {
    let breakdown = if n.is_multiple_of(detector_every) {
        json!([{"detection_type": "prompt_injection", "violation_breakdown": {"total": 1, "high": 1}},
               {"detection_type": "dlp", "violation_breakdown": {"total": 0}}])
    } else {
        json!([{"detection_type": "dlp", "violation_breakdown": {"total": 0}}])
    };
    json!({"session_id": format!("ses-{n}"), "application_id": format!("app-{}", n % 3),
           "application_name": format!("Bot {}", n % 3), "status": "block",
           "detection_type_violation_breakdown": breakdown, "future_field": {"kept": true},
           "api_key": "SYNTHETIC-SECRET-FIELD"})
}

fn action(session: &str, app: &str, k: usize) -> Value {
    json!({"scan_id": format!("scn-{session}-{k}"), "scan_sub_req_id": 0, "session_id": session,
           "application_id": app, "status": if k == 0 { "block" } else { "allow" },
           "profile_name": "Production guardrails",
           "detection_type_violation_breakdown": if k == 0 {
               json!([{"detection_type": "prompt_injection", "violation_breakdown": {"total": 1}}])
           } else { json!([]) },
           "violation_breakdown": {"total": if k == 0 { 1 } else { 0 }},
           "prompt": PROMPT, "response": RESPONSE})
}

impl Script {
    fn route(&mut self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        let query: Vec<(String, String)> = target
            .split_once('?')
            .map(|(_, q)| {
                q.split('&')
                    .filter_map(|kv| kv.split_once('='))
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let q = |name: &str| {
            query
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        if path == "/oauth2/access_token" {
            return raw(
                200,
                &json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        if path == format!("{DASH}sessionsoverview") {
            let offset: usize = q("offset").parse().unwrap_or(0);
            let limit: usize = q("limit").parse().unwrap_or(25);
            let end = (offset + limit).min(self.sessions);
            let items: Vec<Value> = (offset..end)
                .map(|n| session_row(n, self.detector_every))
                .collect();
            return raw(
                200,
                &json!({"items": items, "pagination": {"total_items": self.sessions}}),
            );
        }
        if path == format!("{DASH}session") {
            let session = q("session_id");
            let app = q("app_id");
            let actions: Vec<Value> = (0..3).map(|k| action(&session, &app, k)).collect();
            return raw(
                200,
                &json!({"session_id": session, "application_id": app,
                "application_name": q("app_name").replace('+', " "), "session_actions": actions}),
            );
        }
        if path == format!("{DASH}sessiontransaction") {
            return raw(
                200,
                &json!({"scan_id": q("scan_id"), "scan_sub_req_id": 0, "verdict": "malicious",
                "prompt": PROMPT, "response": RESPONSE,
                "scan_contents": [{"prompt": PROMPT, "response": RESPONSE}],
                "prompt_masked_data": {"data": PROMPT}}),
            );
        }
        if path == CONTENT {
            if q("scan_id") == "scn-missing" {
                return raw(404, &json!({"error": "not found"}));
            }
            return raw(
                200,
                &json!({"scan_id": q("scan_id"), "report_id": "R-1", "category": "malicious",
                "scan_contents": {"prompt": PROMPT, "response": RESPONSE}}),
            );
        }
        raw(404, &json!({"error": "not found"}))
    }
}

fn raw(status: u16, body: &Value) -> Reply {
    let body = serde_json::to_vec(body).expect("json");
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
    ca: TestCa,
}

async fn desk(sessions: usize) -> Desk {
    let ca = TestCa::new("airs-v2 c11 ca");
    let script = Arc::new(Mutex::new(Script {
        sessions,
        detector_every: 7,
    }));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("script").route(seen)
        })),
    )
    .await;
    let mut desk = start(ca, server, script, None).await;
    let origin = format!("https://localhost:{}", desk.server.port);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant =A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    desk
}

async fn start(
    ca: TestCa,
    server: TlsServer,
    script: Arc<Mutex<Script>>,
    restart: Option<(Handles, PersistedState)>,
) -> Desk {
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = match restart {
        None => fixtures::adapters(),
        Some((old, state)) => {
            let (mut adapters, mut handles) =
                fixtures::adapters_with(fixtures::ScriptedStore::with_state(state));
            adapters.keychain = old.vault.clone();
            handles.vault = old.vault;
            adapters.sessions_root = Some(old.sessions_root.clone());
            handles.sessions_root = old.sessions_root;
            (adapters, handles)
        }
    };
    let config = CoreConfig {
        management: ManagementEndpoints::at_origin(&origin),
        ..CoreConfig::default()
    };
    let core = Core::new(config, adapters);
    core.start();
    core.set_transport_policy(TransportPolicy {
        revision: 2,
        proxy: ProxyMode::Direct,
        extra_roots: vec![ca.pem.as_bytes().to_vec()],
    })
    .expect("policy");
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    Desk {
        core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        script,
        server,
        ca,
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

    fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    async fn management_key(&mut self, mode: &str) {
        let context = self.context();
        self.send(
            json!({"kind": "configureManagementCredential", "context": context, "mode": mode,
            "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
        )
        .await
        .expect("management key");
        assert!(eventually(|| self.snap().tenants[0].management.state == ScanState::Ready).await);
    }

    async fn read(&mut self, request: Value) -> ManagementResult {
        let context = self.context();
        let receipt = self
            .send(json!({"kind": "management", "context": context, "request": request}))
            .await
            .expect("admitted");
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = receipt
        else {
            panic!("no operation");
        };
        let Ok(Observation::Management(handle)) =
            self.handle.observe(ViewScope::Management(operation))
        else {
            panic!("management observation");
        };
        tokio::time::timeout(Duration::from_secs(30), handle.result())
            .await
            .expect("result in time")
    }

    async fn open_session(&mut self, n: usize) -> desktop_core::management::SessionRead {
        let request = json!({"kind": "session", "sessionId": format!("ses-{n}"),
            "appId": format!("app-{}", n % 3), "appName": format!("Bot {}", n % 3)});
        match self.read(request).await {
            ManagementResult::Session { session } => session,
            other => panic!("not a session: {other:?}"),
        }
    }

    async fn reveal(&mut self, scan: &str) -> desktop_core::management::ScanContent {
        match self
            .read(json!({"kind": "content", "scanId": scan, "subRequest": 0}))
            .await
        {
            ManagementResult::Content { content } => content,
            other => panic!("not content: {other:?}"),
        }
    }

    async fn search(&mut self, query: &str) -> desktop_core::management::SessionSearch {
        match self.read(json!({"kind": "search", "query": query})).await {
            ManagementResult::Search { search } => search,
            other => panic!("not a search: {other:?}"),
        }
    }

    fn requests(&self, prefix: &str) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| {
                s.request_line
                    .split(' ')
                    .nth(1)
                    .is_some_and(|p| p.starts_with(prefix))
            })
            .collect()
    }

    fn snapshot_files(&self) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(self.adapters.sessions_root.join("investigations"))
            .map(|d| d.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default()
    }

    async fn open_reports(&mut self) {
        let context = self.context();
        self.send(json!({"kind": "openReports", "context": context}))
            .await
            .expect("open reports");
        assert!(eventually(|| self.snap().sessions.reports.state == ReportsState::Ready).await);
    }

    async fn report(&mut self, n: usize, k: usize, reason: &str) -> Result<Receipt, Refusal> {
        let context = self.context();
        self.send(json!({"kind": "saveReport", "context": context,
            "target": {"scanId": format!("scn-ses-{n}-{k}"), "subRequest": 0,
                       "sessionId": format!("ses-{n}"), "appId": format!("app-{}", n % 3),
                       "appName": format!("Bot {}", n % 3)},
            "input": {"kind": "false_positive", "category": "Benign business content", "reason": reason}}))
            .await
    }

    async fn restart(self) -> Desk {
        let state = self.adapters.store.saved().expect("saved");
        let Desk {
            adapters,
            script,
            server,
            ca,
            core,
            ..
        } = self;
        core.shutdown();
        drop(core);
        start(ca, server, script, Some((adapters, state))).await
    }
}

fn items(json_text: &str) -> Vec<Value> {
    serde_json::from_str(json_text).expect("items")
}

fn query_of(seen: &Seen) -> String {
    seen.request_line
        .split(' ')
        .nth(1)
        .and_then(|t| t.split_once('?'))
        .map(|(_, q)| q.to_owned())
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_inventory_is_the_last_day_in_bounded_pages_with_secret_fields_removed() {
    let mut desk = desk(30).await;
    desk.management_key("keychain").await;
    let page = match desk
        .read(json!({"kind": "page", "resource": "sessions", "offset": 0, "limit": 25}))
        .await
    {
        ManagementResult::Page { page } => page,
        other => panic!("not a page: {other:?}"),
    };
    assert_eq!(
        (page.count, page.next_offset, page.complete, page.total),
        (25, Some(25), false, Some(30))
    );
    let rows = items(&page.items);
    assert_eq!(
        rows[0]["future_field"],
        json!({"kept": true}),
        "unknown values kept"
    );
    assert!(
        rows.iter().all(|r| r.get("api_key").is_none()),
        "secret fields removed"
    );
    let seen = desk.requests(&format!("{DASH}sessionsoverview"));
    let query = query_of(&seen[0]);
    for part in ["time_interval=1", "time_unit=day", "limit=25", "offset=0"] {
        assert!(query.contains(part), "{query} has {part}");
    }
    let last = match desk
        .read(json!({"kind": "page", "resource": "sessions", "offset": 25, "limit": 25}))
        .await
    {
        ManagementResult::Page { page } => page,
        other => panic!("not a page: {other:?}"),
    };
    assert_eq!(
        (last.count, last.next_offset, last.complete),
        (5, None, true)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn content_is_read_only_on_reveal_and_never_kept_with_metadata() {
    let mut desk = desk(10).await;
    desk.management_key("keychain").await;
    let session = desk.open_session(1).await;
    assert_eq!((session.actions, session.more), (3, false));
    assert!(!session.session.contains(PROMPT) && !session.session.contains(RESPONSE));
    let sent = desk.requests(&format!("{DASH}session?"));
    let query = query_of(&sent[0]);
    for part in [
        "session_id=ses-1",
        "app_id=app-1",
        "limit=25",
        "offset=0",
        "time_interval=30",
    ] {
        assert!(query.contains(part), "{query} has {part}");
    }
    let transaction = match desk
        .read(
            json!({"kind": "transaction", "sessionId": "ses-1", "appId": "app-1",
            "appName": "Bot 1", "scanId": "scn-ses-1-0", "subRequest": 0}),
        )
        .await
    {
        ManagementResult::Transaction { transaction } => transaction,
        other => panic!("not a transaction: {other:?}"),
    };
    assert!(!transaction.contains(PROMPT) && transaction.contains("malicious"));
    assert!(
        desk.requests(CONTENT).is_empty(),
        "no content before a reveal"
    );
    let content = desk.reveal("scn-ses-1-0").await;
    assert_eq!(
        (content.prompt.as_deref(), content.response.as_deref()),
        (Some(PROMPT), Some(RESPONSE))
    );
    // A second reveal of the same scan is answered by this credential's cache.
    let again = desk.reveal("scn-ses-1-0").await;
    assert_eq!(again, content);
    assert_eq!(desk.requests(CONTENT).len(), 1);
    let view = desk.snap().sessions.investigation.expect("investigation");
    assert_eq!((view.sessions, view.transactions, view.contents), (1, 1, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn searches_are_bounded_like_v1_and_say_what_they_covered() {
    let mut desk = desk(5_000).await;
    desk.management_key("session").await;
    // Session ID: 30 days, pages of 100, at most 40 pages.
    let found = desk.search("ses-150").await;
    assert_eq!(found.kind, SearchKind::SessionId);
    assert_eq!(items(&found.sessions).len(), 1);
    assert!(
        !found.complete,
        "4,000 of 5,000 sessions read proves nothing"
    );
    assert_eq!(found.scanned, 4_000);
    let walk = desk.requests(&format!("{DASH}sessionsoverview"));
    assert_eq!(walk.len(), 40);
    assert!(walk.iter().all(|s| {
        let q = query_of(s);
        q.contains("limit=100") && q.contains("time_interval=30") && q.contains("time_unit=days")
    }));
    // Detector: 24 hours, at most 1,000 sessions, stopping at 100 matches.
    let detector = desk.search("prompt_injection").await;
    assert_eq!(detector.kind, SearchKind::Detector);
    assert_eq!(items(&detector.sessions).len(), 100);
    assert!(detector.match_limit && !detector.complete);
    assert_eq!(
        detector.scanned, 700,
        "stops after the page reaching 100 matches"
    );
    let none = desk.search("toxic_content").await;
    assert_eq!(
        (none.scanned, none.complete, none.match_limit),
        (1_000, false, false)
    );
    // Scan ID: the scan's metadata, never its content; absence is proven.
    let scan = desk.search("scn-ses-2-0").await;
    let metadata = scan.scan.expect("scan metadata");
    assert!(metadata.contains("R-1") && !metadata.contains(PROMPT));
    assert!(scan.complete);
    let missing = desk.search("scn-missing").await;
    assert_eq!((missing.scan, missing.complete), (None, true));
    // Queries v1 would not classify are refused before admission.
    let context = desk.context();
    for query in ["", "Has Spaces", &"x".repeat(300)] {
        assert_eq!(
            desk.send(json!({"kind": "management", "context": context,
                "request": {"kind": "search", "query": query}}))
                .await,
            Err(Refusal::InvalidInput),
            "{query:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_small_window_proves_absence_and_a_cached_session_answers_its_search() {
    let mut desk = desk(120).await;
    desk.management_key("session").await;
    let absent = desk.search("ses-9999").await;
    assert!(items(&absent.sessions).is_empty());
    assert!(absent.complete && absent.scanned == 120);
    let walks = desk.requests(&format!("{DASH}sessionsoverview")).len();
    desk.open_session(4).await;
    let cached = desk.search("ses-4").await;
    assert!(cached.cached && cached.complete);
    let row = &items(&cached.sessions)[0];
    assert_eq!(row["session_id"], "ses-4");
    assert!(row.get("session_actions").is_none() && !cached.sessions.contains(PROMPT));
    assert_eq!(
        desk.requests(&format!("{DASH}sessionsoverview")).len(),
        walks,
        "answered without a walk"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_keychain_credentials_investigation_is_sealed_and_survives_a_restart() {
    let mut desk = desk(10).await;
    desk.management_key("keychain").await;
    desk.open_session(1).await;
    desk.reveal("scn-ses-1-0").await;
    assert!(eventually(|| desk.snapshot_files().len() == 1).await);
    let sealed = std::fs::read(&desk.snapshot_files()[0]).expect("snapshot");
    let text = String::from_utf8_lossy(&sealed);
    assert!(
        !text.contains(PROMPT) && !text.contains("ses-1"),
        "nothing in plaintext"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&desk.snapshot_files()[0])
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // Credential record + its investigation key.
    assert_eq!(desk.adapters.vault.records().len(), 2);
    assert_eq!(
        desk.snap().sessions.investigation.map(|i| i.storage),
        Some(desktop_core::sessions::InvestigationStorage::Protected)
    );
    let mut desk = desk.restart().await;
    let content_reads = desk.requests(CONTENT).len();
    let content = desk.reveal("scn-ses-1-0").await;
    assert_eq!(content.prompt.as_deref(), Some(PROMPT));
    assert_eq!(
        desk.requests(CONTENT).len(),
        content_reads,
        "from the sealed cache"
    );
    // A snapshot that no longer opens (its key changed) is discarded.
    let mut desk = desk.restart().await;
    let key = desk
        .adapters
        .vault
        .records()
        .into_iter()
        .find(|r| desk.adapters.vault.secret(r).is_some_and(|s| s.len() == 32))
        .expect("investigation key");
    desk.adapters.vault.plant(&key, &[7u8; 32]);
    desk.reveal("scn-ses-1-0").await;
    assert_eq!(desk.requests(CONTENT).len(), content_reads + 1, "refetched");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_credentials_investigation_stays_in_memory() {
    let mut desk = desk(10).await;
    desk.management_key("session").await;
    desk.open_session(1).await;
    desk.reveal("scn-ses-1-0").await;
    assert!(desk.snapshot_files().is_empty());
    assert_eq!(
        desk.snap().sessions.investigation.map(|i| i.storage),
        Some(desktop_core::sessions::InvestigationStorage::Memory)
    );
    assert!(desk.adapters.vault.records().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn removing_the_credential_removes_its_sealed_investigation_and_key() {
    let mut desk = desk(10).await;
    desk.management_key("keychain").await;
    desk.open_session(1).await;
    desk.reveal("scn-ses-1-0").await;
    assert!(eventually(|| desk.snapshot_files().len() == 1).await);
    let context = desk.context();
    desk.send(json!({"kind": "removeManagementCredential", "context": context}))
        .await
        .expect("removed");
    assert!(eventually(|| desk.snapshot_files().is_empty()).await);
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
    assert!(desk.snap().sessions.investigation.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn clearing_empties_the_cache_and_exports_carry_metadata_only() {
    let mut desk = desk(10).await;
    desk.management_key("keychain").await;
    desk.open_session(1).await;
    desk.read(
        json!({"kind": "transaction", "sessionId": "ses-1", "appId": "app-1",
        "appName": "Bot 1", "scanId": "scn-ses-1-0", "subRequest": 0}),
    )
    .await;
    desk.reveal("scn-ses-1-0").await;
    let context = desk.context();
    desk.send(json!({"kind": "exportInvestigation", "context": context}))
        .await
        .expect("export");
    assert!(
        eventually(|| desk
            .snap()
            .sessions
            .export
            .is_some_and(|e| e.state == ExportState::Saved))
        .await
    );
    let (name, bytes) = desk.adapters.saver.saved().pop().expect("saved");
    assert!(name.starts_with("investigation-tenant-a-") && name.ends_with(".json"));
    let text = String::from_utf8(bytes).expect("utf8");
    assert!(text.contains("ses-1") && text.contains("scn-ses-1-0"));
    assert!(
        !text.contains(PROMPT) && !text.contains(RESPONSE),
        "no content exported"
    );
    // A cancelled save sheet saves nothing and says so.
    desk.adapters.saver.script(SaveScript::Cancel);
    desk.send(json!({"kind": "exportInvestigation", "context": context}))
        .await
        .expect("export");
    assert!(
        eventually(|| desk
            .snap()
            .sessions
            .export
            .is_some_and(|e| e.state == ExportState::Cancelled && e.name.is_none()))
        .await
    );
    assert!(eventually(|| desk.snapshot_files().len() == 1).await);
    desk.send(json!({"kind": "clearInvestigation", "context": context}))
        .await
        .expect("clear");
    assert!(
        eventually(|| desk.snap().sessions.investigation.is_some_and(|i| (
            i.sessions,
            i.transactions,
            i.contents
        ) == (0, 0, 0)))
        .await
    );
    assert!(eventually(|| desk.snapshot_files().is_empty()).await);
    desk.reveal("scn-ses-1-0").await;
    assert_eq!(
        desk.requests(CONTENT).len(),
        2,
        "cleared content is read again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_are_built_from_scan_metadata_and_kept_edited_removed_and_undone() {
    let mut desk = desk(10).await;
    desk.management_key("keychain").await;
    desk.open_reports().await;
    // A scan this investigation has not read cannot be reported.
    assert_eq!(
        desk.report(1, 0, "Looks fine").await,
        Err(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.snap().sessions.reports.problem,
        Some(desktop_core::sessions::ReportProblem::Scan)
    );
    desk.open_session(1).await;
    desk.report(1, 0, "=SUM(A1) order numbers")
        .await
        .expect("saved");
    let report = desk.snap().sessions.reports.items[0].clone();
    assert_eq!(report.scan_id, "scn-ses-1-0");
    assert_eq!(
        (
            report.verdict.as_str(),
            report.action.as_str(),
            report.profile.as_str()
        ),
        ("malicious", "Blocked", "Production guardrails")
    );
    assert_eq!(report.detectors, ["prompt_injection"]);
    assert_eq!(
        (report.application.as_str(), report.session_id.as_str()),
        ("Bot 1", "ses-1")
    );
    // An empty reason is refused with its problem; nothing changes.
    assert_eq!(desk.report(1, 1, "   ").await, Err(Refusal::InvalidInput));
    assert_eq!(
        desk.snap().sessions.reports.problem,
        Some(desktop_core::sessions::ReportProblem::Reason)
    );
    // A category must be one of v1's quick picks for the kind.
    let context = desk.context();
    assert_eq!(
        desk.send(json!({"kind": "saveReport", "context": context,
            "target": {"scanId": "scn-ses-1-1", "subRequest": 0, "sessionId": "ses-1",
                       "appId": "app-1", "appName": "Bot 1"},
            "input": {"kind": "false_negative", "category": "=HYPERLINK(1)", "reason": "Reason"}}))
            .await,
        Err(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.snap().sessions.reports.problem,
        Some(desktop_core::sessions::ReportProblem::Field)
    );
    desk.report(1, 1, "Benign verdict was wrong")
        .await
        .expect("second");
    // Editing keeps the original report time and position.
    desk.report(1, 0, "Edited reason").await.expect("edited");
    let reports = desk.snap().sessions.reports.items;
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[1].reason, "Edited reason");
    assert_eq!(reports[1].reported_at, report.reported_at);
    // Remove and undo restore it where it stood.
    let context = desk.context();
    desk.send(json!({"kind": "removeReport", "context": context, "scanId": "scn-ses-1-0", "subRequest": 0}))
        .await
        .expect("removed");
    assert_eq!(desk.snap().sessions.reports.items.len(), 1);
    assert!(desk.snap().sessions.reports.undo.is_some());
    desk.send(json!({"kind": "undoReport", "context": context}))
        .await
        .expect("undone");
    assert_eq!(desk.snap().sessions.reports.items, reports);
    assert_eq!(
        desk.send(json!({"kind": "undoReport", "context": context}))
            .await,
        Err(Refusal::InvalidInput),
        "one undo"
    );
    // The file is owner-only metadata, and survives a restart.
    let path = desktop_core::sessions::reports_path(
        &desk.adapters.sessions_root,
        desk.snap().tenants[0].id.as_str(),
    );
    assert!(
        eventually(
            || desktop_core::sessions::load_reports(&path).is_ok_and(|b| b.reports == reports)
        )
        .await
    );
    let text = std::fs::read_to_string(&path).expect("file");
    assert!(!text.contains(PROMPT) && !text.contains(RESPONSE));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let mut desk = desk.restart().await;
    desk.open_reports().await;
    assert_eq!(desk.snap().sessions.reports.items, reports);
    // Only reads reached the service: a report writes nothing there.
    assert!(desk.server.seen().iter().all(|s| {
        s.request_line.starts_with("GET ") || s.request_line.contains("/oauth2/access_token")
    }));
    // Deleting the tenant removes its reports.
    let tenant = desk.snap().tenants[0].id.clone();
    desk.send(json!({"kind": "deleteTenant", "tenant": tenant}))
        .await
        .expect("deleted");
    assert!(eventually(|| !path.exists()).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_export_as_v1_csv_through_the_save_sheet() {
    let mut desk = desk(10).await;
    desk.management_key("session").await;
    desk.open_reports().await;
    desk.open_session(2).await;
    desk.report(2, 0, "-1 \"quoted\", comma")
        .await
        .expect("saved");
    let context = desk.context();
    desk.send(json!({"kind": "exportReports", "context": context}))
        .await
        .expect("export");
    assert!(
        eventually(|| desk
            .snap()
            .sessions
            .export
            .is_some_and(|e| e.state == ExportState::Saved))
        .await
    );
    let (name, bytes) = desk.adapters.saver.saved().pop().expect("saved");
    assert!(
        name.starts_with("fpfn-reports-tenant-a-") && name.ends_with(".csv"),
        "{name}"
    );
    let csv = String::from_utf8(bytes).expect("utf8");
    let lines: Vec<&str> = csv.split("\r\n").collect();
    assert_eq!(
        lines[0],
        "tenant,tsg_id,kind,scan_id,scan_sub_req_id,session_id,application,application_id,\
         verdict,action,detectors,profile,reason_category,reason,reported_at,exported_at"
    );
    assert!(
        lines[1]
            .starts_with("Tenant =A,1234567890,false_positive,scn-ses-2-0,0,ses-2,Bot 2,app-2,")
    );
    // v1's formula guard and RFC 4180 quoting.
    assert!(lines[1].contains(",\"'-1 \"\"quoted\"\", comma\","));
    assert_eq!(
        csv_cell("-1 \"quoted\", comma"),
        "\"'-1 \"\"quoted\"\", comma\""
    );
    assert_eq!(lines.last(), Some(&""), "CRLF-terminated");
    // A failed save says so.
    desk.adapters.saver.script(SaveScript::Fail);
    desk.send(json!({"kind": "exportReports", "context": context}))
        .await
        .expect("export");
    assert!(
        eventually(|| desk
            .snap()
            .sessions
            .export
            .is_some_and(|e| e.state == ExportState::Failed))
        .await
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_need_the_active_context_and_an_unreadable_file_is_kept() {
    let mut desk = desk(10).await;
    desk.management_key("session").await;
    let stale = desk.context();
    let tenant = desk.snap().tenants[0].id.clone();
    let path = desktop_core::sessions::reports_path(&desk.adapters.sessions_root, tenant.as_str());
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    std::fs::write(&path, b"{not json").expect("plant");
    desk.send(json!({"kind": "openReports", "context": stale}))
        .await
        .expect("open");
    assert!(eventually(|| desk.snap().sessions.reports.state == ReportsState::Failed).await);
    assert_eq!(
        desk.snap().sessions.reports.failure,
        Some(desktop_core::sessions::ReportStoreFailure::Unreadable)
    );
    assert_eq!(std::fs::read(&path).expect("kept"), b"{not json");
    assert_eq!(
        desk.send(json!({"kind": "exportReports", "context": stale}))
            .await,
        Err(Refusal::CapabilityMissing)
    );
    // Another context is refused.
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant B", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": "https://localhost:1"}}}),
    )
    .await
    .expect("tenant b");
    let second = desk.snap().tenants[1].id.clone();
    desk.send(json!({"kind": "selectTenant", "tenant": second}))
        .await
        .expect("activate");
    assert_eq!(
        desk.send(json!({"kind": "openReports", "context": stale}))
            .await,
        Err(Refusal::StaleContext)
    );
    assert_eq!(desk.snap().sessions.reports.state, ReportsState::Idle);
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_removes_sealed_investigations_and_reports() {
    let mut desk = desk(10).await;
    desk.management_key("keychain").await;
    desk.open_reports().await;
    desk.open_session(1).await;
    desk.reveal("scn-ses-1-0").await;
    desk.report(1, 0, "Reason").await.expect("saved");
    assert!(eventually(|| desk.snapshot_files().len() == 1).await);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| !desk.adapters.sessions_root.exists()).await);
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_read_reports_its_failure_and_caches_nothing() {
    let mut desk = desk(10).await;
    desk.management_key("session").await;
    let result = desk
        .read(json!({"kind": "content", "scanId": "scn-missing", "subRequest": 0}))
        .await;
    assert!(matches!(
        result,
        ManagementResult::Failed {
            failure: ManagementFailure::NotFound,
            ..
        }
    ));
    assert_eq!(
        desk.snap().sessions.investigation.map(|i| i.contents),
        Some(0)
    );
}
