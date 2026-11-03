//! Immutable shared Academy input: fetch, verify, stage to scratch, and run
//! only allowlisted dependency lifecycle scripts.
//!
//! `.inputs/academy` holds the exact archive of the reviewed Academy commit
//! named in `docs/architecture/inputs.lock.json`; nothing is installed or
//! built there. `shared-prepare` copies verified bytes into `.work/academy`
//! (keeping only previously installed `node_modules`), refusing `.git`,
//! `.env` files and symlinks that escape the tree.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::env::Environment;
use crate::fingerprint;
use crate::{Repo, Result, invalid, io_err, read_json, sha256_hex};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputsLock {
    pub schema: String,
    pub academy: AcademyInput,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcademyInput {
    pub origin: String,
    pub branch: String,
    pub base_commit: String,
    pub commit: String,
    pub tree_sha256: String,
    pub package_version: String,
}

pub fn lock(repo: &Repo) -> Result<InputsLock> {
    read_json(&repo.join("docs/architecture/inputs.lock.json"))
}

fn git_archive(source: &Path, commit: &str, dest: &Path) -> Result<()> {
    let archive = Command::new("/usr/bin/git")
        .args(["-C"])
        .arg(source)
        .args(["archive", "--format=tar", commit])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::var("HOME").unwrap_or_default())
        .output()
        .map_err(io_err(source))?;
    if !archive.status.success() {
        return Err(invalid(format!(
            "git archive {commit} failed: {}",
            String::from_utf8_lossy(&archive.stderr)
        )));
    }
    std::fs::create_dir_all(dest).map_err(io_err(dest))?;
    let mut tar = Command::new("/usr/bin/tar")
        .args(["-x", "-f", "-", "-C"])
        .arg(dest)
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(io_err(dest))?;
    {
        use std::io::Write;
        let mut stdin = tar
            .stdin
            .take()
            .ok_or_else(|| invalid("tar stdin unavailable"))?;
        stdin.write_all(&archive.stdout).map_err(io_err(dest))?;
    }
    let status = tar.wait().map_err(io_err(dest))?;
    if status.success() {
        Ok(())
    } else {
        Err(invalid("extracting the Academy archive failed"))
    }
}

/// Materializes `.inputs/academy` from a git repository at the locked commit.
pub fn fetch(repo: &Repo, source: &Path) -> Result<String> {
    let lock = lock(repo)?;
    let dest = repo.join(".inputs/academy");
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(io_err(&dest))?;
    }
    git_archive(source, &lock.academy.commit, &dest)?;
    let hash = fingerprint::tree_hash(&dest)?;
    if !lock.academy.tree_sha256.is_empty() && hash != lock.academy.tree_sha256 {
        return Err(invalid(format!(
            "fetched Academy tree {hash} does not match the locked {}",
            lock.academy.tree_sha256
        )));
    }
    Ok(hash)
}

fn verify_tree(root: &Path, dir: &Path) -> Result<()> {
    let canonical_root = std::fs::canonicalize(root).map_err(io_err(root))?;
    for entry in std::fs::read_dir(dir)
        .map_err(io_err(dir))?
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            return Err(invalid(format!(
                "input contains git metadata: {}",
                path.display()
            )));
        }
        let meta = std::fs::symlink_metadata(&path).map_err(io_err(&path))?;
        if meta.file_type().is_symlink() {
            // Follow the whole chain; dangling or escaping links are refused.
            let resolved = std::fs::canonicalize(&path)
                .map_err(|_| invalid(format!("symlink does not resolve: {}", path.display())))?;
            if !resolved.starts_with(&canonical_root) {
                return Err(invalid(format!(
                    "symlink escapes the input: {}",
                    path.display()
                )));
            }
        } else if meta.is_dir() {
            verify_tree(root, &path)?;
        }
    }
    Ok(())
}

