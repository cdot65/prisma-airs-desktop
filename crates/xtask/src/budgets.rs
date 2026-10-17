//! Pre-registered performance budgets (T20): the unsigned production
//! footprint build and the evidence check the `native` gate applies to the
//! chunk that registered the budgets (and phases containing it).
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

/// Builds the unsigned production app and DMG and records their sizes.
pub fn footprint(
    repo: &Repo,
    manifest: &Manifest,
    env: &Environment,
    subject: Subject,
) -> Result<Footprint> {
    let (budgets, record, sha) = registration(repo, &subject.to_string())?;
    if budgets.registered_by_chunk != subject.to_string() {
        return Err(invalid(format!(
            "budgets are registered by {}",
            budgets.registered_by_chunk
        )));
    }
    let started_utc = now_utc();
    if started_utc < record.registered_utc {
        return Err(invalid("the clock is before the budget registration"));
    }
    let target = manifest.target.as_str();
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

/// Whether this subject must carry budget evidence.
pub fn required(repo: &Repo, manifest: &Manifest, subject: Subject) -> Result<Option<String>> {
    let path = repo.join(BUDGETS);
    if !path.exists() {
        return Ok(None);
    }
    let budgets: Budgets = read_json(&path)?;
    Ok(manifest
        .native_members(subject)?
        .contains(&budgets.registered_by_chunk)
        .then_some(budgets.registered_by_chunk))
}

/// Reasons the subject's budget evidence does not pass (empty when it does).
pub fn status(repo: &Repo, manifest: &Manifest, subject: Subject) -> Vec<String> {
    match required(repo, manifest, subject) {
        Ok(None) => Vec::new(),
        Ok(Some(chunk)) => evaluate(repo, subject, &chunk).unwrap_or_else(|e| vec![e.to_string()]),
        Err(e) => vec![e.to_string()],
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
        let Some(metric) = metrics.get(&budget.id) else {
            failures.push(format!("{}: no measurement", budget.id));
            continue;
        };
        if metric.unit != budget.unit {
            failures.push(format!(
                "{}: unit {} is not {}",
                budget.id, metric.unit, budget.unit
            ));
            continue;
        }
        if metric.samples.len() < budget.samples {
            failures.push(format!(
                "{}: {} samples, {} required",
                budget.id,
                metric.samples.len(),
                budget.samples
            ));
            continue;
        }
        match statistic(&budget.statistic, &metric.samples) {
            Some(value) if value <= budget.limit => {
                println!(
                    "budget {} {} {:.1} {} <= {}",
                    budget.id, budget.statistic, value, budget.unit, budget.limit
                );
            }
            Some(value) => failures.push(format!(
                "{}: {} {:.1} {} exceeds {}",
                budget.id, budget.statistic, value, budget.unit, budget.limit
            )),
            None => failures.push(format!(
                "{}: undefined statistic {}",
                budget.id, budget.statistic
            )),
        }
    }
    Ok(failures)
}

#[cfg(test)]
mod tests {
    use super::statistic;

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
