//! The sealed Harness engine (C19, D046) as a bundle resource. Before a
//! fixture build, the sealed archive (`resources/harness/darwin-arm64.tar.gz`,
//! Git LFS) is checked against its pin record, extracted to
//! `target/harness/unpacked/harness/`, and every extracted file is checked
//! against the archive's own inventory and the compiled pin. The Harness
//! configuration overlay (`tauri.harness.conf.json`) bundles that directory
//! as `Contents/Resources/harness`.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{Repo, Result, invalid, io_err, sha256_hex};

pub const PIN_RECORD: &str = "resources/harness/pin.json";
pub const UNPACKED: &str = "target/harness/unpacked";

/// Every regular file under `dir`, relative, sorted.
fn files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for entry in std::fs::read_dir(dir).map_err(io_err(dir))? {
        let entry = entry.map_err(io_err(dir))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(io_err(&path))?;
        if kind.is_dir() {
            files(root, &path, out)?;
        } else if kind.is_file() {
            let rel = path.strip_prefix(root).unwrap_or(&path);
            out.push(rel.to_string_lossy().into_owned());
        } else {
            return Err(invalid(format!("{} is not a regular file", path.display())));
        }
    }
    out.sort();
    Ok(())
}

/// Extracts and verifies the sealed engine; returns the extracted directory.
pub fn unpack(repo: &Repo) -> Result<PathBuf> {
    let record_path = repo.join(PIN_RECORD);
    let record: Value =
        serde_json::from_slice(&std::fs::read(&record_path).map_err(io_err(&record_path))?)
            .map_err(|e| invalid(format!("{PIN_RECORD}: {e}")))?;
    let pin = desktop_core::harness::engine::pin();
    for (field, want) in [
        ("binarySha256", &pin.binary_sha256),
        ("resourcesSha256", &pin.resources_sha256),
        ("engineCommit", &pin.engine_commit),
    ] {
        if record[field].as_str() != Some(want.as_str()) {
            return Err(invalid(format!(
                "{PIN_RECORD} {field} differs from the compiled pin"
            )));
        }
    }
    let archive = repo.join(
        record["archive"]
            .as_str()
            .ok_or_else(|| invalid("pin record names no archive"))?,
    );
    let bytes = std::fs::read(&archive).map_err(io_err(&archive))?;
    if Some(sha256_hex(&bytes).as_str()) != record["archiveSha256"].as_str() {
        return Err(invalid(format!(
            "{} is not its pinned bytes (fetch Git LFS objects)",
            archive.display()
        )));
    }
    let out = repo.join(UNPACKED);
    if out.exists() {
        std::fs::remove_dir_all(&out).map_err(io_err(&out))?;
    }
    std::fs::create_dir_all(&out).map_err(io_err(&out))?;
    let status = std::process::Command::new("/usr/bin/tar")
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&out)
        .env_clear()
        .status()
        .map_err(io_err(&archive))?;
    if !status.success() {
        return Err(invalid("the sealed engine archive did not extract"));
    }
    let root = out.join("harness");
    let manifest_path = root.join("MANIFEST.json");
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(io_err(&manifest_path))?)
            .map_err(|e| invalid(format!("MANIFEST.json: {e}")))?;
    let inventory = manifest["files"]
        .as_object()
        .ok_or_else(|| invalid("MANIFEST.json has no files"))?;
    let mut found = Vec::new();
    files(&root, &root, &mut found)?;
    found.retain(|f| f != "MANIFEST.json");
    let listed: Vec<String> = inventory.keys().cloned().collect();
    if found != listed {
        return Err(invalid("the extracted engine differs from its inventory"));
    }
    let mut digest = String::new();
    for (name, hash) in inventory {
        let path = root.join(name);
        let actual = sha256_hex(&std::fs::read(&path).map_err(io_err(&path))?);
        if Some(actual.as_str()) != hash.as_str() {
            return Err(invalid(format!("{name} differs from its inventory hash")));
        }
        digest.push_str(&format!("{name}\0{actual}\n"));
    }
    if sha256_hex(digest.as_bytes()) != pin.resources_sha256
        || inventory.get(&pin.executable).and_then(Value::as_str)
            != Some(pin.binary_sha256.as_str())
    {
        return Err(invalid("the extracted engine is not the pinned inventory"));
    }
    let engine = desktop_core::harness::engine::Engine {
        executable: root
            .join(&pin.executable)
            .canonicalize()
            .map_err(io_err(&root))?,
    };
    desktop_core::harness::engine::verify(&engine, &pin)
        .map_err(|_| invalid("the extracted engine does not verify"))?;
    Ok(root)
}
