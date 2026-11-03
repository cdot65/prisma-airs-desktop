//! Review packets, receipts and the completion gate.
//!
//! The gate never trusts a single summary. It re-derives the decision from the
//! immutable packet, the reviewer-authored verdict (hash-bound and compared
//! with the reviewer's actual final output), the run and native evidence the
//! packet names checked against the approved command plan and native check
//! inventory, the predecessor receipts, and the inputs as they exist now.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::evidence::{NativeAcceptance, NativeExpectation};
use crate::fingerprint::Fingerprints;
use crate::manifest::Command;
use crate::runner::{RUN_SCHEMA, RunKind, RunRecord, Status};
use crate::verdict::{Scores, Verdict};
use crate::{Result, invalid, io_err, sha256_hex};

pub const PACKET_SCHEMA: &str = "airs-v2/review-request/1";
pub const RECEIPT_SCHEMA: &str = "airs-v2/review-receipt/1";
pub const MINIMUM_SCORE: u8 = 9;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HashedPath {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRef {
    pub kind: RunKind,
    pub run_id: String,
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PredecessorRef {
    pub subject: String,
    pub receipt: HashedPath,
}

/// The immutable request packet given to the reviewer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub schema: String,
    pub request_id: String,
    pub attempt_id: String,
    pub subject: String,
    pub kind: String,
    pub created_utc: String,
    pub repository: String,
    pub base_commit: String,
    pub scope: String,
    pub instructions: Vec<String>,
    pub predecessors: Vec<PredecessorRef>,
    pub fingerprints: Fingerprints,
    pub source_inventory: HashedPath,
    pub runs: Vec<RunRef>,
    /// Every committed evidence file for the subject outside `runs/`.
    pub evidence_files: Vec<HashedPath>,
    pub native_acceptance: Option<HashedPath>,
    pub native_executable_sha256: Option<String>,
    pub diff: HashedPath,
    pub limitations: Vec<String>,
    pub verdict_schema: HashedPath,
    pub verdict_path: String,
    pub review_instructions: String,
}

/// Facts about how the reviewer ran, recorded by the controller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransportRecord {
    pub kind: String,
    pub thread: Option<String>,
    pub codex_path: String,
    pub codex_version: String,
    pub codex_sha256: String,
    pub submitted_utc: String,
    pub completed_utc: String,
    /// Process exit status (fresh exec) or the queue command's (thread).
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub raw_output: Option<HashedPath>,
    pub model: Option<String>,
}

/// Controller-written record of one review attempt. Its `passed` and
/// `failures` fields are informational; the gate re-derives the decision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub receipt_id: String,
    pub subject: String,
    pub request_id: String,
    pub attempt_id: String,
    pub transport: TransportRecord,
    pub packet_sha256: String,
    pub verdict_sha256: String,
    pub fingerprints: Fingerprints,
    pub fingerprints_after: Fingerprints,
    pub scores: Scores,
    pub required_fixes: usize,
    pub blockers: usize,
    pub evidence_gaps: usize,
    pub passed: bool,
    pub failures: Vec<String>,
    pub created_utc: String,
}

/// What the approved manifest and native inventory require of an attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expectations {
    /// Exact command plan per required run kind.
    pub plans: Vec<(RunKind, Vec<Command>)>,
    /// Native acceptance requirements; None for phases.
    pub native: Option<NativeExpectation>,
}

/// Everything the gate evaluates, already loaded.
#[derive(Clone, Debug)]
pub struct GateInputs {
    pub receipt_bytes: Vec<u8>,
    pub packet_bytes: Vec<u8>,
    pub verdict_bytes: Option<Vec<u8>>,
    pub attempt_expired: bool,
    pub current: Fingerprints,
    /// Run files named by the packet: (ref, bytes as currently on disk).
    pub runs: Vec<(RunRef, Option<Vec<u8>>)>,
    /// Command logs of those runs, keyed by repository-relative path.
    pub logs: BTreeMap<String, Vec<u8>>,
    pub native_bytes: Option<Vec<u8>>,
    /// Raw reviewer output named by the receipt (thread reply or exec JSONL).
    pub raw_output_bytes: Option<Vec<u8>>,
    /// Predecessor receipts named by the packet, as currently on disk.
    pub predecessors: Vec<(PredecessorRef, Option<Vec<u8>>)>,
    pub expect: Expectations,
}

