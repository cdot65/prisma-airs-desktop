//! Input fingerprints bound into review packets and receipts.
//!
//! * source: every tracked or untracked-but-not-ignored file (path, mode,
//!   content or symlink target) plus the repository `.git/config`, minus a
//!   narrow set of mutable evidence locations that are hashed separately.
//! * dependencies: lockfiles, toolchain pin, the consumed immutable inputs
//!   under `.inputs/`, and the resolved build-tool identities.
//! * build_flags: the subject's manifest entries, Tauri configuration and
//!   capability files, target and fixture-feature policy.
//! * evidence: the subject's committed evidence directory.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::manifest::Manifest;
use crate::{Repo, Result, Subject, invalid, io_err, sha256_hex};

/// Mutable locations excluded from the source fingerprint. Each is hashed
/// independently (evidence fingerprint, receipts) so exclusion cannot hide
/// application sources, lockfiles, tests or configuration.
pub const SOURCE_EXCLUSIONS: [&str; 3] = [
    "docs/planning/status.md",
    "docs/planning/reviews/",
    "docs/planning/evidence/",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprints {
    pub source: String,
    pub dependencies: String,
    pub build_flags: String,
    pub evidence: String,
}

/// One entry of the source inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub mode: String,
    pub sha256: String,
}

pub fn is_excluded(path: &str) -> bool {
    SOURCE_EXCLUSIONS.iter().any(|excluded| {
        if excluded.ends_with('/') {
            path.starts_with(excluded)
        } else {
            path == *excluded
        }
    })
}

/// Lists source files via git: tracked plus untracked files not ignored.
pub fn source_inventory(repo: &Repo) -> Result<Vec<FileEntry>> {
    let output = Command::new("/usr/bin/git")
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(repo.root())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .output()
        .map_err(io_err(repo.root()))?;
    if !output.status.success() {
        return Err(invalid("git ls-files failed"));
    }
    let mut paths: Vec<String> = output
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .filter(|p| !is_excluded(p))
        .collect();
    paths.sort();
    paths.dedup();
    let mut entries = Vec::with_capacity(paths.len() + 1);
    for path in paths {
        entries.push(file_entry(repo.root(), &path)?);
    }
    entries.push(file_entry(repo.root(), ".git/config")?);
    Ok(entries)
}

fn file_entry(root: &Path, relative: &str) -> Result<FileEntry> {
    let full = root.join(relative);
    let Ok(meta) = std::fs::symlink_metadata(&full) else {
        return Ok(FileEntry {
            path: relative.to_owned(),
            mode: "deleted".to_owned(),
            sha256: String::new(),
        });
    };
    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(&full).map_err(io_err(&full))?;
        return Ok(FileEntry {
            path: relative.to_owned(),
            mode: "symlink".to_owned(),
            sha256: sha256_hex(target.as_os_str().as_encoded_bytes()),
        });
    }
    let bytes = std::fs::read(&full).map_err(io_err(&full))?;
    let executable = meta.permissions().mode() & 0o111 != 0;
    Ok(FileEntry {
        path: relative.to_owned(),
        mode: if executable { "100755" } else { "100644" }.to_owned(),
        sha256: sha256_hex(&bytes),
    })
}

pub fn inventory_hash(entries: &[FileEntry]) -> String {
    let text: String = entries
        .iter()
        .map(|e| format!("{}\0{}\0{}\n", e.path, e.mode, e.sha256))
        .collect();
    sha256_hex(text.as_bytes())
}

pub fn source(repo: &Repo) -> Result<String> {
    Ok(inventory_hash(&source_inventory(repo)?))
}

/// Hash of every regular file under a directory (relative paths, sorted).
pub fn tree_hash(dir: &Path) -> Result<String> {
    tree_hash_excluding(dir, &[])
}

/// Like [`tree_hash`], skipping directories with the given names.
pub fn tree_hash_excluding(dir: &Path, skip: &[&str]) -> Result<String> {
    let mut entries = Vec::new();
    collect_tree(dir, dir, skip, &mut entries)?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(inventory_hash(&entries))
}

