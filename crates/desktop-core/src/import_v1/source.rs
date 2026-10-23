//! The v1 source, read only (D050): files under v1's application-support
//! folder and generic passwords in v1's Keychain service. Nothing here can
//! write, create, lock-create or delete anything of v1's.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use zeroize::Zeroizing;

use super::Problem;
use crate::vault::VaultError;

/// v1's Keychain service.
pub const V1_SERVICE: &str = "io.cdot.prisma-airs-desktop";

/// v1's running-instance lock file (present only while v1 has run).
const INSTANCE_LOCK: &str = ".desktop-instance.lock";

/// Reads generic-password data in v1's service. There is no write method:
/// v1's items cannot be changed through this type.
pub trait V1Keychain: Send + Sync + 'static {
    /// The item's data, `None` when there is no such item.
    fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError>;
}

/// A v1 Keychain that cannot be read (no keychain on this build).
pub struct UnavailableKeychain;

impl V1Keychain for UnavailableKeychain {
    fn read(&self, _: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        Err(VaultError::Unavailable)
    }
}

/// Where v1's work is: its folder and its Keychain service.
#[derive(Clone)]
pub struct V1Source {
    pub root: PathBuf,
    pub keychain: Arc<dyn V1Keychain>,
}

/// A path below the v1 root: plain names only (no separators, no dot names).
fn component_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\0')
}

/// The v1 root itself: an absolute, ordinary directory (not a symlink).
pub fn root_state(root: &Path) -> Result<(), Problem> {
    if !root.is_absolute() {
        return Err(Problem::PathRejected);
    }
    match std::fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(Problem::PathRejected),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(Problem::Missing),
        Err(_) => Err(Problem::AccessDenied),
    }
}

/// Walks `parts` below `root`, refusing symlinks and non-directories on the
/// way: the path to read, after every component was checked with `lstat`.
fn checked(root: &Path, parts: &[&str]) -> Result<PathBuf, Problem> {
    root_state(root)?;
    let mut path = root.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        if !component_ok(part) {
            return Err(Problem::PathRejected);
        }
        path.push(part);
        if i + 1 < parts.len() {
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => return Err(Problem::PathRejected),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(Problem::Missing);
                }
                Err(_) => return Err(Problem::AccessDenied),
            }
        }
    }
    Ok(path)
}

/// `O_NOFOLLOW` (macOS `<sys/fcntl.h>`).
const O_NOFOLLOW: i32 = 0x0100;

/// Reads a regular file below the root, at most `max` bytes, never through
/// a symlink. Opened read-only: its bytes, times and mode are unchanged.
pub fn read_file(root: &Path, parts: &[&str], max: u64) -> Result<Zeroizing<Vec<u8>>, Problem> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = checked(root, parts)?;
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Problem::Missing),
        Err(_) => return Err(Problem::AccessDenied),
    };
    if !meta.is_file() {
        return Err(Problem::PathRejected);
    }
    if meta.len() > max {
        return Err(Problem::TooLarge);
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(&path)
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Problem::Missing,
            std::io::ErrorKind::PermissionDenied => Problem::AccessDenied,
            _ => Problem::PathRejected,
        })?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Problem::AccessDenied)?;
    if bytes.len() as u64 > max {
        return Err(Problem::TooLarge);
    }
    Ok(bytes)
}

/// Whether a file or directory exists below the root (no symlink followed).
pub fn exists(root: &Path, parts: &[&str]) -> Result<bool, Problem> {
    let path = match checked(root, parts) {
        Ok(path) => path,
        Err(Problem::Missing) => return Ok(false),
        Err(p) => return Err(p),
    };
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(Problem::AccessDenied),
    }
}

/// The names of the entries of a directory below the root that `keep`
/// accepts, at most `limit` entries in all (more is `TooLarge`). Entries
/// that are not plain files or directories as `kind` says are refused.
pub fn list(
    root: &Path,
    parts: &[&str],
    dirs: bool,
    limit: usize,
    keep: impl Fn(&str) -> bool,
) -> Result<Vec<String>, Problem> {
    let path = checked(root, parts)?;
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Problem::Missing),
        Err(_) => return Err(Problem::AccessDenied),
    };
    if !meta.is_dir() {
        return Err(Problem::PathRejected);
    }
    let mut names = Vec::new();
    let entries = std::fs::read_dir(&path).map_err(|_| Problem::AccessDenied)?;
    for (count, entry) in entries.enumerate() {
        if count >= limit {
            return Err(Problem::TooLarge);
        }
        let entry = entry.map_err(|_| Problem::AccessDenied)?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !keep(&name) {
            continue;
        }
        let kind = entry.file_type().map_err(|_| Problem::AccessDenied)?;
        if (dirs && !kind.is_dir()) || (!dirs && !kind.is_file()) {
            return Err(Problem::PathRejected);
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}

/// A shared lock on v1's instance lock, held while an import reads and
/// settles: v1 holds it exclusively while it runs. The file is opened
/// read-only and never created; without one v1 has not run here.
pub struct SourceLock {
    _file: Option<File>,
}

/// Takes the shared lock without waiting; v1 running (it takes the file
/// exclusively) is `SourceLocked`.
pub fn lock(root: &Path) -> Result<SourceLock, Problem> {
    use std::os::unix::fs::OpenOptionsExt;
    let path = checked(root, &[INSTANCE_LOCK])?;
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW)
        .open(&path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SourceLock { _file: None });
        }
        Err(_) => return Err(Problem::PathRejected),
    };
    match file.try_lock_shared() {
        Ok(()) => Ok(SourceLock { _file: Some(file) }),
        Err(std::fs::TryLockError::WouldBlock) => Err(Problem::SourceLocked),
        Err(_) => Err(Problem::AccessDenied),
    }
}