/// Evaluates a review attempt. An empty list means the gate passes.
pub fn evaluate(inputs: &GateInputs) -> Vec<String> {
    let mut failures = Vec::new();
    let receipt: Receipt = match serde_json::from_slice(&inputs.receipt_bytes) {
        Ok(r) => r,
        Err(e) => return vec![format!("receipt is malformed: {e}")],
    };
    if receipt.schema != RECEIPT_SCHEMA {
        failures.push("receipt has the wrong schema".to_owned());
    }
    let packet: Packet = match serde_json::from_slice(&inputs.packet_bytes) {
        Ok(p) => p,
        Err(e) => {
            failures.push(format!("request packet is malformed: {e}"));
            return failures;
        }
    };
    if packet.schema != PACKET_SCHEMA {
        failures.push("request packet has the wrong schema".to_owned());
    }
    if sha256_hex(&inputs.packet_bytes) != receipt.packet_sha256 {
        failures.push("request packet bytes do not match the receipt (edited packet)".to_owned());
    }
    if receipt.request_id != packet.request_id
        || receipt.attempt_id != packet.attempt_id
        || receipt.subject != packet.subject
    {
        failures.push("receipt identity does not match its request packet".to_owned());
    }
    if inputs.attempt_expired {
        failures.push("the attempt is expired; its verdict cannot advance the gate".to_owned());
    }
    let Some(verdict_bytes) = inputs.verdict_bytes.as_ref() else {
        failures.push("no reviewer-authored verdict exists (submission is not review)".to_owned());
        return failures;
    };
    if sha256_hex(verdict_bytes) != receipt.verdict_sha256 {
        failures.push("verdict bytes do not match the receipt (edited verdict)".to_owned());
    }
    let verdict = match Verdict::parse(verdict_bytes) {
        Err(e) => {
            failures.push(e.to_string());
            None
        }
        Ok(verdict) => {
            if verdict.request_id != packet.request_id {
                failures.push("verdict request_id does not match the packet".to_owned());
            }
            if verdict.attempt_id != packet.attempt_id {
                failures.push("verdict attempt_id does not match the packet".to_owned());
            }
            if verdict.subject != packet.subject {
                failures.push("verdict subject does not match the packet".to_owned());
            }
            if verdict.fingerprints != packet.fingerprints {
                failures.push("verdict fingerprints differ from the frozen packet".to_owned());
            }
            if verdict.scores != receipt.scores {
                failures.push("receipt scores differ from the verdict".to_owned());
            }
            failures.extend(verdict.threshold_failures(MINIMUM_SCORE));
            Some(verdict)
        }
    };
    if receipt.fingerprints != packet.fingerprints {
        failures.push("receipt fingerprints differ from the packet".to_owned());
    }
    if receipt.fingerprints_after != packet.fingerprints {
        failures.push("inputs changed while the review was active".to_owned());
    }
    let current = &inputs.current;
    let frozen = &packet.fingerprints;
    if current.source != frozen.source {
        failures.push("source changed since review (stale receipt)".to_owned());
    }
    if current.dependencies != frozen.dependencies {
        failures.push("dependencies or toolchain changed since review".to_owned());
    }
    if current.build_flags != frozen.build_flags {
        failures.push("build flags/features changed since review".to_owned());
    }
    if current.evidence != frozen.evidence {
        failures.push("evidence changed since review".to_owned());
    }
    failures.extend(transport_failures(inputs, &receipt, verdict.as_ref()));
    failures.extend(predecessor_failures(inputs, &packet));
    failures.extend(run_failures(inputs, &packet));
    if let Some(native) = inputs.expect.native.as_ref() {
        failures.extend(native_failures(inputs, &packet, native));
    }
    failures
}

/// Extracts every JSON object embedded in a text (whole text or substrings
/// beginning at `{`).
fn json_objects(text: &str) -> Vec<serde_json::Value> {
    let mut found = Vec::new();
    for (i, _) in text.match_indices('{') {
        let mut stream =
            serde_json::Deserializer::from_str(&text[i..]).into_iter::<serde_json::Value>();
        if let Some(Ok(value)) = stream.next()
            && value.is_object()
        {
            found.push(value);
        }
    }
    found
}

