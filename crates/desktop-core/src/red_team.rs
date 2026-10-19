//! Red Team and Model/skill observation (C13): read-only summaries of AI
//! Red Teaming scan statistics, scan lists and attack results, Model
//! Security scans and AgentGuard skill scans, with v1's rules.
//!
//! Only aggregates and each row's identifier, status, target type, outcome,
//! counts and time leave the core: scan, target, model and skill names,
//! model URIs, security groups, findings, attack prompts and replies and
//! unknown server fields never do. A skill is shown by a pseudonym (the
//! first eight hex digits of SHA-256 over a per-read random salt and its
//! name), so pseudonyms cannot be linked across reads. Totals the service
//! left out are zero only when nothing else in the reply contradicts that;
//! otherwise the reply is invalid. A list is complete only when the service
//! says so (a total no larger than the rows read, or a short page without a
//! total, and never a capped count).

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::management::ManagementFailure;

/// Rows read per list page (v1).
pub const LIST_LIMIT: u32 = 100;
/// Attack results are read 100 per page, at most ten pages (v1).
pub const ATTACK_PAGES: u8 = 10;
/// Bars kept per group (v1).
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

/// One page (100) of a scan's attack results.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AttackPage {
    pub scan_id: String,
    pub page: u8,
    pub rows: Vec<Attack>,
    pub total: Option<u32>,
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

/// A breakdown label: v1 upper-cases it and reads `_` as a space; a label
/// that is not a short plain word becomes OTHER.
fn label(text: &str) -> String {
    let normalized = text.trim().replace('_', " ").to_uppercase();
    let plain = !normalized.is_empty()
        && normalized.len() <= 40
        && normalized
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-');
    if plain {
        normalized
    } else {
        "OTHER".to_owned()
    }
}

/// A reported breakdown: each entry's `name_key` label and `count` (or
/// `total`), duplicates summed, at most 64 bars. `None` when unreported.
fn bars(reply: &Value, key: &str, name_key: &str) -> Result<Option<Vec<Bar>>, ()> {
    let entries = match reply.get(key) {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::Array(entries)) => entries,
        Some(_) => return Err(()),
    };
    let mut out: Vec<Bar> = Vec::new();
    for entry in entries {
        let name = entry.get(name_key).and_then(Value::as_str).ok_or(())?;
        let value = match optional_count(entry, "count")? {
            Some(n) => n,
            None => optional_count(entry, "total")?.ok_or(())?,
        };
        let label = label(name);
        if let Some(bar) = out.iter_mut().find(|b| b.label == label) {
            bar.count = bar.count.saturating_add(value);
        } else if out.len() < MAX_BARS {
            out.push(Bar {
                label,
                count: value,
            });
        }
    }
    Ok(Some(out))
}

/// Whether any risk rating reports a target.
fn rated_targets(reply: &Value) -> bool {
    reply
        .get("risk_profile")
        .and_then(Value::as_array)
        .is_some_and(|ratings| {
            ratings.iter().any(|r| {
                r.get("targets_by_type")
                    .and_then(Value::as_array)
                    .is_some_and(|t| {
                        t.iter()
                            .any(|e| e.get("count").and_then(count).is_some_and(|n| n > 0))
                    })
            })
        })
}

/// Scan statistics by v1's rules; `None` when the reply is invalid.
pub fn statistics(reply: &Value) -> Option<RedTeamStatistics> {
    if !reply.is_object() {
        return None;
    }
    let status = bars(reply, "scan_status", "name").ok()?;
    let risk = bars(reply, "risk_profile", "risk_rating").ok()?;
    let target_types = bars(reply, "targets_scanned_by_type", "name").ok()?;
    // A missing total is zero only when nothing in the reply is above zero.
    let quiet = [&status, &risk, &target_types].iter().all(|group| {
        group
            .as_ref()
            .is_none_or(|b| b.iter().all(|b| b.count == 0))
    }) && !rated_targets(reply);
    let tally = |key: &str| match optional_count(reply, key) {
        Ok(Some(n)) => Some(n),
        Ok(None) if quiet => Some(0),
        _ => None,
    };
    Some(RedTeamStatistics {
        scans: tally("total_scans")?,
        targets: tally("targets_scanned")?,
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

/// A list page's rows, total and completeness (v1): complete only when the
/// count is not capped and the total is no larger than `before + rows`, or,
/// without a total, the page is short.
fn page<'a>(
    reply: &'a Value,
    rows_key: &str,
    before: u32,
) -> Option<(&'a Vec<Value>, Option<u32>, bool)> {
    let rows = reply.get(rows_key)?.as_array()?;
    let total = match reply.get("pagination").and_then(|p| p.get("total_items")) {
        None | Some(Value::Null) => None,
        Some(v) => Some(count(v)?),
    };
    let capped = reply.get("count_capped").and_then(Value::as_bool) == Some(true);
    let read = before.saturating_add(u32::try_from(rows.len()).unwrap_or(u32::MAX));
    let complete = !capped
        && match total {
            Some(total) => total <= read,
            None => rows.len() < LIST_LIMIT as usize,
        };
    (rows.len() <= LIST_LIMIT as usize).then_some((rows, total, complete))
}

/// Whether a one-row scan list proves there are no scans (the confirmation
/// v1 makes when the statistics are absent).
pub fn proves_no_scans(reply: &Value) -> bool {
    page(reply, "data", 0).is_some_and(|(rows, total, complete)| {
        complete && rows.is_empty() && total.unwrap_or(0) == 0
    })
}

fn id_of(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|id| is_uuid(id))
        .map(str::to_ascii_lowercase)
}

/// The first page of Red Teaming scans; `None` when invalid.
pub fn red_team_scans(reply: &Value) -> Option<RedTeamScans> {
    let (rows, total, complete) = page(reply, "data", 0)?;
    let rows = rows
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
        total,
        complete,
    })
}