fn collect_tree(root: &Path, dir: &Path, skip: &[&str], out: &mut Vec<FileEntry>) -> Result<()> {
    let mut children: Vec<_> = std::fs::read_dir(dir)
        .map_err(io_err(dir))?
        .filter_map(|e| e.ok())
        .collect();
    children.sort_by_key(|e| e.file_name());
    for child in children {
        let path = child.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid("path escaped its root"))?
            .to_string_lossy()
            .into_owned();
        let meta = std::fs::symlink_metadata(&path).map_err(io_err(&path))?;
        if meta.is_dir() {
            if skip.iter().any(|name| child.file_name() == *name) {
                continue;
            }
            collect_tree(root, &path, skip, out)?;
        } else {
            out.push(file_entry(root, &relative)?);
        }
    }
    Ok(())
}

/// Dependency inputs: lockfiles, toolchain pin, immutable inputs, tools.
pub fn dependencies(repo: &Repo, toolchain_hash: &str) -> Result<String> {
    let mut parts: BTreeMap<String, String> = BTreeMap::new();
    for file in [
        "Cargo.lock",
        "pnpm-lock.yaml",
        "rust-toolchain.toml",
        ".npmrc",
        "docs/architecture/inputs.lock.json",
    ] {
        let path = repo.join(file);
        let digest = match std::fs::read(&path) {
            Ok(bytes) => sha256_hex(&bytes),
            Err(_) => "absent".to_owned(),
        };
        parts.insert(file.to_owned(), digest);
    }
    let inputs = repo.join(".inputs");
    if inputs.is_dir() {
        let mut names: Vec<_> = std::fs::read_dir(&inputs)
            .map_err(io_err(&inputs))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for name in names {
            parts.insert(format!(".inputs/{name}"), tree_hash(&inputs.join(&name))?);
        }
    }
    for (label, hash) in consumed_shared(repo)? {
        parts.insert(label, hash);
    }
    parts.insert("toolchain".to_owned(), toolchain_hash.to_owned());
    Ok(sha256_hex(
        serde_json::to_string(&parts).unwrap_or_default().as_bytes(),
    ))
}

/// Build flags: the subject's manifest entries and native configuration,
/// as canonical JSON (sorted keys, compact) so independent tools can
/// recompute it.
pub fn build_flags(repo: &Repo, manifest: &Manifest, subject: Subject) -> Result<String> {
    use serde_json::{Value, json};
    let mut parts: BTreeMap<String, Value> = BTreeMap::new();
    let chunks = if subject.is_phase() {
        manifest.phase_chunks(subject)?
    } else {
        vec![subject]
    };
    for chunk in chunks {
        let entry = manifest.chunk(chunk)?;
        parts.insert(
            format!("chunk/{chunk}"),
            // Keys in sorted order so the canonical form is independent of
            // serde_json's map ordering features.
            json!({
                "fixture": entry.fixture_command,
                "frontend": entry.frontend_commands,
                "host": entry.host_commands,
                "native_prepare": entry.native_prepare,
            }),
        );
    }
    parts.insert("setup".to_owned(), json!(manifest.setup_commands));
    parts.insert(
        "phase_native".to_owned(),
        json!(manifest.phase_native_prepare),
    );
    parts.insert("common".to_owned(), json!(manifest.common_commands));
    parts.insert("shared".to_owned(), json!(manifest.shared_source_commands));
    parts.insert("artifact".to_owned(), json!(manifest.artifact_build));
    parts.insert("target".to_owned(), json!(manifest.target));
    parts.insert(
        "fixture_only".to_owned(),
        json!(manifest.fixture_only_features),
    );
    parts.insert(
        "production_features".to_owned(),
        json!(manifest.requested_production_features),
    );
    for file in BUILD_FLAG_FILES {
        let digest = std::fs::read(repo.join(file))
            .map(|b| sha256_hex(&b))
            .unwrap_or_else(|_| "absent".to_owned());
        parts.insert(file.to_owned(), json!(digest));
    }
    Ok(sha256_hex(
        serde_json::to_string(&parts).unwrap_or_default().as_bytes(),
    ))
}

/// Native configuration files whose bytes are part of the build flags.
pub const BUILD_FLAG_FILES: [&str; 6] = [
    "apps/desktop/src-tauri/tauri.conf.json",
    "apps/desktop/src-tauri/tauri.fixture.conf.json",
    "apps/desktop/src-tauri/capabilities/main.json",
    "apps/desktop/src-tauri/Cargo.toml",
    "Cargo.toml",
    "docs/architecture/native-checks.json",
];