/// The reviewer's actual completion: the transport finished successfully and
/// its final structured output equals verdict.json.
fn transport_failures(
    inputs: &GateInputs,
    receipt: &Receipt,
    verdict: Option<&Verdict>,
) -> Vec<String> {
    let transport = &receipt.transport;
    let Some(reference) = transport.raw_output.as_ref() else {
        return vec!["no raw reviewer output corroborates the verdict".to_owned()];
    };
    let Some(bytes) = inputs.raw_output_bytes.as_ref() else {
        return vec!["raw reviewer output is missing".to_owned()];
    };
    let mut failures = Vec::new();
    if sha256_hex(bytes) != reference.sha256 {
        failures.push("raw reviewer output was edited".to_owned());
    }
    if transport.timed_out {
        failures.push("the reviewer transport timed out".to_owned());
    }
    if transport.exit_code != Some(0) {
        failures.push(format!(
            "the reviewer transport exited with {:?}",
            transport.exit_code
        ));
    }
    let expected = verdict.and_then(|v| serde_json::to_value(v).ok());
    let mut final_outputs: Vec<String> = Vec::new();
    let text = String::from_utf8_lossy(bytes);
    match transport.kind.as_str() {
        "codex-thread" => {
            if transport.thread.is_none() {
                failures.push("thread transport without a thread ID".to_owned());
            }
            for line in text.lines() {
                let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                let timestamp = event["timestamp"].as_str().unwrap_or_default();
                if event["payload"]["type"] == "task_complete"
                    && timestamp >= transport.submitted_utc.as_str()
                    && let Some(message) = event["payload"]["last_agent_message"].as_str()
                {
                    final_outputs.push(message.to_owned());
                }
            }
        }
        "codex-exec-fresh" => {
            let (forbidden, _) = crate::transport::inspect_events(bytes);
            failures.extend(forbidden);
            let mut completed = false;
            let mut last_message = None;
            for line in text.lines() {
                let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                match event["type"].as_str().unwrap_or_default() {
                    "turn.completed" => completed = true,
                    "turn.failed" | "error" => {
                        failures.push("the fresh reviewer turn failed".to_owned())
                    }
                    "item.completed" if event["item"]["type"] == "agent_message" => {
                        last_message = event["item"]["text"].as_str().map(str::to_owned);
                    }
                    _ => {}
                }
            }
            if !completed {
                failures.push("the fresh reviewer turn never completed".to_owned());
            }
            final_outputs.extend(last_message);
        }
        other => failures.push(format!("unknown transport {other}")),
    }
    if final_outputs.is_empty() {
        failures.push("no completed reviewer turn with a final output was observed".to_owned());
    } else if let Some(expected) = expected {
        let matches = final_outputs
            .iter()
            .any(|message| json_objects(message).contains(&expected));
        if !matches {
            failures.push("the reviewer's final output does not equal verdict.json".to_owned());
        }
    }
    failures
}

fn predecessor_failures(inputs: &GateInputs, packet: &Packet) -> Vec<String> {
    let mut failures = Vec::new();
    if inputs.predecessors.len() != packet.predecessors.len() {
        failures.push("predecessor evidence does not match the packet".to_owned());
    }
    for (reference, bytes) in &inputs.predecessors {
        match bytes {
            None => failures.push(format!(
                "predecessor {} receipt is missing",
                reference.subject
            )),
            Some(bytes) if sha256_hex(bytes) != reference.receipt.sha256 => {
                failures.push(format!(
                    "predecessor {} receipt was changed",
                    reference.subject
                ));
            }
            Some(_) => {}
        }
    }
    failures
}

