//! Adversarial tests for the C01 review/completion controller.
//!
//! Each case starts from a fully consistent passing attempt and breaks one
//! invariant. The valid baseline must pass, so every rejection below is caused
//! by the single mutation it names.

use std::path::{Path, PathBuf};
use std::process::Command;

use xtask::gate::{self, GateInputs, NativeBase, Receipt, fixtures};
use xtask::manifest::Manifest;
use xtask::runner::{self, RunKind, Status};
use xtask::verdict::Verdict;
use xtask::{Repo, Subject, complete, fingerprint, ledger, transport};

/// A named JSON mutation applied to an otherwise valid document.
type Mutation = (&'static str, Box<dyn Fn(&mut serde_json::Value)>);

fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

fn rejected(inputs: &GateInputs, needle: &str) {
    let failures = gate::evaluate(inputs);
    assert!(
        failures.iter().any(|f| f.contains(needle)),
        "expected a failure containing {needle:?}, got {failures:?}"
    );
}

fn with_verdict(edit: impl FnOnce(&mut Verdict)) -> GateInputs {
    let mut verdict = fixtures::verdict();
    edit(&mut verdict);
    fixtures::assemble(
        &verdict,
        &[
            fixtures::run(RunKind::Check),
            fixtures::run(RunKind::Test),
            fixtures::run(RunKind::NativePrepare),
        ],
        &fixtures::native(),
    )
}

fn receipt_of(inputs: &GateInputs) -> Receipt {
    serde_json::from_slice(&inputs.receipt_bytes).expect("receipt")
}

fn set_receipt(inputs: &mut GateInputs, receipt: &Receipt) {
    inputs.receipt_bytes = fixtures::to_bytes(receipt);
}

#[test]
fn consistent_attempt_passes() {
    assert_eq!(gate::evaluate(&fixtures::valid()), Vec::<String>::new());
}

#[test]
fn score_eight_in_any_dimension_is_rejected() {
    for dimension in [
        "architecture",
        "behavior",
        "authority_security",
        "validation",
        "overall",
    ] {
        let inputs = with_verdict(|v| match dimension {
            "architecture" => v.scores.architecture = 8,
            "behavior" => v.scores.behavior = 8,
            "authority_security" => v.scores.authority_security = 8,
            "validation" => v.scores.validation = 8,
            _ => v.scores.overall = 8,
        });
        rejected(&inputs, &format!("score {dimension}=8 is below 9"));
    }
}

#[test]
fn required_fix_blocker_or_gap_rejects_despite_nines() {
    rejected(
        &with_verdict(|v| v.required_fixes.push("fix".into())),
        "required fixes remain",
    );
    rejected(
        &with_verdict(|v| v.blockers.push("blocker".into())),
        "blockers remain",
    );
    rejected(
        &with_verdict(|v| v.evidence_gaps.push("gap".into())),
        "evidence gaps remain",
    );
    rejected(
        &with_verdict(|v| v.recommendation = xtask::verdict::Recommendation::Fail),
        "recommendation is fail",
    );
}

#[test]
fn malformed_verdicts_are_rejected() {
    let base = fixtures::valid();
    let mut value: serde_json::Value =
        serde_json::from_slice(base.verdict_bytes.as_ref().expect("verdict")).expect("json");
    let cases: Vec<Mutation> = vec![
        (
            "extra field",
            Box::new(|v| v["approved"] = serde_json::json!(true)),
        ),
        (
            "missing field",
            Box::new(|v| {
                v.as_object_mut().expect("object").remove("evidence_gaps");
            }),
        ),
        (
            "score out of range",
            Box::new(|v| v["scores"]["overall"] = serde_json::json!(11)),
        ),
        (
            "non-integer score",
            Box::new(|v| v["scores"]["overall"] = serde_json::json!(9.5)),
        ),
        (
            "string score",
            Box::new(|v| v["scores"]["overall"] = serde_json::json!("10")),
        ),
        (
            "unknown recommendation",
            Box::new(|v| v["recommendation"] = serde_json::json!("approve")),
        ),
    ];
    for (name, mutate) in cases {
        let mut broken = value.clone();
        mutate(&mut broken);
        let bytes = serde_json::to_vec_pretty(&broken).expect("bytes");
        let mut inputs = fixtures::valid();
        let mut receipt = receipt_of(&inputs);
        receipt.verdict_sha256 = sha(&bytes);
        set_receipt(&mut inputs, &receipt);
        inputs.verdict_bytes = Some(bytes);
        let failures = gate::evaluate(&inputs);
        assert!(
            failures
                .iter()
                .any(|f| f.contains("strict schema") || f.contains("outside 1..=10")),
            "{name}: {failures:?}"
        );
    }
    value["scores"]["overall"] = serde_json::json!(9);
}

#[test]
fn edited_verdict_after_receipt_is_rejected() {
    let mut inputs = fixtures::valid();
    let mut verdict: Verdict =
        serde_json::from_slice(inputs.verdict_bytes.as_ref().expect("verdict")).expect("parse");
    verdict.scores.overall = 10;
    inputs.verdict_bytes = Some(fixtures::to_bytes(&verdict));
    rejected(&inputs, "edited verdict");
}

#[test]
fn edited_packet_and_raw_output_are_rejected() {
    let mut inputs = fixtures::valid();
    inputs.packet_bytes.extend_from_slice(b" ");
    rejected(&inputs, "edited packet");

    let mut inputs = fixtures::valid();
    if let Some(raw) = inputs.raw_output_bytes.as_mut() {
        raw.extend_from_slice(b"forged");
    }
    rejected(&inputs, "raw reviewer output was edited");

    let mut inputs = fixtures::valid();
    inputs.raw_output_bytes = None;
    rejected(&inputs, "raw reviewer output is missing");
}

#[test]
fn queue_acknowledgement_without_verdict_is_rejected() {
    let mut inputs = fixtures::valid();
    inputs.verdict_bytes = None;
    rejected(&inputs, "submission is not review");
}

#[test]
fn mismatched_request_or_fingerprints_are_rejected() {
    rejected(
        &with_verdict(|v| v.request_id = "rq-other".into()),
        "request_id does not match",
    );
    rejected(
        &with_verdict(|v| v.attempt_id = "attempt-09".into()),
        "attempt_id does not match",
    );
    rejected(
        &with_verdict(|v| v.subject = "C02".into()),
        "subject does not match",
    );
    rejected(
        &with_verdict(|v| v.fingerprints.source = "0".repeat(64)),
        "differ from the frozen packet",
    );
}

#[test]
fn receipt_scores_must_match_the_verdict() {
    let mut inputs = fixtures::valid();
    let mut receipt = receipt_of(&inputs);
    receipt.scores.validation = 10;
    set_receipt(&mut inputs, &receipt);
    rejected(&inputs, "receipt scores differ from the verdict");
}

#[test]
fn stale_inputs_are_rejected() {
    let mut inputs = fixtures::valid();
    inputs.current.source = "9".repeat(64);
    rejected(&inputs, "source changed since review");

    let mut inputs = fixtures::valid();
    inputs.current.dependencies = "9".repeat(64);
    rejected(&inputs, "dependencies or toolchain changed");

    let mut inputs = fixtures::valid();
    inputs.current.build_flags = "9".repeat(64);
    rejected(&inputs, "build flags/features changed");

    let mut inputs = fixtures::valid();
    inputs.current.evidence = "9".repeat(64);
    rejected(&inputs, "evidence changed since review");
}

#[test]
fn inputs_changing_during_review_are_rejected() {
    let mut inputs = fixtures::valid();
    let mut receipt = receipt_of(&inputs);
    receipt.fingerprints_after.source = "8".repeat(64);
    set_receipt(&mut inputs, &receipt);
    rejected(&inputs, "inputs changed while the review was active");
}

#[test]
fn expired_attempts_cannot_advance() {
    let mut inputs = fixtures::valid();
    inputs.attempt_expired = true;
    rejected(&inputs, "attempt is expired");
}

#[test]
fn missing_failed_or_mismatched_runs_are_rejected() {
    let missing_test = fixtures::assemble(
        &fixtures::verdict(),
        &[
            fixtures::run(RunKind::Check),
            fixtures::run(RunKind::NativePrepare),
        ],
        &fixtures::native(),
    );
    rejected(&missing_test, "expected exactly one Test run in the packet");

    let mut failed = fixtures::run(RunKind::Test);
    failed.commands[0].status = Status::Blocked;
    let inputs = fixtures::assemble(
        &fixtures::verdict(),
        &[
            fixtures::run(RunKind::Check),
            failed,
            fixtures::run(RunKind::NativePrepare),
        ],
        &fixtures::native(),
    );
    rejected(&inputs, "did not exit successfully");

    let mut other_flags = fixtures::run(RunKind::Check);
    other_flags.inputs_before.build_flags = "7".repeat(64);
    let inputs = fixtures::assemble(
        &fixtures::verdict(),
        &[
            other_flags,
            fixtures::run(RunKind::Test),
            fixtures::run(RunKind::NativePrepare),
        ],
        &fixtures::native(),
    );
    rejected(&inputs, "executed against different inputs");

    let mut inputs = fixtures::valid();
    if let Some((_, bytes)) = inputs.runs.get_mut(1) {
        *bytes = None;
    }
    rejected(&inputs, "is missing");

    let mut inputs = fixtures::valid();
    if let Some((_, Some(bytes))) = inputs.runs.get_mut(0) {
        bytes.extend_from_slice(b" ");
    }
    rejected(&inputs, "was edited after freezing");
}

#[test]
fn missing_or_failing_native_proof_is_rejected() {
    let mut inputs = fixtures::valid();
    inputs.native_bytes = None;
    rejected(&inputs, "native acceptance record is missing");

    let mut native = fixtures::native();
    native.checks[0].result = xtask::evidence::CheckResult::Blocked;
    let inputs = fixtures::assemble(
        &fixtures::verdict(),
        &[
            fixtures::run(RunKind::Check),
            fixtures::run(RunKind::Test),
            fixtures::run(RunKind::NativePrepare),
        ],
        &native,
    );
    rejected(&inputs, "native check C01-N01 is Blocked");

    let mut native = fixtures::native();
    native.app.executable_sha256 = "0".repeat(64);
    let inputs = fixtures::assemble(
        &fixtures::verdict(),
        &[
            fixtures::run(RunKind::Check),
            fixtures::run(RunKind::Test),
            fixtures::run(RunKind::NativePrepare),
        ],
        &native,
    );
    rejected(&inputs, "does not name the prepared fixture executable");
}

#[test]
fn fresh_exec_with_forbidden_tool_use_is_rejected() {
    let mut inputs = fixtures::valid();
    let mut receipt = receipt_of(&inputs);
    let raw = br#"{"type":"item.completed","item":{"type":"mcp_tool_call"}}
{"type":"item.completed","item":{"type":"agent_message","text":"rq-fixture"}}"#
        .to_vec();
    receipt.transport.kind = "codex-exec-fresh".into();
    receipt.transport.thread = None;
    if let Some(r) = receipt.transport.raw_output.as_mut() {
        r.sha256 = sha(&raw);
    }
    set_receipt(&mut inputs, &receipt);
    inputs.raw_output_bytes = Some(raw);
    rejected(&inputs, "reviewer used mcp_tool_call");
}

#[test]
fn receipt_self_reported_pass_is_not_trusted() {
    let mut inputs = with_verdict(|v| v.scores.behavior = 7);
    let mut receipt = receipt_of(&inputs);
    receipt.passed = true;
    receipt.failures.clear();
    set_receipt(&mut inputs, &receipt);
    rejected(&inputs, "score behavior=7 is below 9");
}

#[test]
fn verdict_parser_rejects_scores_out_of_range() {
    let mut value = serde_json::to_value(fixtures::verdict()).expect("value");
    value["scores"]["architecture"] = serde_json::json!(0);
    let error =
        Verdict::parse(&serde_json::to_vec(&value).expect("bytes")).expect_err("must reject");
    assert!(error.to_string().contains("outside 1..=10"));
}

// ---- Manifest, ledger and transport contracts against the real repository ----

fn repo() -> Repo {
    Repo::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

#[test]
fn approved_manifest_parses_with_codex_threshold_nine() {
    let manifest = Manifest::load(&repo().manifest()).expect("checks.json parses strictly");
    assert_eq!(manifest.reviewer.identity, "codex");
    assert!(manifest.reviewer.threshold.values().all(|v| *v == 9));
    assert_eq!(
        manifest.phase_chunks(Subject::Phase(1)).expect("P01"),
        vec![Subject::Chunk(1), Subject::Chunk(2), Subject::Chunk(3)]
    );
    let tests = manifest
        .test_commands(Subject::Chunk(1))
        .expect("C01 tests");
    assert!(
        tests.iter().all(|c| c.expects_tests),
        "every C01 target must execute tests"
    );
    assert!(
        tests
            .iter()
            .any(|c| c.argv.iter().any(|a| a == "shell_controller"))
    );
}

#[test]
fn manifest_rejects_unlisted_programs_placeholders_and_weakened_review() {
    let path = repo().manifest();
    let original: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read")).expect("json");
    let dir = scratch("manifest");
    let mutations: Vec<Mutation> = vec![
        (
            "shell program",
            Box::new(|v| v["common_commands"][0] = serde_json::json!(["sh", "-c", "true"])),
        ),
        (
            "placeholder",
            Box::new(|v| v["common_commands"][0] = serde_json::json!(["cargo", "{reviewer_text}"])),
        ),
        (
            "all features",
            Box::new(|v| {
                v["common_commands"][0] = serde_json::json!(["cargo", "test", "--all-features"])
            }),
        ),
        (
            "threshold 8",
            Box::new(|v| v["reviewer"]["threshold"]["validation"] = serde_json::json!(8)),
        ),
        (
            "claude reviewer",
            Box::new(|v| v["reviewer"]["identity"] = serde_json::json!("claude")),
        ),
        (
            "bypass flag",
            Box::new(|v| {
                if let Some(argv) = v["reviewer"]["fallback_transport"]["argv"].as_array_mut() {
                    argv.push(serde_json::json!(
                        "--dangerously-bypass-approvals-and-sandbox"
                    ));
                }
            }),
        ),
        (
            "writable sandbox",
            Box::new(|v| {
                if let Some(argv) = v["reviewer"]["fallback_transport"]["argv"].as_array_mut() {
                    for a in argv.iter_mut() {
                        if a == "read-only" {
                            *a = serde_json::json!("workspace-write");
                        }
                    }
                }
            }),
        ),
        (
            "--last thread",
            Box::new(|v| {
                v["reviewer"]["preferred_transport"]["argv"] = serde_json::json!([
                    "{codex}",
                    "queue",
                    "--last",
                    "--message",
                    "{transport_message}"
                ])
            }),
        ),
        (
            "unknown field",
            Box::new(|v| v["chunks"]["C01"]["passed"] = serde_json::json!(true)),
        ),
        (
            "missing chunk",
            Box::new(|v| {
                v["chunks"].as_object_mut().expect("chunks").remove("C26");
            }),
        ),
    ];
    for (name, mutate) in mutations {
        let mut value = original.clone();
        mutate(&mut value);
        let file = dir.join(format!("{}.json", name.replace(' ', "-")));
        std::fs::write(&file, serde_json::to_vec(&value).expect("bytes")).expect("write");
        assert!(
            Manifest::load(&file).is_err(),
            "manifest mutation {name:?} must be rejected"
        );
    }
}

#[test]
fn flag_verification_refuses_unadvertised_flags() {
    let help = "Options:\n  --sandbox <MODE>\n  --ephemeral\n  -o, --output-last-message <FILE>";
    let ok: Vec<String> = [
        "{codex}",
        "exec",
        "--sandbox",
        "read-only",
        "--ephemeral",
        "-",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    transport::verify_flags(&ok, help).expect("advertised flags pass");
    let bad: Vec<String> = ["{codex}", "exec", "--no-such-flag"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(transport::verify_flags(&bad, help).is_err());
}

#[test]
fn transport_argv_substitutes_only_validated_values() {
    let template: Vec<String> = [
        "{codex}",
        "queue",
        "--thread",
        "t",
        "--message",
        "{transport_message}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let argv = transport::render_argv(&template, &[("transport_message", "hello; rm -rf /")])
        .expect("render");
    assert_eq!(
        argv,
        vec![
            "codex",
            "queue",
            "--thread",
            "t",
            "--message",
            "hello; rm -rf /"
        ]
    );
    let unresolved: Vec<String> = ["{codex}", "{unknown}"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(transport::render_argv(&unresolved, &[]).is_err());
}

#[test]
fn workflow_ledger_matches_markdown_with_live_requirements() {
    ledger::check(&repo()).expect("committed workflow ledger is current");
    let text = std::fs::read_to_string(repo().join("docs/architecture/workflows.md")).expect("md");
    let rows = ledger::parse_markdown(&text).expect("rows");
    assert_eq!(rows.len(), 66);
    let w04 = rows.iter().find(|r| r.id == "W04").expect("W04");
    assert_eq!(w04.live.requirement, "required");
    let w25 = rows.iter().find(|r| r.id == "W25").expect("W25");
    assert_eq!(w25.live.requirement, "not_required");
    assert!(
        rows.iter().all(|r| r.evidence.installed_artifact == "unrun"
            || r.evidence.installed_artifact != "passed")
    );
}

#[test]
fn test_output_parsing_rejects_zero_test_targets() {
    assert_eq!(
        runner::parse_tests("running 0 tests\ntest result: ok. 0 passed; 0 failed; 0 ignored")
            .map(|t| t.passed),
        Some(0)
    );
    assert_eq!(
        runner::parse_tests("      Tests  2 failed | 10 passed (12)").map(|t| (t.passed, t.failed)),
        Some((10, 2))
    );
    assert!(runner::parse_tests("Finished release build").is_none());
}

// ---- Fingerprints and the append-only ledger on a disposable git checkout ----

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "xtask-{name}-{}-{}",
        std::process::id(),
        sha(name.as_bytes()).get(..8).unwrap_or("x")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

#[test]
fn source_fingerprint_detects_edits_modes_untracked_files_and_git_config() {
    let dir = scratch("fingerprint");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("a.rs"), "fn a() {}").expect("write");
    std::fs::create_dir_all(dir.join("docs/planning/reviews")).expect("mkdir");
    let repo = Repo::new(&dir);
    let base = fingerprint::source(&repo).expect("fp");

    std::fs::write(dir.join("docs/planning/reviews/x.json"), "{}").expect("write");
    std::fs::write(dir.join("docs/planning/status.md"), "status").expect("write");
    assert_eq!(
        fingerprint::source(&repo).expect("fp"),
        base,
        "narrow exclusions do not change the source hash"
    );

    std::fs::write(dir.join("a.rs"), "fn a() { }").expect("write");
    let edited = fingerprint::source(&repo).expect("fp");
    assert_ne!(edited, base, "content edit");

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir.join("a.rs"), std::fs::Permissions::from_mode(0o755))
        .expect("chmod");
    let moded = fingerprint::source(&repo).expect("fp");
    assert_ne!(moded, edited, "mode change");

    std::fs::write(dir.join("new.ts"), "export {}").expect("write");
    let untracked = fingerprint::source(&repo).expect("fp");
    assert_ne!(untracked, moded, "untracked non-ignored file");

    git(&dir, &["config", "core.hooksPath", "/tmp/hooks"]);
    assert_ne!(
        fingerprint::source(&repo).expect("fp"),
        untracked,
        "changed git configuration"
    );
}

#[test]
fn completion_ledger_is_append_only_and_requires_predecessors() {
    let dir = scratch("ledger");
    let repo = Repo::new(&dir);
    let manifest = Manifest::load(&self::repo().manifest()).expect("manifest");
    assert_eq!(
        complete::predecessors(&manifest, Subject::Chunk(4)).expect("C04"),
        vec![Subject::Chunk(3), Subject::Phase(1)]
    );
    assert_eq!(
        complete::predecessors(&manifest, Subject::Chunk(3)).expect("C03"),
        vec![Subject::Chunk(2)]
    );
    assert!(
        complete::require_predecessors(&repo, &manifest, Subject::Chunk(2)).is_err(),
        "C02 without C01"
    );
    // No receipts at all: completion is refused, not inferred.
    std::fs::create_dir_all(repo.reviews_dir(Subject::Chunk(1))).expect("mkdir");
    std::fs::create_dir_all(dir.join("docs/planning/reviews/G01/attempt-05")).expect("mkdir");
    std::fs::write(
        dir.join("docs/planning/reviews/G01/attempt-05/receipt.json"),
        r#"{"passed":true,"scores":{"architecture":9,"behavior":9,"authority_security":9,"validation":9,"overall":9}}"#,
    )
    .expect("write");
    let error = complete::complete(
        &repo,
        &manifest,
        Subject::Chunk(1),
        fixtures::fingerprints(),
    )
    .expect_err("no receipt");
    assert!(
        error.to_string().contains("no review receipt exists"),
        "{error}"
    );
    assert!(
        complete::read_ledger(&repo.completion_ledger())
            .expect("ledger")
            .is_empty()
    );
}

#[test]
fn on_disk_attempt_round_trips_and_detects_tampering() {
    let dir = scratch("attempt");
    let repo = Repo::new(&dir);
    let inputs = fixtures::valid();
    let attempt = dir.join("docs/planning/reviews/C01/attempt-01");
    std::fs::create_dir_all(&attempt).expect("mkdir");
    let mut packet: gate::Packet = serde_json::from_slice(&inputs.packet_bytes).expect("packet");
    packet.verdict_path = attempt.join("verdict.json").display().to_string();
    let packet_bytes = fixtures::to_bytes(&packet);
    let write = |relative: &str, bytes: &[u8]| {
        let path = dir.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, bytes).expect("write");
    };
    std::fs::write(attempt.join("request.json"), &packet_bytes).expect("write");
    std::fs::write(
        attempt.join("verdict.json"),
        inputs.verdict_bytes.as_ref().expect("verdict"),
    )
    .expect("write");
    write(
        "docs/planning/reviews/C01/attempt-01/thread-reply.jsonl",
        inputs.raw_output_bytes.as_ref().expect("raw"),
    );
    for (run_ref, bytes) in &inputs.runs {
        write(&run_ref.path, bytes.as_ref().expect("run"));
    }
    for (path, bytes) in &inputs.logs {
        write(path, bytes);
    }
    for (reference, bytes) in &inputs.predecessors {
        write(
            &reference.receipt.path,
            bytes.as_ref().expect("predecessor"),
        );
    }
    let native = packet.native_acceptance.as_ref().expect("native");
    write(&native.path, inputs.native_bytes.as_ref().expect("native"));
    let mut receipt = receipt_of(&inputs);
    receipt.packet_sha256 = sha(&packet_bytes);
    std::fs::write(attempt.join("receipt.json"), fixtures::to_bytes(&receipt)).expect("write");

    let base = || NativeBase {
        required_checks: fixtures::required_checks(),
        fixture_identifier: fixtures::FIXTURE_ID.to_owned(),
        prepared_bundle: Some(fixtures::BUNDLE.to_owned()),
        prepared_bundle_sha256: Some(fixtures::BUNDLE_SHA.to_owned()),
        prepared_identifier: Some(fixtures::FIXTURE_ID.to_owned()),
        current_executable_sha256: Some(fixtures::EXECUTABLE.to_owned()),
        current_bundle_sha256: Some(fixtures::BUNDLE_SHA.to_owned()),
        current_identifier: Some(fixtures::FIXTURE_ID.to_owned()),
    };
    let load = || {
        gate::load(
            repo.root(),
            &attempt.join("receipt.json"),
            fixtures::fingerprints(),
            fixtures::plans(),
            Some(base()),
        )
        .expect("load")
    };
    assert_eq!(
        gate::evaluate(&load()),
        Vec::<String>::new(),
        "on-disk attempt passes"
    );

    std::fs::write(attempt.join("expired.json"), "{}").expect("write");
    assert!(
        gate::evaluate(&load())
            .iter()
            .any(|f| f.contains("expired"))
    );
    std::fs::remove_file(attempt.join("expired.json")).expect("rm");

    let (log_path, _) = inputs.logs.iter().next().expect("log");
    write(log_path, b"tampered");
    assert!(
        gate::evaluate(&load())
            .iter()
            .any(|f| f.contains("does not match its hash"))
    );
    write(log_path, &inputs.logs[log_path]);

    let (predecessor, _) = &inputs.predecessors[0];
    write(
        &predecessor.receipt.path,
        b"{\"gate\":\"G01\",\"passed\":false}",
    );
    assert!(
        gate::evaluate(&load())
            .iter()
            .any(|f| f.contains("predecessor G01 receipt was changed"))
    );
    std::fs::remove_file(dir.join(&predecessor.receipt.path)).expect("rm");
    assert!(
        gate::evaluate(&load())
            .iter()
            .any(|f| f.contains("predecessor G01 receipt is missing"))
    );
    write(&predecessor.receipt.path, fixtures::PREDECESSOR);

    let mut verdict: Verdict =
        serde_json::from_slice(inputs.verdict_bytes.as_ref().expect("verdict")).expect("v");
    verdict.optional_suggestions.clear();
    std::fs::write(attempt.join("verdict.json"), fixtures::to_bytes(&verdict)).expect("write");
    assert!(
        gate::evaluate(&load())
            .iter()
            .any(|f| f.contains("edited verdict"))
    );
}

// ---- R1: the run evidence must match the approved command plan ----

fn with_runs(edit: impl Fn(&mut Vec<xtask::runner::RunRecord>)) -> GateInputs {
    let mut runs = vec![
        fixtures::run(RunKind::Check),
        fixtures::run(RunKind::Test),
        fixtures::run(RunKind::NativePrepare),
    ];
    edit(&mut runs);
    fixtures::assemble(&fixtures::verdict(), &runs, &fixtures::native())
}

#[test]
fn run_records_must_cover_the_exact_approved_plan() {
    rejected(
        &with_runs(|r| r[0].commands.clear()),
        "recorded 0 commands; the approved plan has 2",
    );
    rejected(
        &with_runs(|r| {
            r[0].commands.pop();
        }),
        "recorded 1 commands",
    );
    rejected(
        &with_runs(|r| {
            let c = r[0].commands[0].clone();
            r[0].commands.push(c);
        }),
        "recorded 3 commands",
    );
    rejected(
        &with_runs(|r| {
            r[1].commands[0].argv = vec![
                "cargo".into(),
                "test".into(),
                "--test".into(),
                "other".into(),
            ]
        }),
        "is not the approved C01/fixture",
    );
    rejected(
        &with_runs(|r| r[0].commands.swap(0, 1)),
        "is not the approved common/1",
    );
}

#[test]
fn run_identity_exit_status_tests_and_logs_are_verified() {
    rejected(
        &with_runs(|r| r[0].subject = "C02".into()),
        "identity does not match its reference",
    );
    rejected(
        &with_runs(|r| r[1].schema = "other".into()),
        "identity does not match its reference",
    );
    rejected(
        &with_runs(|r| r[1].kind = RunKind::Check),
        "expected exactly one Check run",
    );
    rejected(
        &with_runs(|r| r[1].commands[0].exit_code = Some(1)),
        "did not exit successfully",
    );
    rejected(
        &with_runs(|r| r[1].commands[0].exit_code = None),
        "did not exit successfully",
    );
    rejected(
        &with_runs(|r| {
            r[1].commands[0].tests = Some(xtask::runner::TestCounts {
                passed: 0,
                failed: 0,
            })
        }),
        "executed no passing tests",
    );
    rejected(
        &with_runs(|r| r[1].commands[0].tests = None),
        "executed no passing tests",
    );
    rejected(
        &with_runs(|r| r[2].artifacts.clear()),
        "does not record the packet's prepared executable",
    );

    let mut inputs = fixtures::valid();
    let key = inputs.logs.keys().next().expect("log").clone();
    inputs.logs.insert(key, b"edited".to_vec());
    rejected(&inputs, "does not match its hash");
    let mut inputs = fixtures::valid();
    inputs.logs.clear();
    rejected(&inputs, "is missing");
}

#[test]
fn run_ids_must_match_their_references() {
    let mut inputs = fixtures::valid();
    let mut packet: gate::Packet = serde_json::from_slice(&inputs.packet_bytes).expect("packet");
    packet.runs[0].run_id = "substituted".into();
    inputs.packet_bytes = fixtures::to_bytes(&packet);
    inputs.runs[0].0.run_id = "substituted".into();
    let mut receipt = receipt_of(&inputs);
    receipt.packet_sha256 = sha(&inputs.packet_bytes);
    set_receipt(&mut inputs, &receipt);
    rejected(&inputs, "identity does not match its reference");
}

// ---- R2: native acceptance contract and current prepared bytes ----

fn with_native(edit: impl Fn(&mut xtask::evidence::NativeAcceptance)) -> GateInputs {
    let mut native = fixtures::native();
    edit(&mut native);
    fixtures::assemble(
        &fixtures::verdict(),
        &[
            fixtures::run(RunKind::Check),
            fixtures::run(RunKind::Test),
            fixtures::run(RunKind::NativePrepare),
        ],
        &native,
    )
}

#[test]
fn native_record_must_match_the_required_inventory_and_identity() {
    rejected(
        &with_native(|n| {
            n.checks.pop();
        }),
        "required native check C01-N02 is missing",
    );
    rejected(
        &with_native(|n| {
            let c = n.checks[0].clone();
            n.checks.push(c);
        }),
        "appears more than once",
    );
    rejected(
        &with_native(|n| n.checks[0].id = "C01-N99".into()),
        "not in the required inventory",
    );
    rejected(
        &with_native(|n| n.checks[0].observation = "  ".into()),
        "has no observation",
    );
    rejected(
        &with_native(|n| n.checks[0].evidence.clear()),
        "cites no evidence",
    );
    rejected(
        &with_native(|n| n.checks[0].evidence = vec!["docs/elsewhere.png".into()]),
        "does not hash-bind",
    );
    rejected(
        &with_native(|n| n.app.identifier = "io.cdot.prisma-airs-desktop.v2".into()),
        "must use the fixture identity",
    );
    rejected(
        &with_native(|n| n.app.fixture = false),
        "must use the fixture identity",
    );
    rejected(
        &with_native(|n| n.subject = "C02".into()),
        "native record is for C02",
    );
}

#[test]
fn changed_or_missing_prepared_bytes_invalidate_native_acceptance() {
    let mut inputs = fixtures::valid();
    if let Some(native) = inputs.expect.native.as_mut() {
        native.current_executable_sha256 = Some("0".repeat(64));
    }
    rejected(&inputs, "prepared fixture bytes changed");
    let mut inputs = fixtures::valid();
    if let Some(native) = inputs.expect.native.as_mut() {
        native.current_executable_sha256 = None;
    }
    rejected(&inputs, "no longer present");
}

// ---- R3: reviewer completion and final-output provenance ----

#[test]
fn failed_or_incomplete_reviewer_turns_cannot_complete() {
    let mut inputs = fixtures::valid();
    let mut receipt = receipt_of(&inputs);
    receipt.transport.exit_code = Some(1);
    set_receipt(&mut inputs, &receipt);
    rejected(&inputs, "transport exited with Some(1)");

    let mut inputs = fixtures::valid();
    let mut receipt = receipt_of(&inputs);
    receipt.transport.timed_out = true;
    set_receipt(&mut inputs, &receipt);
    rejected(&inputs, "timed out");

    let reseal = |inputs: &mut GateInputs, raw: Vec<u8>| {
        let mut receipt = receipt_of(inputs);
        if let Some(r) = receipt.transport.raw_output.as_mut() {
            r.sha256 = sha(&raw);
        }
        set_receipt(inputs, &receipt);
        inputs.raw_output_bytes = Some(raw);
    };
    let mut inputs = fixtures::valid();
    let progress = br#"{"timestamp":"2026-10-17T00:05:00Z","type":"event_msg","payload":{"type":"agent_message","message":"working on rq-fixture"}}"#.to_vec();
    reseal(&mut inputs, progress);
    rejected(&inputs, "no completed reviewer turn");

    let mut inputs = fixtures::valid();
    let mut other = fixtures::verdict();
    other.scores.overall = 10;
    reseal(&mut inputs, fixtures::thread_reply(&other));
    rejected(&inputs, "final output does not equal verdict.json");

    let mut inputs = fixtures::valid();
    let mut early = String::from_utf8(fixtures::thread_reply(&fixtures::verdict())).expect("utf8");
    early = early.replace("2026-10-17T00:05:00Z", "2026-10-16T00:00:00Z");
    reseal(&mut inputs, early.into_bytes());
    rejected(&inputs, "no completed reviewer turn");
}

#[test]
fn fresh_exec_requires_a_completed_turn_whose_final_message_is_the_verdict() {
    let verdict_text = serde_json::to_string(&fixtures::verdict()).expect("json");
    let item = serde_json::json!({"type": "item.completed", "item": {"type": "agent_message", "text": verdict_text}});
    let good = format!(
        "{item}\n{}\n",
        serde_json::json!({"type": "turn.completed"})
    );
    let exec_inputs = |raw: String| {
        let mut inputs = fixtures::valid();
        let mut receipt = receipt_of(&inputs);
        receipt.transport.kind = "codex-exec-fresh".into();
        receipt.transport.thread = None;
        if let Some(r) = receipt.transport.raw_output.as_mut() {
            r.sha256 = sha(raw.as_bytes());
        }
        set_receipt(&mut inputs, &receipt);
        inputs.raw_output_bytes = Some(raw.into_bytes());
        inputs
    };
    assert_eq!(
        gate::evaluate(&exec_inputs(good.clone())),
        Vec::<String>::new(),
        "genuine fresh result passes"
    );
    rejected(&exec_inputs(format!("{item}\n")), "never completed");
    rejected(
        &exec_inputs(format!(
            "{good}{}\n",
            serde_json::json!({"type": "turn.failed"})
        )),
        "turn failed",
    );
}

// ---- R4: ledger generations, invalidation and predecessor integrity ----

fn passing_receipt(id: &str) -> Vec<u8> {
    let mut receipt = receipt_of(&fixtures::valid());
    receipt.receipt_id = id.to_owned();
    fixtures::to_bytes(&receipt)
}

fn ledger_line(dir: &Path, entry: &complete::Entry) {
    use std::io::Write;
    let path = dir.join("docs/planning/evidence/completions.jsonl");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("open");
    writeln!(file, "{}", serde_json::to_string(entry).expect("json")).expect("write");
}

fn completed(dir: &Path, subject: &str, generation: u32, receipt_id: &str) {
    let relative = format!("docs/planning/reviews/{subject}/attempt-0{generation}/receipt.json");
    let bytes = passing_receipt(receipt_id);
    std::fs::create_dir_all(dir.join(&relative).parent().expect("parent")).expect("mkdir");
    std::fs::write(dir.join(&relative), &bytes).expect("write");
    ledger_line(
        dir,
        &complete::Entry::Completion {
            subject: subject.into(),
            generation,
            receipt_id: receipt_id.into(),
            receipt: relative,
            receipt_sha256: sha(&bytes),
            fingerprints: fixtures::fingerprints(),
            completed_utc: "t".into(),
            owner_acceptance: None,
        },
    );
    ledger_line(
        dir,
        &complete::Entry::Commit {
            subject: subject.into(),
            generation,
            receipt_id: receipt_id.into(),
            commit: "c".into(),
            tree: "t".into(),
            source: "s".into(),
            verified_utc: "t".into(),
        },
    );
}

#[test]
fn ledger_generations_invalidation_and_receipt_integrity() {
    let dir = scratch("generations");
    let repo = Repo::new(&dir);
    let manifest = Manifest::load(&self::repo().manifest()).expect("manifest");
    completed(&dir, "C01", 1, "r-c01-1");
    let ledger = || complete::read_ledger(&repo.completion_ledger()).expect("ledger");
    assert!(complete::is_completed(&repo, &ledger(), Subject::Chunk(1)));
    complete::require_predecessors(&repo, &manifest, Subject::Chunk(2))
        .expect("C02 may start after C01");

    // A tampered or deleted predecessor receipt blocks dependents.
    let receipt = dir.join("docs/planning/reviews/C01/attempt-01/receipt.json");
    std::fs::write(&receipt, b"{}").expect("tamper");
    assert!(complete::require_predecessors(&repo, &manifest, Subject::Chunk(2)).is_err());
    std::fs::remove_file(&receipt).expect("rm");
    assert!(
        complete::completion_problems(&repo, &ledger(), Subject::Chunk(1))
            .iter()
            .any(|p| p.contains("missing"))
    );
    std::fs::write(&receipt, passing_receipt("r-c01-1")).expect("restore");

    // A completion without a commit bound to the same receipt is incomplete.
    completed(&dir, "C02", 1, "r-c02-1");
    ledger_line(
        &dir,
        &complete::Entry::Completion {
            subject: "C03".into(),
            generation: 1,
            receipt_id: "r-c03-1".into(),
            receipt: "docs/planning/reviews/C01/attempt-01/receipt.json".into(),
            receipt_sha256: sha(&passing_receipt("r-c01-1")),
            fingerprints: fixtures::fingerprints(),
            completed_utc: "t".into(),
            owner_acceptance: None,
        },
    );
    ledger_line(
        &dir,
        &complete::Entry::Commit {
            subject: "C03".into(),
            generation: 1,
            receipt_id: "some-other".into(),
            commit: "c".into(),
            tree: "t".into(),
            source: "s".into(),
            verified_utc: "t".into(),
        },
    );
    assert!(
        complete::completion_problems(&repo, &ledger(), Subject::Chunk(3))
            .iter()
            .any(|p| p.contains("no verified commit"))
    );

    // Reopening C01 invalidates it and every later completed subject.
    let written =
        complete::reopen(&repo, &manifest, Subject::Chunk(1), "dependency update").expect("reopen");
    let subjects: Vec<String> = written
        .iter()
        .filter_map(|e| match e {
            complete::Entry::Invalidation { subject, .. } => Some(subject.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(subjects, vec!["C01", "C02", "C03"]);
    assert!(!complete::is_completed(&repo, &ledger(), Subject::Chunk(1)));
    assert!(
        complete::require_predecessors(&repo, &manifest, Subject::Chunk(2)).is_err(),
        "old generation cannot unlock C02"
    );

    // A new generation restores completion; the old receipt ID cannot be reused.
    completed(&dir, "C01", 2, "r-c01-2");
    assert!(complete::is_completed(&repo, &ledger(), Subject::Chunk(1)));
    assert!(
        !complete::is_completed(&repo, &ledger(), Subject::Chunk(2)),
        "dependents stay reopened until re-reviewed"
    );
}

// ---- R5: consumed shared source is bound and verified ----

/// Identical input, scratch and installed copies of the consumed shared
/// packages other than presentation (learner since C05, shared).
fn stage_other_shared(dir: &Path) {
    for name in ["learner", "shared"] {
        for root in [".inputs/academy", ".work/academy"] {
            let path = dir.join(root).join(format!("packages/{name}/src/index.ts"));
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, "export const y = 1").expect("write");
        }
        let installed = dir.join(format!(
            "node_modules/.pnpm/{name}/node_modules/@ai-security-academy/{name}"
        ));
        std::fs::create_dir_all(installed.join("src")).expect("mkdir");
        std::fs::write(installed.join("src/index.ts"), "export const y = 1").expect("write");
        std::fs::create_dir_all(dir.join("apps/desktop/node_modules/@ai-security-academy"))
            .expect("mkdir");
        std::os::unix::fs::symlink(
            &installed,
            dir.join(format!(
                "apps/desktop/node_modules/@ai-security-academy/{name}"
            )),
        )
        .expect("link");
    }
}

#[test]
fn edits_to_consumed_shared_source_are_detected() {
    let dir = scratch("shared");
    let repo = Repo::new(&dir);
    let file = |root: &str| dir.join(root).join("packages/presentation/src/index.ts");
    for root in [".inputs/academy", ".work/academy"] {
        std::fs::create_dir_all(file(root).parent().expect("parent")).expect("mkdir");
        std::fs::write(file(root), "export const x = 1").expect("write");
    }
    let installed =
        dir.join("node_modules/.pnpm/presentation/node_modules/@ai-security-academy/presentation");
    std::fs::create_dir_all(installed.join("src")).expect("mkdir");
    std::fs::write(installed.join("src/index.ts"), "export const x = 1").expect("write");
    std::fs::create_dir_all(dir.join("apps/desktop/node_modules/@ai-security-academy"))
        .expect("mkdir");
    std::os::unix::fs::symlink(
        &installed,
        dir.join("apps/desktop/node_modules/@ai-security-academy/presentation"),
    )
    .expect("link");
    stage_other_shared(&dir);
    fingerprint::verify_consumed_shared(&repo).expect("identical copies verify");
    let before = fingerprint::dependencies(&repo, "toolchain").expect("fp");

    std::fs::write(file(".work/academy"), "export const x = 2").expect("edit scratch");
    assert!(
        fingerprint::verify_consumed_shared(&repo).is_err(),
        "scratch edit detected"
    );
    assert_ne!(
        fingerprint::dependencies(&repo, "toolchain").expect("fp"),
        before,
        "scratch edit changes the dependency fingerprint"
    );
    std::fs::write(file(".work/academy"), "export const x = 1").expect("restore");

    std::fs::write(installed.join("src/index.ts"), "export const x = 3").expect("edit installed");
    assert!(
        fingerprint::verify_consumed_shared(&repo).is_err(),
        "installed copy edit detected"
    );
    assert_ne!(
        fingerprint::dependencies(&repo, "toolchain").expect("fp"),
        before
    );
}

#[test]
fn shared_staging_paths_cannot_escape_their_roots() {
    use xtask::shared::confined;
    let dir = scratch("confine");
    let repo = Repo::new(&dir);
    std::fs::create_dir_all(dir.join(".work/academy")).expect("mkdir");
    std::fs::create_dir_all(dir.join("apps")).expect("mkdir");
    assert!(confined(&repo, Path::new(".work/academy"), ".work").is_ok());
    assert!(
        confined(&repo, Path::new(".work/../apps"), ".work").is_err(),
        "parent components"
    );
    assert!(
        confined(&repo, Path::new("/tmp/x"), ".work").is_err(),
        "absolute paths"
    );
    assert!(
        confined(&repo, Path::new(".inputs/academy"), ".work").is_err(),
        "wrong root"
    );
    let outside = scratch("confine-outside");
    std::os::unix::fs::symlink(&outside, dir.join(".work/escape")).expect("link");
    assert!(
        confined(&repo, Path::new(".work/escape/academy"), ".work").is_err(),
        "symlinked ancestor"
    );

    // Chained symlinks inside an input must resolve within it.
    std::fs::create_dir_all(dir.join(".inputs/academy/pkg")).expect("mkdir");
    std::fs::write(dir.join(".inputs/academy/pkg/file"), "x").expect("write");
    std::os::unix::fs::symlink("pkg", dir.join(".inputs/academy/a")).expect("link");
    std::os::unix::fs::symlink("a/file", dir.join(".inputs/academy/b")).expect("link");
    std::fs::write(dir.join("docs-architecture-placeholder"), "").expect("write");
    std::os::unix::fs::symlink(
        "../../docs-architecture-placeholder",
        dir.join(".inputs/academy/c"),
    )
    .expect("link");
    let err = xtask::shared::prepare(
        &repo,
        Path::new(".inputs/academy"),
        Path::new(".work/academy"),
    )
    .expect_err("escape");
    assert!(
        err.to_string().contains("escapes") || err.to_string().contains("inputs.lock"),
        "{err}"
    );
}

// ---- attempt-02 R3: destructive staging is confined end to end ----

#[test]
fn shared_prepare_rejects_escapes_before_touching_anything() {
    let dir = scratch("prepare-e2e");
    let repo = Repo::new(&dir);
    std::fs::create_dir_all(dir.join(".inputs/academy/src")).expect("mkdir");
    std::fs::write(dir.join(".inputs/academy/src/a.ts"), "export {}").expect("write");
    std::fs::write(dir.join(".inputs/academy/.env.example"), "X=1").expect("write");
    let tree = fingerprint::tree_hash(&dir.join(".inputs/academy")).expect("hash");
    std::fs::create_dir_all(dir.join("docs/architecture")).expect("mkdir");
    std::fs::write(
        dir.join("docs/architecture/inputs.lock.json"),
        format!(r#"{{"schema":"airs-v2/inputs/1","academy":{{"origin":"o","branch":"b","base_commit":"c","commit":"d","tree_sha256":"{tree}","package_version":"1"}}}}"#),
    )
    .expect("lock");
    std::fs::create_dir_all(dir.join("apps")).expect("mkdir");
    std::fs::write(dir.join("apps/sentinel.txt"), "keep").expect("write");
    let outside = scratch("prepare-e2e-outside");
    std::fs::write(outside.join("sentinel.txt"), "keep").expect("write");
    std::fs::create_dir_all(dir.join(".work")).expect("mkdir");
    std::os::unix::fs::symlink(&outside, dir.join(".work/escape")).expect("link");

    let prepare = |scratch: &str| {
        xtask::shared::prepare(&repo, Path::new(".inputs/academy"), Path::new(scratch))
    };
    for bad in [
        ".work/../apps",
        "/tmp/elsewhere",
        ".work/escape/academy",
        ".work",
        ".inputs/academy",
    ] {
        assert!(prepare(bad).is_err(), "{bad} must be refused");
    }
    assert!(
        xtask::shared::prepare(
            &repo,
            Path::new(".work/../.inputs/academy"),
            Path::new(".work/academy")
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("apps/sentinel.txt")).expect("sentinel"),
        "keep"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("sentinel.txt")).expect("sentinel"),
        "keep"
    );

    prepare(".work/academy").expect("confined staging succeeds");
    assert_eq!(
        std::fs::read_to_string(dir.join(".work/academy/src/a.ts")).expect("staged"),
        "export {}"
    );
    assert!(
        !dir.join(".work/academy/.env.example").exists(),
        ".env files are never staged"
    );
}

// ---- attempt-02 R2: the Academy web consumer scratch tree is bound ----

#[test]
fn edits_to_the_web_consumer_scratch_tree_are_detected() {
    let dir = scratch("web-scratch");
    let repo = Repo::new(&dir);
    for root in [".inputs/academy", ".work/academy"] {
        for (rel, body) in [
            ("src/components/shell/app-shell.tsx", "a"),
            ("vitest.presentation.config.ts", "b"),
            ("packages/presentation/src/index.ts", "c"),
        ] {
            let path = dir.join(root).join(rel);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(path, body).expect("write");
        }
    }
    let installed =
        dir.join("node_modules/.pnpm/p/node_modules/@ai-security-academy/presentation/src");
    std::fs::create_dir_all(&installed).expect("mkdir");
    std::fs::write(installed.join("index.ts"), "c").expect("write");
    std::fs::create_dir_all(dir.join("apps/desktop/node_modules/@ai-security-academy"))
        .expect("mkdir");
    std::os::unix::fs::symlink(
        installed.parent().expect("pkg"),
        dir.join("apps/desktop/node_modules/@ai-security-academy/presentation"),
    )
    .expect("link");
    // Documented generated outputs may appear in scratch.
    std::fs::create_dir_all(dir.join(".work/academy/dist")).expect("mkdir");
    std::fs::write(dir.join(".work/academy/dist/index.html"), "built").expect("write");
    stage_other_shared(&dir);
    fingerprint::verify_consumed_shared(&repo).expect("generated outputs are excluded");
    let before = fingerprint::dependencies(&repo, "toolchain").expect("fp");
    for rel in [
        "src/components/shell/app-shell.tsx",
        "vitest.presentation.config.ts",
    ] {
        let path = dir.join(".work/academy").join(rel);
        std::fs::write(&path, "edited").expect("edit");
        assert!(
            fingerprint::verify_consumed_shared(&repo).is_err(),
            "{rel} edit detected"
        );
        assert_ne!(
            fingerprint::dependencies(&repo, "toolchain").expect("fp"),
            before,
            "{rel} edit changes the fingerprint"
        );
        std::fs::write(&path, if rel.starts_with("src") { "a" } else { "b" }).expect("restore");
    }
    std::fs::write(dir.join(".work/academy/src/extra.ts"), "x").expect("add");
    assert!(
        fingerprint::verify_consumed_shared(&repo).is_err(),
        "added scratch file detected"
    );
}

// ---- attempt-02 R4: the whole prepared bundle and its identity ----

#[test]
fn prepared_bundle_bytes_path_and_identifier_are_bound() {
    let mut inputs = fixtures::valid();
    inputs
        .expect
        .native
        .as_mut()
        .expect("native")
        .current_bundle_sha256 = Some("0".repeat(64));
    rejected(&inputs, "bundle changed or is missing");
    let mut inputs = fixtures::valid();
    inputs
        .expect
        .native
        .as_mut()
        .expect("native")
        .current_bundle_sha256 = None;
    rejected(&inputs, "bundle changed or is missing");
    let mut inputs = fixtures::valid();
    inputs
        .expect
        .native
        .as_mut()
        .expect("native")
        .current_identifier = Some("io.cdot.prisma-airs-desktop.v2".into());
    rejected(&inputs, "CFBundleIdentifier is not the fixture identity");
    rejected(
        &with_native(|n| n.app.bundle = "target/other.app".into()),
        "names a bundle other than the prepared fixture",
    );
    rejected(
        &with_runs(|r| r[2].artifacts.retain(|a| a.name != "fixture-bundle")),
        "does not record the prepared fixture bundle",
    );
}

// ---- attempt-02 R5: phases need current integrated native evidence ----

#[test]
fn phases_require_integrated_native_runs_and_acceptance() {
    let manifest = Manifest::load(&repo().manifest()).expect("manifest");
    assert!(complete::required_runs(Subject::Phase(1)).contains(&RunKind::NativePrepare));
    let native = manifest
        .plan(Subject::Phase(1), RunKind::NativePrepare)
        .expect("plan");
    let last = native.last().expect("native-prepare");
    assert!(
        last.argv
            .windows(2)
            .any(|w| w[0] == "--phase" && w[1] == "P01"),
        "{:?}",
        last.argv
    );
    // A consistent all-nine attempt without native evidence fails.
    let mut inputs = fixtures::valid();
    let mut packet: gate::Packet = serde_json::from_slice(&inputs.packet_bytes).expect("packet");
    packet.native_acceptance = None;
    inputs.packet_bytes = fixtures::to_bytes(&packet);
    let mut receipt = receipt_of(&inputs);
    receipt.packet_sha256 = sha(&inputs.packet_bytes);
    set_receipt(&mut inputs, &receipt);
    rejected(&inputs, "missing native acceptance evidence");
    // The phase's required checks are every member chunk's inventory.
    let members: Vec<String> = manifest
        .phase_chunks(Subject::Phase(1))
        .expect("P01")
        .iter()
        .map(Subject::to_string)
        .collect();
    let inventory = repo().join("docs/architecture/native-checks.json");
    let (required, _) =
        xtask::evidence::required_checks(&inventory, &members).expect("P01 inventories");
    for member in ["C01", "C02", "C03"] {
        assert!(
            required
                .iter()
                .any(|id| id.starts_with(&format!("{member}-N"))),
            "{member}'s checks are part of P01: {required:?}"
        );
    }
    // A phase fails closed while any member has no inventory yet.
    let later: Vec<String> = manifest
        .phase_chunks(Subject::Phase(3))
        .expect("P03")
        .iter()
        .map(Subject::to_string)
        .collect();
    let error =
        xtask::evidence::required_checks(&inventory, &later).expect_err("P03 inventories pending");
    // The first member still without an inventory (it moves as chunks land).
    let inventories: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&inventory).expect("inventory")).expect("json");
    let pending = later
        .iter()
        .find(|c| inventories["chunks"].get(c.as_str()).is_none())
        .expect("a P03 member without an inventory");
    assert!(
        error.to_string().contains(pending.as_str()),
        "a phase fails closed until each member has an inventory: {error}"
    );
}

#[test]
fn native_status_and_completion_resolve_phase_members_alike() {
    let manifest = Manifest::load(&repo().manifest()).expect("manifest");
    assert_eq!(
        manifest.native_members(Subject::Phase(1)).expect("P01"),
        vec!["C01", "C02", "C03"]
    );
    assert_eq!(
        manifest.native_members(Subject::Chunk(2)).expect("C02"),
        vec!["C02"]
    );
    // A phase's native status covers every member inventory, as completion
    // does, instead of looking for a P01 inventory.
    let dir = scratch("phase-native");
    let repo = Repo::new(&dir);
    let checks = |chunk: &str| serde_json::json!([{ "id": format!("{chunk}-N01"), "check": "x" }]);
    let inventory = serde_json::json!({
        "schema": "airs-v2/native-checks/1",
        "description": "test",
        "fixture_identifier": "io.example.fixture",
        "chunks": { "C01": checks("C01"), "C02": checks("C02"), "C03": checks("C03") },
    });
    std::fs::create_dir_all(dir.join("docs/architecture")).expect("mkdir");
    std::fs::write(
        dir.join("docs/architecture/native-checks.json"),
        inventory.to_string(),
    )
    .expect("write");
    let evidence = dir.join("docs/planning/evidence/P01");
    std::fs::create_dir_all(&evidence).expect("mkdir");
    std::fs::write(evidence.join("capture.png"), b"png").expect("write");
    let record = serde_json::json!({
        "schema": "airs-v2/native-acceptance/1",
        "subject": "P01",
        "recorded_utc": "2026-10-17T00:00:00Z",
        "method": "cua",
        "performed_by": "test",
        "host": "test",
        "app": { "bundle": "b.app", "identifier": "io.example.fixture", "executable_sha256": "x", "fixture": true },
        "checks": [{
            "id": "C01-N01", "description": "d", "result": "pass", "observation": "o",
            "evidence": ["docs/planning/evidence/P01/capture.png"],
        }],
        "limitations": [],
    });
    std::fs::write(evidence.join("native-acceptance.json"), record.to_string()).expect("write");
    let reasons = xtask::native::acceptance_status(&repo, &manifest, Subject::Phase(1), "x");
    assert!(
        reasons
            .iter()
            .all(|r| !r.contains("no native check inventory")),
        "{reasons:?}"
    );
    for missing in ["C02-N01", "C03-N01"] {
        assert!(
            reasons
                .iter()
                .any(|r| r.contains(&format!("required native check {missing} is missing"))),
            "{reasons:?}"
        );
    }
    assert!(
        reasons
            .iter()
            .all(|r| !r.contains("C01-N01 is not in the required inventory")),
        "{reasons:?}"
    );
}

#[test]
fn thread_wait_requires_a_completed_turn_not_progress() {
    let progress = br#"{"timestamp":"t","type":"event_msg","payload":{"type":"agent_message","message":"reviewing rq-1"}}"#;
    assert!(!transport::has_completed_turn(progress));
    let done = fixtures::thread_reply(&fixtures::verdict());
    let mut both = progress.to_vec();
    both.push(b'\n');
    both.extend_from_slice(&done);
    assert!(transport::has_completed_turn(&both));
}

#[test]
fn owner_acceptance_takes_only_the_threshold_and_is_honored_only_when_recorded() {
    let eight = |receipt: &mut Receipt| {
        receipt.scores.architecture = 8;
        receipt.scores.behavior = 8;
        receipt.scores.authority_security = 8;
        receipt.scores.validation = 8;
        receipt.scores.overall = 8;
        receipt.required_fixes = 2;
        receipt.passed = false;
    };
    let mut receipt = receipt_of(&fixtures::valid());
    eight(&mut receipt);
    let threshold: Vec<String> = [
        "score architecture=8 is below 9",
        "score behavior=8 is below 9",
        "score authority_security=8 is below 9",
        "score validation=8 is below 9",
        "score overall=8 is below 9",
        "2 required fixes remain",
        "reviewer recommendation is fail",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    // All 8s with required fixes: the threshold alone may be accepted.
    assert_eq!(
        complete::owner_acceptable(&receipt, &threshold),
        Ok(threshold.clone())
    );
    // Anything else failing is never accepted.
    let mut tampered = threshold.clone();
    tampered.push("verdict bytes do not match the receipt (edited verdict)".into());
    assert!(complete::owner_acceptable(&receipt, &tampered).is_err());
    let mut stale = threshold.clone();
    stale.push("source changed since review (stale receipt)".into());
    assert!(complete::owner_acceptable(&receipt, &stale).is_err());
    // Below 8, or with an evidence gap or blocker: refused.
    let mut seven = receipt.clone();
    seven.scores.validation = 7;
    assert!(complete::owner_acceptable(&seven, &threshold).is_err());
    // Evidence gaps may be accepted with the threshold (recorded), blockers never.
    let mut gap = receipt.clone();
    gap.evidence_gaps = 1;
    let mut with_gap = threshold.clone();
    with_gap.push("1 evidence gaps remain".into());
    assert_eq!(
        complete::owner_acceptable(&gap, &with_gap),
        Ok(with_gap.clone())
    );
    assert!(complete::owner_acceptable(&gap, &threshold).is_ok());
    let mut blocker = receipt.clone();
    blocker.blockers = 1;
    assert!(complete::owner_acceptable(&blocker, &threshold).is_err());

    // In the ledger: an 8/10 receipt counts only with the recorded acceptance.
    let dir = scratch("owner-acceptance");
    let repo = Repo::new(&dir);
    let relative = "docs/planning/reviews/C01/attempt-01/receipt.json".to_owned();
    let bytes = fixtures::to_bytes(&receipt);
    std::fs::create_dir_all(dir.join(&relative).parent().expect("parent")).expect("mkdir");
    std::fs::write(dir.join(&relative), &bytes).expect("write");
    let completion = |owner: Option<complete::OwnerAcceptance>, generation: u32, id: &str| {
        complete::Entry::Completion {
            subject: "C01".into(),
            generation,
            receipt_id: id.into(),
            receipt: relative.clone(),
            receipt_sha256: sha(&bytes),
            fingerprints: fixtures::fingerprints(),
            completed_utc: "t".into(),
            owner_acceptance: owner,
        }
    };
    let commit = |generation: u32, id: &str| complete::Entry::Commit {
        subject: "C01".into(),
        generation,
        receipt_id: id.into(),
        commit: "c".into(),
        tree: "t".into(),
        source: "s".into(),
        verified_utc: "t".into(),
    };
    ledger_line(&dir, &completion(None, 1, "r-1"));
    ledger_line(&dir, &commit(1, "r-1"));
    let ledger = || complete::read_ledger(&repo.completion_ledger()).expect("ledger");
    assert!(
        !complete::is_completed(&repo, &ledger(), Subject::Chunk(1)),
        "8/10 without acceptance"
    );
    let accepted = complete::OwnerAcceptance {
        decision: "D032".into(),
        reason: "owner accepted 8/10".into(),
        minimum_score: complete::OWNER_MINIMUM_SCORE,
        accepted_failures: threshold,
    };
    ledger_line(&dir, &completion(Some(accepted), 2, "r-2"));
    ledger_line(&dir, &commit(2, "r-2"));
    assert!(
        complete::is_completed(&repo, &ledger(), Subject::Chunk(1)),
        "8/10 with the owner's acceptance"
    );
}

#[test]
fn d034_phases_need_nine_and_every_eight_chunk_is_raised_before_sign_off() {
    let dir = scratch("d034");
    let repo = Repo::new(&dir);
    let manifest = Manifest::load(&self::repo().manifest()).expect("manifest");
    // A phase review is never owner-accepted.
    let error = complete::complete_with(
        &repo,
        &manifest,
        Subject::Phase(1),
        fixtures::fingerprints(),
        Some("good enough"),
    )
    .expect_err("phase owner acceptance");
    assert!(error.to_string().contains("D034"), "{error}");
    // A phase whose chunk was accepted at 8/10 cannot complete until that
    // chunk passes a 9/10 review.
    let completion = |subject: &str, owner: bool| complete::Entry::Completion {
        subject: subject.into(),
        generation: 1,
        receipt_id: format!("r-{subject}"),
        receipt: format!("docs/planning/reviews/{subject}/attempt-01/receipt.json"),
        receipt_sha256: "0".repeat(64),
        fingerprints: fixtures::fingerprints(),
        completed_utc: "t".into(),
        owner_acceptance: owner.then(|| complete::OwnerAcceptance {
            decision: "D034".into(),
            reason: "chunk gate 8/10".into(),
            minimum_score: complete::OWNER_MINIMUM_SCORE,
            accepted_failures: vec!["score overall=8 is below 9".into()],
        }),
    };
    for (subject, owner) in [("C01", false), ("C02", true), ("C03", false)] {
        ledger_line(&dir, &completion(subject, owner));
    }
    let entries = complete::read_ledger(&repo.completion_ledger()).expect("ledger");
    assert!(complete::owner_accepted(&entries, Subject::Chunk(2)));
    assert!(!complete::owner_accepted(&entries, Subject::Chunk(1)));
    let error = complete::complete(
        &repo,
        &manifest,
        Subject::Phase(1),
        fixtures::fingerprints(),
    )
    .expect_err("C02 below 9");
    assert!(
        error
            .to_string()
            .contains("C02 must first pass a 9/10 review (D034)"),
        "{error}"
    );
}