/// Evidence: the subject's committed evidence directory (empty if absent).
pub fn evidence(repo: &Repo, subject: Subject) -> Result<String> {
    let dir = repo.evidence_dir(subject);
    if dir.is_dir() {
        tree_hash(&dir)
    } else {
        Ok(sha256_hex(b""))
    }
}

pub fn all(
    repo: &Repo,
    manifest: &Manifest,
    subject: Subject,
    toolchain_hash: &str,
) -> Result<Fingerprints> {
    Ok(Fingerprints {
        source: source(repo)?,
        dependencies: dependencies(repo, toolchain_hash)?,
        build_flags: build_flags(repo, manifest, subject)?,
        evidence: evidence(repo, subject)?,
    })
}

/// The shared presentation package locations actually consumed by builds:
/// the locked input subtree, the staged scratch copy and the copy installed
/// into the desktop's node_modules. Each is hashed into the dependency
/// fingerprint; editing any of them invalidates bound receipts.
pub const SHARED_PACKAGE: &str = "packages/presentation";

pub fn consumed_shared(repo: &Repo) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    let web = repo.join(".work/academy");
    out.push((
        "shared/web-scratch".to_owned(),
        if web.is_dir() {
            academy_consumer_hash(&web)?
        } else {
            "absent".to_owned()
        },
    ));
    let locations = [
        (
            "shared/input",
            repo.join(".inputs/academy").join(SHARED_PACKAGE),
        ),
        (
            "shared/scratch",
            repo.join(".work/academy").join(SHARED_PACKAGE),
        ),
        (
            "shared/installed",
            std::fs::canonicalize(
                repo.join("apps/desktop/node_modules/@ai-security-academy/presentation"),
            )
            .unwrap_or_default(),
        ),
    ];
    for (label, path) in locations {
        let hash = if path.is_dir() {
            tree_hash_excluding(&path, &["node_modules"])?
        } else {
            "absent".to_owned()
        };
        out.push((label.to_owned(), hash));
    }
    Ok(out)
}

/// Generated outputs the Academy web checks write into scratch; everything
/// else in `.work/academy` must equal the locked input.
pub const ACADEMY_GENERATED: [&str; 4] = [
    "dist",
    "public/excalidraw-assets",
    "packages/shared/tsconfig.tsbuildinfo",
    // rollup-plugin-visualizer report written by `vite build`.
    "stats.html",
];

/// Hash of an Academy tree as consumed by the web checks: excludes
/// node_modules at any depth, the documented generated outputs and `.env*`
/// files (never staged).
pub fn academy_consumer_hash(dir: &Path) -> Result<String> {
    let mut entries = Vec::new();
    collect_tree(dir, dir, &["node_modules"], &mut entries)?;
    entries.retain(|e| {
        let name = e.path.rsplit('/').next().unwrap_or(&e.path);
        let generated = ACADEMY_GENERATED
            .iter()
            .any(|g| e.path == *g || e.path.starts_with(&format!("{g}/")));
        !generated && name != ".env" && !name.starts_with(".env.")
    });
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(inventory_hash(&entries))
}

/// Fails unless the staged web scratch tree, the staged package and the
/// installed package all equal the locked input.
pub fn verify_consumed_shared(repo: &Repo) -> Result<()> {
    let input_tree = repo.join(".inputs/academy");
    let scratch_tree = repo.join(".work/academy");
    if input_tree.is_dir()
        && scratch_tree.is_dir()
        && academy_consumer_hash(&input_tree)? != academy_consumer_hash(&scratch_tree)?
    {
        return Err(invalid(
            ".work/academy differs from the locked Academy input (outside documented generated outputs); re-run shared-prepare",
        ));
    }
    let hashes = consumed_shared(repo)?;
    let input = &hashes[1].1;
    if input == "absent" {
        return Err(invalid(
            "locked shared input .inputs/academy is absent; run fetch-inputs",
        ));
    }
    for (label, hash) in &hashes[2..] {
        if hash != input {
            return Err(invalid(format!(
                "{label} differs from the locked shared input; re-run shared-prepare and pnpm install"
            )));
        }
    }
    Ok(())
}