fn run_failures(inputs: &GateInputs, packet: &Packet) -> Vec<String> {
    let mut failures = Vec::new();
    let mut kinds_seen = BTreeSet::new();
    for (kind, _) in &inputs.expect.plans {
        if packet.runs.iter().filter(|r| r.kind == *kind).count() != 1 {
            failures.push(format!("expected exactly one {kind:?} run in the packet"));
        }
    }
    for (run_ref, bytes) in &inputs.runs {
        let id = &run_ref.run_id;
        let Some(bytes) = bytes else {
            failures.push(format!("run {id} is missing"));
            continue;
        };
        if sha256_hex(bytes) != run_ref.sha256 {
            failures.push(format!("run {id} was edited after freezing"));
            continue;
        }
        let record: RunRecord = match serde_json::from_slice(bytes) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("run {id} is malformed: {e}"));
                continue;
            }
        };
        if record.schema != RUN_SCHEMA
            || record.subject != packet.subject
            || record.kind != run_ref.kind
            || record.run_id != run_ref.run_id
            || run_ref.path
                != format!(
                    "docs/planning/evidence/{}/runs/{}/run.json",
                    packet.subject, record.run_id
                )
        {
            failures.push(format!("run {id} identity does not match its reference"));
        }
        kinds_seen.insert(format!("{:?}", record.kind));
        if record.status != Status::Passed || record.failure.is_some() {
            failures.push(format!("run {id} did not pass"));
        }
        let Some((_, plan)) = inputs.expect.plans.iter().find(|(k, _)| *k == record.kind) else {
            failures.push(format!("run {id} is of an unexpected kind"));
            continue;
        };
        if record.commands.len() != plan.len() {
            failures.push(format!(
                "run {id} recorded {} commands; the approved plan has {}",
                record.commands.len(),
                plan.len()
            ));
        }
        for (i, expected) in plan.iter().enumerate() {
            let Some(command) = record.commands.get(i) else {
                break;
            };
            let label = &expected.label;
            if command.label != *label || command.argv != expected.argv {
                failures.push(format!("run {id} command {i} is not the approved {label}"));
            }
            if command.status != Status::Passed || command.exit_code != Some(0) {
                failures.push(format!(
                    "run {id} command {label} did not exit successfully"
                ));
            }
            if expected.expects_tests {
                match &command.tests {
                    Some(t) if t.passed > 0 && t.failed == 0 => {}
                    _ => failures.push(format!(
                        "run {id} command {label} executed no passing tests"
                    )),
                }
            }
            let log_path = format!(
                "docs/planning/evidence/{}/runs/{}/{}",
                packet.subject, record.run_id, command.log
            );
            match inputs.logs.get(&log_path) {
                Some(log) if sha256_hex(log) == command.output_sha256 => {}
                Some(_) => {
                    failures.push(format!("run {id} log for {label} does not match its hash"))
                }
                None => failures.push(format!("run {id} log for {label} is missing")),
            }
        }
        for state in [&record.inputs_before, &record.inputs_after] {
            if state.source != packet.fingerprints.source
                || state.dependencies != packet.fingerprints.dependencies
                || state.build_flags != packet.fingerprints.build_flags
            {
                failures.push(format!("run {id} executed against different inputs"));
                break;
            }
        }
        if record.kind == RunKind::NativePrepare {
            let artifact = |name: &str| {
                record
                    .artifacts
                    .iter()
                    .find(|a| a.name == name)
                    .map(|a| a.sha256.clone())
            };
            if artifact("fixture-executable").as_deref()
                != packet.native_executable_sha256.as_deref()
                || packet.native_executable_sha256.is_none()
            {
                failures.push(format!(
                    "run {id} does not record the packet's prepared executable"
                ));
            }
            if let Some(native) = inputs.expect.native.as_ref() {
                let bundle = artifact("fixture-bundle");
                if bundle.is_none() || bundle != native.prepared_bundle_sha256 {
                    failures.push(format!(
                        "run {id} does not record the prepared fixture bundle"
                    ));
                }
            }
        }
    }
    failures
}

fn native_failures(
    inputs: &GateInputs,
    packet: &Packet,
    expect: &NativeExpectation,
) -> Vec<String> {
    let Some(reference) = packet.native_acceptance.as_ref() else {
        return vec!["missing native acceptance evidence".to_owned()];
    };
    let Some(bytes) = inputs.native_bytes.as_ref() else {
        return vec!["native acceptance record is missing".to_owned()];
    };
    if sha256_hex(bytes) != reference.sha256 {
        return vec!["native acceptance record was edited after freezing".to_owned()];
    }
    if packet.native_executable_sha256.as_deref()
        != Some(expect.prepared_executable_sha256.as_str())
    {
        return vec![
            "the packet's prepared executable differs from the native expectation".to_owned(),
        ];
    }
    match NativeAcceptance::parse(bytes) {
        Ok(record) => record.failures(&packet.subject, expect),
        Err(e) => vec![e.to_string()],
    }
}

