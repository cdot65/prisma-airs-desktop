//! Pre-registered performance budgets (T20): the unsigned production
//! footprint build and the evidence check the `native` gate applies to the
//! chunk that registered the budgets, the cumulative chunk (C24) and the
//! phases containing either.
//!
//! Evidence must name the registered budgets hash and start after the
//! registration; statistics are recomputed here from the raw samples, never
//! taken from the measurement's own summary. A missing, retroactive or
//! mismatched input fails.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::env::Environment;
use crate::manifest::Manifest;
use crate::native::{Prepared, bundle_path, plist_value};
use crate::{Repo, Result, Subject, invalid, io_err, now_utc, read_json, sha256_hex, write_json};

pub const BUDGETS: &str = "docs/architecture/budgets.json";
const PRODUCT: &str = "Prisma AIRS Desktop v2";

#[derive(Clone, Debug, Deserialize)]
pub struct Budgets {
    pub version: u32,
    pub registered_by_chunk: String,
    pub budgets: Vec<Budget>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Budget {
    pub id: String,
    pub statistic: String,
    pub limit: f64,
    pub unit: String,
    pub samples: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub schema: String,
    pub budgets: String,
    pub budgets_sha256: String,
    pub version: u32,
    pub registered_utc: String,
    pub base_commit: String,
    pub statement: String,
    /// The registration this one replaces (its record stays in evidence).
    #[serde(default)]
    pub supersedes: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metric {
    pub unit: String,
    pub samples: Vec<f64>,
}

/// The measurement script's record (tools/perf/measure_c03.py).
#[derive(Clone, Debug, Deserialize)]
pub struct Perf {
    pub schema: String,
    pub label: String,
    pub budgets_sha256: String,
    pub started_utc: String,
    pub executable_sha256: String,
    pub metrics: BTreeMap<String, Metric>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Footprint {
    pub schema: String,
    pub budgets_sha256: String,
    pub started_utc: String,
    pub finished_utc: String,
    pub identifier: String,
    pub app: String,
    pub app_tree_sha256: String,
    pub dmg: String,
    pub dmg_sha256: String,
    pub metrics: BTreeMap<String, Metric>,
    pub commands: Vec<Vec<String>>,
}

fn registration(repo: &Repo, chunk: &str) -> Result<(Budgets, Registration, String)> {
    let path = repo.join(BUDGETS);
    let bytes = std::fs::read(&path).map_err(io_err(&path))?;
    let sha = sha256_hex(&bytes);
    let budgets: Budgets = read_json(&path)?;
    let record: Registration = read_json(&repo.join(format!(
        "docs/planning/evidence/{chunk}/budget-registration.json"
    )))?;
    if record.budgets != BUDGETS
        || record.budgets_sha256 != sha
        || record.version != budgets.version
    {
        return Err(invalid(format!(
            "{BUDGETS} (sha256 {sha}, version {}) does not match its registration (sha256 {}, version {}); a change needs a new registered version",
            budgets.version, record.budgets_sha256, record.version
        )));
    }
    Ok((budgets, record, sha))
}

fn file_bytes(dir: &Path) -> Result<u64> {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).map_err(io_err(&d))? {
            let entry = entry.map_err(io_err(&d))?;
            let meta = std::fs::symlink_metadata(entry.path()).map_err(io_err(&entry.path()))?;
            if meta.is_dir() {
                stack.push(entry.path());
            } else if meta.is_file() {
                total += meta.len();
            }
        }
    }
    Ok(total)
}

const MIB: f64 = 1024.0 * 1024.0;

/// Builds the complete unsigned production app and DMG, the sealed Harness
/// engine included (C19, D046; budgets.json version 5), and records their
/// sizes.
pub fn footprint(
    repo: &Repo,
    manifest: &Manifest,
    env: &Environment,
    subject: Subject,
) -> Result<Footprint> {
    let Some(chunk) = required(repo, manifest, subject)? else {
        return Err(invalid(format!(
            "{subject} does not contain the chunk that registered the budgets"
        )));
    };
    let (_, record, sha) = registration(repo, &chunk)?;
    let started_utc = now_utc();
    if started_utc < record.registered_utc {
        return Err(invalid("the clock is before the budget registration"));
    }
    let target = manifest.target.as_str();
    // The sealed engine the production app ships with (C19).
    crate::harness::unpack(repo)?;
    let commands: Vec<Vec<String>> = [
        vec!["pnpm", "--filter", "@prisma-airs/desktop", "build"],
        vec![
            "pnpm",
            "--filter",
            "@prisma-airs/desktop",
            "exec",
            "tauri",
            "build",
            "--ci",
            "--no-sign",
            "--target",
            target,
            "--bundles",
            "app,dmg",
            // The sealed Harness engine as a bundle resource (C19, D046).
            "--config",
            "src-tauri/tauri.harness.conf.json",
        ],
    ]
    .iter()
    .map(|argv| argv.iter().map(|s| (*s).to_owned()).collect())
    .collect();
    for argv in &commands {
        let status = env
            .command(argv, repo.root())?
            .status()
            .map_err(io_err(repo.root()))?;
        if !status.success() {
            return Err(invalid(format!("{} failed with {status}", argv.join(" "))));
        }
    }
    let app = bundle_path(repo, target, PRODUCT);
    let identifier = plist_value(&app, "CFBundleIdentifier")?;
    if identifier != crate::native::PRODUCTION_IDENTIFIER {
        return Err(invalid(format!(
            "footprint bundle has identifier {identifier}"
        )));
    }
    let version = plist_value(&app, "CFBundleShortVersionString")?;
    let dmg = repo.join(format!(
        "target/{target}/release/bundle/dmg/{PRODUCT}_{version}_aarch64.dmg"
    ));
    let dmg_bytes = std::fs::read(&dmg).map_err(io_err(&dmg))?;
    let relative = |p: &Path| {
        p.strip_prefix(repo.root())
            .unwrap_or(p)
            .display()
            .to_string()
    };
    let mut metrics = BTreeMap::new();
    metrics.insert(
        "app-bundle-bytes".to_owned(),
        Metric {
            unit: "MiB".to_owned(),
            samples: vec![file_bytes(&app)? as f64 / MIB],
        },
    );
    metrics.insert(
        "dmg-bytes".to_owned(),
        Metric {
            unit: "MiB".to_owned(),
            samples: vec![dmg_bytes.len() as f64 / MIB],
        },
    );
    let footprint = Footprint {
        schema: "airs-v2-footprint/1".to_owned(),
        budgets_sha256: sha,
        started_utc,
        finished_utc: now_utc(),
        identifier,
        app: relative(&app),
        app_tree_sha256: crate::fingerprint::tree_hash(&app)?,
        dmg: relative(&dmg),
        dmg_sha256: sha256_hex(&dmg_bytes),
        metrics,
        commands,
    };
    write_json(
        &repo.evidence_dir(subject).join("footprint.json"),
        &footprint,
    )?;
    Ok(footprint)
}

/// Nearest-rank statistic over raw samples.
pub fn statistic(name: &str, samples: &[f64]) -> Option<f64> {
    let mut sorted: Vec<f64> = samples.iter().copied().filter(|v| v.is_finite()).collect();
    if sorted.len() != samples.len() || sorted.is_empty() {
        return None;
    }
    sorted.sort_by(f64::total_cmp);
    let rank =
        |q: f64| sorted[((q * sorted.len() as f64).ceil() as usize).clamp(1, sorted.len()) - 1];
    match name {
        "p95" => Some(rank(0.95)),
        "median" => Some(rank(0.5)),
        "max" | "value" => sorted.last().copied(),
        _ => None,
    }
}

/// The chunk that re-measures every registered budget on its own build
/// (C24's cumulative closure, T20): the core budgets and every workload,
/// each recorded under its measuring chunk's name.
pub const CUMULATIVE: u8 = 24;

fn cumulative(subject: Subject) -> bool {
    subject == Subject::Chunk(CUMULATIVE)
}

/// Whether the subject re-measures every registered budget, the footprint
/// included: the cumulative chunk, and a phase containing it. The phase
/// reviews a later tree than the chunk did, so the chunk's measurements
/// cannot stand in for its own.
fn carries_every_budget(subject: Subject, members: &[String]) -> bool {
    cumulative(subject)
        || (subject.is_phase() && members.contains(&Subject::Chunk(CUMULATIVE).to_string()))
}

/// Whether this subject must carry budget evidence.
pub fn required(repo: &Repo, manifest: &Manifest, subject: Subject) -> Result<Option<String>> {
    let path = repo.join(BUDGETS);
    if !path.exists() {
        return Ok(None);
    }
    let budgets: Budgets = read_json(&path)?;
    let members = manifest.native_members(subject)?;
    Ok(
        (carries_every_budget(subject, &members) || members.contains(&budgets.registered_by_chunk))
            .then_some(budgets.registered_by_chunk),
    )
}

/// Reasons the subject's budget evidence does not pass (empty when it does).
pub fn status(repo: &Repo, manifest: &Manifest, subject: Subject) -> Vec<String> {
    let mut failures = match required(repo, manifest, subject) {
        Ok(None) => Vec::new(),
        Ok(Some(chunk)) => evaluate(repo, subject, &chunk).unwrap_or_else(|e| vec![e.to_string()]),
        Err(e) => vec![e.to_string()],
    };
    match manifest.native_members(subject) {
        Ok(members) => {
            for workload in WORKLOADS {
                if carries_every_budget(subject, &members)
                    || members.iter().any(|m| m == workload.measured_by)
                {
                    failures.extend(
                        evaluate_workload(repo, subject, workload)
                            .unwrap_or_else(|e| vec![e.to_string()]),
                    );
                }
            }
        }
        Err(e) => failures.push(e.to_string()),
    }
    failures
}

/// A later workload's budgets, registered (as a reviewed table) by one chunk
/// before the chunk that implements the workload measures it.
pub struct Workload {
    pub registered_by: &'static str,
    pub registration: &'static str,
    pub measured_by: &'static str,
    pub evidence: &'static str,
}

/// The Academy hosted content and learner sync workload (registered in C04,
/// measured in C05), the large management collections workload (registered
/// by C11 before it implemented Sessions, measured in C11) and the large
/// red-team/model collections workload (registered by C13 before it
/// implemented Red Team observation, measured in C13) and the large HTTP
/// bodies and downloads workload (registered by C15 before it implemented
/// API Client execution, measured in C15), the Harness native-engine
/// startup and memory workload (C19), the protected Harness history
/// workload (C20), the Harness tool workload (C21) and the MCP server
/// session workload (C22, each registered before its chunk was built);
/// each also in the phases containing its measuring chunk.
pub const WORKLOADS: &[Workload] = &[
    Workload {
        registered_by: "C04",
        registration: "docs/planning/evidence/C04/workload-budget-registration.json",
        measured_by: "C05",
        evidence: "workload-perf.json",
    },
    Workload {
        registered_by: "C11",
        registration: "docs/planning/evidence/C11/workload-budget-registration.json",
        measured_by: "C11",
        evidence: "workload-perf.json",
    },
    Workload {
        registered_by: "C13",
        registration: "docs/planning/evidence/C13/workload-budget-registration.json",
        measured_by: "C13",
        evidence: "workload-perf.json",
    },
    Workload {
        registered_by: "C15",
        registration: "docs/planning/evidence/C15/workload-budget-registration.json",
        measured_by: "C15",
        evidence: "workload-perf.json",
    },
    // Registered and measured by C18; listed here from C24, whose
    // cumulative closure found it missing (C18 and P05 did not evaluate
    // it; their recorded measurement passes its table).
    Workload {
        registered_by: "C18",
        registration: "docs/planning/evidence/C18/workload-budget-registration.json",
        measured_by: "C18",
        evidence: "workload-perf.json",
    },
    Workload {
        registered_by: "C19",
        registration: "docs/planning/evidence/C19/workload-budget-registration.json",
        measured_by: "C19",
        evidence: "workload-perf.json",
    },
    Workload {
        registered_by: "C20",
        registration: "docs/planning/evidence/C20/workload-budget-registration.json",
        measured_by: "C20",
        evidence: "workload-perf.json",
    },
    Workload {
        registered_by: "C21",
        registration: "docs/planning/evidence/C21/workload-budget-registration.json",
        measured_by: "C21",
        evidence: "workload-perf.json",
    },
    Workload {
        registered_by: "C22",
        registration: "docs/planning/evidence/C22/workload-budget-registration.json",
        measured_by: "C22",
        evidence: "workload-perf.json",
    },
    // Long typed input under an active assistive client (C03's open
    // observation), registered by C24 before it measured it.
    Workload {
        registered_by: "C24",
        registration: "docs/planning/evidence/C24/workload-budget-registration.json",
        measured_by: "C24",
        evidence: "workload-perf.json",
    },
    // The v2 tenant switch (D058): the new tenant's Home from cached
    // scopes, registered by C30 before it measured it.
    Workload {
        registered_by: "C30",
        registration: "docs/planning/evidence/C30/workload-budget-registration.json",
        measured_by: "C30",
        evidence: "workload-perf.json",
    },
];

/// Budgets from a registered Markdown table:
/// `| id | statistic | <limit> <unit> | samples | method |`.
pub fn workload_budgets(table: &str) -> Result<Vec<Budget>> {
    let mut budgets = Vec::new();
    for line in table.lines() {
        let cells: Vec<&str> = line
            .trim()
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if cells.len() < 5 || cells[0] == "Budget" || cells[0].starts_with("---") {
            continue;
        }
        let (limit, unit) = cells[2]
            .split_once(' ')
            .ok_or_else(|| invalid(format!("budget row without a unit: {line}")))?;
        budgets.push(Budget {
            id: cells[0].to_owned(),
            statistic: cells[1].to_owned(),
            limit: limit
                .parse()
                .map_err(|_| invalid(format!("budget limit is not a number: {line}")))?,
            unit: unit.to_owned(),
            samples: cells[3]
                .parse()
                .map_err(|_| invalid(format!("budget sample count is not a number: {line}")))?,
        });
    }
    if budgets.is_empty() {
        return Err(invalid("the registered workload table lists no budgets"));
    }
    Ok(budgets)
}

/// The workload measurement record (tools/perf/measure_c05.py).
#[derive(Clone, Debug, Deserialize)]
pub struct WorkloadPerf {
    pub schema: String,
    pub label: String,
    pub registration_sha256: String,
    pub started_utc: String,
    pub executable_sha256: String,
    pub metrics: BTreeMap<String, Metric>,
}

/// Where a subject's measurement of `workload` is recorded: the measuring
/// chunk's `workload-perf.json`, or in a phase (which may measure several)
/// `workload-perf-<measuring chunk>.json`, falling back to the one name
/// phases with a single workload used.
fn workload_evidence(dir: &std::path::Path, subject: Subject, workload: &Workload) -> String {
    match subject {
        // The cumulative chunk records every other chunk's workload under
        // that chunk's name.
        Subject::Chunk(c) if cumulative(subject) && format!("C{c:02}") != workload.measured_by => {
            format!("workload-perf-{}.json", workload.measured_by)
        }
        Subject::Chunk(_) => workload.evidence.to_owned(),
        Subject::Phase(_) => {
            let named = format!("workload-perf-{}.json", workload.measured_by);
            if dir.join(&named).exists() {
                named
            } else {
                workload.evidence.to_owned()
            }
        }
    }
}

fn evaluate_workload(repo: &Repo, subject: Subject, workload: &Workload) -> Result<Vec<String>> {
    let record: Registration = read_json(&repo.join(workload.registration))?;
    let table_path = repo.join(&record.budgets);
    let table = std::fs::read(&table_path).map_err(io_err(&table_path))?;
    let table_sha = sha256_hex(&table);
    if table_sha != record.budgets_sha256 {
        return Err(invalid(format!(
            "{} (sha256 {table_sha}) does not match its {} registration ({}); a change needs a new registered version",
            record.budgets, workload.registered_by, record.budgets_sha256
        )));
    }
    let budgets = workload_budgets(&String::from_utf8_lossy(&table))?;
    let dir = repo.evidence_dir(subject);
    let mut failures = Vec::new();
    let prepared: Option<Prepared> = read_json(&dir.join("native-prepare.json")).ok();
    let evidence = workload_evidence(&dir, subject, workload);
    let perf: WorkloadPerf = match read_json(&dir.join(&evidence)) {
        Ok(perf) => perf,
        Err(e) => return Ok(vec![format!("blocked: {e}")]),
    };
    if perf.schema != "airs-v2-workload-perf/1" || perf.label != "acceptance" {
        failures.push(format!("{evidence} is not an acceptance measurement"));
    }
    if perf.registration_sha256 != table_sha {
        failures.push(format!(
            "{evidence} names budgets {} not the registered {table_sha}",
            perf.registration_sha256
        ));
    }
    if perf.started_utc < record.registered_utc {
        failures.push(format!(
            "{evidence} started before the budgets were registered"
        ));
    }
    if prepared.as_ref().map(|p| p.executable_sha256.as_str())
        != Some(perf.executable_sha256.as_str())
    {
        failures.push(format!(
            "{evidence} was not measured on the prepared fixture executable"
        ));
    }
    for budget in &budgets {
        failures.extend(judge(budget, perf.metrics.get(&budget.id)));
    }
    Ok(failures)
}

/// One budget against its raw samples (statistic recomputed here).
fn judge(budget: &Budget, metric: Option<&Metric>) -> Option<String> {
    let Some(metric) = metric else {
        return Some(format!("{}: no measurement", budget.id));
    };
    if metric.unit != budget.unit {
        return Some(format!(
            "{}: unit {} is not {}",
            budget.id, metric.unit, budget.unit
        ));
    }
    if metric.samples.len() < budget.samples {
        return Some(format!(
            "{}: {} samples, {} required",
            budget.id,
            metric.samples.len(),
            budget.samples
        ));
    }
    match statistic(&budget.statistic, &metric.samples) {
        Some(value) if value <= budget.limit => {
            println!(
                "budget {} {} {:.1} {} <= {}",
                budget.id, budget.statistic, value, budget.unit, budget.limit
            );
            None
        }
        Some(value) => Some(format!(
            "{}: {} {:.1} {} exceeds {}",
            budget.id, budget.statistic, value, budget.unit, budget.limit
        )),
        None => Some(format!(
            "{}: undefined statistic {}",
            budget.id, budget.statistic
        )),
    }
}

fn evaluate(repo: &Repo, subject: Subject, chunk: &str) -> Result<Vec<String>> {
    let (budgets, record, sha) = registration(repo, chunk)?;
    let dir = repo.evidence_dir(subject);
    let mut failures = Vec::new();
    let mut metrics: BTreeMap<String, Metric> = BTreeMap::new();
    let prepared: Option<Prepared> = read_json(&dir.join("native-prepare.json")).ok();
    match read_json::<Perf>(&dir.join("perf.json")) {
        Ok(perf) => {
            if perf.schema != "airs-v2-perf/1" || perf.label != "acceptance" {
                failures.push("perf.json is not an acceptance measurement".to_owned());
            }
            if perf.budgets_sha256 != sha {
                failures.push(format!(
                    "perf.json names budgets {} not the registered {sha}",
                    perf.budgets_sha256
                ));
            }
            if perf.started_utc < record.registered_utc {
                failures.push("perf.json started before the budgets were registered".to_owned());
            }
            if prepared.as_ref().map(|p| p.executable_sha256.as_str())
                != Some(perf.executable_sha256.as_str())
            {
                failures.push(
                    "perf.json was not measured on the prepared fixture executable".to_owned(),
                );
            }
            metrics.extend(perf.metrics);
        }
        Err(e) => failures.push(format!("blocked: {e}")),
    }
    match read_json::<Footprint>(&dir.join("footprint.json")) {
        Ok(footprint) => {
            if footprint.budgets_sha256 != sha || footprint.started_utc < record.registered_utc {
                failures.push("footprint.json does not follow the registered budgets".to_owned());
            }
            let current = repo.join(&footprint.app);
            if crate::fingerprint::tree_hash(&current).ok().as_deref()
                != Some(footprint.app_tree_sha256.as_str())
            {
                failures.push("the measured production app is no longer the one on disk; run xtask footprint again".to_owned());
            }
            metrics.extend(footprint.metrics);
        }
        Err(e) => failures.push(format!("blocked: {e}")),
    }
    for budget in &budgets.budgets {
        failures.extend(judge(budget, metrics.get(&budget.id)));
    }
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::{
        Budget, CUMULATIVE, Metric, WORKLOADS, carries_every_budget, cumulative, judge, statistic,
        workload_budgets, workload_evidence,
    };
    use crate::subject::Subject;

    #[test]
    fn the_cumulative_chunk_carries_every_workload_under_its_measuring_chunk() {
        let c24 = Subject::Chunk(CUMULATIVE);
        assert!(cumulative(c24));
        assert!(!cumulative(Subject::Chunk(22)) && !cumulative(Subject::Phase(7)));
        // Every registered workload's owner is listed, C18 included.
        for owner in [
            "C05", "C11", "C13", "C15", "C18", "C19", "C20", "C21", "C22", "C24",
        ] {
            assert!(WORKLOADS.iter().any(|w| w.measured_by == owner), "{owner}");
        }
        let dir = std::path::Path::new("/nonexistent");
        for w in WORKLOADS {
            let named = workload_evidence(dir, c24, w);
            if w.measured_by == "C24" {
                assert_eq!(named, "workload-perf.json");
            } else {
                assert_eq!(named, format!("workload-perf-{}.json", w.measured_by));
            }
            // Another chunk keeps its own name.
            assert_eq!(workload_evidence(dir, Subject::Chunk(22), w), w.evidence);
        }
    }

    #[test]
    fn the_phase_containing_the_cumulative_chunk_measures_every_budget_itself() {
        let members = |ids: &[&str]| ids.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(carries_every_budget(
            Subject::Phase(7),
            &members(&["C23", "C24"])
        ));
        assert!(carries_every_budget(
            Subject::Chunk(CUMULATIVE),
            &members(&["C24"])
        ));
        assert!(!carries_every_budget(
            Subject::Phase(8),
            &members(&["C25", "C26"])
        ));
        assert!(!carries_every_budget(
            Subject::Phase(6),
            &members(&["C19", "C22"])
        ));
        assert!(!carries_every_budget(
            Subject::Chunk(23),
            &members(&["C23"])
        ));
    }

    #[test]
    fn a_budget_fails_over_its_limit_short_of_samples_or_in_another_unit() {
        let budget = Budget {
            id: "b".into(),
            statistic: "p95".into(),
            limit: 10.0,
            unit: "ms".into(),
            samples: 3,
        };
        let metric = |unit: &str, samples: Vec<f64>| Metric {
            unit: unit.into(),
            samples,
        };
        assert_eq!(
            judge(&budget, Some(&metric("ms", vec![1.0, 2.0, 10.0]))),
            None
        );
        assert!(judge(&budget, Some(&metric("ms", vec![1.0, 2.0, 10.5]))).is_some());
        assert!(judge(&budget, Some(&metric("ms", vec![1.0, 2.0]))).is_some());
        assert!(judge(&budget, Some(&metric("s", vec![1.0, 2.0, 3.0]))).is_some());
        assert!(judge(&budget, None).is_some());
    }

    #[test]
    fn the_registered_workload_table_parses_exactly() {
        let table = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/planning/evidence/C04/workload-budget.md"),
        )
        .expect("table");
        let parsed: Vec<(String, String, f64, String, usize)> = workload_budgets(&table)
            .expect("budgets")
            .into_iter()
            .map(|b| (b.id, b.statistic, b.limit, b.unit, b.samples))
            .collect();
        assert_eq!(
            parsed,
            vec![
                (
                    "learner-call-overhead".into(),
                    "p95".into(),
                    75.0,
                    "ms".into(),
                    100
                ),
                (
                    "catalog-to-lesson".into(),
                    "p95".into(),
                    1500.0,
                    "ms".into(),
                    20
                ),
                (
                    "academy-process-tree-resident".into(),
                    "max".into(),
                    450.0,
                    "MiB".into(),
                    20
                ),
            ]
        );
        assert!(workload_budgets("| Budget | Statistic |\n|---|---|").is_err());
        assert!(
            workload_budgets("| x | p95 | 75 | 1 | m |").is_err(),
            "no unit"
        );
    }

    #[test]
    fn nearest_rank_statistics() {
        let samples: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(statistic("p95", &samples), Some(19.0));
        assert_eq!(statistic("median", &samples), Some(10.0));
        assert_eq!(statistic("max", &samples), Some(20.0));
        assert_eq!(statistic("p95", &[]), None);
        assert_eq!(statistic("p95", &[1.0, f64::NAN]), None);
        assert_eq!(statistic("mean", &samples), None);
    }
}
