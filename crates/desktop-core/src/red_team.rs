//! Red Team and Model/skill observation (C13): read-only summaries of AI
//! Red Teaming scan statistics, scan lists and attack results, Model
//! Security scans and AgentGuard skill scans, with v1's rules.
//!
//! Only aggregates and each row's identifier, status, target type, outcome,
//! counts and time leave the core: scan, target, model and skill names,
//! model URIs, security groups, findings, attack prompts and replies and
//! unknown server fields never do. A skill is shown by a pseudonym (the
//! first eight hex digits of SHA-256 over a per-read random salt and its
//! name), so pseudonyms cannot be linked across reads. Statistics must be a
//! recognised, error-free reply; totals the service left out are zero only
//! when every reported count is zero, otherwise the reply is invalid. A list
//! is complete only when the service says so (a total no larger than the
//! rows actually received, or a short page without a total, and never a
//! capped count); a capped total is a lower bound.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::management::ManagementFailure;

/// Rows read per list page (v1).
pub const LIST_LIMIT: u32 = 100;
/// Attack results are read 100 per page, at most ten pages (v1).
pub const ATTACK_PAGES: u8 = 10;
/// Entries a breakdown may report (v1); more make the reply invalid.
pub const MAX_BARS: usize = 64;
/// The statistics windows v1 offered, in days.
pub const WINDOWS: [u8; 3] = [7, 15, 30];

/// One labelled count in a reported breakdown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    pub label: String,
    pub count: u32,
}

/// AI Red Teaming scan statistics for a window. A group the service did not
/// report is `None` (unknown), never an empty breakdown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RedTeamStatistics {
    pub scans: u32,
    pub targets: u32,
    pub status: Option<Vec<Bar>>,
    pub risk: Option<Vec<Bar>>,
    pub target_types: Option<Vec<Bar>>,
    /// The statistics were absent (404) and an empty scan list proved that
    /// the tenant has no scans.
    pub no_scans: bool,
}

/// One AI Red Teaming scan: identifiers, status, target type and time only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RedTeamScan {
    pub id: String,
    pub status: Option<String>,
    pub job_type: Option<String>,
    pub target_id: Option<String>,
    pub target_type: Option<String>,
    pub created_at: Option<String>,
}

/// The first page of scans: at most 100 rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RedTeamScans {
    pub rows: Vec<RedTeamScan>,
    pub total: Option<u32>,
    /// The service capped its count: `total` is a lower bound.
    pub capped: bool,
    pub complete: bool,
}

/// One Red Teaming read: each source keeps its data or its failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RedTeamObservation {
    pub read_at: u32,
    pub window: u8,
    pub statistics: Option<RedTeamStatistics>,
    pub statistics_failure: Option<ManagementFailure>,
    pub scans: Option<RedTeamScans>,
    pub scans_failure: Option<ManagementFailure>,
}

/// One Model Security scan: its outcome and time (never the model URI).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelScan {
    pub id: String,
    pub outcome: Option<String>,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelScans {
    pub rows: Vec<ModelScan>,
    pub total: Option<u32>,
    /// The service capped its count: `total` is a lower bound.
    pub capped: bool,
    pub complete: bool,
}

/// AgentGuard's 30-day skill statistics (counts only).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkillStatistics {
    pub unique_skills: u32,
    pub vulnerabilities: u32,
}

/// One skill scan, named by a per-read pseudonym.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkillScan {
    pub pseudonym: String,
    pub status: Option<String>,
    pub outcome: Option<String>,
    /// Vulnerabilities the scan reported; `None` while unreported.
    pub vulnerabilities: Option<u32>,
    pub created_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SkillScans {
    pub rows: Vec<SkillScan>,
    pub total: Option<u32>,
    /// The service capped its count: `total` is a lower bound.
    pub capped: bool,
    pub complete: bool,
}

/// One Model and skill read: three independent sources.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SupplyChainObservation {
    pub read_at: u32,
    pub models: Option<ModelScans>,
    pub models_failure: Option<ManagementFailure>,
    pub skill_statistics: Option<SkillStatistics>,
    pub skill_statistics_failure: Option<ManagementFailure>,
    pub skills: Option<SkillScans>,
    pub skills_failure: Option<ManagementFailure>,
}

/// One attack result: its outcome and classification, never the prompt,
/// the target's reply or any detail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Attack {
    /// Position in the report (1-based).
    pub number: u32,
    /// `true` succeeded, `false` defended, `None` not rated.
    pub threat: Option<bool>,
    pub severity: Option<String>,
    pub attack_type: Option<String>,
    pub sub_category: Option<String>,
    pub multi_turn: Option<bool>,
}

