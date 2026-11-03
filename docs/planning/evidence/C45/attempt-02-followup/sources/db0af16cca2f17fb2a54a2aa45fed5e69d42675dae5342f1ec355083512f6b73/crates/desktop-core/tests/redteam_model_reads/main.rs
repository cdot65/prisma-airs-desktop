//! C13 Red Team and Model/skill observation through the production
//! authority interface against synthetic TLS product endpoints: v1's reads
//! (statistics for a window, the first 100 scans, attack results 100 per
//! page up to ten pages, Model Security scans, AgentGuard 30-day statistics
//! and skill scans), per-source states that never pass for zero, the
//! no-scans confirmation, completeness, per-read skill pseudonyms, and
//! nothing but GETs carrying no names, URIs, findings or prompts.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::{self, Handles};
use desktop_core::management::{ManagementEndpoints, ManagementFailure, ManagementResult};
use desktop_core::red_team::{
    AttackPage, Bar, RedTeamObservation, SupplyChainObservation, pseudonym,
};
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, Intent, Observation, Receipt, Refusal, ScanState,
    ShellSnapshot, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c13";
const TSG: &str = "1234567890";
const SCAN: &str = "11111111-2222-4333-8444-555555555555";
const TARGET: &str = "66666666-7777-4888-8999-aaaaaaaaaaaa";
const RT: &str = "/ai-red-teaming/data-plane";
const MS: &str = "/aims/data";
const AG: &str = "/aiag/data";
const PRIVATE: [&str; 6] = [
    "Secret campaign",
    "Prod support bot",
    "s3://private-bucket/model.safetensors",
    "private-skill-name",
    "ATTACK-PROMPT-TEXT",
    "Finance group",
];

/// What the synthetic endpoints answer: a status and a body per path.
struct Script {
    statistics: (u16, Value),
    scans: (u16, Value),
    attacks: (u16, Value),
    models: (u16, Value),
    skill_statistics: (u16, Value),
    skills: (u16, Value),
}

impl Script {
    fn route(&self, seen: &Seen) -> Reply {
        let target = seen.request_line.split(' ').nth(1).unwrap_or("");
        let path = target.split('?').next().unwrap_or("");
        let (status, body) = match path {
            "/oauth2/access_token" => (
                200,
                json!({"access_token": "mgmt-access", "token_type": "Bearer", "expires_in": 3600}),
            ),
            p if p == format!("{RT}/v1/dashboard/scan-statistics") => self.statistics.clone(),
            p if p == format!("{RT}/v1/scan") => self.scans.clone(),
            p if p == format!("{RT}/v1/report/static/{SCAN}/list-attacks") => self.attacks.clone(),
            p if p == format!("{MS}/v1/scans") => self.models.clone(),
            p if p == format!("{AG}/v1/stats/scans") => self.skill_statistics.clone(),
            p if p == format!("{AG}/v1/scans") => self.skills.clone(),
            _ => (404, json!({"error": "not found"})),
        };
        let body = serde_json::to_vec(&body).expect("json");
        Reply::Raw {
            head: format!(
                "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
                body.len()
            ),
            body,
        }
    }
}

fn red_team_scans(n: usize, total: Option<u64>) -> Value {
    let rows: Vec<Value> = (0..n)
        .map(|i| {
            json!({"uuid": format!("{:08x}-2222-4333-8444-555555555555", i), "tsg_id": TSG,
                   "name": "Secret campaign", "target": {"name": "Prod support bot"},
                   "target_id": TARGET, "target_type": "APPLICATION", "job_type": "STATIC",
                   "status": if i == 0 { "COMPLETED" } else { "RUNNING" },
                   "created_at": "2026-10-18T10:00:00Z"})
        })
        .collect();
    json!({"data": rows, "pagination": {"total_items": total}})
}

