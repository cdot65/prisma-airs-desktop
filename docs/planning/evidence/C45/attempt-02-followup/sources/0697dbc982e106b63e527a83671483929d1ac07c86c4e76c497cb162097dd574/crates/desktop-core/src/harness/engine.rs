//! The compiled engine pin (`resources/harness/pin.json`, made by
//! `tools/harness/seal.py`) and the check every start makes before the
//! engine runs or any credential is read: the bundled executable is a
//! regular file (not a link), unchanged while it is read, and its SHA-256
//! is the pinned one.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::HarnessFailure;

const PIN: &str = include_str!("../../../../resources/harness/pin.json");
/// Larger than the pinned engine (about 320 MiB), and bounded.
const MAX_EXECUTABLE_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub version: u32,
    pub protocol: String,
    pub runtime_version: String,
    pub engine_commit: String,
    pub executable: String,
    pub binary_sha256: String,
    pub resources_sha256: String,
}

/// The pin this build was compiled with.
pub fn pin() -> Pin {
    serde_json::from_str(PIN).expect("the compiled engine pin parses")
}

/// The engine bundled with the app, by its path (the host resolves it
/// inside the app's resources; the presentation never supplies it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    pub executable: PathBuf,
}

/// File identity, to notice a replacement while the file is read.
fn identity(meta: &std::fs::Metadata) -> (u64, u64, u64, i128) {
    use std::os::unix::fs::MetadataExt;
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
    )
}

/// Verifies `engine` against `pin`. Blocking (it reads the whole file).
pub fn verify(engine: &Engine, pin: &Pin) -> Result<(), HarnessFailure> {
    let path: &Path = &engine.executable;
    let unavailable = |_| HarnessFailure::EngineUnavailable;
    if !path.is_absolute()
        || path.file_name().and_then(|n| n.to_str()) != Some(pin.executable.as_str())
    {
        return Err(HarnessFailure::EngineUnavailable);
    }
    let before = std::fs::symlink_metadata(path).map_err(unavailable)?;
    if !before.file_type().is_file() || before.len() > MAX_EXECUTABLE_BYTES {
        return Err(HarnessFailure::EngineUnavailable);
    }
    if path.canonicalize().map_err(unavailable)? != path {
        return Err(HarnessFailure::EngineUnavailable);
    }
    let mut file = std::fs::File::open(path).map_err(unavailable)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(unavailable)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_EXECUTABLE_BYTES {
            return Err(HarnessFailure::EngineUnavailable);
        }
        hash.update(&buf[..n]);
    }
    let after = std::fs::symlink_metadata(path).map_err(unavailable)?;
    if identity(&before) != identity(&after) || total != before.len() {
        return Err(HarnessFailure::EngineUnavailable);
    }
    if hex::encode(hash.finalize()) != pin.binary_sha256 {
        return Err(HarnessFailure::EngineUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiled_pin_names_the_reviewed_engine() {
        let p = pin();
        assert_eq!(p.version, 1);
        assert_eq!(p.protocol, super::super::protocol::PROTOCOL);
        assert_eq!(p.engine_commit, "9331a5839bc6a763d94dee7584746f58bbbe1181");
        assert_eq!(p.executable, "airs-harness");
        // The Developer ID signed engine (C26), resealed from the unsigned
        // f8e49cee… build.
        assert_eq!(
            p.binary_sha256,
            "383bcfb5f8d36d2367793a594ceeaf89a5c0fd618d6436397e9c2e22a682c1c5"
        );
    }

    #[test]
    fn only_the_pinned_bytes_at_a_real_path_verify() {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("fixture")
            .join(format!("airs-engine-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("fixture");
        let exe = dir.join("airs-harness");
        std::fs::write(&exe, b"not the engine").expect("fixture");
        let mut p = pin();
        let engine = Engine {
            executable: exe.clone(),
        };
        assert_eq!(verify(&engine, &p), Err(HarnessFailure::EngineUnavailable));
        p.binary_sha256 = hex::encode(Sha256::digest(b"not the engine"));
        assert_eq!(verify(&engine, &p), Ok(()));
        // Another name, a relative path, a link or a directory never verify.
        let other = dir.join("other");
        std::fs::write(&other, b"not the engine").expect("fixture");
        assert!(verify(&Engine { executable: other }, &p).is_err());
        assert!(
            verify(
                &Engine {
                    executable: "airs-harness".into()
                },
                &p
            )
            .is_err()
        );
        let link_dir = dir.join("link");
        std::fs::create_dir_all(&link_dir).expect("fixture");
        let link = link_dir.join("airs-harness");
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&exe, &link).expect("fixture");
        assert!(verify(&Engine { executable: link }, &p).is_err());
        // The real file reached through a linked folder is not its own path.
        let linked = dir.join("linked");
        let _ = std::fs::remove_file(&linked);
        std::os::unix::fs::symlink(&dir, &linked).expect("fixture");
        assert!(
            verify(
                &Engine {
                    executable: linked.join("airs-harness")
                },
                &p
            )
            .is_err()
        );
        let as_dir = dir.join("d").join("airs-harness");
        std::fs::create_dir_all(&as_dir).expect("fixture");
        assert!(verify(&Engine { executable: as_dir }, &p).is_err());
        std::fs::remove_dir_all(&dir).expect("fixture");
    }
}