/// One page (100) of a scan's attack results. `complete` says that the
/// rows received so far, this page's included, cover the service's total.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AttackPage {
    pub scan_id: String,
    pub page: u8,
    pub rows: Vec<Attack>,
    pub total: Option<u32>,
    /// The service capped its count: `total` is a lower bound.
    pub capped: bool,
    pub complete: bool,
}

/// Whether `id` is a canonical UUID (the services' job identifiers).
pub fn is_uuid(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// A whole, non-negative count that fits; anything else is not a count.
fn count(value: &Value) -> Option<u32> {
    let n = value.as_f64()?;
    (n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX)).then_some(n as u32)
}

/// A present count, an absent one (`Ok(None)`), or an invalid one.
fn optional_count(row: &Value, key: &str) -> Result<Option<u32>, ()> {
    match row.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => count(v).map(Some).ok_or(()),
    }
}

/// A short display token (status, outcome, type): trimmed, at most 64
/// characters, no control characters; anything else is dropped.
fn token(row: &Value, key: &str) -> Option<String> {
    let text = row.get(key)?.as_str()?.trim();
    (!text.is_empty() && text.chars().count() <= 64 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}

/// The labels v1 shows for each breakdown; any other label is OTHER.
const STATUSES: [&str; 11] = [
    "ALLOWED",
    "BLOCKED",
    "PENDING",
    "ERROR",
    "FAILED",
    "PASSED",
    "COMPLETED",
    "RUNNING",
    "IN_PROGRESS",
    "QUEUED",
    "CANCELLED",
];
const RISKS: [&str; 4] = ["LOW", "MEDIUM", "HIGH", "CRITICAL"];
const TARGET_TYPES: [&str; 3] = ["APPLICATION", "MODEL", "AGENT"];

/// A breakdown label (v1): a known label upper-cased with `_` read as a
/// space; any other label becomes OTHER.
fn label(text: &str, known: &[&str]) -> String {
    let upper = text.to_uppercase();
    if known.contains(&upper.as_str()) {
        upper.replace('_', " ")
    } else {
        "OTHER".to_owned()
    }
}

/// A reported breakdown of at most 64 entries: each entry's `name_key` label
/// and `count` (or `total`), labels summed (`None` when unreported), and
/// whether any entry counts above zero.
fn bars(
    reply: &Value,
    key: &str,
    name_key: &str,
    known: &[&str],
) -> Result<(Option<Vec<Bar>>, bool), ()> {
    let entries = match reply.get(key) {
        None | Some(Value::Null) => return Ok((None, false)),
        Some(Value::Array(entries)) if entries.len() <= MAX_BARS => entries,
        Some(_) => return Err(()),
    };
    let mut out: Vec<Bar> = Vec::new();
    let mut positive = false;
    for entry in entries {
        let name = entry.get(name_key).and_then(Value::as_str).ok_or(())?;
        let value = match optional_count(entry, "count")? {
            Some(n) => n,
            None => optional_count(entry, "total")?.ok_or(())?,
        };
        positive |= value > 0;
        let label = label(name, known);
        if let Some(bar) = out.iter_mut().find(|b| b.label == label) {
            bar.count = bar.count.saturating_add(value);
        } else {
            out.push(Bar {
                label,
                count: value,
            });
        }
    }
    Ok((Some(out), positive))
}

/// Whether a reply reports a failure (v1): `success: false`, or an `error`
/// or `errors` field.
fn reports_failure(reply: &Value) -> bool {
    reply.as_object().is_some_and(|fields| {
        fields.get("success") == Some(&Value::Bool(false))
            || fields.contains_key("error")
            || fields.contains_key("errors")
    })
}

/// A recognised, error-free reply: an object that reports no failure and
/// carries at least one of `known`.
fn recognised(reply: &Value, known: &[&str]) -> bool {
    !reports_failure(reply)
        && reply
            .as_object()
            .is_some_and(|fields| known.iter().any(|k| fields.contains_key(*k)))
}

/// Whether any risk rating reports a target (v1): every rating's
/// `targets_by_type` is validated first, each an array of at most 64 objects
/// with a whole, non-negative `count`; anything else is invalid.
fn rated_targets(reply: &Value) -> Result<bool, ()> {
    let ratings = match reply.get("risk_profile") {
        None | Some(Value::Null) => return Ok(false),
        Some(Value::Array(ratings)) => ratings,
        Some(_) => return Err(()),
    };
    let mut rated = false;
    for rating in ratings {
        let targets = match rating.get("targets_by_type") {
            None | Some(Value::Null) => continue,
            Some(Value::Array(targets)) if targets.len() <= MAX_BARS => targets,
            Some(_) => return Err(()),
        };
        for target in targets {
            if !target.is_object() {
                return Err(());
            }
            rated |= target.get("count").and_then(count).ok_or(())? > 0;
        }
    }
    Ok(rated)
}

/// Scan statistics by v1's rules; `None` when the reply is invalid.
pub fn statistics(reply: &Value) -> Option<RedTeamStatistics> {
    const TOTALS: [&str; 2] = ["total_scans", "targets_scanned"];
    const GROUPS: [&str; 3] = ["scan_status", "risk_profile", "targets_scanned_by_type"];
    if !recognised(reply, &[TOTALS.as_slice(), GROUPS.as_slice()].concat()) {
        return None;
    }
    let (status, busy_status) = bars(reply, "scan_status", "name", &STATUSES).ok()?;
    let (risk, busy_risk) = bars(reply, "risk_profile", "risk_rating", &RISKS).ok()?;
    let (target_types, busy_types) =
        bars(reply, "targets_scanned_by_type", "name", &TARGET_TYPES).ok()?;
    let totals = TOTALS
        .map(|key| optional_count(reply, key))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    // A missing total is zero only when nothing in the reply, a reported
    // total or any breakdown entry before the bar limit, is above zero.
    let rated = rated_targets(reply).ok()?;
    let quiet = !(busy_status || busy_risk || busy_types || rated)
        && totals.iter().all(|n| n.unwrap_or(0) == 0);
    let tally = |n: Option<u32>| match n {
        Some(n) => Some(n),
        None => quiet.then_some(0),
    };
    Some(RedTeamStatistics {
        scans: tally(totals[0])?,
        targets: tally(totals[1])?,
        status,
        risk,
        target_types,
        no_scans: false,
    })
}

/// The statistics for a tenant whose absent statistics were confirmed by an
/// empty scan list.
pub fn no_scans() -> RedTeamStatistics {
    RedTeamStatistics {
        scans: 0,
        targets: 0,
        status: None,
        risk: None,
        target_types: None,
        no_scans: true,
    }
}

/// A list page: its rows, total, whether that total is capped, and whether
/// the list is complete (v1): only when the count is not capped and the
/// total is no larger than the `received` rows before this page plus its
/// own, or, without a total, the page is short.
struct Page<'a> {
    rows: &'a Vec<Value>,
    total: Option<u32>,
    capped: bool,
    complete: bool,
}

