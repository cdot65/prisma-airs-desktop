//! Native SDK source qualification (D027): runs the published SDK package's
//! own unit tests on this Mac from the exact `.crate` the workspace locks.
//!
//! The `.crate` is taken from the local Cargo registry cache, its SHA-256 is
//! required to equal the Cargo.lock checksum, and it is extracted fresh into
//! a confined `.work/` scratch directory. Its tests run with the package's
//! own shipped Cargo.lock (`--locked`), the accepted feature set and a
//! separate target directory; output streams to the controller log so test
//! counts are recorded. Nothing from an SDK worktree is used.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{Repo, Result, invalid, io_err, sha256_hex, shared};

/// The locked registry package.
#[derive(Debug, PartialEq, Eq)]
pub struct LockedPackage {
    pub version: String,
    pub checksum: String,
}

/// Finds `name`'s crates.io entry in Cargo.lock. Exactly one is required.
pub fn locked_package(lock: &str, name: &str) -> Result<LockedPackage> {
    let mut found = Vec::new();
    for block in lock.split("[[package]]").skip(1) {
        let field = |key: &str| {
            block
                .lines()
                .find_map(|l| l.strip_prefix(&format!("{key} = \"")))
                .and_then(|v| v.strip_suffix('"'))
                .map(str::to_owned)
        };
        if field("name").as_deref() == Some(name)
            && field("source").as_deref()
                == Some("registry+https://github.com/rust-lang/crates.io-index")
        {
            match (field("version"), field("checksum")) {
                (Some(version), Some(checksum)) => found.push(LockedPackage { version, checksum }),
                _ => {
                    return Err(invalid(format!(
                        "Cargo.lock entry for {name} lacks a version or checksum"
                    )));
                }
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(invalid(format!("{name} is not a locked crates.io package"))),
        _ => Err(invalid(format!("{name} is locked more than once"))),
    }
}

fn cached_crate(home: &Path, name: &str, version: &str) -> Result<PathBuf> {
    let cache = home.join(".cargo/registry/cache");
    let file = format!("{name}-{version}.crate");
    let mut matches = Vec::new();
    for entry in std::fs::read_dir(&cache).map_err(io_err(&cache))? {
        let path = entry.map_err(io_err(&cache))?.path().join(&file);
        if path.is_file() {
            matches.push(path);
        }
    }
    matches.into_iter().next().ok_or_else(|| {
        invalid(format!(
            "{file} is not in the local Cargo registry cache; run cargo fetch --locked first"
        ))
    })
}

/// Verifies, extracts and tests `name` with `features` in `scratch`.
pub fn test(repo: &Repo, name: &str, features: &str, scratch: &Path) -> Result<()> {
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        || !features
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b',')
    {
        return Err(invalid(
            "package and feature names must be plain identifiers",
        ));
    }
    let scratch = shared::confined(repo, scratch, ".work")?;
    let lock_path = repo.join("Cargo.lock");
    let lock = std::fs::read_to_string(&lock_path).map_err(io_err(&lock_path))?;
    let package = locked_package(&lock, name)?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| invalid("HOME is not set"))?;
    let archive = cached_crate(&home, name, &package.version)?;
    let bytes = std::fs::read(&archive).map_err(io_err(&archive))?;
    let actual = sha256_hex(&bytes);
    if actual != package.checksum {
        return Err(invalid(format!(
            "{} checksum {actual} does not match Cargo.lock {}",
            archive.display(),
            package.checksum
        )));
    }
    if scratch.exists() {
        std::fs::remove_dir_all(&scratch).map_err(io_err(&scratch))?;
    }
    std::fs::create_dir_all(&scratch).map_err(io_err(&scratch))?;
    let status = Command::new("/usr/bin/tar")
        .args(["-xzf"])
        .arg(&archive)
        .arg("-C")
        .arg(&scratch)
        .status()
        .map_err(io_err(&archive))?;
    if !status.success() {
        return Err(invalid("the SDK package could not be extracted"));
    }
    let source = scratch.join(format!("{name}-{}", package.version));
    let manifest = source.join("Cargo.toml");
    if !manifest.is_file() || !source.join("Cargo.lock").is_file() {
        return Err(invalid(
            "the SDK package has no manifest or shipped Cargo.lock",
        ));
    }
    println!(
        "sdk-test: {name} {} sha256 {} (matches Cargo.lock), features [{features}]",
        package.version, package.checksum
    );
    let target = repo.join("target/sdk-tests");
    let status = Command::new("cargo")
        .args([
            "test",
            "--locked",
            "--no-default-features",
            "--features",
            features,
            "--lib",
            "--manifest-path",
        ])
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", &target)
        .current_dir(&source)
        .status()
        .map_err(io_err(&source))?;
    if !status.success() {
        return Err(invalid(format!("{name} unit tests failed")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactly_one_registry_entry_is_accepted() {
        let lock = r#"
[[package]]
name = "scm-sdk"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"

[[package]]
name = "other"
version = "1.0.0"
"#;
        assert_eq!(
            locked_package(lock, "scm-sdk").expect("found"),
            LockedPackage {
                version: "0.1.0".into(),
                checksum: "abc".into()
            }
        );
        assert!(locked_package(lock, "missing").is_err());
        let path_dep = "[[package]]\nname = \"scm-sdk\"\nversion = \"0.1.0\"\n";
        assert!(
            locked_package(path_dep, "scm-sdk").is_err(),
            "a path/git source is not the pinned package"
        );
        assert!(locked_package(&format!("{lock}{lock}"), "scm-sdk").is_err());
    }
}