fn attacks(n: usize, total: u64) -> Value {
    let rows: Vec<Value> = (0..n)
        .map(|i| {
            json!({"uuid": SCAN, "job_id": SCAN, "prompt": "ATTACK-PROMPT-TEXT", "threat": i % 3 == 0,
                   "severity": "HIGH", "attack_type": "NORMAL", "category": "security",
                   "sub_category": "JAILBREAK", "sub_category_display_name": "Jailbreak",
                   "multi_turn": false, "extra_info": {"response": "TARGET-REPLY"}})
        })
        .collect();
    json!({"data": rows, "pagination": {"total_items": total}})
}

fn default_script() -> Script {
    Script {
        statistics: (
            200,
            json!({"total_scans": 12, "targets_scanned": 3,
                "scan_status": [{"name": "COMPLETED", "count": 8}, {"name": "RUNNING", "count": 2},
                                {"name": "FAILED", "count": 2}],
                "risk_profile": [{"risk_rating": "HIGH", "total": 1}],
                "targets_scanned_by_type": [{"name": "APPLICATION", "count": 3}]}),
        ),
        scans: (200, red_team_scans(3, Some(3))),
        attacks: (200, attacks(100, 250)),
        models: (
            200,
            json!({"scans": [{"uuid": SCAN, "model_uri": "s3://private-bucket/model.safetensors",
                "security_group_name": "Finance group", "eval_outcome": "BLOCKED",
                "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 140}}),
        ),
        skill_statistics: (
            200,
            json!({"unique_skills_scanned": {"count": 4, "percent_change": null},
                "total_vulnerabilities_found": {"count": 6, "percent_change": 10},
                "top_vulnerability": {"vulnerability_type": "prompt_injection", "finding_count": 3}}),
        ),
        skills: (
            200,
            json!({"scans": [{"uuid": SCAN, "name": "private-skill-name", "status": "COMPLETED",
                "eval_outcome": "ALLOWED", "summary": {"vulnerability_count": 2, "attack_chain_count": 0},
                "created_at": "2026-10-18T10:00:00Z"}], "pagination": {"total_items": 1}}),
        ),
    }
}

struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    _adapters: Handles,
    script: Arc<Mutex<Script>>,
    server: TlsServer,
}

async fn desk() -> Desk {
    let ca = TestCa::new("airs-v2 c13 ca");
    let script = Arc::new(Mutex::new(default_script()));
    let routed = Arc::clone(&script);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("script").route(seen)
        })),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = fixtures::adapters();
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
    let mut desk = Desk {
        _core: core,
        handle,
        session,
        sequence: 0,
        _adapters: handles,
        script,
        server,
    };
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "keychain",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("management key");
    assert!(eventually(|| desk.snap().tenants[0].management.state == ScanState::Ready).await);
    desk
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

    fn script(&self) -> std::sync::MutexGuard<'_, Script> {
        self.script.lock().expect("script")
    }

    async fn read(&mut self, request: Value) -> Result<ManagementResult, Refusal> {
        let context = self.context();
        let Receipt::Admitted {
            operation: Some(operation),
            ..
        } = self
            .send(json!({"kind": "management", "context": context, "request": request}))
            .await?
        else {
            panic!("no operation");
        };
        let Ok(Observation::Management(handle)) =
            self.handle.observe(ViewScope::Management(operation))
        else {
            panic!("management observation");
        };
        Ok(
            tokio::time::timeout(Duration::from_secs(20), handle.result())
                .await
                .expect("result in time"),
        )
    }

    async fn red_team(&mut self, window: u8) -> RedTeamObservation {
        match self
            .read(json!({"kind": "redTeam", "window": window}))
            .await
            .expect("admitted")
        {
            ManagementResult::RedTeam { observation } => observation,
            other => panic!("not a Red Teaming read: {other:?}"),
        }
    }

    async fn supply_chain(&mut self) -> SupplyChainObservation {
        match self
            .read(json!({"kind": "supplyChain"}))
            .await
            .expect("admitted")
        {
            ManagementResult::SupplyChain { observation } => observation,
            other => panic!("not a Model/skill read: {other:?}"),
        }
    }

    async fn attacks(&mut self, page: u8, received: u32) -> AttackPage {
        match self
            .read(json!({"kind": "attacks", "scanId": SCAN, "page": page, "received": received}))
            .await
            .expect("admitted")
        {
            ManagementResult::Attacks { attacks } => attacks,
            other => panic!("not attack results: {other:?}"),
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
}

fn leaks(value: &impl serde::Serialize) -> Vec<&'static str> {
    let text = serde_json::to_string(value).expect("json");
    PRIVATE
        .iter()
        .copied()
        .chain(["TARGET-REPLY"])
        .filter(|p| text.contains(p))
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn red_teaming_reads_statistics_and_scans_for_a_window() {
    let mut desk = desk().await;
    let read = desk.red_team(7).await;
    let statistics = read.statistics.as_ref().expect("statistics");
    assert_eq!((statistics.scans, statistics.targets), (12, 3));
    assert_eq!(
        statistics.status.as_deref(),
        Some(
            &[
                Bar {
                    label: "COMPLETED".into(),
                    count: 8
                },
                Bar {
                    label: "RUNNING".into(),
                    count: 2
                },
                Bar {
                    label: "FAILED".into(),
                    count: 2
                },
            ][..]
        )
    );
    let scans = read.scans.as_ref().expect("scans");
    assert_eq!(scans.rows.len(), 3);
    assert!(scans.complete);
    assert_eq!(scans.rows[0].status.as_deref(), Some("COMPLETED"));
    assert_eq!(scans.rows[0].target_id.as_deref(), Some(TARGET));
    assert_eq!(read.window, 7);
    assert!(leaks(&read).is_empty(), "{:?}", leaks(&read));
    // v1's exact requests: the window as a date range, the first 100 scans,
    // no tenant header on the Red Teaming plane.
    let stats = desk.requests(&format!("{RT}/v1/dashboard/scan-statistics"));
    assert_eq!(stats.len(), 1);
    assert!(stats[0].request_line.contains("date_range=LAST_7_DAYS"));
    let list = desk.requests(&format!("{RT}/v1/scan"));
    assert!(list[0].request_line.contains("skip=0") && list[0].request_line.contains("limit=100"));
    assert!(
        desk.requests(RT)
            .iter()
            .all(|s| s.header("x-tsg-id").is_none())
    );
    assert!(
        desk.server
            .seen()
            .iter()
            .filter(|s| !s.request_line.contains("/oauth2/"))
            .all(|s| s.request_line.starts_with("GET "))
    );
    // Other windows are v1's; anything else is refused at admission.
    assert_eq!(desk.red_team(30).await.window, 30);
    assert_eq!(
        desk.read(json!({"kind": "redTeam", "window": 14}))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn absent_statistics_mean_no_scans_only_when_an_empty_list_proves_it() {
    let mut desk = desk().await;
    {
        let mut script = desk.script();
        script.statistics = (404, json!({"error": "no statistics"}));
        script.scans = (200, json!({"data": [], "pagination": {"total_items": 0}}));
    }
    let read = desk.red_team(30).await;
    let statistics = read.statistics.expect("confirmed");
    assert!(statistics.no_scans);
    assert_eq!((statistics.scans, statistics.targets), (0, 0));
    let probe = desk.requests(&format!("{RT}/v1/scan"));
    assert!(probe[0].request_line.contains("limit=1"), "a one-row probe");
    // Scans exist: the absence stays a failure, never zero.
    desk.script().scans = (200, red_team_scans(2, Some(2)));
    let read = desk.red_team(30).await;
    assert_eq!(read.statistics, None);
    assert_eq!(read.statistics_failure, Some(ManagementFailure::NotFound));
    assert_eq!(read.scans.expect("scans").rows.len(), 2);
    // A malformed confirmation proves nothing: with the list itself invalid
    // too, the read fails with the absence, never as "no scans".
    for confirmation in [
        json!({"data": [], "pagination": "invalid"}),
        json!({"data": []}),
        json!({"data": [], "count_capped": null, "pagination": {"total_items": 0}}),
    ] {
        desk.script().scans = (200, confirmation.clone());
        let r = desk
            .read(json!({"kind": "redTeam", "window": 30}))
            .await
            .expect("admitted");
        assert!(
            matches!(
                r,
                ManagementResult::Failed {
                    failure: ManagementFailure::NotFound,
                    ..
                }
            ),
            "{confirmation}: {r:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn each_source_keeps_its_own_state_and_a_read_with_none_fails() {
    let mut desk = desk().await;
    desk.script().statistics = (403, json!({"error": "denied"}));
    let read = desk.red_team(15).await;
    assert_eq!(read.statistics_failure, Some(ManagementFailure::Denied));
    assert!(read.scans.is_some());
    // An unreported total contradicted by the breakdown is invalid.
    desk.script().statistics = (
        200,
        json!({"scan_status": [{"name": "COMPLETED", "count": 2}]}),
    );
    let read = desk.red_team(15).await;
    assert_eq!(
        read.statistics_failure,
        Some(ManagementFailure::InvalidResponse)
    );
    // A list longer than a page, or partial, is never complete.
    desk.script().scans = (200, red_team_scans(100, None));
    let read = desk.red_team(15).await;
    assert!(!read.scans.expect("scans").complete);
    desk.script().scans = (429, json!({"error": "slow down"}));
    let failed = desk
        .read(json!({"kind": "redTeam", "window": 15}))
        .await
        .expect("admitted");
    assert!(
        matches!(failed, ManagementResult::Failed { .. }),
        "no source answered: {failed:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn models_and_skills_read_three_sources_with_per_read_pseudonyms() {
    let mut desk = desk().await;
    let first = desk.supply_chain().await;
    let models = first.models.as_ref().expect("models");
    assert_eq!(models.total, Some(140));
    assert!(!models.complete, "1 of 140 read");
    assert_eq!(models.rows[0].outcome.as_deref(), Some("BLOCKED"));
    let statistics = first.skill_statistics.as_ref().expect("statistics");
    assert_eq!(
        (statistics.unique_skills, statistics.vulnerabilities),
        (4, 6)
    );
    let skills = first.skills.as_ref().expect("skills");
    assert_eq!(skills.rows[0].vulnerabilities, Some(2));
    assert_eq!(skills.rows[0].pseudonym.len(), 8);
    assert!(leaks(&first).is_empty(), "{:?}", leaks(&first));
    assert!(
        !serde_json::to_string(&first)
            .expect("json")
            .contains("prompt_injection")
    );
    // A new read draws a new salt: pseudonyms do not link across reads.
    let second = desk.supply_chain().await;
    assert_ne!(
        second.skills.expect("skills").rows[0].pseudonym,
        skills.rows[0].pseudonym
    );
    assert_ne!(
        pseudonym(&[0; 16], "private-skill-name"),
        skills.rows[0].pseudonym
    );
    // v1's queries; AgentGuard carries the tenant header, Model Security not.
    let model_reads = desk.requests(&format!("{MS}/v1/scans"));
    assert!(
        model_reads[0]
            .request_line
            .contains("isBackgroundRefresh=false")
    );
    assert!(model_reads[0].header("x-tsg-id").is_none());
    let stats = desk.requests(&format!("{AG}/v1/stats/scans"));
    assert!(stats[0].request_line.contains("time_period=30_DAYS"));
    assert_eq!(stats[0].header("x-tsg-id"), Some(TSG));
    // One source refused: the others still answer.
    desk.script().models = (403, json!({"error": "not licensed"}));
    let partial = desk.supply_chain().await;
    assert_eq!(partial.models_failure, Some(ManagementFailure::Denied));
    assert!(partial.skill_statistics.is_some() && partial.skills.is_some());
    // Quiet statistics with missing counts are zero; contradicted ones fail.
    desk.script().skill_statistics = (
        200,
        json!({"unique_skills_scanned": {"count": null}, "total_vulnerabilities_found": {"count": null},
               "top_vulnerability": null}),
    );
    let quiet = desk.supply_chain().await;
    assert_eq!(
        quiet
            .skill_statistics
            .map(|s| (s.unique_skills, s.vulnerabilities)),
        Some((0, 0))
    );
    desk.script().skill_statistics = (
        200,
        json!({"unique_skills_scanned": {"count": null}, "total_vulnerabilities_found": {"count": 2},
               "top_vulnerability": {"vulnerability_type": "x", "finding_count": 2}}),
    );
    let contradicted = desk.supply_chain().await;
    assert_eq!(
        contradicted.skill_statistics_failure,
        Some(ManagementFailure::InvalidResponse)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn attack_results_are_read_100_per_page_up_to_ten_pages_without_prompts() {
    let mut desk = desk().await;
    let first = desk.attacks(0, 0).await;
    assert_eq!(first.rows.len(), 100);
    assert_eq!(first.total, Some(250));
    assert!(!first.complete);
    assert_eq!(first.rows[0].threat, Some(true));
    assert_eq!(first.rows[1].threat, Some(false));
    assert_eq!(first.rows[0].sub_category.as_deref(), Some("Jailbreak"));
    assert!(leaks(&first).is_empty(), "{:?}", leaks(&first));
    desk.script().attacks = (200, attacks(50, 250));
    let third = desk.attacks(2, 200).await;
    assert_eq!(third.rows[0].number, 201);
    assert!(third.complete, "200 received and 50 more cover 250");
    let seen = desk.requests(&format!("{RT}/v1/report/static/{SCAN}/list-attacks"));
    assert!(
        seen[1].request_line.contains("skip=200") && seen[1].request_line.contains("limit=100")
    );
    // The tenth page is the last; identifiers must be job UUIDs.
    assert_eq!(desk.attacks(9, 900).await.page, 9);
    for request in [
        json!({"kind": "attacks", "scanId": SCAN, "page": 10, "received": 0}),
        json!({"kind": "attacks", "scanId": "../../v1/scan", "page": 0, "received": 0}),
        // No more than 100 rows can have been received per earlier page.
        json!({"kind": "attacks", "scanId": SCAN, "page": 1, "received": 101}),
    ] {
        assert_eq!(
            desk.read(request.clone()).await.err(),
            Some(Refusal::InvalidInput),
            "{request}"
        );
    }
    // An outcome that is not a boolean makes the page invalid.
    desk.script().attacks = (200, json!({"data": [{"threat": "yes"}]}));
    let invalid = desk
        .read(json!({"kind": "attacks", "scanId": SCAN, "page": 0, "received": 0}))
        .await
        .expect("admitted");
    assert!(matches!(
        invalid,
        ManagementResult::Failed {
            failure: ManagementFailure::InvalidResponse,
            ..
        }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_need_a_management_client() {
    let ca = TestCa::new("airs-v2 c13 none");
    let server = TlsServer::start(ca.leaf(&["localhost"]), Reply::Close).await;
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, _handles) = fixtures::adapters();
    let core = Core::new(
        CoreConfig {
            management: ManagementEndpoints::at_origin(&origin),
            ..CoreConfig::default()
        },
        adapters,
    );
    core.start();
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    let send = |sequence: u64, intent: Value| {
        let handle = handle.clone();
        let session = session.clone();
        async move {
            handle
                .dispatch(
                    Delivery { session, sequence },
                    serde_json::from_value(intent).expect("intent"),
                )
                .await
        }
    };
    send(
        1,
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    let Observation::Shell { snapshot, .. } = handle.observe(ViewScope::Shell).expect("shell")
    else {
        panic!("shell");
    };
    let context = snapshot.active.expect("active").context.as_str().to_owned();
    assert_eq!(
        send(
            2,
            json!({"kind": "management", "context": context, "request": {"kind": "supplyChain"}})
        )
        .await
        .err(),
        Some(Refusal::CapabilityMissing)
    );
    assert!(server.seen().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn attack_coverage_counts_rows_received_and_capped_totals_are_bounds() {
    let mut desk = desk().await;
    // Empty pages against a total of 150 never claim every attack, however
    // far the pages advance.
    desk.script().attacks = (200, attacks(0, 150));
    for page in 0..3 {
        let read = desk.attacks(page, 0).await;
        assert!(read.rows.is_empty() && !read.complete, "page {page}");
        assert_eq!(read.total, Some(150));
    }
    // An empty report is complete.
    desk.script().attacks = (200, attacks(0, 0));
    assert!(desk.attacks(0, 0).await.complete);
    // A capped count is carried as a lower bound, never complete.
    let mut capped = attacks(10, 10);
    capped["count_capped"] = json!(true);
    desk.script().attacks = (200, capped);
    let read = desk.attacks(0, 0).await;
    assert!(read.capped && !read.complete);
    let mut scans = red_team_scans(3, Some(1000));
    scans["count_capped"] = json!(true);
    desk.script().scans = (200, scans);
    let listed = desk.red_team(30).await.scans.expect("scans");
    assert!(listed.capped && !listed.complete);
    assert_eq!(listed.total, Some(1000));
    desk.script().models = (
        200,
        json!({"scans": [], "count_capped": true, "pagination": {"total_items": 500}}),
    );
    let models = desk.supply_chain().await.models.expect("models");
    assert!(models.capped && !models.complete);
    // Malformed pagination never establishes coverage.
    for page in [
        json!({"data": [], "pagination": "invalid"}),
        json!({"data": [], "count_capped": null, "pagination": {"total_items": 0}}),
    ] {
        desk.script().attacks = (200, page.clone());
        let r = desk
            .read(json!({"kind": "attacks", "scanId": SCAN, "page": 0, "received": 0}))
            .await
            .expect("admitted");
        assert!(
            matches!(
                r,
                ManagementResult::Failed {
                    failure: ManagementFailure::InvalidResponse,
                    ..
                }
            ),
            "{page}: {r:?}"
        );
    }
    desk.script().models = (200, json!({"scans": [], "pagination": "invalid"}));
    let read = desk.supply_chain().await;
    assert_eq!(
        read.models_failure,
        Some(ManagementFailure::InvalidResponse)
    );
    assert!(read.skills.is_some() && read.skill_statistics.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_statistics_fail_alone_and_the_other_sources_stay_usable() {
    let mut desk = desk().await;
    let mut late = vec![];
    for i in 0..64 {
        late.push(json!({"name": format!("S{i}"), "count": 0}));
    }
    late.push(json!({"name": "LATE", "count": 4}));
    for body in [
        json!({}),
        json!({"message": "Internal error"}),
        json!({"error": "denied", "total_scans": 0, "targets_scanned": 0}),
        json!({"targets_scanned": 5}),
        json!({"total_scans": 0, "targets_scanned": 0, "scan_status": late}),
        json!({"targets_scanned": 0, "scan_status": [{"name": "FUTURE_STATUS", "count": 4}]}),
        json!({"success": false, "total_scans": 0, "targets_scanned": 0}),
        // Nested rated targets are validated before the source is quiet.
        json!({"risk_profile": [{"risk_rating": "LOW", "total": 0,
            "targets_by_type": [{"name": "MODEL", "count": -1}]}]}),
        json!({"risk_profile": [{"risk_rating": "LOW", "total": 0, "targets_by_type": "MODEL"}]}),
    ] {
        desk.script().statistics = (200, body.clone());
        let read = desk.red_team(7).await;
        assert_eq!(
            read.statistics_failure,
            Some(ManagementFailure::InvalidResponse),
            "{body}"
        );
        assert!(read.statistics.is_none());
        assert_eq!(read.scans.expect("scans usable").rows.len(), 3, "{body}");
    }
    for body in [
        json!({}),
        json!({"message": "Internal error"}),
        json!({"errors": [{"code": "E"}], "unique_skills_scanned": {"count": 0}}),
        json!({"total_vulnerabilities_found": {"count": 2}, "top_vulnerability": null}),
        json!({"unique_skills_scanned": 4, "total_vulnerabilities_found": {"count": 0}}),
    ] {
        desk.script().skill_statistics = (200, body.clone());
        let read = desk.supply_chain().await;
        assert_eq!(
            read.skill_statistics_failure,
            Some(ManagementFailure::InvalidResponse),
            "{body}"
        );
        assert!(read.models.is_some() && read.skills.is_some(), "{body}");
    }
}