fn page<'a>(reply: &'a Value, rows_key: &str, received: u32) -> Option<Page<'a>> {
    if reports_failure(reply) {
        return None;
    }
    let rows = reply.get(rows_key)?.as_array()?;
    // v1: the pagination object is required; its total may be omitted.
    let pagination = reply.get("pagination")?.as_object()?;
    let total = match pagination.get("total_items") {
        None | Some(Value::Null) => None,
        Some(v) => Some(count(v)?),
    };
    // A total below the rows on this very page is not a total.
    if total.is_some_and(|t| (t as usize) < rows.len()) {
        return None;
    }
    // A present flag must be a boolean (v1).
    let capped = match reply.get("count_capped") {
        None => false,
        Some(v) => v.as_bool()?,
    };
    let read = received.saturating_add(u32::try_from(rows.len()).unwrap_or(u32::MAX));
    let complete = !capped
        && match total {
            Some(total) => total <= read,
            None => rows.len() < LIST_LIMIT as usize,
        };
    // Every row is an object (v1).
    (rows.len() <= LIST_LIMIT as usize && rows.iter().all(Value::is_object)).then_some(Page {
        rows,
        total,
        capped,
        complete,
    })
}

/// Whether a one-row scan list proves there are no scans (the confirmation
/// v1 makes when the statistics are absent).
pub fn proves_no_scans(reply: &Value) -> bool {
    page(reply, "data", 0)
        .is_some_and(|p| p.complete && p.rows.is_empty() && p.total.unwrap_or(0) == 0)
}

fn id_of(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|id| is_uuid(id))
        .map(str::to_ascii_lowercase)
}