/// Loads gate inputs for a receipt from disk. `native` is completed with the
/// packet's evidence index and prepared executable.
pub fn load(
    root: &Path,
    receipt_path: &Path,
    current: Fingerprints,
    plans: Vec<(RunKind, Vec<Command>)>,
    native: Option<NativeBase>,
) -> Result<GateInputs> {
    let receipt_bytes = std::fs::read(receipt_path).map_err(io_err(receipt_path))?;
    let attempt_dir = receipt_path
        .parent()
        .ok_or_else(|| invalid("receipt has no attempt directory"))?;
    let packet_path = attempt_dir.join("request.json");
    let packet_bytes = std::fs::read(&packet_path).map_err(io_err(&packet_path))?;
    let packet: Packet = serde_json::from_slice(&packet_bytes)
        .map_err(|e| invalid(format!("request packet is malformed: {e}")))?;
    let verdict_bytes = std::fs::read(&packet.verdict_path).ok();
    let read = |relative: &str| -> Option<Vec<u8>> { std::fs::read(root.join(relative)).ok() };
    let runs: Vec<(RunRef, Option<Vec<u8>>)> = packet
        .runs
        .iter()
        .map(|r| (r.clone(), read(&r.path)))
        .collect();
    let mut logs = BTreeMap::new();
    for (run_ref, bytes) in &runs {
        let Some(record) = bytes
            .as_ref()
            .and_then(|b| serde_json::from_slice::<RunRecord>(b).ok())
        else {
            continue;
        };
        let dir = run_ref.path.trim_end_matches("run.json");
        for command in &record.commands {
            let path = format!("{dir}{}", command.log);
            if let Some(log) = read(&path) {
                logs.insert(path, log);
            }
        }
    }
    let native_bytes = packet
        .native_acceptance
        .as_ref()
        .and_then(|n| read(&n.path));
    let receipt: Receipt = serde_json::from_slice(&receipt_bytes)
        .map_err(|e| invalid(format!("receipt is malformed: {e}")))?;
    let raw_output_bytes = receipt
        .transport
        .raw_output
        .as_ref()
        .and_then(|r| read(&r.path));
    let predecessors = packet
        .predecessors
        .iter()
        .map(|p| (p.clone(), read(&p.receipt.path)))
        .collect();
    let native = native.map(|base| NativeExpectation {
        required_checks: base.required_checks,
        fixture_identifier: base.fixture_identifier,
        prepared_executable_sha256: packet.native_executable_sha256.clone().unwrap_or_default(),
        prepared_bundle: base.prepared_bundle,
        prepared_bundle_sha256: base.prepared_bundle_sha256,
        prepared_identifier: base.prepared_identifier,
        current_executable_sha256: base.current_executable_sha256,
        current_bundle_sha256: base.current_bundle_sha256,
        current_identifier: base.current_identifier,
        evidence_files: packet
            .evidence_files
            .iter()
            .map(|e| e.path.clone())
            .collect(),
    });
    Ok(GateInputs {
        receipt_bytes,
        packet_bytes,
        verdict_bytes,
        attempt_expired: attempt_dir.join("expired.json").exists(),
        current,
        runs,
        logs,
        native_bytes,
        raw_output_bytes,
        predecessors,
        expect: Expectations { plans, native },
    })
}

/// Native requirements known before reading a packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeBase {
    pub required_checks: Vec<String>,
    pub fixture_identifier: String,
    pub prepared_bundle: Option<String>,
    pub prepared_bundle_sha256: Option<String>,
    pub prepared_identifier: Option<String>,
    pub current_executable_sha256: Option<String>,
    pub current_bundle_sha256: Option<String>,
    pub current_identifier: Option<String>,
}

pub fn attempt_dir_of(receipt: &Path) -> PathBuf {
    receipt.parent().map(Path::to_path_buf).unwrap_or_default()
}

#[cfg(feature = "test-fixtures")]
pub mod fixtures {
    //! A self-consistent passing review attempt for adversarial tests.

    use super::*;
    use crate::evidence::{CheckResult, Method, NATIVE_SCHEMA, NativeApp, NativeCheck};
    use crate::runner::{Artifact, CommandResult, InputState, TestCounts};
    use crate::verdict::Recommendation;