/// Reads one v1 Keychain item, at most `max` bytes.
pub fn read_item(
    source: &V1Source,
    account: &str,
    max: usize,
) -> Result<Option<Zeroizing<Vec<u8>>>, Problem> {
    match source.keychain.read(account) {
        Ok(Some(bytes)) if bytes.len() > max => Err(Problem::TooLarge),
        Ok(found) => Ok(found),
        Err(VaultError::NotFound) => Ok(None),
        Err(VaultError::Locked) => Err(Problem::KeychainLocked),
        Err(VaultError::Denied) => Err(Problem::AccessDenied),
        Err(VaultError::Corrupt) => Err(Problem::Corrupt),
        Err(VaultError::Unavailable) => Err(Problem::KeychainLocked),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).expect("entropy");
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("tmp")
            .join(format!("airs-v1-src-{}", hex::encode(entropy)));
        std::fs::create_dir_all(&dir).expect("dir");
        dir
    }

    #[test]
    fn reads_are_bounded_and_never_follow_symlinks() {
        let root = root();
        std::fs::write(root.join("tenants.json"), b"{}").expect("write");
        assert_eq!(
            &**read_file(&root, &["tenants.json"], 8).expect("read"),
            b"{}"
        );
        assert_eq!(
            read_file(&root, &["tenants.json"], 1),
            Err(Problem::TooLarge)
        );
        assert_eq!(read_file(&root, &["absent.json"], 8), Err(Problem::Missing));
        std::os::unix::fs::symlink(root.join("tenants.json"), root.join("link.json"))
            .expect("link");
        assert_eq!(
            read_file(&root, &["link.json"], 8),
            Err(Problem::PathRejected)
        );
        std::fs::create_dir(root.join("real")).expect("dir");
        std::fs::write(root.join("real").join("a.bin"), b"x").expect("write");
        std::os::unix::fs::symlink(root.join("real"), root.join("alias")).expect("link");
        assert_eq!(
            read_file(&root, &["alias", "a.bin"], 8),
            Err(Problem::PathRejected)
        );
        assert_eq!(
            read_file(&root, &["..", "x"], 8),
            Err(Problem::PathRejected)
        );
        assert_eq!(
            read_file(&root, &["real/a.bin"], 8),
            Err(Problem::PathRejected)
        );
        assert_eq!(
            read_file(Path::new("relative"), &["a"], 8),
            Err(Problem::PathRejected)
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn listing_is_bounded_and_typed() {
        let root = root();
        std::fs::create_dir(root.join("d")).expect("dir");
        for n in 0..3 {
            std::fs::write(root.join("d").join(format!("attempt-{n}.bin")), b"x").expect("w");
        }
        let names = list(&root, &["d"], false, 8, |n| n.ends_with(".bin")).expect("list");
        assert_eq!(names.len(), 3);
        assert_eq!(
            list(&root, &["d"], false, 2, |_| true),
            Err(Problem::TooLarge)
        );
        assert_eq!(
            list(&root, &["d"], true, 8, |_| true),
            Err(Problem::PathRejected)
        );
        assert_eq!(
            list(&root, &["none"], true, 8, |_| true),
            Err(Problem::Missing)
        );
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn the_instance_lock_is_shared_never_created() {
        let root = root();
        assert!(lock(&root).is_ok());
        assert!(!root.join(INSTANCE_LOCK).exists(), "never created");
        std::fs::write(root.join(INSTANCE_LOCK), b"").expect("write");
        let ours = lock(&root).expect("shared");
        let again = lock(&root).expect("shared twice");
        drop((ours, again));
        // v1 running holds it exclusively.
        let v1 = File::open(root.join(INSTANCE_LOCK)).expect("open");
        v1.try_lock().expect("exclusive");
        assert!(matches!(lock(&root), Err(Problem::SourceLocked)));
        drop(v1);
        assert!(lock(&root).is_ok());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
