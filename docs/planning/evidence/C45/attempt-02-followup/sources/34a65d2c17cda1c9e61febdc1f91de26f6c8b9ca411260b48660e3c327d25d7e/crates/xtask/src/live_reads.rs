//! Read-only families (C25) through the production `management` intent:
//! L02 management identity and inventories, L05 sessions, L06 Red Team and
//! Model/skill observations, L07 Gateway reads and L10 operational
//! observation. Each family signs in with its own designated management
//! OAuth client (session-only); receipts record result shapes and sanitized
//! identifiers, never item content.

use std::collections::BTreeMap;

use desktop_core::management::{DetailFound, ManagementFailure, ManagementResult};
use serde_json::{Value, json};

use crate::live_run::{DeskOptions, FamilyResult, Found, Outcomes, Run, Spec, summarize};
use crate::{Result, now_utc};

const INVENTORIES: [&str; 10] = [
    "profiles",
    "topics",
    "applications",
    "apiKeys",
    "deploymentProfiles",
    "dlpCatalog",
    "dataProfiles",
    "dataPatterns",
    "filteringProfiles",
    "dictionaries",
];

const GATEWAY_SCOPED: [&str; 5] = [
    "providers",
    "routes",
    "guardrails",
    "serviceKeys",
    "userKeys",
];