    pub const EXECUTABLE: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    pub const BUNDLE_SHA: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    pub const BUNDLE: &str = "target/fixture.app";
    pub const FIXTURE_ID: &str = "io.cdot.airs-desktop.fixture";
    pub const EVIDENCE: &str = "docs/planning/evidence/C01/native/capture.json";

    pub fn fingerprints() -> Fingerprints {
        Fingerprints {
            source: "a".repeat(64),
            dependencies: "b".repeat(64),
            build_flags: "c".repeat(64),
            evidence: "d".repeat(64),
        }
    }

    pub fn verdict() -> Verdict {
        Verdict {
            request_id: "rq-fixture".to_owned(),
            attempt_id: "attempt-01".to_owned(),
            subject: "C01".to_owned(),
            fingerprints: fingerprints(),
            scores: Scores {
                architecture: 9,
                behavior: 9,
                authority_security: 9,
                validation: 9,
                overall: 9,
            },
            required_fixes: vec![],
            blockers: vec![],
            evidence_gaps: vec![],
            optional_suggestions: vec!["optional".to_owned()],
            limitations: vec![],
            inspected_files: vec!["crates/xtask/src/gate.rs".to_owned()],
            recommendation: Recommendation::Pass,
        }
    }

    /// The approved plan used by the fixture: two check commands, one test
    /// target and the native-prepare command.
    pub fn plans() -> Vec<(RunKind, Vec<Command>)> {
        let command = |label: &str, argv: &[&str], tests: bool| Command {
            label: label.to_owned(),
            argv: argv.iter().map(|s| (*s).to_owned()).collect(),
            expects_tests: tests,
        };
        vec![
            (
                RunKind::Check,
                vec![
                    command(
                        "common/1",
                        &["cargo", "fmt", "--all", "--", "--check"],
                        false,
                    ),
                    command(
                        "common/2",
                        &[
                            "pnpm",
                            "--filter",
                            "@prisma-airs/desktop",
                            "exec",
                            "vitest",
                            "run",
                        ],
                        true,
                    ),
                ],
            ),
            (
                RunKind::Test,
                vec![command(
                    "C01/fixture",
                    &[
                        "cargo",
                        "test",
                        "--locked",
                        "-p",
                        "xtask",
                        "--test",
                        "shell_controller",
                    ],
                    true,
                )],
            ),
            (
                RunKind::NativePrepare,
                vec![command(
                    "native-prepare",
                    &[
                        "cargo",
                        "run",
                        "--locked",
                        "-p",
                        "xtask",
                        "--",
                        "native-prepare",
                        "--chunk",
                        "C01",
                    ],
                    false,
                )],
            ),
        ]
    }

    pub fn log_bytes(kind: RunKind, label: &str) -> Vec<u8> {
        format!("{kind:?} {label}\ntest result: ok. 3 passed; 0 failed\n").into_bytes()
    }

