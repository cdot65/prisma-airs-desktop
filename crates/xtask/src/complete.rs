//! The append-only completion ledger, generations and predecessor rules.
//!
//! `complete` is a read-only validation of the current inputs against a
//! review receipt followed by one appended line. Lines are never edited.
//! Each subject's completions carry a generation; `reopen` appends an
//! invalidation for the subject's current generation and for every later
//! completed subject, after which a new review completes a new generation.
//! A subject counts as complete only when its latest generation has a
//! completion, a commit bound to the same receipt, no invalidation, and a
//! receipt file whose bytes still match the ledger.
//!
//! Owner acceptance (D032): the owner may accept a review below the 9/10
//! threshold. `complete --owner-accept REASON` then accepts the newest
//! receipt only if every score is at least [`OWNER_MINIMUM_SCORE`], it has no
//! blockers, and nothing but the threshold (scores, required fixes, evidence
//! gaps, the recommendation) fails: the packet, verdict, transport,
//! fingerprints, runs and native evidence must all still check. The completion records the reason and every accepted
//! failure; verdicts are never edited.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::evidence;
use crate::fingerprint::{self, Fingerprints};
use crate::gate::{self, NativeBase, Receipt};
use crate::manifest::{Command as PlannedCommand, Manifest};
use crate::native::{self, Prepared};
use crate::runner::RunKind;
use crate::{Repo, Result, Subject, invalid, io_err, now_utc, read_json, sha256_hex};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Entry {
    Completion {
        subject: String,
        generation: u32,
        receipt_id: String,
        receipt: String,
        receipt_sha256: String,
        fingerprints: Fingerprints,
        completed_utc: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        owner_acceptance: Option<OwnerAcceptance>,
    },
    Commit {
        subject: String,
        generation: u32,
        receipt_id: String,
        commit: String,
        tree: String,
        source: String,
        verified_utc: String,
    },
    Invalidation {
        subject: String,
        generation: u32,
        reason: String,
        invalidated_utc: String,
    },
}

/// The lowest score an owner may accept (D032).
pub const OWNER_MINIMUM_SCORE: u8 = 8;

/// The owner's acceptance of a review below the threshold, as recorded.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerAcceptance {
    pub decision: String,
    pub reason: String,
    pub minimum_score: u8,
    pub accepted_failures: Vec<String>,
}

/// The gate failures an owner may accept for `receipt` (the threshold
/// alone), or why it cannot be accepted.
pub fn owner_acceptable(
    receipt: &Receipt,
    failures: &[String],
) -> std::result::Result<Vec<String>, String> {
    let named = receipt.scores.named();
    if let Some((name, score)) = named.iter().find(|(_, s)| *s < OWNER_MINIMUM_SCORE) {
        return Err(format!(
            "score {name}={score} is below the owner minimum {OWNER_MINIMUM_SCORE}"
        ));
    }
    if receipt.blockers > 0 {
        return Err("blockers remain".to_owned());
    }
    let mut threshold: Vec<String> = named
        .iter()
        .filter(|(_, s)| *s < gate::MINIMUM_SCORE)
        .map(|(name, s)| format!("score {name}={s} is below {}", gate::MINIMUM_SCORE))
        .collect();
    if receipt.required_fixes > 0 {
        threshold.push(format!("{} required fixes remain", receipt.required_fixes));
    }
    if receipt.evidence_gaps > 0 {
        threshold.push(format!("{} evidence gaps remain", receipt.evidence_gaps));
    }
    threshold.push("reviewer recommendation is fail".to_owned());
    let other: Vec<&String> = failures.iter().filter(|f| !threshold.contains(f)).collect();
    if !other.is_empty() {
        return Err(format!(
            "failures other than the threshold remain: {}",
            other
                .iter()
                .map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    Ok(failures.to_vec())
}

pub fn read_ledger(path: &Path) -> Result<Vec<Entry>> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            serde_json::from_str(l)
                .map_err(|e| invalid(format!("corrupt completion ledger line: {e}")))
        })
        .collect()
}