/// The first page of Model Security scans; `None` when invalid.
pub fn model_scans(reply: &Value) -> Option<ModelScans> {
    let (rows, total, complete) = page(reply, "scans", 0)?;
    let rows = rows
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
        total,
        complete,
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
    let (rows, total, complete) = page(reply, "scans", 0)?;
    let rows = rows
        .iter()
        .map(|row| {
            let name = row.get("name").and_then(Value::as_str)?;
            let vulnerabilities = match row.get("summary") {
                None | Some(Value::Null) => None,
                Some(summary) => optional_count(summary, "vulnerability_count").ok()?,
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
        total,
        complete,
    })
}

/// AgentGuard's 30-day statistics: an absent count is zero only when no top
/// vulnerability is reported; `None` when invalid.
pub fn skill_statistics(reply: &Value) -> Option<SkillStatistics> {
    let quiet = matches!(reply.get("top_vulnerability"), None | Some(Value::Null));
    let tally = |key: &str| match reply.get(key) {
        None | Some(Value::Null) => quiet.then_some(0),
        Some(group) => match optional_count(group, "count") {
            Ok(Some(n)) => Some(n),
            Ok(None) if quiet => Some(0),
            _ => None,
        },
    };
    reply.is_object().then_some(())?;
    Some(SkillStatistics {
        unique_skills: tally("unique_skills_scanned")?,
        vulnerabilities: tally("total_vulnerabilities_found")?,
    })
}

/// One page of attack results (`page` 0-9); `None` when invalid.
pub fn attacks(reply: &Value, scan_id: &str, page_index: u8) -> Option<AttackPage> {
    let before = u32::from(page_index) * LIST_LIMIT;
    let (rows, total, complete) = page(reply, "data", before)?;
    let rows = rows
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
        total,
        complete,
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
            complete(json!({"data": rows(101)})),
            None,
            "more than a page is invalid"
        );
        assert_eq!(complete(json!({"data": [{"uuid": "not-a-uuid"}]})), None);
        assert!(proves_no_scans(
            &json!({"data": [], "pagination": {"total_items": 0}})
        ));
        assert!(proves_no_scans(&json!({"data": []})));
        assert!(!proves_no_scans(
            &json!({"data": [], "pagination": {"total_items": 2}})
        ));
        assert!(!proves_no_scans(&json!({"data": rows(1)})));
    }

    #[test]
    fn rows_keep_no_names_uris_or_findings() {
        let scans = red_team_scans(&json!({"data": [{"uuid": ID, "name": "Secret campaign",
            "status": "COMPLETED", "target": {"name": "Prod bot"}, "target_id": ID,
            "target_type": "APPLICATION", "job_type": "STATIC", "extra": "x"}]}))
        .expect("valid");
        let text = serde_json::to_string(&scans).expect("json");
        assert!(
            !text.contains("Secret campaign")
                && !text.contains("Prod bot")
                && !text.contains("extra")
        );
        let models = model_scans(
            &json!({"scans": [{"uuid": ID, "model_uri": "s3://bucket/model",
            "security_group_name": "Group", "eval_outcome": "BLOCKED"}]}),
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
            &json!({"data": [{"threat": null}, {"threat": false}, {}]}),
            ID,
            0,
        )
        .expect("valid");
        assert_eq!(
            page.rows.iter().map(|a| a.threat).collect::<Vec<_>>(),
            [None, Some(false), None]
        );
        assert!(page.complete, "a short page without a total");
        assert_eq!(attacks(&json!({"data": [{"threat": "yes"}]}), ID, 0), None);
        assert!(is_uuid(ID) && !is_uuid("../x") && !is_uuid(&ID.replace('-', "_")));
    }
}
