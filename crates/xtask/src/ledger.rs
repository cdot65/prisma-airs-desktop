//! Machine-readable workflow ledger generated from docs/architecture/workflows.md.
//!
//! Each row keeps the four evidence categories separate and starts unrun;
//! a row's live requirement is `required` whenever its Live column names an
//! L-family (or says required), otherwise `not_required` with the stated
//! non-live class as the reason. Access status starts unassessed.

use serde::{Deserialize, Serialize};

use crate::{Repo, Result, invalid, io_err, read_json, write_json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub source_implementation: String,
    pub fixture_contract: String,
    pub native_live: String,
    pub installed_artifact: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveRequirement {
    pub requirement: String,
    pub reason: String,
    pub access_status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub id: String,
    pub workflow: String,
    pub chunks: String,
    pub dependency_contract: String,
    pub live_column: String,
    pub live: LiveRequirement,
    pub evidence: Evidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ledger {
    pub schema: String,
    pub source: String,
    pub rows: Vec<Row>,
}

const LEDGER_PATH: &str = "docs/architecture/workflow-ledger.json";

pub fn parse_markdown(text: &str) -> Result<Vec<Row>> {
    let mut rows = Vec::new();
    for line in text.lines().filter(|l| l.starts_with("| W")) {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() != 5 {
            return Err(invalid(format!(
                "workflow row has {} cells: {line}",
                cells.len()
            )));
        }
        let live_column = cells[4].to_owned();
        let required = live_column.contains("required")
            || live_column
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|token| {
                    token.len() == 3
                        && token.starts_with('L')
                        && token[1..].bytes().all(|b| b.is_ascii_digit())
                });
        rows.push(Row {
            id: cells[0].to_owned(),
            workflow: cells[1].to_owned(),
            chunks: cells[2].to_owned(),
            dependency_contract: cells[3].to_owned(),
            live: LiveRequirement {
                requirement: if required { "required" } else { "not_required" }.to_owned(),
                reason: if required {
                    "Live column names an L-family representative operation".to_owned()
                } else {
                    format!("non-live evidence classes only: {live_column}")
                },
                access_status: "not-assessed".to_owned(),
            },
            live_column,
            evidence: Evidence {
                source_implementation: "unrun".to_owned(),
                fixture_contract: "unrun".to_owned(),
                native_live: "unrun".to_owned(),
                installed_artifact: "unrun".to_owned(),
            },
        });
    }
    let expected = 66;
    if rows.len() != expected {
        return Err(invalid(format!(
            "expected {expected} workflow rows, found {}",
            rows.len()
        )));
    }
    Ok(rows)
}

/// Writes the ledger, preserving recorded evidence statuses of existing rows.
pub fn generate(repo: &Repo) -> Result<()> {
    let path = repo.join("docs/architecture/workflows.md");
    let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
    let mut rows = parse_markdown(&text)?;
    if let Ok(existing) = read_json::<Ledger>(&repo.join(LEDGER_PATH)) {
        for row in &mut rows {
            if let Some(old) = existing.rows.iter().find(|r| r.id == row.id) {
                row.evidence = old.evidence.clone();
                row.live.access_status = old.live.access_status.clone();
            }
        }
    }
    write_json(
        &repo.join(LEDGER_PATH),
        &Ledger {
            schema: "airs-v2/workflow-ledger/1".to_owned(),
            source: "docs/architecture/workflows.md".to_owned(),
            rows,
        },
    )
}

/// Fails when the committed ledger's row identities drift from the Markdown.
pub fn check(repo: &Repo) -> Result<()> {
    let path = repo.join("docs/architecture/workflows.md");
    let text = std::fs::read_to_string(&path).map_err(io_err(&path))?;
    let expected = parse_markdown(&text)?;
    let committed: Ledger = read_json(&repo.join(LEDGER_PATH))?;
    if committed.rows.len() != expected.len() {
        return Err(invalid(
            "workflow ledger row count differs from workflows.md",
        ));
    }
    for (have, want) in committed.rows.iter().zip(&expected) {
        if have.id != want.id
            || have.workflow != want.workflow
            || have.chunks != want.chunks
            || have.dependency_contract != want.dependency_contract
            || have.live_column != want.live_column
            || have.live.requirement != want.live.requirement
        {
            return Err(invalid(format!(
                "workflow ledger row {} is out of date",
                want.id
            )));
        }
    }
    Ok(())
}

/// The chunks a row's `chunks` cell names (`C08,C09`, `C03–C24`).
fn named_chunks(cell: &str) -> Vec<u8> {
    let number = |s: &str| {
        s.trim()
            .strip_prefix('C')
            .and_then(|n| n.parse::<u8>().ok())
    };
    let mut out = Vec::new();
    for part in cell.split(',') {
        match part.split_once('–') {
            Some((a, b)) => {
                if let (Some(a), Some(b)) = (number(a), number(b)) {
                    out.extend(a..=b);
                }
            }
            None => out.extend(number(part)),
        }
    }
    out
}

/// One check's outcome per live receipt, by workflow ID.
fn live_outcomes(repo: &Repo) -> Result<std::collections::BTreeMap<String, Vec<String>>> {
    let mut by_workflow: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let root = repo.join("docs/planning/evidence/C25/live");
    let Ok(families) = std::fs::read_dir(&root) else {
        return Ok(by_workflow);
    };
    for family in families.flatten() {
        let name = family.file_name().to_string_lossy().into_owned();
        if !(name.starts_with('L') && family.path().is_dir()) {
            continue;
        }
        for receipt in std::fs::read_dir(family.path())
            .map_err(io_err(&family.path()))?
            .flatten()
        {
            let path = receipt.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let value: serde_json::Value = read_json(&path)?;
            let outcome = value["outcome"].as_str().unwrap_or("unknown").to_owned();
            for w in value["workflows"].as_array().into_iter().flatten() {
                if let Some(w) = w.as_str() {
                    by_workflow
                        .entry(w.to_owned())
                        .or_default()
                        .push(outcome.clone());
                }
            }
        }
    }
    Ok(by_workflow)
}

/// A chunk's newest native acceptance record: every check passed.
fn native_passed(repo: &Repo, chunk: u8) -> bool {
    let file = format!("native-acceptance-C{chunk:02}.json");
    let Ok(dirs) = std::fs::read_dir(repo.join("docs/planning/evidence")) else {
        return false;
    };
    let newest = dirs
        .flatten()
        .filter_map(|d| read_json::<serde_json::Value>(&d.path().join(&file)).ok())
        .max_by(|a, b| {
            a["recorded_utc"]
                .as_str()
                .unwrap_or("")
                .cmp(b["recorded_utc"].as_str().unwrap_or(""))
        });
    newest.is_some_and(|n| {
        n["checks"]
            .as_array()
            .is_some_and(|c| !c.is_empty() && c.iter().all(|c| c["result"] == "pass"))
    })
}

/// A chunk has a passed test gate run.
fn test_passed(repo: &Repo, chunk: u8) -> bool {
    let runs = repo.evidence_dir(crate::Subject::Chunk(chunk)).join("runs");
    std::fs::read_dir(runs).is_ok_and(|d| {
        d.flatten().any(|r| {
            r.file_name().to_string_lossy().starts_with("test-")
                && read_json::<serde_json::Value>(&r.path().join("run.json"))
                    .is_ok_and(|v| v["status"] == "passed")
        })
    })
}

/// The live column of a required row from its receipts.
fn live_status(outcomes: Option<&Vec<String>>) -> &'static str {
    match outcomes {
        None => "no receipt",
        Some(o) if o.iter().all(|s| s == "passed") => "passed",
        Some(o)
            if o.iter()
                .any(|s| s == "failed" || s == "needs-reconciliation") =>
        {
            "failed"
        }
        Some(o) if o.iter().any(|s| s == "blocked") => "blocked",
        Some(_) => "native app (C26)",
    }
}

