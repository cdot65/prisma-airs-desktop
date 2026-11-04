//! Executes manifest commands and records source-bound run evidence.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::env::{Environment, ToolIdentity};
use crate::fingerprint;
use crate::manifest::{Command, Manifest};
use crate::{Repo, Result, Subject, invalid, io_err, now_utc, random_hex, sha256_hex, write_json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunKind {
    Check,
    Test,
    NativePrepare,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Passed,
    Failed,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestCounts {
    pub passed: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandResult {
    pub label: String,
    pub argv: Vec<String>,
    pub cwd: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u128,
    pub output_sha256: String,
    pub log: String,
    pub tests: Option<TestCounts>,
    pub status: Status,
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputState {
    pub source: String,
    pub dependencies: String,
    pub build_flags: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub schema: String,
    pub run_id: String,
    pub subject: String,
    pub kind: RunKind,
    pub started_utc: String,
    pub finished_utc: String,
    pub toolchain: Vec<ToolIdentity>,
    pub env: std::collections::BTreeMap<String, String>,
    pub inputs_before: InputState,
    pub inputs_after: InputState,
    pub commands: Vec<CommandResult>,
    pub status: Status,
    pub failure: Option<String>,
    /// Artifacts produced by the run (path relative to repo and SHA-256).
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub name: String,
    pub path: String,
    pub sha256: String,
}

pub const RUN_SCHEMA: &str = "airs-v2/run/1";

pub struct Runner<'a> {
    pub repo: &'a Repo,
    pub manifest: &'a Manifest,
    pub env: &'a Environment,
    pub subject: Subject,
}

impl Runner<'_> {
    fn inputs(&self) -> Result<InputState> {
        Ok(InputState {
            source: fingerprint::source(self.repo)?,
            dependencies: fingerprint::dependencies(self.repo, &self.env.toolchain_hash())?,
            build_flags: fingerprint::build_flags(self.repo, self.manifest, self.subject)?,
        })
    }

    /// Runs commands in order, stopping at the first failure. Every command
    /// after a failure is recorded as blocked rather than silently skipped.
    /// Leading `setup/*` commands materialize locked inputs (scratch staging,
    /// frozen install); the run's input fingerprint is taken after them and
    /// must not change while the remaining commands run.
    pub fn run(
        &self,
        kind: RunKind,
        commands: &[Command],
        extra: &dyn Fn(&mut RunRecord) -> Result<()>,
    ) -> Result<RunRecord> {
        let run_id = format!(
            "{}-{}-{}",
            kind_name(kind),
            now_utc().replace([':', '-'], ""),
            random_hex(3)?
        );
        let dir = self
            .repo
            .evidence_dir(self.subject)
            .join("runs")
            .join(&run_id);
        let started_utc = now_utc();
        let mut inputs_before = None;
        let mut results = Vec::new();
        let mut failure = None;
        for command in commands {
            if inputs_before.is_none() && !command.label.starts_with("setup/") {
                inputs_before = Some(self.inputs()?);
            }
            if failure.is_some() {
                results.push(blocked(command, "an earlier command failed"));
                continue;
            }
            let result = self.execute(command, &dir)?;
            if result.status != Status::Passed {
                failure = Some(format!(
                    "{} {}",
                    result.label,
                    result.note.clone().unwrap_or_default()
                ));
            }
            results.push(result);
        }
        let inputs_after = self.inputs()?;
        let inputs_before = match inputs_before {
            Some(state) => state,
            None => inputs_after.clone(),
        };
        if failure.is_none() && inputs_before != inputs_after {
            failure = Some("inputs changed while commands ran".to_owned());
        }
        let mut record = RunRecord {
            schema: RUN_SCHEMA.to_owned(),
            run_id,
            subject: self.subject.to_string(),
            kind,
            started_utc,
            finished_utc: now_utc(),
            toolchain: self.env.tools.clone(),
            env: self.env.recorded_vars(),
            inputs_before,
            inputs_after,
            commands: results,
            status: Status::Passed,
            failure,
            artifacts: Vec::new(),
        };
        if record.failure.is_none()
            && let Err(error) = extra(&mut record)
        {
            record.failure = Some(error.to_string());
        }
        record.status = if record.failure.is_some() {
            Status::Failed
        } else {
            Status::Passed
        };
        write_json(&dir.join("run.json"), &record)?;
        Ok(record)
    }

    fn execute(&self, command: &Command, dir: &Path) -> Result<CommandResult> {
        let argv = substitute(&command.argv, self.subject)?;
        let log_rel = format!("logs/{}.log", command.label.replace('/', "-"));
        let log_path = dir.join(&log_rel);
        let started = Instant::now();
        let mut process = match self.env.command(&argv, self.repo.root()) {
            Ok(process) => process,
            Err(error) => {
                let mut result = blocked(command, &error.to_string());
                result.argv = argv;
                return Ok(result);
            }
        };
        let output = process
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .and_then(|child| child.wait_with_output())
            .map_err(io_err(self.repo.root()))?;
        let mut log = output.stdout.clone();
        log.extend_from_slice(b"\n--- stderr ---\n");
        log.extend_from_slice(&output.stderr);
        crate::write_atomic(&log_path, &log)?;
        let text = String::from_utf8_lossy(&log);
        let tests = if command.expects_tests {
            parse_tests(&text)
        } else {
            None
        };
        let mut note = None;
        let mut status = if output.status.success() {
            Status::Passed
        } else {
            Status::Failed
        };
        if status == Status::Passed && command.expects_tests {
            match &tests {
                Some(t) if t.passed > 0 && t.failed == 0 => {}
                Some(t) if t.failed > 0 => {
                    status = Status::Failed;
                    note = Some(format!("{} tests failed", t.failed));
                }
                _ => {
                    status = Status::Failed;
                    note = Some("no tests were executed (absent or zero-test target)".to_owned());
                }
            }
        }
        // A filtered workspace command that selected nothing ran nothing.
        if status == Status::Passed
            && argv.iter().any(|a| a == "--filter")
            && text.contains("No projects matched")
        {
            status = Status::Failed;
            note = Some("the package filter selected no projects".to_owned());
        }
        if status == Status::Failed && note.is_none() {
            note = Some(format!("exit status {:?}", output.status.code()));
        }
        Ok(CommandResult {
            label: command.label.clone(),
            argv,
            cwd: ".".to_owned(),
            exit_code: output.status.code(),
            duration_ms: started.elapsed().as_millis(),
            output_sha256: sha256_hex(&log),
            log: log_rel,
            tests,
            status,
            note,
        })
    }
}

fn blocked(command: &Command, reason: &str) -> CommandResult {
    CommandResult {
        label: command.label.clone(),
        argv: command.argv.clone(),
        cwd: ".".to_owned(),
        exit_code: None,
        duration_ms: 0,
        output_sha256: String::new(),
        log: String::new(),
        tests: None,
        status: Status::Blocked,
        note: Some(reason.to_owned()),
    }
}

fn kind_name(kind: RunKind) -> &'static str {
    match kind {
        RunKind::Check => "check",
        RunKind::Test => "test",
        RunKind::NativePrepare => "native-prepare",
    }
}

/// Replaces `{chunk}` with the validated subject; nothing else is substituted.
pub fn substitute(argv: &[String], subject: Subject) -> Result<Vec<String>> {
    argv.iter()
        .map(|arg| match arg.as_str() {
            "{chunk}" | "{phase}" => Ok(subject.to_string()),
            other if other.contains('{') => Err(invalid(format!("unexpected placeholder {other}"))),
            other => Ok(other.to_owned()),
        })
        .collect()
}

/// Extracts executed test counts from cargo test or vitest output.
pub fn parse_tests(text: &str) -> Option<TestCounts> {
    let mut found = false;
    let mut counts = TestCounts {
        passed: 0,
        failed: 0,
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("test result: ") {
            found = true;
            counts.passed += number_before(rest, " passed").unwrap_or(0);
            counts.failed += number_before(rest, " failed").unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("Tests ") {
            // vitest: "Tests  3 failed | 12 passed (15)"
            found = true;
            counts.passed += number_before(rest, " passed").unwrap_or(0);
            counts.failed += number_before(rest, " failed").unwrap_or(0);
        }
    }
    found.then_some(counts)
}

fn number_before(text: &str, marker: &str) -> Option<u64> {
    let end = text.find(marker)?;
    let head = &text[..end];
    let start = head
        .rfind(|c: char| !c.is_ascii_digit())
        .map_or(0, |i| i + 1);
    head[start..].parse().ok()
}

/// Loads every run record for a subject.
pub fn load_runs(repo: &Repo, subject: Subject) -> Result<Vec<(PathBuf, RunRecord)>> {
    let dir = repo.evidence_dir(subject).join("runs");
    let mut runs = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(runs);
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path().join("run.json");
        if path.is_file() {
            runs.push((path.clone(), crate::read_json::<RunRecord>(&path)?));
        }
    }
    runs.sort_by(|a, b| {
        a.1.started_utc
            .cmp(&b.1.started_utc)
            .then(a.1.run_id.cmp(&b.1.run_id))
    });
    Ok(runs)
}

/// Reads a log back for display (bounded).
pub fn read_log_tail(path: &Path, max: usize) -> String {
    let mut buf = Vec::new();
    if let Ok(mut file) = std::fs::File::open(path) {
        let _ = file.read_to_end(&mut buf);
    }
    let start = buf.len().saturating_sub(max);
    String::from_utf8_lossy(&buf[start..]).into_owned()
}