/// Resolves a repository-relative path that must stay under `allowed`
/// (a direct child directory of the repository). `..` components and
/// symlinked ancestors that leave the allowed root are refused.
pub fn confined(repo: &Repo, relative: &Path, allowed: &str) -> Result<std::path::PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(invalid(format!(
            "{} must be a plain relative path under {allowed}",
            relative.display()
        )));
    }
    let root = repo.join(allowed);
    std::fs::create_dir_all(&root).map_err(io_err(&root))?;
    let canonical_root = std::fs::canonicalize(&root).map_err(io_err(&root))?;
    let canonical_repo = std::fs::canonicalize(repo.root()).map_err(io_err(repo.root()))?;
    if canonical_root != canonical_repo.join(allowed) {
        return Err(invalid(format!(
            "{allowed} must be a real directory inside the repository"
        )));
    }
    let path = repo.join(relative);
    if !path.starts_with(&root) {
        return Err(invalid(format!(
            "{} is not under {allowed}",
            relative.display()
        )));
    }
    // Every existing ancestor must resolve inside the allowed root.
    let mut probe = path.clone();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let resolved = std::fs::canonicalize(&probe).map_err(io_err(&probe))?;
    if !resolved.starts_with(&canonical_root) {
        return Err(invalid(format!(
            "{} resolves outside {allowed}",
            relative.display()
        )));
    }
    Ok(path)
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(io_err(to))?;
    for entry in std::fs::read_dir(from)
        .map_err(io_err(from))?
        .filter_map(|e| e.ok())
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".env" || name.starts_with(".env.") {
            continue;
        }
        let source = entry.path();
        let dest = to.join(&name);
        let meta = std::fs::symlink_metadata(&source).map_err(io_err(&source))?;
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&source).map_err(io_err(&source))?;
            std::os::unix::fs::symlink(&target, &dest).map_err(io_err(&dest))?;
        } else if meta.is_dir() {
            copy_tree(&source, &dest)?;
        } else {
            std::fs::copy(&source, &dest).map_err(io_err(&dest))?;
        }
    }
    Ok(())
}

/// Removes everything in scratch except installed dependency directories.
fn clear_scratch(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .map_err(io_err(dir))?
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        let name = entry.file_name();
        if name == "node_modules" {
            continue;
        }
        let meta = std::fs::symlink_metadata(&path).map_err(io_err(&path))?;
        if meta.is_dir() && !meta.file_type().is_symlink() {
            // Keep nested package node_modules; clear the rest of the tree.
            clear_scratch(&path)?;
            if std::fs::read_dir(&path)
                .map_err(io_err(&path))?
                .next()
                .is_none()
            {
                std::fs::remove_dir(&path).map_err(io_err(&path))?;
            }
        } else {
            std::fs::remove_file(&path).map_err(io_err(&path))?;
        }
    }
    Ok(())
}

