//! Fixture native builds for acceptance (`native-prepare`) and the `native`
//! gate that requires an actual acceptance record for the prepared bytes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::Environment;
use crate::evidence::NativeAcceptance;
use crate::fingerprint;
use crate::manifest::Manifest;
use crate::runner::Artifact;
use crate::{Repo, Result, Subject, invalid, io_err, now_utc, read_json, sha256_hex, write_json};

pub const FIXTURE_IDENTIFIER: &str = "io.cdot.prisma-airs-desktop.v2.fixture";
pub const PRODUCTION_IDENTIFIER: &str = "io.cdot.prisma-airs-desktop.v2";
const EXECUTABLE: &str = "prisma-airs-desktop-v2";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prepared {
    pub subject: String,
    pub built_utc: String,
    pub bundle: String,
    pub identifier: String,
    pub executable_sha256: String,
    pub bundle_sha256: String,
    pub features: Vec<String>,
    pub target: String,
    pub commands: Vec<Vec<String>>,
}

fn run(env: &Environment, repo: &Repo, argv: &[&str]) -> Result<()> {
    let argv: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
    let status = env
        .command(&argv, repo.root())?
        .status()
        .map_err(io_err(repo.root()))?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid(format!("{} failed with {status}", argv.join(" "))))
    }
}

/// The fixed fixture build commands (frontend then Tauri app bundle).
pub fn build_commands(target: &str) -> Vec<Vec<&str>> {
    vec![
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
            "app",
            "--features",
            "test-fixtures",
            "--config",
            "src-tauri/tauri.fixture.conf.json",
        ],
    ]
}

pub fn bundle_path(repo: &Repo, target: &str, product: &str) -> PathBuf {
    repo.join(format!(
        "target/{target}/release/bundle/macos/{product}.app"
    ))
}

pub fn plist_value(bundle: &Path, key: &str) -> Result<String> {
    let plist = bundle.join("Contents/Info.plist");
    let output = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(&plist)
        .env_clear()
        .output()
        .map_err(io_err(&plist))?;
    if !output.status.success() {
        return Err(invalid(format!(
            "cannot read {key} from {}",
            plist.display()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn prepare(
    repo: &Repo,
    manifest: &Manifest,
    env: &Environment,
    subject: Subject,
    target: &str,
    features: &[String],
) -> Result<Prepared> {
    if target != manifest.target {
        return Err(invalid(format!(
            "target {target} is not the manifest target {}",
            manifest.target
        )));
    }
    if features != manifest.fixture_only_features.as_slice() {
        return Err(invalid(
            "native-prepare requires exactly the fixture-only features",
        ));
    }
    let commands = build_commands(target);
    for argv in &commands {
        run(env, repo, argv)?;
    }
    let bundle = bundle_path(repo, target, "Prisma AIRS Desktop v2 Fixture");
    let identifier = plist_value(&bundle, "CFBundleIdentifier")?;
    if identifier != FIXTURE_IDENTIFIER {
        return Err(invalid(format!(
            "fixture bundle has identifier {identifier}"
        )));
    }
    let executable = bundle.join("Contents/MacOS").join(EXECUTABLE);
    let bytes = std::fs::read(&executable).map_err(io_err(&executable))?;
    let prepared = Prepared {
        subject: subject.to_string(),
        built_utc: now_utc(),
        bundle: bundle
            .strip_prefix(repo.root())
            .unwrap_or(&bundle)
            .display()
            .to_string(),
        identifier,
        executable_sha256: sha256_hex(&bytes),
        bundle_sha256: fingerprint::tree_hash(&bundle)?,
        features: features.to_vec(),
        target: target.to_owned(),
        commands: commands
            .iter()
            .map(|argv| argv.iter().map(|s| (*s).to_owned()).collect())
            .collect(),
    };
    write_json(
        &repo.evidence_dir(subject).join("native-prepare.json"),
        &prepared,
    )?;
    Ok(prepared)
}

/// The prepared bundle as it exists now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentBundle {
    pub executable_sha256: Option<String>,
    pub bundle_sha256: Option<String>,
    pub identifier: Option<String>,
}

pub fn current_state(repo: &Repo, prepared: &Prepared) -> CurrentBundle {
    let bundle = repo.join(&prepared.bundle);
    CurrentBundle {
        executable_sha256: std::fs::read(bundle.join("Contents/MacOS").join(EXECUTABLE))
            .ok()
            .map(|bytes| sha256_hex(&bytes)),
        bundle_sha256: bundle
            .is_dir()
            .then(|| fingerprint::tree_hash(&bundle).ok())
            .flatten(),
        identifier: plist_value(&bundle, "CFBundleIdentifier").ok(),
    }
}

/// Artifact entries recorded on the native-prepare run.
pub fn artifacts(repo: &Repo, subject: Subject) -> Result<Vec<Artifact>> {
    let prepared: Prepared = read_json(&repo.evidence_dir(subject).join("native-prepare.json"))?;
    Ok(vec![
        Artifact {
            name: "fixture-executable".to_owned(),
            path: format!("{}/Contents/MacOS/{EXECUTABLE}", prepared.bundle),
            sha256: prepared.executable_sha256,
        },
        Artifact {
            name: "fixture-bundle".to_owned(),
            path: prepared.bundle,
            sha256: prepared.bundle_sha256,
        },
    ])
}

/// Reasons the subject's native acceptance is not yet established for the
/// executable just prepared (the evidence index here is the evidence
/// directory as it exists now; review binds it by hash).
pub fn acceptance_status(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
    executable_sha256: &str,
) -> Vec<String> {
    let path = repo.evidence_dir(subject).join("native-acceptance.json");
    let Ok(bytes) = std::fs::read(&path) else {
        return vec![format!(
            "blocked: {} is missing; record actual CUA/operator native acceptance for executable {executable_sha256}",
            path.display()
        )];
    };
    let members = match manifest.native_members(subject) {
        Ok(members) => members,
        Err(e) => return vec![e.to_string()],
    };
    let (required_checks, fixture_identifier) = match crate::evidence::required_checks(
        &repo.join("docs/architecture/native-checks.json"),
        &members,
    ) {
        Ok(found) => found,
        Err(e) => return vec![e.to_string()],
    };
    let mut evidence_files = std::collections::BTreeSet::new();
    let mut stack = vec![repo.evidence_dir(subject)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(rel) = p.strip_prefix(repo.root()) {
                evidence_files.insert(rel.display().to_string());
            }
        }
    }
    let prepared: Option<Prepared> =
        read_json(&repo.evidence_dir(subject).join("native-prepare.json")).ok();
    let current = prepared.as_ref().map(|p| current_state(repo, p));
    let expect = crate::evidence::NativeExpectation {
        required_checks,
        fixture_identifier,
        prepared_executable_sha256: executable_sha256.to_owned(),
        prepared_bundle: prepared.as_ref().map(|p| p.bundle.clone()),
        prepared_bundle_sha256: prepared.as_ref().map(|p| p.bundle_sha256.clone()),
        prepared_identifier: prepared.as_ref().map(|p| p.identifier.clone()),
        current_executable_sha256: current.as_ref().and_then(|c| c.executable_sha256.clone()),
        current_bundle_sha256: current.as_ref().and_then(|c| c.bundle_sha256.clone()),
        current_identifier: current.and_then(|c| c.identifier),
        evidence_files,
    };
    match NativeAcceptance::parse(&bytes) {
        Ok(record) => record.failures(&subject.to_string(), &expect),
        Err(e) => vec![e.to_string()],
    }
}