fn page_items(result: &ManagementResult) -> Vec<Value> {
    match result {
        ManagementResult::Page { page } => serde_json::from_str(&page.items).unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn id_of(item: &Value, field: &str) -> Option<String> {
    item.get(field).and_then(|v| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

fn ok(result: &ManagementResult) -> bool {
    !matches!(result, ManagementResult::Failed { .. })
}

fn denied(result: &ManagementResult) -> bool {
    matches!(
        result,
        ManagementResult::Failed {
            failure: ManagementFailure::Denied,
            ..
        }
    )
}

/// Runs `reads` on the desk and records one check from their summaries;
/// passes when every read succeeded.
async fn reads(
    desk: &mut crate::live_run::Desk,
    requests: Vec<(String, Value)>,
) -> Result<(bool, Vec<(String, ManagementResult)>, Value)> {
    let mut all = true;
    let mut results = Vec::new();
    let mut shown = serde_json::Map::new();
    for (label, request) in requests {
        let result = desk.read(request).await?;
        all &= ok(&result);
        shown.insert(label.clone(), summarize(&result));
        results.push((label, result));
    }
    Ok((all, results, Value::Object(shown)))
}

struct Ctx<'r, 'a> {
    run: &'r mut Run<'a>,
    id: String,
    outcomes: Outcomes,
    stop: Option<crate::live_run::Stop>,
}

impl Ctx<'_, '_> {
    fn record(
        &mut self,
        spec: Spec,
        started: String,
        inputs: BTreeMap<String, Value>,
        found: Found,
    ) -> Result<()> {
        let stop = self.run.settle(
            &self.id,
            spec,
            started,
            inputs,
            found,
            None,
            &mut self.outcomes,
        )?;
        if self.stop.is_none() {
            self.stop = stop;
        }
        Ok(())
    }

    fn block_all(&mut self, specs: &[Spec], reason: &str) -> Result<()> {
        for spec in specs {
            self.record(*spec, now_utc(), BTreeMap::new(), Found::blocked(reason))?;
        }
        Ok(())
    }
}

pub(crate) async fn run(run: &mut Run<'_>, id: &str) -> Result<FamilyResult> {
    let mut ctx = Ctx {
        run,
        id: id.to_owned(),
        outcomes: Vec::new(),
        stop: None,
    };
    match id {
        "L02" => l02(&mut ctx).await?,
        "L05" => l05(&mut ctx).await?,
        "L06" => l06(&mut ctx).await?,
        "L07" => l07(&mut ctx).await?,
        _ => l10(&mut ctx).await?,
    }
    Ok(FamilyResult {
        outcomes: ctx.outcomes,
        stop: ctx.stop,
    })
}

const L02_VERIFY: Spec = Spec {
    id: "L02-verify",
    workflows: &["W02"],
};
const L02_INVENTORIES: Spec = Spec {
    id: "L02-inventories",
    workflows: &["W07", "W08", "W09", "W10", "W12"],
};
const L02_PAGINATION: Spec = Spec {
    id: "L02-pagination",
    workflows: &["W07"],
};
const L02_DETAIL: Spec = Spec {
    id: "L02-detail",
    workflows: &["W07"],
};
const L02_DENIED: Spec = Spec {
    id: "L02-least-privilege-denied",
    workflows: &["W02"],
};

async fn l02(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let all = [L02_VERIFY, L02_INVENTORIES, L02_PAGINATION, L02_DETAIL];
    let inputs = ctx.run.inputs("L02", &["client_secret"]);
    let mut desk = match ctx
        .run
        .management_desk("L02", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => {
            ctx.block_all(&all, &e)?;
            ctx.block_all(&[L02_DENIED], &e)?;
            return Ok(());
        }
    };
    let started = now_utc();
    let verify = desk.read(json!({"kind": "verify"})).await?;
    let found = Found::check(
        matches!(verify, ManagementResult::Verified),
        "the management client was granted a token and read one profile",
        "the management client could not be verified",
    )
    .with("verify", summarize(&verify))
    .with("scan_key_used", json!(false));
    ctx.record(L02_VERIFY, started, inputs.clone(), found)?;

    let started = now_utc();
    let requests = INVENTORIES
        .iter()
        .map(|r| {
            (
                (*r).to_owned(),
                json!({"kind": "page", "resource": r, "offset": 0, "limit": 25}),
            )
        })
        .collect();
    let (all_ok, results, shown) = reads(&mut desk, requests).await?;
    let found = Found::check(
        all_ok,
        "every supported Runtime and DLP inventory returned a page",
        "an inventory could not be read",
    )
    .with("pages", shown);
    ctx.record(L02_INVENTORIES, started, inputs.clone(), found)?;

    let started = now_utc();
    let first = desk
        .read(json!({"kind": "page", "resource": "profiles", "offset": 0, "limit": 1}))
        .await?;
    let found = match &first {
        ManagementResult::Page { page } => match page.next_offset {
            Some(next) => {
                let second = desk
                    .read(
                        json!({"kind": "page", "resource": "profiles", "offset": next, "limit": 1}),
                    )
                    .await?;
                let a = page_items(&first)
                    .first()
                    .and_then(|i| id_of(i, "profile_id"));
                let b = page_items(&second)
                    .first()
                    .and_then(|i| id_of(i, "profile_id"));
                Found::check(
                    ok(&second) && a.is_some() && a != b,
                    "the second page continued where the first ended",
                    "the second page did not advance",
                )
                .with("first", summarize(&first))
                .with("second", summarize(&second))
            }
            None => Found::blocked(
                "the profile inventory has a single page; pagination needs at least two profiles",
            )
            .with("first", summarize(&first)),
        },
        _ => Found::failed("the first profile page could not be read")
            .with("first", summarize(&first)),
    };
    ctx.record(L02_PAGINATION, started, inputs.clone(), found)?;

    let started = now_utc();
    let target = results
        .iter()
        .find(|(r, _)| r == "profiles")
        .and_then(|(_, res)| page_items(res).first().and_then(|i| id_of(i, "profile_id")));
    let found = match target {
        Some(target) => {
            let detail = desk
                .read(json!({"kind": "detail", "resource": "profiles", "id": target}))
                .await?;
            Found::check(
                matches!(&detail, ManagementResult::Detail { detail } if detail.found == DetailFound::Found),
                "the profile was found by its bounded inventory walk",
                "the profile detail could not be read",
            )
            .with("detail", summarize(&detail))
            .id("profile_id", &target)
        }
        None => Found::blocked("no profile is listed to read in detail"),
    };
    ctx.record(L02_DETAIL, started, inputs, found)?;
    drop(desk);

    // A designated, more restricted principal must be refused: a read of
    // `restricted_denied_resource` when one is designated, otherwise a
    // write (a read-only principal reads, so only its write can be denied).
    match ctx.run.nonsecret("L02", "restricted_denied_resource") {
        Some(resource) => l02_read_denied(ctx, &resource).await,
        None => l02_write_denied(ctx).await,
    }
}

async fn l02_read_denied(ctx: &mut Ctx<'_, '_>, resource: &str) -> Result<()> {
    let started = now_utc();
    let mut inputs = ctx.run.inputs("L02", &["restricted_client_secret"]);
    inputs.insert("mode".to_owned(), json!("read"));
    let found = match ctx
        .run
        .management_desk_as(
            "L02",
            "restricted_client_id",
            "restricted_client_secret",
            DeskOptions::default(),
        )
        .await?
    {
        Err(e) => Found::blocked(e),
        Ok(mut desk) => {
            let read = desk
                .read(json!({"kind": "page", "resource": resource, "offset": 0, "limit": 1}))
                .await?;
            if ok(&read) {
                Found::failed("unexpected authority: the restricted principal read the inventory")
                    .with("read", summarize(&read))
                    .stopping("unexpected authority", false)
            } else {
                Found::check(
                    denied(&read),
                    "the service denied the restricted principal; nothing was shown",
                    "the read failed, but not as a permission denial",
                )
                .with("read", summarize(&read))
            }
        }
    };
    ctx.record(L02_DENIED, started, inputs, found)
}

/// A change failure that is the service refusing the principal.
fn permission_denied(failure: &Value) -> bool {
    failure["kind"] == "service"
        && matches!(failure["failure"].as_str(), Some("denied" | "unauthorized"))
}

/// The topic named `name`, found by a bounded walk of the topic inventory:
/// its ID when present, `Ok(None)` when the whole inventory was read
/// without it, `Err` when the walk could not finish.
async fn find_topic(
    desk: &mut crate::live_run::Desk,
    name: &str,
) -> Result<std::result::Result<Option<String>, String>> {
    let mut offset = 0u32;
    for _ in 0..20 {
        let page = desk
            .read(json!({"kind": "page", "resource": "topics", "offset": offset, "limit": 50}))
            .await?;
        let ManagementResult::Page { page: listed } = &page else {
            return Ok(Err("the topic inventory could not be read".to_owned()));
        };
        let items = page_items(&page);
        if let Some(item) = items.iter().find(|t| t["topic_name"] == name) {
            return Ok(Ok(id_of(item, "topic_id").or_else(|| Some(String::new()))));
        }
        match listed.next_offset {
            Some(next) if !listed.complete && next > offset => offset = next,
            _ if listed.complete || items.is_empty() => return Ok(Ok(None)),
            _ => return Ok(Err("the topic inventory did not advance".to_owned())),
        }
    }
    Ok(Err(
        "the topic inventory is longer than the bounded walk".to_owned()
    ))
}

/// The restricted principal attempts a reviewed create of a disposable
/// topic through the production change path; the service must refuse it.
/// The full principal then reads the topics back and deletes the topic if
/// it exists after all, which stops the run as unexpected authority.
async fn l02_write_denied(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let started = now_utc();
    let mut inputs = ctx.run.inputs("L02", &["restricted_client_secret"]);
    inputs.insert("mode".to_owned(), json!("write"));
    let Some(prefix) = ctx.run.prefix("L02").filter(|p| !p.is_empty()) else {
        let found = Found::blocked(
            "a write denial needs L02's disposable_prefix (or designate restricted_denied_resource for a read denial)",
        );
        return ctx.record(L02_DENIED, started, inputs, found);
    };
    let name = format!("{prefix}lp-{}", crate::random_hex(4)?);
    if let Err(e) = crate::live_run::disposable(Some(&prefix), &name) {
        return ctx.record(L02_DENIED, started, inputs, Found::blocked(e));
    }
    let body = json!({"topic_name": name, "description": "C25 least-privilege probe",
        "examples": ["c25 synthetic example"]});
    inputs.insert("kind".to_owned(), json!("topics"));
    inputs.insert("action".to_owned(), json!("create"));
    inputs.insert("body".to_owned(), body.clone());
    let mut restricted = match ctx
        .run
        .management_desk_as(
            "L02",
            "restricted_client_id",
            "restricted_client_secret",
            DeskOptions::default(),
        )
        .await?
    {
        Ok(d) => d,
        Err(e) => return ctx.record(L02_DENIED, started, inputs, Found::blocked(e)),
    };
    let draft = json!({"kind": "topics", "action": "create", "target": "",
        "body": body.to_string()});
    let applied = crate::live_changes::apply(&mut restricted, &draft).await?;
    drop(restricted);
    let (refused, outcome, failure) = match &applied {
        crate::live_changes::Applied::Review(failure) => (
            permission_denied(failure),
            "review".to_owned(),
            failure.clone(),
        ),
        crate::live_changes::Applied::Settled { result, .. } => (
            result["outcome"] == "rejected" && permission_denied(&result["failure"]),
            result["outcome"].as_str().unwrap_or("unknown").to_owned(),
            result["failure"].clone(),
        ),
    };

    // The full principal reads back whether the topic exists.
    let mut full = match ctx
        .run
        .management_desk("L02", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => {
            let found = Found::failed(format!(
                "the full principal could not read back the probe topic: {e}"
            ))
            .with("outcome", json!(outcome))
            .with("failure", failure)
            .stopping("the probe's effect is not verified", true);
            return ctx.record(L02_DENIED, started, inputs, found);
        }
    };
    let mut found = match find_topic(&mut full, &name).await? {
        Err(e) => Found::failed(format!("the probe's readback did not finish: {e}"))
            .stopping("the probe's effect is not verified", true),
        Ok(None) if refused => {
            let mut f = Found::passed(
                "the service refused the restricted principal's write; the full principal read the topics back and none was created",
            );
            f.readback = "absent".to_owned();
            f
        }
        Ok(None) if outcome == "unknown" => {
            let mut f = Found::failed(
                "the restricted write's outcome is unknown; no topic was read back, so it is not shown to be refused",
            )
            .stopping("an uncertain outcome", true);
            f.readback = "absent".to_owned();
            f
        }
        Ok(None) => {
            let mut f =
                Found::failed("the write was not applied, but not refused as a permission denial");
            f.readback = "absent".to_owned();
            f
        }
        Ok(Some(id)) => {
            // It exists: the restricted principal has write authority.
            let mut cleanup =
                "the probe topic exists but has no ID to delete; reconcile it".to_owned();
            let mut reconcile = true;
            if !id.is_empty() {
                let delete = json!({"kind": "topics", "action": "delete", "target": id,
                    "body": "{}"});
                if let crate::live_changes::Applied::Settled { result, .. } =
                    crate::live_changes::apply(&mut full, &delete).await?
                    && result["outcome"] == "completed"
                    && result["readback"] == "absent"
                {
                    cleanup = "deleted by the full principal and read back as absent".to_owned();
                    reconcile = false;
                } else {
                    cleanup = "the full principal's delete did not settle as absent; reconcile it"
                        .to_owned();
                }
            }
            let mut f = Found::failed(
                "unexpected authority: the restricted principal's write created the topic",
            )
            .id("created_id", &id)
            .stopping("unexpected authority", reconcile);
            f.readback = "found".to_owned();
            f.cleanup = cleanup;
            f
        }
    };
    found = found
        .with("outcome", json!(outcome))
        .with("failure", failure)
        .with("probe_name", json!(name));
    if found.cleanup == "nothing created" {
        found.cleanup = "nothing was created; the full principal read the topics back".to_owned();
    }
    drop(full);
    ctx.record(L02_DENIED, started, inputs, found)
}

const L05_INVENTORY: Spec = Spec {
    id: "L05-inventory",
    workflows: &["W17"],
};
const L05_SESSION: Spec = Spec {
    id: "L05-session-detail",
    workflows: &["W17"],
};
const L05_SEARCH: Spec = Spec {
    id: "L05-search",
    workflows: &["W18"],
};
const L05_CONTENT: Spec = Spec {
    id: "L05-content-reveal",
    workflows: &["W19"],
};

async fn l05(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let all = [L05_INVENTORY, L05_SESSION, L05_SEARCH, L05_CONTENT];
    let inputs = ctx.run.inputs("L05", &["client_secret"]);
    let mut desk = match ctx
        .run
        .management_desk("L05", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => return ctx.block_all(&all, &e),
    };
    let started = now_utc();
    let page = desk
        .read(json!({"kind": "page", "resource": "sessions", "offset": 0, "limit": 25}))
        .await?;
    let found = Found::check(
        ok(&page),
        "the 24-hour sessions inventory returned a page with its completeness",
        "the sessions inventory could not be read",
    )
    .with("page", summarize(&page));
    ctx.record(L05_INVENTORY, started, inputs.clone(), found)?;

    // Only a synthetic session is opened (never another customer's
    // content): the designated one, or else the newest session of the app
    // name L01's synthetic scans carry.
    let designated = (
        ctx.run.nonsecret("L05", "session_id"),
        ctx.run.nonsecret("L05", "session_app_id"),
        ctx.run.nonsecret("L05", "session_app_name"),
    );
    let (session, app_id, app_name, discovered) = match designated {
        (Some(session), Some(app_id), Some(app_name)) => (session, app_id, app_name, None),
        _ => match discover_session(ctx, &mut desk).await? {
            Ok((session, app_id, app_name, how)) => (session, app_id, app_name, Some(how)),
            Err(reason) => {
                return ctx.block_all(&[L05_SESSION, L05_SEARCH, L05_CONTENT], &reason);
            }
        },
    };
    let started = now_utc();
    let detail = desk
        .read(
            json!({"kind": "session", "sessionId": session, "appId": app_id, "appName": app_name}),
        )
        .await?;
    let which = if discovered.is_some() {
        "discovered synthetic"
    } else {
        "designated"
    };
    let mut found = Found::check(
        matches!(detail, ManagementResult::Session { .. }),
        format!("the {which} session's actions were read"),
        format!("the {which} session could not be read"),
    )
    .with("session", summarize(&detail))
    .id("session_id", &session);
    if let Some(how) = &discovered {
        found = found.with("discovered", how.clone());
    }
    // The scan to open: the designated one, except that a discovered
    // session uses one of its own scans when the designated one is not in
    // it.
    let actions: Vec<String> = match &detail {
        ManagementResult::Session { session } => serde_json::from_str::<Value>(&session.session)
            .ok()
            .and_then(|v| v["session_actions"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|a| id_of(a, "scan_id"))
            .collect(),
        _ => Vec::new(),
    };
    let designated_scan = ctx.run.nonsecret("L05", "scan_id");
    let scan_target = match (&discovered, designated_scan) {
        (None, scan) => scan,
        (Some(_), Some(scan)) if actions.contains(&scan) => Some(scan),
        (Some(_), _) => actions.first().cloned(),
    };
    if let Some(scan) = scan_target.clone() {
        let tx = desk
            .read(
                json!({"kind": "transaction", "sessionId": session, "appId": app_id,
                "appName": app_name, "scanId": scan, "subRequest": 0}),
            )
            .await?;
        if !matches!(tx, ManagementResult::Transaction { .. }) {
            found = Found::failed("the designated transaction's metadata could not be read")
                .id("session_id", &session);
        }
        found = found
            .with("transaction", summarize(&tx))
            .id("scan_id", &scan);
    }
    ctx.record(L05_SESSION, started, inputs.clone(), found)?;

    // The app searches by scan ID (a UUID), by a `ses…` session ID or by
    // detector name (v1); AIRS session IDs are the caller's trace IDs, so
    // the session's own scan is the query.
    let started = now_utc();
    let found = match scan_target.clone() {
        None => Found::blocked("the session lists no scan to search for"),
        Some(scan) => match desk.read(json!({"kind": "search", "query": scan})).await {
            Err(e) => Found::failed(format!("the search was refused: {e}")).id("scan_id", &scan),
            Ok(search) => Found::check(
                matches!(search, ManagementResult::Search { .. }),
                "the bounded search by the session's scan ID completed and states its completeness",
                "the search failed",
            )
            .with("search", summarize(&search))
            .id("scan_id", &scan),
        },
    };
    ctx.record(L05_SEARCH, started, inputs.clone(), found)?;

    let started = now_utc();
    let found = match (
        scan_target,
        ctx.run.nonsecret("L05", "reveal_content").as_deref(),
    ) {
        (Some(scan), Some("synthetic")) => {
            let content = desk
                .read(json!({"kind": "content", "scanId": scan, "subRequest": 0}))
                .await?;
            let lengths = match &content {
                ManagementResult::Content { content } => serde_json::to_value(content)
                    .ok()
                    .map(|v| {
                        json!({
                            "prompt_chars": v["prompt"].as_str().map(str::len),
                            "response_chars": v["response"].as_str().map(str::len),
                        })
                    })
                    .unwrap_or(Value::Null),
                _ => Value::Null,
            };
            Found::check(
                matches!(content, ManagementResult::Content { .. }),
                "the synthetic scan's content was revealed on explicit request (only its length is recorded)",
                "the content could not be revealed",
            )
            .with("content", summarize(&content))
            .with("lengths", lengths)
        }
        _ => Found::blocked(
            "content is revealed only for a synthetic scan when reveal_content = synthetic",
        ),
    };
    ctx.record(L05_CONTENT, started, inputs, found)?;
    Ok(())
}

/// The timestamp fields a session row may carry, most telling first.
const SESSION_TIMES: [&str; 7] = [
    "last_seen",
    "last_updated",
    "updated_at",
    "end_time",
    "start_time",
    "created_at",
    "timestamp",
];

/// Pages of the sessions inventory walked to find a synthetic session.
const SESSION_PAGES: u32 = 8;

/// The newest session whose application name is the one L01's synthetic
/// scans carry: its session ID, application ID and name, and how it was
/// chosen. Sessions of any other application are never chosen.
async fn discover_session(
    ctx: &mut Ctx<'_, '_>,
    desk: &mut crate::live_run::Desk,
) -> Result<std::result::Result<(String, String, String, Value), String>> {
    let Some(app) = ctx.run.nonsecret("L01", "app_name") else {
        return Ok(Err(
            "L05 designates no synthetic session, and L01 names no app_name to discover one by"
                .to_owned(),
        ));
    };
    let mut matches = Vec::new();
    let mut offset = 0u32;
    let mut walked = 0u32;
    while walked < SESSION_PAGES {
        walked += 1;
        let page = desk
            .read(json!({"kind": "page", "resource": "sessions", "offset": offset, "limit": 50}))
            .await?;
        let ManagementResult::Page { page: listed } = &page else {
            return Ok(Err("the sessions inventory could not be walked".to_owned()));
        };
        let items = page_items(&page);
        matches.extend(
            items
                .iter()
                .filter(|s| s["application_name"] == app.as_str())
                .cloned(),
        );
        match listed.next_offset {
            Some(next) if !listed.complete && next > offset && !items.is_empty() => offset = next,
            _ => break,
        }
    }
    if matches.is_empty() {
        return Ok(Err(format!(
            "L05 designates no synthetic session and none of application {app} is in the last 24 hours ({walked} page(s) walked)"
        )));
    }
    let field = SESSION_TIMES
        .iter()
        .find(|f| matches.iter().all(|m| m[**f].is_string()));
    let chosen = match field {
        Some(f) => matches
            .iter()
            .max_by(|a, b| a[*f].as_str().cmp(&b[*f].as_str()))
            .cloned(),
        None => matches.first().cloned(),
    }
    .unwrap_or(Value::Null);
    let (Some(session), Some(app_id)) = (
        id_of(&chosen, "session_id"),
        id_of(&chosen, "application_id").or_else(|| id_of(&chosen, "app_id")),
    ) else {
        return Ok(Err(format!(
            "the newest session of application {app} names no session or application ID"
        )));
    };
    let how = json!({
        "by": "app_name",
        "app_name": app,
        "candidates": matches.len(),
        "chosen_by": field.map_or("first in the service's order".to_owned(), |f| format!("newest {f}")),
    });
    Ok(Ok((session, app_id, app, how)))
}

const L06_RED_TEAM: Spec = Spec {
    id: "L06-red-team",
    workflows: &["W23"],
};
const L06_SUPPLY: Spec = Spec {
    id: "L06-model-and-skills",
    workflows: &["W24"],
};
const L06_ATTACKS: Spec = Spec {
    id: "L06-attack-report",
    workflows: &["W23"],
};

async fn l06(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let all = [L06_RED_TEAM, L06_SUPPLY, L06_ATTACKS];
    let inputs = ctx.run.inputs("L06", &["client_secret"]);
    let mut desk = match ctx
        .run
        .management_desk("L06", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => return ctx.block_all(&all, &e),
    };
    let started = now_utc();
    let red = desk.read(json!({"kind": "redTeam", "window": 30})).await?;
    let found = Found::check(
        matches!(red, ManagementResult::RedTeam { .. }),
        "existing Red Teaming statistics and scans were read (no scan was started)",
        "the Red Teaming observation could not be read",
    )
    .with("red_team", summarize(&red));
    ctx.record(L06_RED_TEAM, started, inputs.clone(), found)?;

    let started = now_utc();
    let supply = desk.read(json!({"kind": "supplyChain"})).await?;
    let found = Found::check(
        matches!(supply, ManagementResult::SupplyChain { .. }),
        "existing Model Security scans and skill statistics were read, with partial sources disclosed",
        "the Model/skill observation could not be read",
    )
    .with("supply_chain", summarize(&supply));
    ctx.record(L06_SUPPLY, started, inputs.clone(), found)?;

    let started = now_utc();
    // The attack report's scan: the designated one, or else the newest
    // completed scan the read-only list shows (no scan is ever started).
    let designated = ctx.run.nonsecret("L06", "red_team_scan_id");
    let discovered = match (&designated, &red) {
        (None, ManagementResult::RedTeam { observation }) => observation
            .scans
            .as_ref()
            .and_then(|scans| {
                scans
                    .rows
                    .iter()
                    .filter(|r| {
                        r.status
                            .as_deref()
                            .is_some_and(|s| s.eq_ignore_ascii_case("completed"))
                    })
                    .max_by(|a, b| a.created_at.cmp(&b.created_at))
            })
            .map(|r| r.id.clone()),
        _ => None,
    };
    let found = match designated.or_else(|| discovered.clone()) {
        Some(scan) => {
            let attacks = desk
                .read(json!({"kind": "attacks", "scanId": scan, "page": 0, "received": 0}))
                .await?;
            let mut f = Found::check(
                matches!(attacks, ManagementResult::Attacks { .. }),
                "the scan's first attack page was read with its completeness",
                "the attack report could not be read",
            )
            .with("attacks", summarize(&attacks))
            .id("red_team_scan_id", &scan);
            if discovered.is_some() {
                f = f.with(
                    "discovered",
                    json!({"by": "newest completed scan in the read-only list"}),
                );
            }
            f
        }
        None => Found::blocked(
            "L06 designates no red_team_scan_id and the read-only list shows no completed scan",
        ),
    };
    ctx.record(L06_ATTACKS, started, inputs, found)?;
    Ok(())
}

const L07_WORKSPACES: Spec = Spec {
    id: "L07-workspaces",
    workflows: &["W34"],
};
const L07_INVENTORIES: Spec = Spec {
    id: "L07-inventories",
    workflows: &["W35"],
};
const L07_PREREQUISITES: Spec = Spec {
    id: "L07-prerequisites",
    workflows: &["W36"],
};
const L07_DENIED: Spec = Spec {
    id: "L07-least-privilege-denied",
    workflows: &["W34"],
};

async fn l07(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let all = [L07_WORKSPACES, L07_INVENTORIES, L07_PREREQUISITES];
    let inputs = ctx.run.inputs("L07", &["client_secret"]);
    let Some(workspace) = ctx.run.nonsecret("L07", "workspace_id") else {
        ctx.block_all(&all, "L07 designates no workspace_id")?;
        return l07_denied(ctx).await;
    };
    let mut desk = match ctx
        .run
        .management_desk("L07", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => {
            ctx.block_all(&all, &e)?;
            return l07_denied(ctx).await;
        }
    };
    let started = now_utc();
    let found = match desk.select_workspace(&workspace).await {
        Ok(_) => Found::passed("the designated workspace was listed and selected natively")
            .id("workspace_id", &workspace),
        Err(e) => Found::failed(e.to_string()),
    };
    let selected = found.outcome == "passed";
    ctx.record(L07_WORKSPACES, started, inputs.clone(), found)?;
    if !selected {
        ctx.block_all(
            &[L07_INVENTORIES, L07_PREREQUISITES],
            "the designated workspace could not be selected",
        )?;
        return l07_denied(ctx).await;
    }
    let reference = desk.workspace_ref();
    let started = now_utc();
    let mut requests: Vec<(String, Value)> = GATEWAY_SCOPED
        .iter()
        .map(|r| {
            (
                (*r).to_owned(),
                json!({"kind": "gateway", "workspace": reference,
                       "read": {"kind": "inventory", "resource": r}}),
            )
        })
        .collect();
    requests.push((
        "integrations".to_owned(),
        json!({"kind": "gateway", "workspace": null,
               "read": {"kind": "inventory", "resource": "integrations"}}),
    ));
    let (all_ok, _, shown) = reads(&mut desk, requests).await?;
    let found = Found::check(
        all_ok,
        "every Gateway inventory of the selected workspace, and the tenant's integrations, was read",
        "a Gateway inventory could not be read",
    )
    .with("inventories", shown);
    ctx.record(L07_INVENTORIES, started, inputs.clone(), found)?;

    let started = now_utc();
    let pre = desk
        .read(json!({"kind": "gateway", "workspace": reference, "read": {"kind": "prerequisites"}}))
        .await?;
    let found = Found::check(
        ok(&pre),
        "the routing, provider, key and IAM prerequisites were read for the selected workspace",
        "the prerequisites could not be read",
    )
    .with("prerequisites", summarize(&pre));
    ctx.record(L07_PREREQUISITES, started, inputs, found)?;
    drop(desk);
    l07_denied(ctx).await
}

/// The first object anywhere in `value` whose `name` is `name`.
fn named<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            if map.get("name").and_then(Value::as_str) == Some(name) {
                return Some(value);
            }
            map.values().find_map(|v| named(v, name))
        }
        Value::Array(items) => items.iter().find_map(|v| named(v, name)),
        _ => None,
    }
}

/// Whether the selected workspace lists a service key named `name`.
async fn find_service_key(
    desk: &mut crate::live_run::Desk,
    name: &str,
) -> Result<std::result::Result<Option<String>, String>> {
    let reference = desk.workspace_ref();
    let read = desk
        .read(json!({"kind": "gateway", "workspace": reference,
            "read": {"kind": "inventory", "resource": "serviceKeys"}}))
        .await?;
    // The list's rows are a JSON array carried as text.
    let ManagementResult::Gateway {
        gateway: desktop_core::gateway::GatewayResult::List { list },
    } = &read
    else {
        return Ok(Err("the service key inventory could not be read".to_owned()));
    };
    let Ok(rows) = serde_json::from_str::<Value>(&list.rows) else {
        return Ok(Err(
            "the service key inventory's rows could not be parsed".to_owned()
        ));
    };
    if let Some(key) = named(&rows, name) {
        return Ok(Ok(Some(id_of(key, "id").unwrap_or_default())));
    }
    if !list.complete {
        return Ok(Err("the service key inventory is incomplete".to_owned()));
    }
    Ok(Ok(None))
}

/// The restricted principal attempts a reviewed create of a disposable
/// service key in the designated workspace; the service must refuse it.
/// A read-only principal may list workspaces, so its least privilege is
/// shown by a refused write (as for L02). The full principal then reads
/// the keys back and deletes the probe if it exists after all, which stops
/// the run as unexpected authority. A designated
/// `restricted_denied_resource` keeps the read-denial mode.
async fn l07_denied(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    if ctx
        .run
        .nonsecret("L07", "restricted_denied_resource")
        .is_some()
    {
        return l07_read_denied(ctx).await;
    }
    let started = now_utc();
    let mut inputs = ctx.run.inputs("L07", &["restricted_client_secret"]);
    inputs.insert("mode".to_owned(), json!("write"));
    let Some(prefix) = ctx.run.prefix("L07").filter(|p| !p.is_empty()) else {
        let found = Found::blocked(
            "a write denial needs L07's disposable_prefix (or designate restricted_denied_resource for a read denial)",
        );
        return ctx.record(L07_DENIED, started, inputs, found);
    };
    let Some(workspace) = ctx.run.nonsecret("L07", "workspace_id") else {
        return ctx.record(
            L07_DENIED,
            started,
            inputs,
            Found::blocked("L07 names no workspace_id"),
        );
    };
    let name = format!("{prefix}lp-{}", crate::random_hex(4)?);
    if let Err(e) = crate::live_run::disposable(Some(&prefix), &name) {
        return ctx.record(L07_DENIED, started, inputs, Found::blocked(e));
    }
    let body = json!({"name": name, "scopes": ["completions.write"], "type": "service"});
    inputs.insert("kind".to_owned(), json!("gatewayServiceKeys"));
    inputs.insert("action".to_owned(), json!("create"));
    inputs.insert("body".to_owned(), body.clone());
    let mut restricted = match ctx
        .run
        .management_desk_as(
            "L07",
            "restricted_client_id",
            "restricted_client_secret",
            DeskOptions::default(),
        )
        .await?
    {
        Ok(d) => d,
        Err(e) => return ctx.record(L07_DENIED, started, inputs, Found::blocked(e)),
    };
    if restricted.select_workspace(&workspace).await.is_err() {
        let found = Found::blocked("the restricted principal could not select the workspace");
        return ctx.record(L07_DENIED, started, inputs, found);
    }
    let mut draft = json!({"kind": "gatewayServiceKeys", "action": "create", "target": "",
        "body": body.to_string()});
    draft["workspace"] = json!(restricted.workspace_ref());
    let applied = crate::live_changes::apply(&mut restricted, &draft).await?;
    drop(restricted);
    let (refused, outcome, failure) = match &applied {
        crate::live_changes::Applied::Review(failure) => (
            permission_denied(failure),
            "review".to_owned(),
            failure.clone(),
        ),
        crate::live_changes::Applied::Settled { result, .. } => (
            result["outcome"] == "rejected" && permission_denied(&result["failure"]),
            result["outcome"].as_str().unwrap_or("unknown").to_owned(),
            result["failure"].clone(),
        ),
    };
    let mut full = match ctx
        .run
        .management_desk("L07", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => {
            let found = Found::failed(format!(
                "the full principal could not read back the probe key: {e}"
            ))
            .with("outcome", json!(outcome))
            .with("failure", failure)
            .stopping("the probe's effect is not verified", true);
            return ctx.record(L07_DENIED, started, inputs, found);
        }
    };
    if full.select_workspace(&workspace).await.is_err() {
        let found = Found::failed("the full principal could not select the workspace to read back")
            .with("outcome", json!(outcome))
            .stopping("the probe's effect is not verified", true);
        return ctx.record(L07_DENIED, started, inputs, found);
    }
    let mut found = match find_service_key(&mut full, &name).await? {
        Err(e) => Found::failed(format!("the probe's readback did not finish: {e}"))
            .stopping("the probe's effect is not verified", true),
        Ok(None) if refused => {
            let mut f = Found::passed(
                "the service refused the restricted principal's write; the full principal read the service keys back and none was created",
            );
            f.readback = "absent".to_owned();
            f
        }
        Ok(None) if outcome == "unknown" => {
            let mut f = Found::failed(
                "the restricted write's outcome is unknown; no key was read back, so it is not shown to be refused",
            )
            .stopping("an uncertain outcome", true);
            f.readback = "absent".to_owned();
            f
        }
        Ok(None) => {
            let mut f =
                Found::failed("the write was not applied, but not refused as a permission denial");
            f.readback = "absent".to_owned();
            f
        }
        Ok(Some(id)) => {
            let mut cleanup =
                "the probe key exists but has no ID to delete; reconcile it".to_owned();
            let mut reconcile = true;
            if !id.is_empty() {
                let mut delete = json!({"kind": "gatewayServiceKeys", "action": "delete",
                    "target": id, "body": "{}"});
                delete["workspace"] = json!(full.workspace_ref());
                if let crate::live_changes::Applied::Settled { result, .. } =
                    crate::live_changes::apply(&mut full, &delete).await?
                    && result["outcome"] == "completed"
                    && result["readback"] == "absent"
                {
                    cleanup = "deleted by the full principal and read back as absent".to_owned();
                    reconcile = false;
                } else {
                    cleanup = "the full principal's delete did not settle as absent; reconcile it"
                        .to_owned();
                }
            }
            let mut f = Found::failed(
                "unexpected authority: the restricted principal's write created the service key",
            )
            .id("created_id", &id)
            .stopping("unexpected authority", reconcile);
            f.readback = "found".to_owned();
            f.cleanup = cleanup;
            f
        }
    };
    found = found
        .with("outcome", json!(outcome))
        .with("failure", failure)
        .with("probe_name", json!(name));
    ctx.record(L07_DENIED, started, inputs, found)
}

async fn l07_read_denied(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let started = now_utc();
    let inputs = ctx.run.inputs("L07", &["restricted_client_secret"]);
    let found = match ctx
        .run
        .management_desk_as(
            "L07",
            "restricted_client_id",
            "restricted_client_secret",
            DeskOptions::default(),
        )
        .await?
    {
        Err(e) => Found::blocked(e),
        Ok(mut desk) => {
            let read = desk
                .read(json!({"kind": "gateway", "workspace": null,
                    "read": {"kind": "inventory", "resource": "workspaces"}}))
                .await?;
            if ok(&read) {
                Found::failed(
                    "unexpected authority: the restricted principal listed the workspaces",
                )
                .with("read", summarize(&read))
                .stopping("unexpected authority", false)
            } else {
                Found::check(
                    denied(&read),
                    "the service denied the restricted principal",
                    "the read failed, but not as a permission denial",
                )
                .with("read", summarize(&read))
            }
        }
    };
    ctx.record(L07_DENIED, started, inputs, found)
}

const L10_RUNTIME: Spec = Spec {
    id: "L10-runtime-overview",
    workflows: &["W21"],
};
const L10_GATEWAY: Spec = Spec {
    id: "L10-gateway-observation",
    workflows: &["W41"],
};

async fn l10(ctx: &mut Ctx<'_, '_>) -> Result<()> {
    let inputs = ctx.run.inputs("L10", &["client_secret"]);
    let mut desk = match ctx
        .run
        .management_desk("L10", DeskOptions::default())
        .await?
    {
        Ok(d) => d,
        Err(e) => return ctx.block_all(&[L10_RUNTIME, L10_GATEWAY], &e),
    };
    let started = now_utc();
    let overview = desk.read(json!({"kind": "overview"})).await?;
    let found = Found::check(
        matches!(overview, ManagementResult::Overview { .. }),
        "the Runtime Home/Inbox sources were read with their freshness and completeness",
        "the Runtime overview could not be read",
    )
    .with("overview", summarize(&overview));
    ctx.record(L10_RUNTIME, started, inputs.clone(), found)?;

    let started = now_utc();
    let Some(workspace) = ctx.run.nonsecret("L10", "workspace_id") else {
        return ctx.record(
            L10_GATEWAY,
            started,
            inputs,
            Found::blocked("L10 designates no workspace_id"),
        );
    };
    let found = match desk.select_workspace(&workspace).await {
        Err(e) => Found::failed(e.to_string()),
        Ok(reference) => {
            let days: u8 = ctx
                .run
                .nonsecret("L10", "window_days")
                .and_then(|d| d.parse().ok())
                .unwrap_or(7);
            let requests = vec![
                (
                    "usage".to_owned(),
                    json!({"kind": "gateway", "workspace": reference,
                        "read": {"kind": "observe", "read": {"kind": "usage", "days": days}}}),
                ),
                (
                    "logs".to_owned(),
                    json!({"kind": "gateway", "workspace": reference,
                        "read": {"kind": "observe", "read": {"kind": "logs", "days": days, "page": 0}}}),
                ),
                (
                    "limits".to_owned(),
                    json!({"kind": "gateway", "workspace": reference,
                        "read": {"kind": "observe", "read": {"kind": "limits"}}}),
                ),
            ];
            let (all_ok, _, shown) = reads(&mut desk, requests).await?;
            Found::check(
                all_ok,
                "the workspace's usage, logs and limit policies were read with their freshness",
                "a Gateway observation could not be read",
            )
            .with("observations", shown)
            .id("workspace_id", &workspace)
        }
    };
    ctx.record(L10_GATEWAY, started, inputs, found)
}
