//! Exact third-party notice inventory for what the desktop app ships.
//!
//! Rust: every package reachable from `desktop-host` through normal
//! dependencies in the locked graph (a superset across targets). JavaScript:
//! the production dependency tree of `@prisma-airs/desktop` as installed. A
//! package without a declared license fails the check; C26 compares this
//! inventory with the signed bundle.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::env::Environment;
use crate::{Repo, Result, invalid, io_err, read_json, sha256_hex, write_json};

pub const INVENTORY: &str = "docs/architecture/notice-inventory.json";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub ecosystem: String,
    pub name: String,
    pub version: String,
    pub license: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub schema: String,
    pub scope: String,
    pub packages: Vec<Package>,
}

fn output(env: &Environment, repo: &Repo, argv: &[&str]) -> Result<Vec<u8>> {
    let argv: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
    let out = env
        .command(&argv, repo.root())?
        .output()
        .map_err(io_err(repo.root()))?;
    if !out.status.success() {
        return Err(invalid(format!(
            "{} failed: {}",
            argv.join(" "),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    Ok(out.stdout)
}

fn rust_packages(env: &Environment, repo: &Repo) -> Result<Vec<Package>> {
    let bytes = output(
        env,
        repo,
        &["cargo", "metadata", "--locked", "--format-version", "1"],
    )?;
    let meta: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    let packages: BTreeMap<&str, &serde_json::Value> = meta["packages"]
        .as_array()
        .ok_or_else(|| invalid("cargo metadata has no packages"))?
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?, p)))
        .collect();
    let nodes: BTreeMap<&str, &serde_json::Value> = meta["resolve"]["nodes"]
        .as_array()
        .ok_or_else(|| invalid("cargo metadata has no resolve graph"))?
        .iter()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();
    let root = packages
        .iter()
        .find(|(_, p)| p["name"] == "desktop-host")
        .map(|(id, _)| *id)
        .ok_or_else(|| invalid("desktop-host not in metadata"))?;
    let mut seen = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = nodes.get(id) else { continue };
        for dep in node["deps"].as_array().into_iter().flatten() {
            let normal = dep["dep_kinds"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|k| k["kind"].is_null());
            if normal && let Some(pkg) = dep["pkg"].as_str() {
                stack.push(pkg);
            }
        }
    }
    Ok(seen
        .into_iter()
        .filter_map(|id| packages.get(id))
        .filter(|p| p["source"].is_string())
        .map(|p| Package {
            ecosystem: "cargo".to_owned(),
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            version: p["version"].as_str().unwrap_or_default().to_owned(),
            license: p["license"].as_str().unwrap_or_default().to_owned(),
        })
        .collect())
}

/// Packages that state their license only in a license file, recorded after
/// review with that file's SHA-256: the license counts as declared only
/// while the installed file is byte-identical.
const LICENSE_FILES: &[(&str, &str, &str, &str, &str)] = &[(
    "khroma",
    "2.1.0",
    "license",
    "66b333b0f66759a0b710459e03f7029abe17f4358114a128d2c972e642961b49",
    "MIT",
)];

/// The license a package manifest declares: `license`, the legacy
/// `licenses` array, or a reviewed license file.
fn declared_license(manifest: &serde_json::Value, dir: &std::path::Path) -> String {
    if let serde_json::Value::String(s) = &manifest["license"] {
        return s.clone();
    }
    if let Some(list) = manifest["licenses"].as_array() {
        let types: Vec<&str> = list.iter().filter_map(|l| l["type"].as_str()).collect();
        if !types.is_empty() && types.len() == list.len() {
            return types.join(" OR ");
        }
    }
    let name = manifest["name"].as_str().unwrap_or_default();
    let version = manifest["version"].as_str().unwrap_or_default();
    LICENSE_FILES
        .iter()
        .find(|(n, v, ..)| *n == name && *v == version)
        .and_then(|(_, _, file, sha, license)| {
            let bytes = std::fs::read(dir.join(file)).ok()?;
            (sha256_hex(&bytes) == *sha).then(|| (*license).to_owned())
        })
        .unwrap_or_default()
}