/// Records each row's evidence from the repository's own records: chunk
/// completions (a passing review and a verified commit), passed test gate
/// runs, the newest native acceptance records, the C25 live receipts and
/// the designated live scope. The installed artifact is C26's.
pub fn record(repo: &Repo) -> Result<()> {
    let path = repo.join(LEDGER_PATH);
    let mut ledger: Ledger = read_json(&path)?;
    let entries = crate::complete::read_ledger(&repo.completion_ledger())?;
    let live = live_outcomes(repo)?;
    let scope: serde_json::Value =
        read_json(&repo.join("docs/planning/evidence/C25/live-scope.json"))?;
    let completed = |c: u8| crate::complete::is_completed(repo, &entries, crate::Subject::Chunk(c));
    for row in &mut ledger.rows {
        let chunks = named_chunks(&row.chunks);
        if chunks.is_empty() {
            return Err(invalid(format!("row {} names no chunk", row.id)));
        }
        let all = |f: &dyn Fn(u8) -> bool| chunks.iter().all(|c| f(*c));
        row.evidence.source_implementation = if all(&completed) {
            "passed"
        } else {
            "incomplete"
        }
        .to_owned();
        row.evidence.fixture_contract = if all(&|c| test_passed(repo, c)) {
            "passed"
        } else {
            "incomplete"
        }
        .to_owned();
        let native = if all(&|c| c > 24 || native_passed(repo, c)) {
            "passed"
        } else {
            "incomplete"
        };
        row.evidence.native_live = if row.live.requirement == "required" {
            format!("native {native}; live {}", live_status(live.get(&row.id)))
        } else {
            format!("native {native}; live not required")
        };
        row.evidence.installed_artifact = "C26".to_owned();
        if let Some(status) = scope["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["id"] == row.id.as_str()))
            .and_then(|r| r["access_status"].as_str())
        {
            row.live.access_status = status.to_owned();
        }
    }
    write_json(&path, &ledger)
}

#[cfg(test)]
mod record_tests {
    use super::{live_status, named_chunks};

    #[test]
    fn a_rows_chunks_and_live_status_are_read_exactly() {
        assert_eq!(named_chunks("C08,C09"), vec![8, 9]);
        assert_eq!(named_chunks("C23–C26"), vec![23, 24, 25, 26]);
        assert_eq!(named_chunks("C02"), vec![2]);
        let o = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(live_status(None), "no receipt");
        assert_eq!(live_status(Some(&o(&["passed", "passed"]))), "passed");
        assert_eq!(live_status(Some(&o(&["passed", "blocked"]))), "blocked");
        assert_eq!(live_status(Some(&o(&["blocked", "failed"]))), "failed");
        assert_eq!(
            live_status(Some(&o(&["passed", "native-app-required"]))),
            "native app (C26)"
        );
    }
}