    pub fn run(kind: RunKind) -> RunRecord {
        let fp = fingerprints();
        let state = InputState {
            source: fp.source,
            dependencies: fp.dependencies,
            build_flags: fp.build_flags,
        };
        let plan = plans()
            .into_iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, p)| p)
            .unwrap_or_default();
        RunRecord {
            schema: RUN_SCHEMA.to_owned(),
            run_id: format!("{kind:?}-fixture"),
            subject: "C01".to_owned(),
            kind,
            started_utc: "2026-10-17T00:00:00Z".to_owned(),
            finished_utc: "2026-10-17T00:01:00Z".to_owned(),
            toolchain: vec![],
            env: Default::default(),
            inputs_before: state.clone(),
            inputs_after: state,
            commands: plan
                .iter()
                .map(|c| CommandResult {
                    label: c.label.clone(),
                    argv: c.argv.clone(),
                    cwd: ".".to_owned(),
                    exit_code: Some(0),
                    duration_ms: 1,
                    output_sha256: sha256_hex(&log_bytes(kind, &c.label)),
                    log: format!("logs/{}.log", c.label.replace('/', "-")),
                    tests: c.expects_tests.then_some(TestCounts {
                        passed: 3,
                        failed: 0,
                    }),
                    status: Status::Passed,
                    note: None,
                })
                .collect(),
            status: Status::Passed,
            failure: None,
            artifacts: if kind == RunKind::NativePrepare {
                vec![
                    Artifact {
                        name: "fixture-executable".to_owned(),
                        path: "target/fixture.app/Contents/MacOS/prisma-airs-desktop".to_owned(),
                        sha256: EXECUTABLE.to_owned(),
                    },
                    Artifact {
                        name: "fixture-bundle".to_owned(),
                        path: BUNDLE.to_owned(),
                        sha256: BUNDLE_SHA.to_owned(),
                    },
                ]
            } else {
                vec![]
            },
        }
    }

    pub fn required_checks() -> Vec<String> {
        vec!["C01-N01".to_owned(), "C01-N02".to_owned()]
    }

    pub fn native() -> NativeAcceptance {
        NativeAcceptance {
            schema: NATIVE_SCHEMA.to_owned(),
            subject: "C01".to_owned(),
            recorded_utc: "2026-10-17T00:02:00Z".to_owned(),
            method: Method::Cua,
            performed_by: "fixture".to_owned(),
            host: "fixture".to_owned(),
            app: NativeApp {
                bundle: BUNDLE.to_owned(),
                identifier: FIXTURE_ID.to_owned(),
                executable_sha256: EXECUTABLE.to_owned(),
                fixture: true,
            },
            checks: required_checks()
                .into_iter()
                .map(|id| NativeCheck {
                    id,
                    description: "check".to_owned(),
                    result: CheckResult::Pass,
                    observation: "observed".to_owned(),
                    evidence: vec![EVIDENCE.to_owned()],
                })
                .collect(),
            limitations: vec![],
        }
    }

    pub fn to_bytes<T: Serialize>(value: &T) -> Vec<u8> {
        serde_json::to_vec_pretty(value).unwrap_or_default()
    }

    /// A thread transcript line completing the turn with `verdict`.
    pub fn thread_reply(verdict: &Verdict) -> Vec<u8> {
        let message = serde_json::to_string_pretty(verdict).unwrap_or_default();
        let event = serde_json::json!({
            "timestamp": "2026-10-17T00:05:00Z",
            "type": "event_msg",
            "payload": {"type": "task_complete", "last_agent_message": message}
        });
        let mut line = serde_json::to_vec(&event).unwrap_or_default();
        line.push(b'\n');
        line
    }

    pub const PREDECESSOR: &[u8] = br#"{"gate":"G01","passed":true}"#;

    /// Builds consistent inputs from the given parts (hashes recomputed).
    pub fn assemble(
        verdict: &Verdict,
        runs: &[RunRecord],
        native: &NativeAcceptance,
    ) -> GateInputs {
        let verdict_bytes = to_bytes(verdict);
        let run_bytes: Vec<Vec<u8>> = runs.iter().map(to_bytes).collect();
        let native_bytes = to_bytes(native);
        let run_refs: Vec<RunRef> = runs
            .iter()
            .zip(&run_bytes)
            .map(|(r, b)| RunRef {
                kind: r.kind,
                run_id: r.run_id.clone(),
                path: format!("docs/planning/evidence/C01/runs/{}/run.json", r.run_id),
                sha256: sha256_hex(b),
            })
            .collect();
        let mut logs = BTreeMap::new();
        for run in runs {
            for command in &run.commands {
                logs.insert(
                    format!(
                        "docs/planning/evidence/C01/runs/{}/{}",
                        run.run_id, command.log
                    ),
                    log_bytes(run.kind, &command.label),
                );
            }
        }
        let predecessor = PredecessorRef {
            subject: "G01".to_owned(),
            receipt: HashedPath {
                path: "docs/planning/reviews/G01/attempt-05/receipt.json".to_owned(),
                sha256: sha256_hex(PREDECESSOR),
            },
        };
        let packet = Packet {
            schema: PACKET_SCHEMA.to_owned(),
            request_id: "rq-fixture".to_owned(),
            attempt_id: "attempt-01".to_owned(),
            subject: "C01".to_owned(),
            kind: "chunk".to_owned(),
            created_utc: "2026-10-17T00:03:00Z".to_owned(),
            repository: "/fixture".to_owned(),
            base_commit: "0".repeat(40),
            scope: "fixture".to_owned(),
            instructions: vec![],
            predecessors: vec![predecessor.clone()],
            fingerprints: fingerprints(),
            source_inventory: HashedPath {
                path: "inventory.json".to_owned(),
                sha256: "1".repeat(64),
            },
            runs: run_refs.clone(),
            evidence_files: vec![HashedPath {
                path: EVIDENCE.to_owned(),
                sha256: "5".repeat(64),
            }],
            native_acceptance: Some(HashedPath {
                path: "docs/planning/evidence/C01/native-acceptance.json".to_owned(),
                sha256: sha256_hex(&native_bytes),
            }),
            native_executable_sha256: Some(EXECUTABLE.to_owned()),
            diff: HashedPath {
                path: "diff.patch".to_owned(),
                sha256: "2".repeat(64),
            },
            limitations: vec![],
            verdict_schema: HashedPath {
                path: "schema.json".to_owned(),
                sha256: "3".repeat(64),
            },
            verdict_path: "/fixture/verdict.json".to_owned(),
            review_instructions: "fixture".to_owned(),
        };
        let packet_bytes = to_bytes(&packet);
        let raw = thread_reply(verdict);
        let receipt = Receipt {
            schema: RECEIPT_SCHEMA.to_owned(),
            receipt_id: "C01-attempt-01-rq-fixture".to_owned(),
            subject: "C01".to_owned(),
            request_id: "rq-fixture".to_owned(),
            attempt_id: "attempt-01".to_owned(),
            transport: TransportRecord {
                kind: "codex-thread".to_owned(),
                thread: Some("thread".to_owned()),
                codex_path: "/codex".to_owned(),
                codex_version: "codex-cli fixture".to_owned(),
                codex_sha256: "4".repeat(64),
                submitted_utc: "2026-10-17T00:04:00Z".to_owned(),
                completed_utc: "2026-10-17T00:05:00Z".to_owned(),
                exit_code: Some(0),
                timed_out: false,
                raw_output: Some(HashedPath {
                    path: "docs/planning/reviews/C01/attempt-01/thread-reply.jsonl".to_owned(),
                    sha256: sha256_hex(&raw),
                }),
                model: None,
            },
            packet_sha256: sha256_hex(&packet_bytes),
            verdict_sha256: sha256_hex(&verdict_bytes),
            fingerprints: fingerprints(),
            fingerprints_after: fingerprints(),
            scores: verdict.scores.clone(),
            required_fixes: verdict.required_fixes.len(),
            blockers: verdict.blockers.len(),
            evidence_gaps: verdict.evidence_gaps.len(),
            passed: true,
            failures: vec![],
            created_utc: "2026-10-17T00:06:00Z".to_owned(),
        };
        GateInputs {
            receipt_bytes: to_bytes(&receipt),
            packet_bytes,
            verdict_bytes: Some(verdict_bytes),
            attempt_expired: false,
            current: fingerprints(),
            runs: run_refs
                .into_iter()
                .zip(run_bytes.into_iter().map(Some))
                .collect(),
            logs,
            native_bytes: Some(native_bytes),
            raw_output_bytes: Some(raw),
            predecessors: vec![(predecessor, Some(PREDECESSOR.to_vec()))],
            expect: Expectations {
                plans: plans(),
                native: Some(NativeExpectation {
                    required_checks: required_checks(),
                    fixture_identifier: FIXTURE_ID.to_owned(),
                    prepared_executable_sha256: EXECUTABLE.to_owned(),
                    prepared_bundle: Some(BUNDLE.to_owned()),
                    prepared_bundle_sha256: Some(BUNDLE_SHA.to_owned()),
                    prepared_identifier: Some(FIXTURE_ID.to_owned()),
                    current_executable_sha256: Some(EXECUTABLE.to_owned()),
                    current_bundle_sha256: Some(BUNDLE_SHA.to_owned()),
                    current_identifier: Some(FIXTURE_ID.to_owned()),
                    evidence_files: [EVIDENCE.to_owned()].into_iter().collect(),
                }),
            },
        }
    }

    pub fn valid() -> GateInputs {
        assemble(
            &verdict(),
            &[
                run(RunKind::Check),
                run(RunKind::Test),
                run(RunKind::NativePrepare),
            ],
            &native(),
        )
    }
}