fn js_packages(env: &Environment, repo: &Repo) -> Result<Vec<Package>> {
    let bytes = output(
        env,
        repo,
        &[
            "pnpm",
            "--filter",
            "@prisma-airs/desktop",
            "list",
            "--prod",
            "--depth",
            "Infinity",
            "--json",
        ],
    )?;
    let tree: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    let mut found = BTreeSet::new();
    let mut stack: Vec<&serde_json::Value> = tree
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["dependencies"].as_object())
        .flat_map(|deps| deps.values())
        .collect();
    while let Some(dep) = stack.pop() {
        let path = dep["path"].as_str().unwrap_or_default();
        let manifest: serde_json::Value =
            read_json(&std::path::Path::new(path).join("package.json")).unwrap_or_default();
        let name = manifest["name"].as_str().unwrap_or_default().to_owned();
        let license = declared_license(&manifest, std::path::Path::new(path));
        // Workspace/Academy-owned inputs are covered by their own records.
        if !name.starts_with("@prisma-airs/") && !name.starts_with("@ai-security-academy/") {
            found.insert(Package {
                ecosystem: "npm".to_owned(),
                name,
                version: manifest["version"].as_str().unwrap_or_default().to_owned(),
                license,
            });
        }
        if let Some(children) = dep["dependencies"].as_object() {
            stack.extend(children.values());
        }
    }
    Ok(found.into_iter().collect())
}

pub fn build(env: &Environment, repo: &Repo) -> Result<Inventory> {
    let mut packages = rust_packages(env, repo)?;
    packages.extend(js_packages(env, repo)?);
    packages.sort();
    packages.dedup();
    let unknown: Vec<String> = packages
        .iter()
        .filter(|p| p.license.trim().is_empty())
        .map(|p| format!("{}:{}@{}", p.ecosystem, p.name, p.version))
        .collect();
    if !unknown.is_empty() {
        return Err(invalid(format!(
            "packages without a declared license: {}",
            unknown.join(", ")
        )));
    }
    Ok(Inventory {
        schema: "airs-v2/notice-inventory/1".to_owned(),
        scope: "registry packages reachable from desktop-host (normal deps, all targets) and the production npm tree of @prisma-airs/desktop; Academy-owned presentation is recorded in inputs.lock.json".to_owned(),
        packages,
    })
}

pub fn run(env: &Environment, repo: &Repo, check: bool) -> Result<()> {
    let fresh = build(env, repo)?;
    if check {
        let committed: Inventory = read_json(&repo.join(INVENTORY))?;
        if committed != fresh {
            return Err(invalid(
                "notice inventory differs from the resolved dependencies; run `xtask notices`",
            ));
        }
        Ok(())
    } else {
        write_json(&repo.join(INVENTORY), &fresh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_and_reviewed_file_licenses_count_only_when_exact() {
        let dir = std::env::temp_dir().join(format!("airs-notices-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let legacy = serde_json::json!({"name": "fuzzy", "version": "0.1.3",
            "licenses": [{"type": "MIT", "url": "x"}]});
        assert_eq!(declared_license(&legacy, &dir), "MIT");
        let partial = serde_json::json!({"name": "p", "version": "1", "licenses": [{"url": "x"}]});
        assert_eq!(declared_license(&partial, &dir), "");
        let khroma = serde_json::json!({"name": "khroma", "version": "2.1.0"});
        assert_eq!(declared_license(&khroma, &dir), "", "no file");
        std::fs::write(dir.join("license"), b"changed").expect("write");
        assert_eq!(declared_license(&khroma, &dir), "", "different file");
        let other = serde_json::json!({"name": "khroma", "version": "2.1.1"});
        assert_eq!(declared_license(&other, &dir), "", "other version");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
