//! Build-time controller for Prisma AIRS Desktop v2.
//!
//! The controller runs only the argv arrays in the approved command manifest
//! (`docs/architecture/checks.json`) under an explicit environment contract,
//! fingerprints the inputs they consume, freezes those inputs for independent
//! Codex review (D026), and validates review receipts before recording an
//! append-only completion. It never ships with the application.

pub mod bindings;
pub mod budgets;
pub mod cli;
pub mod complete;
pub mod env;
pub mod evidence;
pub mod fingerprint;
pub mod gate;
pub mod harness;
pub mod ledger;
pub mod live;
pub mod live_academy;
pub mod live_adapters;
pub mod live_changes;
pub mod live_inference;
pub mod live_mcp;
pub mod live_reads;
pub mod live_run;
pub mod live_scan;
pub mod live_workbench;
pub mod manifest;
pub mod native;
pub mod notices;
pub mod package;
pub mod paths;
pub mod review;
pub mod runner;
pub mod sdk;
pub mod shared;
pub mod signing;
pub mod subject;
pub mod transport;
pub mod verdict;

pub use paths::Repo;
pub use subject::Subject;

/// Errors surface as messages; every failure exits non-zero.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("json error in {path}: {source}")]
    Json {
        path: std::path::PathBuf,
        source: serde_json::Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}

pub(crate) fn io_err(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> Error + '_ {
    move |source| Error::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The PATH git runs with: the system tools, plus the directory of the Git
/// LFS command the repository's filter needs (the sealed Harness archive,
/// C19 and D046), found on the caller's PATH. Nothing else is inherited.
pub(crate) fn git_path() -> String {
    let mut path = String::from("/usr/bin:/bin");
    let lfs = std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p).find(|dir| dir.is_absolute() && dir.join("git-lfs").is_file())
    });
    if let Some(dir) = lfs
        .as_deref()
        .and_then(|d| d.to_str())
        .filter(|d| !d.contains(':'))
    {
        path.push(':');
        path.push_str(dir);
    }
    path
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Result<T> {
    let bytes = std::fs::read(path).map_err(io_err(path))?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes JSON atomically (temp file then rename) with a trailing newline.
pub(crate) fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|source| Error::Json {
        path: path.to_path_buf(),
        source,
    })?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let tmp = path.with_extension("tmp-write");
    std::fs::write(&tmp, bytes).map_err(io_err(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io_err(path))
}

/// Creates a file that must not already exist (immutable records).
pub(crate) fn write_new(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_err(path))?;
    file.write_all(bytes).map_err(io_err(path))
}

pub(crate) fn now_utc() -> String {
    // RFC 3339 UTC from the system clock without extra dependencies.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    // Howard Hinnant's days-to-civil algorithm.
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub(crate) fn random_hex(bytes: usize) -> Result<String> {
    use std::io::Read;
    let mut buf = vec![0u8; bytes];
    let path = std::path::Path::new("/dev/urandom");
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(io_err(path))?;
    Ok(hex::encode(buf))
}