pub fn prepare(repo: &Repo, input: &Path, scratch: &Path) -> Result<()> {
    // Confinement first: nothing is read, deleted or copied until both paths
    // are plain relative paths that resolve under .inputs/ and .work/.
    let input = confined(repo, input, ".inputs")?;
    let scratch = confined(repo, scratch, ".work")?;
    if scratch == repo.join(".work") {
        return Err(invalid(
            "shared-prepare needs a scratch directory below .work/",
        ));
    }
    let lock = lock(repo)?;
    verify_tree(&input, &input)?;
    let hash = fingerprint::tree_hash(&input)?;
    if hash != lock.academy.tree_sha256 {
        return Err(invalid(format!(
            "{} hashes to {hash}, not the locked {}",
            input.display(),
            lock.academy.tree_sha256
        )));
    }
    clear_scratch(&scratch)?;
    copy_tree(&input, &scratch)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LifecycleAllowlist {
    pub schema: String,
    pub description: String,
    pub scripts: Vec<AllowedScript>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AllowedScript {
    pub package: String,
    pub version: String,
    pub hook: String,
    pub script: String,
    pub script_sha256: String,
    /// `run` executes the reviewed script; `skip` records that it was
    /// reviewed and is not required for the scoped commands.
    pub action: String,
    pub reason: String,
}

const HOOKS: [&str; 3] = ["preinstall", "install", "postinstall"];

/// Every installed package with a lifecycle hook, under any `node_modules`
/// directory of the scratch tree (hoisted or isolated pnpm layouts).
/// One discovered lifecycle hook: package directory, name, version, hook, script.
pub type DiscoveredScript = (PathBuf, String, String, String, String);

pub fn discover_scripts(scratch: &Path) -> Result<Vec<DiscoveredScript>> {
    let mut found = Vec::new();
    let mut module_dirs = vec![scratch.join("node_modules")];
    if let Ok(packages) = std::fs::read_dir(scratch.join("packages")) {
        module_dirs.extend(
            packages
                .filter_map(|e| e.ok())
                .map(|e| e.path().join("node_modules")),
        );
    }
    while let Some(modules) = module_dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&modules) else {
            continue;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".bin"
                || std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_symlink())
            {
                continue;
            }
            if name == ".pnpm" {
                if let Ok(store) = std::fs::read_dir(&path) {
                    module_dirs.extend(
                        store
                            .filter_map(|e| e.ok())
                            .map(|e| e.path().join("node_modules")),
                    );
                }
                continue;
            }
            let package_dirs: Vec<PathBuf> = if name.starts_with('@') {
                std::fs::read_dir(&path)
                    .map(|scoped| scoped.filter_map(|e| e.ok()).map(|e| e.path()).collect())
                    .unwrap_or_default()
            } else {
                vec![path]
            };
            for dir in package_dirs {
                if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) {
                    continue;
                }
                module_dirs.push(dir.join("node_modules"));
                let Ok(value) = read_json::<serde_json::Value>(&dir.join("package.json")) else {
                    continue;
                };
                let name = value["name"].as_str().unwrap_or_default().to_owned();
                let version = value["version"].as_str().unwrap_or_default().to_owned();
                for hook in HOOKS {
                    if let Some(script) = value["scripts"][hook].as_str() {
                        found.push((
                            dir.clone(),
                            name.clone(),
                            version.clone(),
                            hook.to_owned(),
                            script.to_owned(),
                        ));
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| (&a.1, &a.2, &a.3, &a.0).cmp(&(&b.1, &b.2, &b.3, &b.0)));
    Ok(found)
}

pub fn lifecycle(
    repo: &Repo,
    env: &Environment,
    allowlist: &Path,
    scratch: &Path,
) -> Result<Vec<String>> {
    let allow: LifecycleAllowlist = read_json(&repo.join(allowlist))?;
    let scratch = confined(repo, scratch, ".work")?;
    let mut ran = Vec::new();
    let mut unknown = Vec::new();
    let mut planned: BTreeMap<(String, PathBuf), String> = BTreeMap::new();
    for (dir, name, version, hook, script) in discover_scripts(&scratch)? {
        let digest = sha256_hex(script.as_bytes());
        let reviewed = allow.scripts.iter().find(|a| {
            a.package == name
                && a.version == version
                && a.hook == hook
                && a.script == script
                && a.script_sha256 == digest
        });
        match reviewed.map(|a| a.action.as_str()) {
            Some("run") => {
                planned.insert((format!("{name}@{version}:{hook}"), dir), script);
            }
            Some("skip") => {}
            _ => unknown.push(format!("{name}@{version} {hook}: {script}")),
        }
    }
    if !unknown.is_empty() {
        return Err(invalid(format!(
            "unknown dependency lifecycle scripts (not run): {}",
            unknown.join("; ")
        )));
    }
    for ((label, dir), script) in planned {
        // Allowlisted scripts are fixed `node <file>` invocations; run them as
        // argv without a shell.
        let argv: Vec<String> = script.split_whitespace().map(str::to_owned).collect();
        if argv.first().map(String::as_str) != Some("node") {
            return Err(invalid(format!(
                "allowlisted script {label} is not a plain node invocation"
            )));
        }
        let status = env.command(&argv, &dir)?.status().map_err(io_err(&dir))?;
        if !status.success() {
            return Err(invalid(format!("lifecycle script {label} failed")));
        }
        ran.push(label);
    }
    Ok(ran)
}
