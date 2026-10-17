//! Strict command-line parsing and dispatch for `cargo run -p xtask -- ...`.

use std::path::PathBuf;

use crate::env::Environment;
use crate::manifest::Manifest;
use crate::review::TransportChoice;
use crate::runner::{RunKind, Runner, Status};
use crate::{
    Repo, Result, Subject, bindings, complete, fingerprint, gate, invalid, ledger, native, package,
    review, shared, transport,
};

const USAGE: &str = "usage: xtask <command> [options]
  check|test        --chunk Cxx|--phase Pxx --manifest docs/architecture/checks.json
  native            --chunk Cxx --manifest docs/architecture/checks.json
  native-prepare    --chunk Cxx --target aarch64-apple-darwin --features test-fixtures
  review            --chunk Cxx|--phase Pxx --reviewer codex --manifest docs/architecture/checks.json [--transport auto|thread|fresh] [--limitation TEXT]... [--native-pending REASON]
  complete          --chunk Cxx|--phase Pxx --manifest docs/architecture/checks.json
  reopen            --chunk Cxx|--phase Pxx --reason TEXT --manifest docs/architecture/checks.json
  verify-commit     --chunk Cxx|--phase Pxx
  validate-receipt  --receipt PATH --chunk Cxx|--phase Pxx [--gate-only]
  fingerprint       --chunk Cxx|--phase Pxx
  handshake
  bindings          [--check]
  ledger            [--check]
  notices           [--check]
  fetch-inputs      --source PATH_TO_ACADEMY_GIT_REPO
  shared-prepare    --input .inputs/academy --scratch .work/academy
  shared-lifecycle  --allowlist docs/architecture/shared-lifecycle.json --scratch .work/academy [--list]
  sdk-test          --package scm-sdk --features scan --scratch .work/scm-sdk
  package           --kind preview|release-candidate --manifest docs/architecture/checks.json";

struct Args {
    command: String,
    options: Vec<(String, Option<String>)>,
}

impl Args {
    fn parse(raw: Vec<String>) -> Result<Self> {
        let mut iter = raw.into_iter();
        let command = iter.next().ok_or_else(|| invalid(USAGE))?;
        let mut options = Vec::new();
        let rest: Vec<String> = iter.collect();
        let mut i = 0;
        while i < rest.len() {
            let flag = &rest[i];
            if !flag.starts_with("--") {
                return Err(invalid(format!("unexpected argument {flag:?}\n{USAGE}")));
            }
            let value = rest.get(i + 1).filter(|v| !v.starts_with("--")).cloned();
            i += if value.is_some() { 2 } else { 1 };
            options.push((flag.clone(), value));
        }
        Ok(Self { command, options })
    }

    fn value(&self, flag: &str) -> Option<&str> {
        self.options
            .iter()
            .find(|(f, _)| f == flag)
            .and_then(|(_, v)| v.as_deref())
    }

    fn values(&self, flag: &str) -> Vec<String> {
        self.options
            .iter()
            .filter(|(f, _)| f == flag)
            .filter_map(|(_, v)| v.clone())
            .collect()
    }

    fn has(&self, flag: &str) -> bool {
        self.options.iter().any(|(f, _)| f == flag)
    }

    fn require(&self, flag: &str) -> Result<&str> {
        self.value(flag)
            .ok_or_else(|| invalid(format!("{} requires {flag}\n{USAGE}", self.command)))
    }

    fn only(&self, allowed: &[&str]) -> Result<()> {
        for (flag, _) in &self.options {
            if !allowed.contains(&flag.as_str()) {
                return Err(invalid(format!(
                    "{} does not accept {flag}\n{USAGE}",
                    self.command
                )));
            }
        }
        Ok(())
    }

    fn subject(&self) -> Result<Subject> {
        match (self.value("--chunk"), self.value("--phase")) {
            (Some(c), None) => match Subject::parse(c)? {
                s @ Subject::Chunk(_) => Ok(s),
                _ => Err(invalid("--chunk expects C01-C26")),
            },
            (None, Some(p)) => match Subject::parse(p)? {
                s @ Subject::Phase(_) => Ok(s),
                _ => Err(invalid("--phase expects P01-P08")),
            },
            _ => Err(invalid(format!(
                "exactly one of --chunk or --phase is required\n{USAGE}"
            ))),
        }
    }

    fn manifest(&self, repo: &Repo) -> Result<Manifest> {
        let given = self.require("--manifest")?;
        if given != "docs/architecture/checks.json" {
            return Err(invalid(
                "only the approved manifest docs/architecture/checks.json is accepted",
            ));
        }
        Manifest::load(&repo.manifest())
    }
}