fn append(path: &Path, entry: &Entry) -> Result<()> {
    let line = serde_json::to_string(entry).map_err(|e| invalid(e.to_string()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(io_err(path))?;
    writeln!(file, "{line}").map_err(io_err(path))
}

/// The latest completion generation of a subject and its entry.
fn latest_completion(entries: &[Entry], subject: Subject) -> Option<&Entry> {
    let id = subject.to_string();
    entries
        .iter()
        .filter(|e| matches!(e, Entry::Completion { subject, .. } if *subject == id))
        .max_by_key(|e| match e {
            Entry::Completion { generation, .. } => *generation,
            _ => 0,
        })
}

fn highest_generation(entries: &[Entry], subject: Subject) -> u32 {
    let id = subject.to_string();
    entries
        .iter()
        .filter_map(|e| match e {
            Entry::Completion {
                subject,
                generation,
                ..
            } if *subject == id => Some(*generation),
            _ => None,
        })
        .max()
        .unwrap_or(0)
}

/// Why `subject` is not currently complete (empty when it is).
pub fn completion_problems(repo: &Repo, entries: &[Entry], subject: Subject) -> Vec<String> {
    let id = subject.to_string();
    let Some(Entry::Completion {
        generation,
        receipt_id,
        receipt,
        receipt_sha256,
        owner_acceptance,
        ..
    }) = latest_completion(entries, subject)
    else {
        return vec![format!("{subject} has no completion")];
    };
    let mut problems = Vec::new();
    if entries.iter().any(|e| matches!(e, Entry::Invalidation { subject, generation: g, .. } if *subject == id && g == generation)) {
        problems.push(format!("{subject} generation {generation} was invalidated"));
    }
    let committed = entries.iter().any(|e| {
        matches!(e, Entry::Commit { subject, generation: g, receipt_id: r, .. } if *subject == id && g == generation && r == receipt_id)
    });
    if !committed {
        problems.push(format!(
            "{subject} generation {generation} has no verified commit"
        ));
    }
    match std::fs::read(repo.join(receipt)) {
        Err(_) => problems.push(format!("{subject} receipt {receipt} is missing")),
        Ok(bytes) => {
            if sha256_hex(&bytes) != *receipt_sha256 {
                problems.push(format!(
                    "{subject} receipt {receipt} was changed after completion"
                ));
            } else if let Ok(parsed) = serde_json::from_slice::<Receipt>(&bytes) {
                let minimum = if owner_acceptance.is_some() {
                    OWNER_MINIMUM_SCORE
                } else {
                    gate::MINIMUM_SCORE
                };
                let scores_ok = parsed.scores.named().iter().all(|(_, s)| *s >= minimum);
                let accepted = match owner_acceptance {
                    // Owner-accepted (D032): the threshold alone may fail.
                    Some(_) => scores_ok && parsed.blockers == 0,
                    None => {
                        parsed.passed
                            && scores_ok
                            && parsed.required_fixes + parsed.blockers + parsed.evidence_gaps == 0
                    }
                };
                if !accepted {
                    problems.push(format!(
                        "{subject} receipt does not record a passing review"
                    ));
                }
            } else {
                problems.push(format!("{subject} receipt is malformed"));
            }
        }
    }
    problems
}

pub fn is_completed(repo: &Repo, entries: &[Entry], subject: Subject) -> bool {
    completion_problems(repo, entries, subject).is_empty()
}

/// Subjects that must be complete (reviewed and committed) before `subject`.
pub fn predecessors(manifest: &Manifest, subject: Subject) -> Result<Vec<Subject>> {
    match subject {
        Subject::Chunk(1) => Ok(Vec::new()),
        Subject::Chunk(n) => {
            let mut required = vec![Subject::Chunk(n - 1)];
            let phase = manifest.phase_of(subject)?;
            let previous_phase = manifest.phase_of(Subject::Chunk(n - 1))?;
            if phase != previous_phase {
                required.push(previous_phase);
            }
            Ok(required)
        }
        Subject::Phase(_) => manifest.phase_chunks(subject),
    }
}

/// The historical planning gate C01 depends on (reviewed by Claude, D019).
pub fn g01_receipt(repo: &Repo) -> PathBuf {
    repo.join("docs/planning/reviews/G01/attempt-05/receipt.json")
}

pub fn check_g01(repo: &Repo) -> Result<()> {
    let receipt: serde_json::Value = read_json(&g01_receipt(repo))?;
    let passed = receipt.get("passed").and_then(|v| v.as_bool()) == Some(true);
    let scores_ok = receipt
        .get("scores")
        .and_then(|s| s.as_object())
        .is_some_and(|s| s.len() == 5 && s.values().all(|v| v.as_u64().is_some_and(|n| n >= 9)));
    if passed && scores_ok {
        Ok(())
    } else {
        Err(invalid(
            "historical G01 receipt does not record a passing design gate",
        ))
    }
}

pub fn require_predecessors(repo: &Repo, manifest: &Manifest, subject: Subject) -> Result<()> {
    if subject == Subject::Chunk(1) {
        return check_g01(repo);
    }
    let entries = read_ledger(&repo.completion_ledger())?;
    for required in predecessors(manifest, subject)? {
        let problems = completion_problems(repo, &entries, required);
        if !problems.is_empty() {
            return Err(invalid(format!(
                "{subject} requires {required}: {}",
                problems.join("; ")
            )));
        }
    }
    Ok(())
}

/// Every chunk and every phase needs check, test and native-prepare runs;
/// a phase's native evidence is for its integrated tree.
pub fn required_runs(_subject: Subject) -> Vec<RunKind> {
    vec![RunKind::Check, RunKind::Test, RunKind::NativePrepare]
}

/// Approved command plans, one per required run kind.
pub type Plans = Vec<(RunKind, Vec<PlannedCommand>)>;

/// The approved command plans and native requirements for a subject.
pub fn expectations(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
) -> Result<(Plans, Option<NativeBase>)> {
    let plans = required_runs(subject)
        .into_iter()
        .map(|kind| Ok((kind, manifest.plan(subject, kind)?)))
        .collect::<Result<Vec<_>>>()?;
    let members = manifest.native_members(subject)?;
    let (required_checks, fixture_identifier) =
        evidence::required_checks(&repo.join("docs/architecture/native-checks.json"), &members)?;
    let prepared =
        read_json::<Prepared>(&repo.evidence_dir(subject).join("native-prepare.json")).ok();
    let current = prepared.as_ref().map(|p| native::current_state(repo, p));
    Ok((
        plans,
        Some(NativeBase {
            required_checks,
            fixture_identifier,
            prepared_bundle: prepared.as_ref().map(|p| p.bundle.clone()),
            prepared_bundle_sha256: prepared.as_ref().map(|p| p.bundle_sha256.clone()),
            prepared_identifier: prepared.as_ref().map(|p| p.identifier.clone()),
            current_executable_sha256: current.as_ref().and_then(|c| c.executable_sha256.clone()),
            current_bundle_sha256: current.as_ref().and_then(|c| c.bundle_sha256.clone()),
            current_identifier: current.and_then(|c| c.identifier),
        }),
    ))
}

/// Evaluates one receipt exactly as `complete` does.
pub fn evaluate_receipt(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
    receipt: &Path,
    current: Fingerprints,
) -> Result<Vec<String>> {
    let (plans, native) = expectations(repo, manifest, subject)?;
    let inputs = gate::load(repo.root(), receipt, current, plans, native)?;
    let mut failures = gate::evaluate(&inputs);
    if subject == Subject::Chunk(1) {
        failures.extend(bootstrap_failures(receipt, &inputs.receipt_bytes));
    }
    Ok(failures)
}

/// Bootstrap evidence for C01: the independent reference validator and the
/// controller agree on this exact receipt and on every adversarial mutation.
/// It lives in the attempt directory, outside the hash-bound evidence.
#[derive(Clone, Debug, Deserialize)]
struct Bootstrap {
    receipt_sha256: String,
    reference_accepted: bool,
    controller_accepted: bool,
    adversarial_cases: usize,
    adversarial_agreement: bool,
}

fn bootstrap_failures(receipt: &Path, receipt_bytes: &[u8]) -> Vec<String> {
    let path = gate::attempt_dir_of(receipt).join("bootstrap/result.json");
    let Ok(record) = read_json::<Bootstrap>(&path) else {
        return vec![
            "C01 bootstrap: independent reference validation result is missing".to_owned(),
        ];
    };
    let mut failures = Vec::new();
    if record.receipt_sha256 != sha256_hex(receipt_bytes) {
        failures.push("C01 bootstrap result is for a different receipt".to_owned());
    }
    if !record.reference_accepted || !record.controller_accepted {
        failures.push("C01 bootstrap: the validators did not both accept the receipt".to_owned());
    }
    if !record.adversarial_agreement || record.adversarial_cases == 0 {
        failures
            .push("C01 bootstrap: validators did not agree on every adversarial case".to_owned());
    }
    failures
}

/// Validates the newest passing receipt and appends a completion line.
pub fn complete(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
    current: Fingerprints,
) -> Result<Entry> {
    complete_with(repo, manifest, subject, current, None)
}

/// [`complete`], or with `owner_accept` the newest receipt the owner may
/// accept below the threshold (D032).
pub fn complete_with(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
    current: Fingerprints,
    owner_accept: Option<&str>,
) -> Result<Entry> {
    if owner_accept.is_some_and(|r| r.trim().is_empty()) {
        return Err(invalid("--owner-accept needs the owner's reason"));
    }
    require_predecessors(repo, manifest, subject)?;
    let ledger = repo.completion_ledger();
    let entries = read_ledger(&ledger)?;
    if is_completed(repo, &entries, subject) {
        return Err(invalid(format!(
            "{subject} is already complete; reopen it before completing a new generation"
        )));
    }
    let mut attempts: Vec<PathBuf> = std::fs::read_dir(repo.reviews_dir(subject))
        .map_err(io_err(&repo.reviews_dir(subject)))?
        .filter_map(|e| e.ok())
        .map(|e| e.path().join("receipt.json"))
        .filter(|p| p.is_file())
        .collect();
    attempts.sort();
    let mut last_failures = vec!["no review receipt exists".to_owned()];
    for receipt_path in attempts.iter().rev() {
        let failures = evaluate_receipt(repo, manifest, subject, receipt_path, current.clone())?;
        let bytes = std::fs::read(receipt_path).map_err(io_err(receipt_path))?;
        let receipt: Receipt =
            serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
        let owner_acceptance = match (owner_accept, failures.is_empty()) {
            (_, true) => None,
            (Some(reason), false) => match owner_acceptable(&receipt, &failures) {
                Ok(accepted) => Some(OwnerAcceptance {
                    decision: "D032".to_owned(),
                    reason: reason.trim().to_owned(),
                    minimum_score: OWNER_MINIMUM_SCORE,
                    accepted_failures: accepted,
                }),
                Err(why) => {
                    last_failures = vec![format!("owner acceptance refused: {why}")];
                    // Only the newest receipt may be owner-accepted.
                    break;
                }
            },
            (None, false) => {
                last_failures = failures;
                continue;
            }
        };
        if entries.iter().any(|e| matches!(e, Entry::Completion { receipt_id, .. } if *receipt_id == receipt.receipt_id)) {
            return Err(invalid("duplicate receipt ID; a receipt can complete only once"));
        }
        let entry = Entry::Completion {
            subject: subject.to_string(),
            generation: highest_generation(&entries, subject) + 1,
            receipt_id: receipt.receipt_id,
            receipt: receipt_path
                .strip_prefix(repo.root())
                .unwrap_or(receipt_path)
                .display()
                .to_string(),
            receipt_sha256: sha256_hex(&bytes),
            fingerprints: current,
            completed_utc: now_utc(),
            owner_acceptance,
        };
        append(&ledger, &entry)?;
        return Ok(entry);
    }
    Err(invalid(format!(
        "{subject} cannot complete: {}",
        last_failures.join("; ")
    )))
}

/// Invalidates the subject's current generation and every later completed
/// subject, so they must be re-reviewed and completed again.
pub fn reopen(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
    reason: &str,
) -> Result<Vec<Entry>> {
    let ledger = repo.completion_ledger();
    let entries = read_ledger(&ledger)?;
    let mut affected = vec![subject];
    let order: Vec<Subject> = (1..=26u8)
        .flat_map(|n| {
            let chunk = Subject::Chunk(n);
            let mut out = vec![chunk];
            if let Ok(phase) = manifest.phase_of(chunk)
                && manifest
                    .phase_chunks(phase)
                    .ok()
                    .and_then(|c| c.last().copied())
                    == Some(chunk)
            {
                out.push(phase);
            }
            out
        })
        .collect();
    let position = order
        .iter()
        .position(|s| *s == subject)
        .ok_or_else(|| invalid("unknown subject"))?;
    affected.extend(order[position + 1..].iter().copied());
    let mut written = Vec::new();
    for target in affected {
        if latest_completion(&entries, target).is_none() {
            continue;
        }
        let generation = highest_generation(&entries, target);
        let entry = Entry::Invalidation {
            subject: target.to_string(),
            generation,
            reason: if target == subject {
                reason.to_owned()
            } else {
                format!("depends on reopened {subject}: {reason}")
            },
            invalidated_utc: now_utc(),
        };
        append(&ledger, &entry)?;
        written.push(entry);
    }
    if written.is_empty() {
        return Err(invalid(format!("{subject} has no completion to reopen")));
    }
    Ok(written)
}

fn git(repo: &Repo, args: &[&str]) -> Result<String> {
    let output = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(repo.root())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .output()
        .map_err(io_err(repo.root()))?;
    if !output.status.success() {
        return Err(invalid(format!("git {} failed", args.join(" "))));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Verifies HEAD contains exactly the reviewed source and records the commit
/// against the subject's latest completion generation.
pub fn verify_commit(repo: &Repo, subject: Subject) -> Result<Entry> {
    let ledger = repo.completion_ledger();
    let entries = read_ledger(&ledger)?;
    let Some(Entry::Completion {
        generation,
        receipt_id,
        fingerprints,
        ..
    }) = latest_completion(&entries, subject)
    else {
        return Err(invalid(format!(
            "{subject} has no completion to bind a commit to"
        )));
    };
    let id = subject.to_string();
    if entries.iter().any(|e| matches!(e, Entry::Invalidation { subject, generation: g, .. } if *subject == id && g == generation)) {
        return Err(invalid(format!("{subject} generation {generation} is invalidated")));
    }
    let status = git(repo, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    let dirty: Vec<&str> = status
        .lines()
        .map(|l| l.get(3..).unwrap_or(l))
        .filter(|p| !fingerprint::is_excluded(p))
        .collect();
    if !dirty.is_empty() {
        return Err(invalid(format!(
            "working tree has uncommitted source changes: {}",
            dirty.join(", ")
        )));
    }
    let source = fingerprint::source(repo)?;
    if source != fingerprints.source {
        return Err(invalid(
            "committed source differs from the reviewed source fingerprint",
        ));
    }
    let entry = Entry::Commit {
        subject: id,
        generation: *generation,
        receipt_id: receipt_id.clone(),
        commit: git(repo, &["rev-parse", "HEAD"])?,
        tree: git(repo, &["rev-parse", "HEAD^{tree}"])?,
        source,
        verified_utc: now_utc(),
    };
    append(&ledger, &entry)?;
    Ok(entry)
}