/// The first page of Red Teaming scans; `None` when invalid.
pub fn red_team_scans(reply: &Value) -> Option<RedTeamScans> {
    let page = page(reply, "data", 0)?;
    let rows = page
        .rows
        .iter()
        .map(|row| {
            Some(RedTeamScan {
                id: id_of(row, "uuid")?,
                status: token(row, "status"),
                job_type: token(row, "job_type"),
                target_id: id_of(row, "target_id"),
                target_type: token(row, "target_type"),
                created_at: token(row, "created_at"),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RedTeamScans {
        rows,
        total: page.total,
        capped: page.capped,
        complete: page.complete,
    })
}

/// The first page of Model Security scans; `None` when invalid.
pub fn model_scans(reply: &Value) -> Option<ModelScans> {
    let page = page(reply, "scans", 0)?;
    let rows = page
        .rows
        .iter()
        .map(|row| {
            Some(ModelScan {
                id: id_of(row, "uuid")?,
                outcome: token(row, "eval_outcome"),
                created_at: token(row, "created_at"),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(ModelScans {
        rows,
        total: page.total,
        capped: page.capped,
        complete: page.complete,
    })
}

/// A per-read salt for skill pseudonyms.
pub fn salt() -> Option<[u8; 16]> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).ok()?;
    Some(salt)
}

/// A skill's pseudonym for one read.
pub fn pseudonym(salt: &[u8; 16], name: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(salt);
    hash.update(name.as_bytes());
    hex::encode(hash.finalize())[..8].to_owned()
}

/// The first page of skill scans; `None` when invalid.
pub fn skill_scans(reply: &Value, salt: &[u8; 16]) -> Option<SkillScans> {
    let page = page(reply, "scans", 0)?;
    let rows = page
        .rows
        .iter()
        .map(|row| {
            let name = row.get("name").and_then(Value::as_str)?;
            let vulnerabilities = match row.get("summary") {
                None | Some(Value::Null) => None,
                Some(summary) if summary.is_object() => {
                    optional_count(summary, "vulnerability_count").ok()?
                }
                Some(_) => return None,
            };
            Some(SkillScan {
                pseudonym: pseudonym(salt, name),
                status: token(row, "status"),
                outcome: token(row, "eval_outcome"),
                vulnerabilities,
                created_at: token(row, "created_at"),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(SkillScans {
        rows,
        total: page.total,
        capped: page.capped,
        complete: page.complete,
    })
}

/// AgentGuard's 30-day statistics: a recognised, error-free reply whose
/// groups are objects; an absent count is zero only when no top
/// vulnerability is reported and every reported count is zero. `None` when
/// invalid.
pub fn skill_statistics(reply: &Value) -> Option<SkillStatistics> {
    const GROUPS: [&str; 2] = ["unique_skills_scanned", "total_vulnerabilities_found"];
    if !recognised(reply, &[GROUPS.as_slice(), &["top_vulnerability"]].concat()) {
        return None;
    }
    let counts = GROUPS
        .map(|key| match reply.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(group) if group.is_object() => optional_count(group, "count"),
            Some(_) => Err(()),
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let quiet = matches!(reply.get("top_vulnerability"), None | Some(Value::Null))
        && counts.iter().all(|n| n.unwrap_or(0) == 0);
    let tally = |n: Option<u32>| match n {
        Some(n) => Some(n),
        None => quiet.then_some(0),
    };
    Some(SkillStatistics {
        unique_skills: tally(counts[0])?,
        vulnerabilities: tally(counts[1])?,
    })
}

/// One page of attack results (`page` 0-9) after `received` rows were
/// received on the pages before it; `None` when invalid. Rows are numbered
/// from the page's offset; completeness counts only rows received.
pub fn attacks(reply: &Value, scan_id: &str, page_index: u8, received: u32) -> Option<AttackPage> {
    let before = u32::from(page_index) * LIST_LIMIT;
    let page = page(reply, "data", received)?;
    let rows = page
        .rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let threat = match row.get("threat") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_bool()?),
            };
            let multi_turn = match row.get("multi_turn") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_bool()?),
            };
            Some(Attack {
                number: before + u32::try_from(i).ok()? + 1,
                threat,
                severity: token(row, "severity"),
                attack_type: token(row, "attack_type"),
                sub_category: token(row, "sub_category_display_name")
                    .or_else(|| token(row, "sub_category")),
                multi_turn,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(AttackPage {
        scan_id: scan_id.to_owned(),
        page: page_index,
        rows,
        total: page.total,
        capped: page.capped,
        complete: page.complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ID: &str = "11111111-2222-4333-8444-555555555555";

    #[test]
    fn statistics_follow_v1s_tallies_and_bars() {
        let reply = json!({"total_scans": 12, "targets_scanned": 3,
            "scan_status": [{"name": "COMPLETED", "count": 9}, {"name": "in_progress", "count": 2},
                            {"name": "<b>bad</b>", "count": 1}, {"name": "completed", "count": 1}],
            "risk_profile": [{"risk_rating": "HIGH", "total": 2}],
            "targets_scanned_by_type": null});
        let s = statistics(&reply).expect("valid");
        assert_eq!((s.scans, s.targets), (12, 3));
        assert_eq!(
            s.status,
            Some(vec![
                Bar {
                    label: "COMPLETED".into(),
                    count: 10
                },
                Bar {
                    label: "IN PROGRESS".into(),
                    count: 2
                },
                Bar {
                    label: "OTHER".into(),
                    count: 1
                },
            ])
        );
        assert_eq!(
            s.risk,
            Some(vec![Bar {
                label: "HIGH".into(),
                count: 2
            }])
        );
        assert_eq!(s.target_types, None, "unreported is unknown, not empty");
        // Missing totals are zero only for a quiet reply.
        assert_eq!(
            statistics(&json!({"scan_status": [{"name": "FAILED", "count": 0}]})).map(|s| s.scans),
            Some(0)
        );
        assert_eq!(
            statistics(&json!({"scan_status": [{"name": "FAILED", "count": 1}]})),
            None
        );
        assert_eq!(
            statistics(
                &json!({"total_scans": null, "targets_scanned": 0, "risk_profile": [
                {"risk_rating": "LOW", "total": 0, "targets_by_type": [{"name": "MODEL", "count": 1}]}]})
            ),
            None,
            "a rated target contradicts a missing total"
        );
        assert_eq!(
            statistics(&json!({"total_scans": -1, "targets_scanned": 0})),
            None
        );
        assert_eq!(
            statistics(&json!({"total_scans": 1.5, "targets_scanned": 0})),
            None
        );
        assert_eq!(statistics(&json!([])), None);
    }

    #[test]
    fn statistics_need_a_recognised_error_free_reply() {
        for reply in [
            json!({}),
            json!({"message": "Internal error"}),
            json!({"error": "denied"}),
            json!({"errors": [{"code": "E1"}], "total_scans": 0, "targets_scanned": 0}),
            json!({"error": {"message": "x"}, "scan_status": []}),
        ] {
            assert_eq!(statistics(&reply), None, "{reply}");
        }
        // Any error field, or `success: false`, reports a failure (v1).
        assert_eq!(
            statistics(&json!({"error": null, "total_scans": 1, "targets_scanned": 1})),
            None
        );
        assert_eq!(
            statistics(&json!({"success": false, "total_scans": 1, "targets_scanned": 1})),
            None
        );
        assert_eq!(red_team_scans(&json!({"data": [], "error": "x"})), None);
        for reply in [
            json!({}),
            json!({"message": "x"}),
            json!({"error": "denied", "unique_skills_scanned": {"count": 1}}),
            json!({"unique_skills_scanned": 4, "total_vulnerabilities_found": {"count": 0}}),
        ] {
            assert_eq!(skill_statistics(&reply), None, "{reply}");
        }
    }

    #[test]
    fn missing_totals_are_zero_only_when_every_count_is_zero() {
        // A positive reported total contradicts its missing partner.
        assert_eq!(statistics(&json!({"targets_scanned": 5})), None);
        assert_eq!(
            statistics(&json!({"targets_scanned": 0})).map(|s| (s.scans, s.targets)),
            Some((0, 0))
        );
        // Every entry is read: a positive last one contradicts a missing
        // total, and more than 64 entries make the reply invalid.
        let mut status: Vec<Value> = (1..MAX_BARS)
            .map(|_| json!({"name": "FAILED", "count": 0}))
            .collect();
        status.push(json!({"name": "COMPLETED", "count": 3}));
        let reply = json!({"total_scans": 3, "targets_scanned": 1, "scan_status": status});
        assert_eq!(
            statistics(&reply).and_then(|s| s.status).map(|b| b.len()),
            Some(2)
        );
        let mut missing = reply.clone();
        missing
            .as_object_mut()
            .expect("object")
            .remove("total_scans");
        assert_eq!(statistics(&missing), None);
        let mut long = reply.clone();
        long["scan_status"]
            .as_array_mut()
            .expect("array")
            .push(json!({"name": "FAILED", "count": 0}));
        assert_eq!(statistics(&long), None);
        // Skill statistics: a positive count contradicts a missing one.
        assert_eq!(
            skill_statistics(&json!({"total_vulnerabilities_found": {"count": 3},
                "top_vulnerability": null})),
            None
        );
        assert_eq!(
            skill_statistics(&json!({"unique_skills_scanned": {"count": 2}})),
            None
        );
        assert_eq!(
            skill_statistics(&json!({"unique_skills_scanned": {"count": 0}})),
            Some(SkillStatistics {
                unique_skills: 0,
                vulnerabilities: 0
            })
        );
    }

    #[test]
    fn known_labels_are_shown_in_words_and_any_other_is_other() {
        assert_eq!(label("in_progress", &STATUSES), "IN PROGRESS");
        assert_eq!(label("Critical", &RISKS), "CRITICAL");
        assert_eq!(label("FUTURE_STATUS", &STATUSES), "OTHER");
        assert_eq!(
            label("HIGH", &STATUSES),
            "OTHER",
            "labels are per breakdown"
        );
        assert_eq!(label(" COMPLETED", &STATUSES), "OTHER");
        let s = statistics(&json!({"total_scans": 2, "targets_scanned": 1,
            "scan_status": [{"name": "FUTURE_STATUS", "count": 1}, {"name": "NEXT_STATE", "count": 1}],
            "targets_scanned_by_type": [{"name": "agent", "count": 1}]}))
        .expect("valid");
        assert_eq!(
            s.status,
            Some(vec![Bar {
                label: "OTHER".into(),
                count: 2
            }])
        );
        assert_eq!(
            s.target_types,
            Some(vec![Bar {
                label: "AGENT".into(),
                count: 1
            }])
        );
        // Unknown labels still count as activity for a missing total.
        assert_eq!(
            statistics(&json!({"targets_scanned": 0,
                "scan_status": [{"name": "FUTURE_STATUS", "count": 1}]})),
            None
        );
    }

    #[test]
    fn rated_targets_are_validated_before_a_source_counts_as_quiet() {
        let rating = |targets: Value| json!({"risk_profile": [{"risk_rating": "LOW", "total": 0, "targets_by_type": targets}]});
        let many: Vec<Value> = (0..=MAX_BARS)
            .map(|_| json!({"name": "MODEL", "count": 0}))
            .collect();
        for targets in [
            json!([{"name": "MODEL", "count": -1}]),
            json!([{"name": "MODEL", "count": 1.5}]),
            json!([{"name": "MODEL"}]),
            json!([3]),
            json!({"name": "MODEL", "count": 0}),
            json!("MODEL"),
            json!(many),
        ] {
            let reply = rating(targets);
            assert_eq!(statistics(&reply), None, "{reply}");
        }
        assert_eq!(statistics(&json!({"risk_profile": "LOW"})), None);
        // Valid, all-zero nested groups stay quiet.
        let quiet = statistics(&rating(json!([{"name": "MODEL", "count": 0},
            {"name": "unknown-type", "count": 0}])))
        .expect("quiet");
        assert_eq!((quiet.scans, quiet.targets), (0, 0));
        assert_eq!(statistics(&rating(json!(null))).map(|s| s.scans), Some(0));
    }

    #[test]
    fn list_pages_follow_v1s_shape_rules() {
        let row = json!({"uuid": ID});
        assert_eq!(
            red_team_scans(&json!({"data": [row, row], "pagination": {"total_items": 1}})),
            None,
            "a total below the rows on the page"
        );
        assert_eq!(
            red_team_scans(&json!({"data": [], "count_capped": "yes", "pagination": {}})),
            None
        );
        // A present flag must be a boolean, and the pagination object is
        // required (its total may be omitted).
        assert_eq!(
            red_team_scans(&json!({"data": [row], "count_capped": null, "pagination": {}})),
            None
        );
        for pagination in [json!(null), json!("invalid"), json!([]), json!(3)] {
            let reply = json!({"data": [], "pagination": pagination});
            assert_eq!(red_team_scans(&reply), None, "{reply}");
            assert!(!proves_no_scans(&reply), "{reply}");
            assert_eq!(attacks(&reply, ID, 0, 0), None, "{reply}");
        }
        assert_eq!(red_team_scans(&json!({"data": []})), None);
        // Rows are objects; a skill's summary too.
        assert_eq!(
            attacks(&json!({"data": [3], "pagination": {}}), ID, 0, 0),
            None
        );
        assert_eq!(
            skill_scans(
                &json!({"scans": [{"name": "s", "summary": 2}], "pagination": {}}),
                &[0; 16]
            ),
            None
        );
        assert!(
            red_team_scans(&json!({"data": [row], "pagination": {}}))
                .expect("valid")
                .complete
        );
    }

    #[test]
    fn attack_coverage_counts_rows_received() {
        let rows = |n: usize| (0..n).map(|_| json!({"threat": false})).collect::<Vec<_>>();
        let page_of = |n: usize, total: u64, page: u8, received: u32| {
            attacks(
                &json!({"data": rows(n), "pagination": {"total_items": total}}),
                ID,
                page,
                received,
            )
            .expect("valid")
        };
        // 100 + 100 + 50 covers 250.
        assert!(!page_of(100, 250, 0, 0).complete);
        assert!(!page_of(100, 250, 1, 100).complete);
        let last = page_of(50, 250, 2, 200);
        assert!(last.complete);
        assert_eq!(last.rows[0].number, 201);
        // Empty pages against a positive total never cover it, however far
        // the offsets advance.
        assert!(!page_of(0, 150, 0, 0).complete);
        assert!(!page_of(0, 150, 1, 0).complete);
        assert!(!page_of(0, 150, 2, 0).complete);
        assert!(!page_of(50, 150, 2, 0).complete);
        // An empty report is complete.
        assert!(page_of(0, 0, 0, 0).complete);
        // A capped count is a lower bound and never complete.
        let capped = attacks(
            &json!({"data": rows(10), "count_capped": true, "pagination": {"total_items": 10}}),
            ID,
            0,
            0,
        )
        .expect("valid");
        assert!(capped.capped && !capped.complete);
        let scans = red_team_scans(
            &json!({"data": [], "count_capped": true, "pagination": {"total_items": 1000}}),
        )
        .expect("valid");
        assert!(scans.capped && !scans.complete);
        assert!(
            !red_team_scans(&json!({"data": [], "pagination": {"total_items": 0}}))
                .expect("valid")
                .capped
        );
    }

    #[test]
    fn lists_are_complete_only_when_the_service_says_so() {
        let rows = |n: usize| {
            (0..n)
                .map(|_| json!({"uuid": ID, "name": "secret-name"}))
                .collect::<Vec<_>>()
        };
        let complete = |reply: Value| red_team_scans(&reply).map(|s| s.complete);
        assert_eq!(
            complete(json!({"data": rows(3), "pagination": {"total_items": 3}})),
            Some(true)
        );
        assert_eq!(
            complete(json!({"data": rows(3), "pagination": {"total_items": 4}})),
            Some(false)
        );
        assert_eq!(
            complete(json!({"data": rows(3), "pagination": {}})),
            Some(true)
        );
        assert_eq!(
            complete(json!({"data": rows(100), "pagination": {}})),
            Some(false)
        );
        assert_eq!(
            model_scans(
                &json!({"scans": [], "count_capped": true, "pagination": {"total_items": 0}})
            )
            .map(|s| s.complete),
            Some(false)
        );
        assert_eq!(
            complete(json!({"data": rows(101), "pagination": {}})),
            None,
            "more than a page is invalid"
        );
        assert_eq!(
            complete(json!({"data": [{"uuid": "not-a-uuid"}], "pagination": {}})),
            None
        );
        assert!(proves_no_scans(
            &json!({"data": [], "pagination": {"total_items": 0}})
        ));
        assert!(proves_no_scans(&json!({"data": [], "pagination": {}})));
        assert!(!proves_no_scans(
            &json!({"data": [], "pagination": {"total_items": 2}})
        ));
        assert!(!proves_no_scans(
            &json!({"data": rows(1), "pagination": {}})
        ));
    }

    /// V2-T21: a scan reply carrying `target_name` (top level or nested)
    /// yields identifiers and types only; the name never reaches the read
    /// scope the Administration Targets tab renders from.
    #[test]
    fn a_target_name_never_reaches_the_targets_scope() {
        let scans = red_team_scans(&json!({"data": [{"uuid": ID, "status": "COMPLETED",
            "target_id": ID, "target_name": "Payments assistant (prod)",
            "target": {"target_name": "Payments assistant (prod)", "name": "Payments assistant (prod)"},
            "target_type": "APPLICATION", "job_type": "STATIC"}], "pagination": {}}))
        .expect("valid");
        let text = serde_json::to_string(&scans).expect("json");
        assert!(!text.contains("Payments assistant"), "{text}");
        assert!(!text.contains("target_name") && !text.contains("targetName"));
        assert_eq!(scans.rows[0].target_id.as_deref(), Some(ID));
        assert_eq!(scans.rows[0].target_type.as_deref(), Some("APPLICATION"));
    }

    #[test]
    fn rows_keep_no_names_uris_or_findings() {
        let scans = red_team_scans(&json!({"data": [{"uuid": ID, "name": "Secret campaign",
            "status": "COMPLETED", "target": {"name": "Prod bot"}, "target_id": ID,
            "target_type": "APPLICATION", "job_type": "STATIC", "extra": "x"}], "pagination": {}}))
        .expect("valid");
        let text = serde_json::to_string(&scans).expect("json");
        assert!(
            !text.contains("Secret campaign")
                && !text.contains("Prod bot")
                && !text.contains("extra")
        );
        let models = model_scans(
            &json!({"scans": [{"uuid": ID, "model_uri": "s3://bucket/model",
            "security_group_name": "Group", "eval_outcome": "BLOCKED"}], "pagination": {}}),
        )
        .expect("valid");
        let text = serde_json::to_string(&models).expect("json");
        assert!(!text.contains("s3://") && !text.contains("Group"));
        assert_eq!(models.rows[0].outcome.as_deref(), Some("BLOCKED"));
        let salt = [7u8; 16];
        let skills = skill_scans(
            &json!({"scans": [{"name": "my-private-skill", "status": "COMPLETED",
            "summary": {"vulnerability_count": 2, "attack_chain_count": 1}, "eval_outcome": null}],
            "pagination": {"total_items": 1}}),
            &salt,
        )
        .expect("valid");
        let text = serde_json::to_string(&skills).expect("json");
        assert!(!text.contains("my-private-skill"));
        assert_eq!(
            skills.rows[0].pseudonym,
            pseudonym(&salt, "my-private-skill")
        );
        assert_eq!(skills.rows[0].pseudonym.len(), 8);
        assert_ne!(
            pseudonym(&[8u8; 16], "my-private-skill"),
            skills.rows[0].pseudonym
        );
        assert_eq!(skills.rows[0].vulnerabilities, Some(2));
        let page = attacks(
            &json!({"data": [{"uuid": ID, "prompt": "PROMPT-TEXT", "threat": true,
            "severity": "HIGH", "sub_category_display_name": "Jailbreak", "multi_turn": false,
            "extra_info": {"reply": "REPLY"}}], "pagination": {"total_items": 101}}),
            ID,
            1,
            100,
        )
        .expect("valid");
        let text = serde_json::to_string(&page).expect("json");
        assert!(!text.contains("PROMPT-TEXT") && !text.contains("REPLY"));
        assert_eq!(page.rows[0].number, 101);
        assert!(
            page.complete,
            "100 before and one more covers the total of 101"
        );
    }

    #[test]
    fn skill_statistics_are_zero_only_when_quiet() {
        let s = skill_statistics(
            &json!({"unique_skills_scanned": {"count": 4, "percent_change": null},
            "total_vulnerabilities_found": {"count": 7}, "top_vulnerability": {"vulnerability_type": "x", "finding_count": 7}}),
        );
        assert_eq!(
            s,
            Some(SkillStatistics {
                unique_skills: 4,
                vulnerabilities: 7
            })
        );
        assert_eq!(
            skill_statistics(
                &json!({"unique_skills_scanned": {"count": null}, "top_vulnerability": null})
            ),
            Some(SkillStatistics {
                unique_skills: 0,
                vulnerabilities: 0
            })
        );
        assert_eq!(
            skill_statistics(&json!({"unique_skills_scanned": {"count": null},
                "total_vulnerabilities_found": {"count": 3}, "top_vulnerability": {"vulnerability_type": "x"}})),
            None
        );
    }

    #[test]
    fn attack_outcomes_are_read_not_inferred() {
        let page = attacks(
            &json!({"data": [{"threat": null}, {"threat": false}, {}], "pagination": {}}),
            ID,
            0,
            0,
        )
        .expect("valid");
        assert_eq!(
            page.rows.iter().map(|a| a.threat).collect::<Vec<_>>(),
            [None, Some(false), None]
        );
        assert!(page.complete, "a short page without a total");
        assert_eq!(
            attacks(
                &json!({"data": [{"threat": "yes"}], "pagination": {}}),
                ID,
                0,
                0
            ),
            None
        );
        assert!(is_uuid(ID) && !is_uuid("../x") && !is_uuid(&ID.replace('-', "_")));
    }
}
