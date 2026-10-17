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
