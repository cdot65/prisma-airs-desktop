//! Freezes inputs, writes the immutable request packet, delivers it to Codex
//! and records a receipt derived from the reviewer-authored verdict.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use crate::complete::{self, Entry, read_ledger};
use crate::env::Environment;
use crate::fingerprint::{self, Fingerprints};
use crate::gate::{self, HashedPath, Packet, PredecessorRef, Receipt, RunRef, TransportRecord};
use crate::manifest::Manifest;
use crate::runner::{self, RunKind, Status};
use crate::transport::{self, Codex};
use crate::verdict::Verdict;
use crate::{
    Repo, Result, Subject, invalid, io_err, now_utc, random_hex, sha256_hex, write_json, write_new,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportChoice {
    /// Preferred thread when its handshake succeeds, else a fresh reviewer.
    Auto,
    Thread,
    Fresh,
}

/// The commit the current subject's diff is measured from.
fn base_commit(repo: &Repo) -> Result<String> {
    let entries = read_ledger(&repo.completion_ledger())?;
    let last = entries.iter().rev().find_map(|e| match e {
        Entry::Commit { commit, .. } => Some(commit.clone()),
        _ => None,
    });
    // Before any completion, the reviewed planning baseline.
    Ok(last.unwrap_or_else(|| "931916bf7469fba71aa15ee2a9cb6d2d194abf83".to_owned()))
}

/// Diff from base including untracked files, via a throwaway index so the
/// real index is never modified.
fn write_diff(repo: &Repo, base: &str, out: &Path) -> Result<HashedPath> {
    let index = std::env::temp_dir().join(format!("airs-v2-review-index-{}", random_hex(6)?));
    let git = |args: &[&str]| -> Result<Vec<u8>> {
        let output = Command::new("/usr/bin/git")
            .args(args)
            .current_dir(repo.root())
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", std::env::var("HOME").unwrap_or_default())
            .env("GIT_INDEX_FILE", &index)
            .output()
            .map_err(io_err(repo.root()))?;
        if !output.status.success() {
            return Err(invalid(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        Ok(output.stdout)
    };
    git(&["read-tree", base])?;
    git(&["add", "-A", "--", "."])?;
    let diff = git(&[
        "diff",
        "--cached",
        "--stat",
        "--patch",
        base,
        "--",
        ".",
        ":(exclude)docs/planning/reviews",
        ":(exclude)*.lock",
        ":(exclude)pnpm-lock.yaml",
        ":(exclude)docs/planning/evidence/*/runs",
    ])?;
    let _ = std::fs::remove_file(&index);
    write_new(out, &diff)?;
    Ok(HashedPath {
        path: rel(repo, out),
        sha256: sha256_hex(&diff),
    })
}

fn rel(repo: &Repo, path: &Path) -> String {
    path.strip_prefix(repo.root())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn hashed(repo: &Repo, path: &Path) -> Result<HashedPath> {
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    Ok(HashedPath {
        path: rel(repo, path),
        sha256: sha256_hex(&bytes),
    })
}

/// Latest passing run of each required kind executed against `fingerprints`.
fn select_runs(
    repo: &Repo,
    subject: Subject,
    fingerprints: &Fingerprints,
) -> Result<(Vec<RunRef>, Option<String>)> {
    let runs = runner::load_runs(repo, subject)?;
    let mut selected = Vec::new();
    let mut executable = None;
    for kind in complete::required_runs(subject) {
        let found = runs.iter().rev().find(|(_, r)| {
            r.kind == kind
                && r.status == Status::Passed
                && r.inputs_before.source == fingerprints.source
                && r.inputs_before.dependencies == fingerprints.dependencies
                && r.inputs_before.build_flags == fingerprints.build_flags
        });
        let Some((path, record)) = found else {
            return Err(invalid(format!(
                "no passing {kind:?} run against the current inputs; run it before requesting review"
            )));
        };
        if kind == RunKind::NativePrepare {
            executable = record
                .artifacts
                .iter()
                .find(|a| a.name == "fixture-executable")
                .map(|a| a.sha256.clone());
        }
        let bytes = std::fs::read(path).map_err(io_err(path))?;
        selected.push(RunRef {
            kind,
            run_id: record.run_id.clone(),
            path: rel(repo, path),
            sha256: sha256_hex(&bytes),
        });
    }
    Ok((selected, executable))
}

/// Hashes every evidence file for the subject outside `runs/` (run records
/// are bound individually through [`RunRef`]).
fn evidence_files(repo: &Repo, subject: Subject) -> Result<Vec<HashedPath>> {
    let dir = repo.evidence_dir(subject);
    let mut out = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if path != dir.join("runs") {
                    stack.push(path);
                }
            } else {
                out.push(hashed(repo, &path)?);
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn next_attempt(dir: &Path) -> Result<(String, PathBuf)> {
    std::fs::create_dir_all(dir).map_err(io_err(dir))?;
    let mut highest = 0u32;
    for entry in std::fs::read_dir(dir)
        .map_err(io_err(dir))?
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(n) = name
            .strip_prefix("attempt-")
            .and_then(|n| n.parse::<u32>().ok())
        {
            highest = highest.max(n);
        }
    }
    let id = format!("attempt-{:02}", highest + 1);
    Ok((id.clone(), dir.join(id)))
}

/// Expires every earlier attempt that never produced a receipt.
fn expire_pending(dir: &Path, reason: &str) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let attempt = entry.path();
        if attempt.join("request.json").is_file()
            && !attempt.join("receipt.json").is_file()
            && !attempt.join("expired.json").is_file()
        {
            write_json(
                &attempt.join("expired.json"),
                &serde_json::json!({ "expired_utc": now_utc(), "reason": reason }),
            )?;
        }
    }
    Ok(())
}

fn extract_block(markdown: &str, index: usize) -> Result<String> {
    markdown
        .split("```text\n")
        .nth(index + 1)
        .and_then(|rest| rest.split("\n```").next())
        .map(str::to_owned)
        .ok_or_else(|| invalid("review prompt is missing its text block"))
}

fn instructions(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
) -> Result<(String, String, Vec<String>)> {
    let chunk_prompt_path = repo.join("docs/planning/prompts/review-chunk.md");
    let chunk_prompt =
        std::fs::read_to_string(&chunk_prompt_path).map_err(io_err(&chunk_prompt_path))?;
    let message = extract_block(&chunk_prompt, 0)?;
    if subject.is_phase() {
        let path = repo.join("docs/planning/prompts/review-phase.md");
        let phase_prompt = std::fs::read_to_string(&path).map_err(io_err(&path))?;
        let chunks: Vec<String> = manifest
            .phase_chunks(subject)?
            .iter()
            .map(Subject::to_string)
            .collect();
        let text = extract_block(&phase_prompt, 0)?
            .replace("PHASE_ID", &subject.to_string())
            .replace("CHUNK_LIST", &chunks.join(", "));
        let files = chunks
            .iter()
            .map(|c| format!("docs/planning/chunks/{c}.md"))
            .collect();
        Ok((message, text, files))
    } else {
        let text = extract_block(&chunk_prompt, 1)?.replace("CHUNK_ID", &subject.to_string());
        Ok((
            message,
            text,
            vec![format!("docs/planning/chunks/{subject}.md")],
        ))
    }
}

fn predecessor_refs(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
) -> Result<Vec<PredecessorRef>> {
    if subject == Subject::Chunk(1) {
        return Ok(vec![PredecessorRef {
            subject: "G01".to_owned(),
            receipt: hashed(repo, &complete::g01_receipt(repo))?,
        }]);
    }
    let entries = read_ledger(&repo.completion_ledger())?;
    let mut refs = Vec::new();
    for required in complete::predecessors(manifest, subject)? {
        let receipt = entries.iter().rev().find_map(|e| match e {
            Entry::Completion {
                subject: s,
                receipt,
                ..
            } if *s == required.to_string() => Some(receipt.clone()),
            _ => None,
        });
        let receipt =
            receipt.ok_or_else(|| invalid(format!("{required} has no completion receipt")))?;
        refs.push(PredecessorRef {
            subject: required.to_string(),
            receipt: hashed(repo, &repo.join(receipt))?,
        });
    }
    Ok(refs)
}

pub struct Outcome {
    pub receipt: PathBuf,
    pub passed: bool,
    pub failures: Vec<String>,
}

pub fn review(
    repo: &Repo,
    manifest: &Manifest,
    env: &Environment,
    subject: Subject,
    choice: TransportChoice,
    limitations: Vec<String>,
    native_pending: Option<String>,
) -> Result<Outcome> {
    let mut limitations = limitations;
    complete::require_predecessors(repo, manifest, subject)?;
    fingerprint::verify_consumed_shared(repo)?;
    let reviewer = &manifest.reviewer;
    let codex = Codex::resolve(env, reviewer, repo.root())?;
    let fingerprints = fingerprint::all(repo, manifest, subject, &env.toolchain_hash())?;
    let (runs, executable) = select_runs(repo, subject, &fingerprints)?;
    let native_path = repo.evidence_dir(subject).join("native-acceptance.json");
    let native_acceptance = if native_path.is_file() {
        Some(hashed(repo, &native_path)?)
    } else if let Some(reason) = native_pending {
        // An explicitly requested early review: the missing record is stated
        // to the reviewer and the gate fails by construction.
        limitations.push(format!(
            "MANDATORY NATIVE ACCEPTANCE IS MISSING ({reason}); this attempt cannot pass the gate"
        ));
        None
    } else {
        return Err(invalid(
            "native acceptance record is missing; mandatory native evidence is blocked, not optional",
        ));
    };
    let reviews = repo.reviews_dir(subject);
    expire_pending(&reviews, "superseded by a new review attempt")?;

    let mut choice = choice;
    loop {
        let (attempt_id, attempt_dir) = next_attempt(&reviews)?;
        let request_id = format!(
            "rq-{subject}-{}-{}",
            now_utc().replace([':', '-'], ""),
            random_hex(4)?
        );
        let inventory = fingerprint::source_inventory(repo)?;
        let inventory_path = attempt_dir.join("source-inventory.json");
        write_new(
            &inventory_path,
            &serde_json::to_vec_pretty(&inventory).unwrap_or_default(),
        )?;
        let base = base_commit(repo)?;
        let diff = write_diff(repo, &base, &attempt_dir.join("diff.patch"))?;
        let (message_template, review_instructions, instruction_files) =
            instructions(repo, manifest, subject)?;
        let verdict_path = attempt_dir.join("verdict.json");
        let packet_path = attempt_dir.join("request.json");
        let packet = Packet {
            schema: gate::PACKET_SCHEMA.to_owned(),
            request_id: request_id.clone(),
            attempt_id: attempt_id.clone(),
            subject: subject.to_string(),
            kind: if subject.is_phase() { "phase" } else { "chunk" }.to_owned(),
            created_utc: now_utc(),
            repository: repo.root().display().to_string(),
            base_commit: base,
            scope: if subject.is_phase() {
                format!("cumulative phase review of {subject} and the earlier features it affects")
            } else {
                format!("implementation chunk {subject}")
            },
            instructions: instruction_files,
            predecessors: predecessor_refs(repo, manifest, subject)?,
            fingerprints: fingerprints.clone(),
            source_inventory: hashed(repo, &inventory_path)?,
            runs: runs.clone(),
            evidence_files: evidence_files(repo, subject)?,
            native_acceptance: native_acceptance.clone(),
            native_executable_sha256: executable.clone(),
            diff,
            limitations: limitations.clone(),
            verdict_schema: hashed(repo, &repo.verdict_schema())?,
            verdict_path: verdict_path.display().to_string(),
            review_instructions,
        };
        let packet_bytes =
            serde_json::to_vec_pretty(&packet).map_err(|e| invalid(e.to_string()))?;
        write_new(&packet_path, &packet_bytes)?;
        let message = message_template
            .replace("REQUEST_PACKET_PATH", &packet_path.display().to_string())
            .replace("VERDICT_PATH", &verdict_path.display().to_string());

        let use_thread = match choice {
            TransportChoice::Fresh => false,
            TransportChoice::Thread => true,
            TransportChoice::Auto => {
                match transport::handshake(
                    env,
                    reviewer,
                    repo.root(),
                    &repo.transport_dir(),
                    Duration::from_secs(300),
                ) {
                    Ok(route) => {
                        eprintln!("handshake confirmed: {}", route.display());
                        true
                    }
                    Err(error) => {
                        eprintln!("handshake failed ({error}); using a fresh Codex reviewer");
                        false
                    }
                }
            }
        };

        let submitted_utc = now_utc();
        let (kind, thread, raw_output, model, exit_code, timed_out) = if use_thread {
            let ack = transport::queue(env, reviewer, repo.root(), &message)?;
            write_json(&attempt_dir.join("queue-ack.json"), &ack)?;
            eprintln!(
                "submitted to thread {} ({}); waiting for verdict",
                reviewer.preferred_transport.thread, ack.stdout
            );
            if transport::wait_for_file(&verdict_path, reviewer, "thread verdict").is_none() {
                write_json(
                    &attempt_dir.join("expired.json"),
                    &serde_json::json!({ "expired_utc": now_utc(), "reason": "no verdict from the preferred thread within the bounded wait; switching to a fresh reviewer" }),
                )?;
                choice = TransportChoice::Fresh;
                continue;
            }
            let thread_id = reviewer.preferred_transport.thread.clone();
            let reply = transport::wait_for_thread_reply(
                &thread_id,
                &request_id,
                &submitted_utc,
                Duration::from_secs(600),
            );
            let reply_path = attempt_dir.join("thread-reply.jsonl");
            write_new(&reply_path, &reply)?;
            (
                "codex-thread",
                Some(thread_id),
                Some(hashed(repo, &reply_path)?),
                None,
                ack.exit_code,
                false,
            )
        } else {
            let prompt = format!(
                "You are a fresh, independent Codex reviewer with no prior context for this project. The repository is {}. {}\n\nThe request packet lists the scope, predecessor receipts, fingerprints, run evidence, diff and instructions; read it and the files it names. Your final message must be only the verdict JSON matching the provided output schema.",
                repo.root().display(),
                message
            );
            let result = transport::exec_fresh(
                env,
                reviewer,
                repo.root(),
                &repo.verdict_schema(),
                &verdict_path,
                &attempt_dir,
                &prompt,
            )?;
            let raw = hashed(repo, &result.stdout_path)?;
            (
                "codex-exec-fresh",
                None,
                Some(raw),
                result.model,
                result.exit_code,
                result.timed_out,
            )
        };

        let verdict_bytes = std::fs::read(&verdict_path).map_err(io_err(&verdict_path))?;
        let fingerprints_after = fingerprint::all(repo, manifest, subject, &env.toolchain_hash())?;
        let (scores, counts) = match Verdict::parse(&verdict_bytes) {
            Ok(v) => (
                v.scores.clone(),
                (
                    v.required_fixes.len(),
                    v.blockers.len(),
                    v.evidence_gaps.len(),
                ),
            ),
            Err(e) => return Err(invalid(format!("attempt {attempt_id}: {e}"))),
        };
        let mut receipt = Receipt {
            schema: gate::RECEIPT_SCHEMA.to_owned(),
            receipt_id: format!("{subject}-{attempt_id}-{request_id}"),
            subject: subject.to_string(),
            request_id,
            attempt_id,
            transport: TransportRecord {
                kind: kind.to_owned(),
                thread,
                codex_path: codex.path.clone(),
                codex_version: codex.version.clone(),
                codex_sha256: codex.sha256.clone(),
                submitted_utc,
                completed_utc: now_utc(),
                exit_code,
                timed_out,
                raw_output,
                model,
            },
            packet_sha256: sha256_hex(&packet_bytes),
            verdict_sha256: sha256_hex(&verdict_bytes),
            fingerprints: fingerprints.clone(),
            fingerprints_after,
            scores,
            required_fixes: counts.0,
            blockers: counts.1,
            evidence_gaps: counts.2,
            passed: false,
            failures: Vec::new(),
            created_utc: now_utc(),
        };
        let receipt_path = attempt_dir.join("receipt.json");
        // Evaluate through the same gate `complete` uses (bootstrap evidence
        // for C01 is produced after the receipt and checked at completion).
        let provisional = attempt_dir.join("provisional").join("receipt.json");
        std::fs::create_dir_all(attempt_dir.join("provisional")).map_err(io_err(&attempt_dir))?;
        std::fs::write(
            &provisional,
            serde_json::to_vec_pretty(&receipt).map_err(|e| invalid(e.to_string()))?,
        )
        .map_err(io_err(&provisional))?;
        std::fs::copy(&packet_path, attempt_dir.join("provisional/request.json"))
            .map_err(io_err(&packet_path))?;
        let (plans, native) = complete::expectations(repo, manifest, subject)?;
        let current = fingerprint::all(repo, manifest, subject, &env.toolchain_hash())?;
        let failures = gate::evaluate(&gate::load(
            repo.root(),
            &provisional,
            current,
            plans,
            native,
        )?);
        let _ = std::fs::remove_dir_all(attempt_dir.join("provisional"));
        receipt.passed = failures.is_empty();
        receipt.failures = failures.clone();
        write_new(
            &receipt_path,
            &serde_json::to_vec_pretty(&receipt).map_err(|e| invalid(e.to_string()))?,
        )?;
        return Ok(Outcome {
            receipt: receipt_path,
            passed: failures.is_empty(),
            failures,
        });
    }
}