pub fn main(raw: Vec<String>) -> Result<()> {
    let repo = Repo::discover();
    let args = Args::parse(raw)?;
    match args.command.as_str() {
        "check" | "test" => {
            args.only(&["--chunk", "--phase", "--manifest"])?;
            let manifest = args.manifest(&repo)?;
            let subject = args.subject()?;
            let env = Environment::prepare(&repo)?;
            let kind = if args.command == "check" {
                RunKind::Check
            } else {
                RunKind::Test
            };
            let commands = manifest.plan(subject, kind)?;
            let shared_env;
            let env_ref = if kind == RunKind::Check
                && commands.iter().any(|c| c.label.starts_with("shared/"))
            {
                let version = shared::lock(&repo)?.academy.package_version;
                shared_env = env.clone().with("npm_package_version", &version);
                &shared_env
            } else {
                &env
            };
            let runner = Runner {
                repo: &repo,
                manifest: &manifest,
                env: env_ref,
                subject,
            };
            let record = runner.run(kind, &commands, &|_| intrinsic_checks(&repo, &manifest))?;
            report(&record)
        }
        "native-prepare" => {
            args.only(&["--chunk", "--phase", "--target", "--features"])?;
            let manifest = Manifest::load(&repo.manifest())?;
            let subject = args.subject()?;
            let env = Environment::prepare(&repo)?;
            let features: Vec<String> = args
                .require("--features")?
                .split(',')
                .map(str::to_owned)
                .collect();
            let prepared = native::prepare(
                &repo,
                &manifest,
                &env,
                subject,
                args.require("--target")?,
                &features,
            )?;
            println!(
                "prepared {} ({}) executable sha256 {}",
                prepared.bundle, prepared.identifier, prepared.executable_sha256
            );
            Ok(())
        }
        "native" => {
            args.only(&["--chunk", "--phase", "--manifest"])?;
            let manifest = args.manifest(&repo)?;
            let subject = args.subject()?;
            let env = Environment::prepare(&repo)?;
            let commands = manifest.plan(subject, RunKind::NativePrepare)?;
            let runner = Runner {
                repo: &repo,
                manifest: &manifest,
                env: &env,
                subject,
            };
            let record = runner.run(RunKind::NativePrepare, &commands, &|record| {
                fingerprint::verify_consumed_shared(&repo)?;
                record.artifacts.extend(native::artifacts(&repo, subject)?);
                Ok(())
            })?;
            report(&record)?;
            let sha = record
                .artifacts
                .first()
                .map(|a| a.sha256.clone())
                .unwrap_or_default();
            let pending = native::acceptance_status(&repo, &manifest, subject, &sha);
            if pending.is_empty() {
                println!("native acceptance recorded for executable {sha}");
                Ok(())
            } else {
                Err(invalid(pending.join("; ")))
            }
        }
        "review" => {
            args.only(&[
                "--chunk",
                "--phase",
                "--reviewer",
                "--manifest",
                "--transport",
                "--limitation",
                "--native-pending",
            ])?;
            if args.require("--reviewer")? != "codex" {
                return Err(invalid("the reviewer is codex (D026)"));
            }
            let manifest = args.manifest(&repo)?;
            let subject = args.subject()?;
            let env = Environment::prepare(&repo)?;
            let choice = match args.value("--transport").unwrap_or("auto") {
                "auto" => TransportChoice::Auto,
                "thread" => TransportChoice::Thread,
                "fresh" => TransportChoice::Fresh,
                other => return Err(invalid(format!("unknown transport {other}"))),
            };
            let outcome = review::review(
                &repo,
                &manifest,
                &env,
                subject,
                choice,
                args.values("--limitation"),
                args.value("--native-pending").map(str::to_owned),
            )?;
            println!("receipt: {}", outcome.receipt.display());
            if outcome.passed {
                println!("review gate: PASS");
                Ok(())
            } else {
                Err(invalid(format!(
                    "review gate: FAIL\n  - {}",
                    outcome.failures.join("\n  - ")
                )))
            }
        }
        "complete" => {
            args.only(&["--chunk", "--phase", "--manifest"])?;
            let manifest = args.manifest(&repo)?;
            let subject = args.subject()?;
            let env = Environment::prepare(&repo)?;
            let current = fingerprint::all(&repo, &manifest, subject, &env.toolchain_hash())?;
            let entry = complete::complete(&repo, &manifest, subject, current)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&entry).unwrap_or_default()
            );
            Ok(())
        }
        "reopen" => {
            args.only(&["--chunk", "--phase", "--reason", "--manifest"])?;
            let manifest = args.manifest(&repo)?;
            let written =
                complete::reopen(&repo, &manifest, args.subject()?, args.require("--reason")?)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&written).unwrap_or_default()
            );
            Ok(())
        }
        "verify-commit" => {
            args.only(&["--chunk", "--phase"])?;
            let entry = complete::verify_commit(&repo, args.subject()?)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&entry).unwrap_or_default()
            );
            Ok(())
        }
        "validate-receipt" => {
            args.only(&["--receipt", "--chunk", "--phase", "--gate-only"])?;
            let manifest = Manifest::load(&repo.manifest())?;
            let subject = args.subject()?;
            let env = Environment::prepare(&repo)?;
            let current = fingerprint::all(&repo, &manifest, subject, &env.toolchain_hash())?;
            let receipt = PathBuf::from(args.require("--receipt")?);
            // --gate-only omits C01's bootstrap record, which is produced by
            // comparing this very evaluation with the reference validator.
            let failures = if args.has("--gate-only") {
                let (plans, native) = complete::expectations(&repo, &manifest, subject)?;
                gate::evaluate(&gate::load(repo.root(), &receipt, current, plans, native)?)
            } else {
                complete::evaluate_receipt(&repo, &manifest, subject, &receipt, current)?
            };
            if failures.is_empty() {
                println!("receipt valid against current inputs");
                Ok(())
            } else {
                Err(invalid(format!(
                    "receipt rejected:\n  - {}",
                    failures.join("\n  - ")
                )))
            }
        }
        "fingerprint" => {
            args.only(&["--chunk", "--phase"])?;
            let manifest = Manifest::load(&repo.manifest())?;
            let env = Environment::prepare(&repo)?;
            let fp = fingerprint::all(&repo, &manifest, args.subject()?, &env.toolchain_hash())?;
            println!("{}", serde_json::to_string_pretty(&fp).unwrap_or_default());
            Ok(())
        }
        "handshake" => {
            args.only(&[])?;
            let manifest = Manifest::load(&repo.manifest())?;
            let env = Environment::prepare(&repo)?;
            let codex = transport::Codex::resolve(&env, &manifest.reviewer, repo.root())?;
            println!(
                "codex {} ({}) sha256 {}",
                codex.version, codex.path, codex.sha256
            );
            let route = transport::handshake(
                &env,
                &manifest.reviewer,
                repo.root(),
                &repo.transport_dir(),
                std::time::Duration::from_secs(300),
            )?;
            println!("route ready: {}", route.display());
            Ok(())
        }
        "bindings" => {
            args.only(&["--check"])?;
            bindings::run(&repo, args.has("--check"))
        }
        "notices" => {
            args.only(&["--check"])?;
            let env = Environment::prepare(&repo)?;
            crate::notices::run(&env, &repo, args.has("--check"))
        }
        "ledger" => {
            args.only(&["--check"])?;
            if args.has("--check") {
                ledger::check(&repo)
            } else {
                ledger::generate(&repo)
            }
        }
        "fetch-inputs" => {
            args.only(&["--source"])?;
            let hash = shared::fetch(&repo, &PathBuf::from(args.require("--source")?))?;
            println!(".inputs/academy tree sha256 {hash}");
            Ok(())
        }
        "shared-prepare" => {
            args.only(&["--input", "--scratch"])?;
            shared::prepare(
                &repo,
                &PathBuf::from(args.require("--input")?),
                &PathBuf::from(args.require("--scratch")?),
            )
        }
        "sdk-test" => {
            args.only(&["--package", "--features", "--scratch"])?;
            crate::sdk::test(
                &repo,
                args.require("--package")?,
                args.require("--features")?,
                &PathBuf::from(args.require("--scratch")?),
            )
        }
        "shared-lifecycle" => {
            args.only(&["--allowlist", "--scratch", "--list"])?;
            let scratch = PathBuf::from(args.require("--scratch")?);
            if args.has("--list") {
                for (_, name, version, hook, script) in
                    shared::discover_scripts(&repo.join(&scratch))?
                {
                    println!("{name}@{version} {hook}: {script}");
                }
                return Ok(());
            }
            let env = Environment::prepare(&repo)?;
            let ran = shared::lifecycle(
                &repo,
                &env,
                &PathBuf::from(args.require("--allowlist")?),
                &scratch,
            )?;
            println!("ran allowlisted lifecycle scripts: {ran:?}");
            Ok(())
        }
        "package" => {
            args.only(&["--kind", "--manifest"])?;
            let _manifest = args.manifest(&repo)?;
            let kind = package::Kind::parse(args.require("--kind")?)?;
            let completed = package::gate(&repo, kind)?;
            println!(
                "package gate passed for completed chunks {completed:?}; artifact build/signing validators are owned by C26"
            );
            Ok(())
        }
        _ => Err(invalid(USAGE)),
    }
}

/// Controller-intrinsic checks recorded with every check/test run.
fn intrinsic_checks(repo: &Repo, manifest: &Manifest) -> Result<()> {
    ledger::check(repo)?;
    fingerprint::verify_consumed_shared(repo)?;
    crate::env::refuse_env_files(repo)?;
    let _ = manifest;
    Ok(())
}

fn report(record: &crate::runner::RunRecord) -> Result<()> {
    for command in &record.commands {
        let tests = command
            .tests
            .as_ref()
            .map(|t| format!(" tests passed={} failed={}", t.passed, t.failed))
            .unwrap_or_default();
        println!(
            "{:?} {} {}{}",
            command.status,
            command.label,
            command.argv.join(" "),
            tests
        );
    }
    println!("run {} {:?}", record.run_id, record.status);
    match record.status {
        Status::Passed => Ok(()),
        _ => Err(invalid(
            record
                .failure
                .clone()
                .unwrap_or_else(|| "run failed".to_owned()),
        )),
    }
}
