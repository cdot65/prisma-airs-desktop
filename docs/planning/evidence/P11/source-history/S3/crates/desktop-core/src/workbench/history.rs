//! API Client run history and response bodies (C15). Each collection's
//! runs are a sealed index in the private API Client folder (newest first,
//! at most 200 runs, 30 days and 1 GiB of bodies); each response body is a
//! file of sealed frames (XChaCha20-Poly1305 under the profile key, 1 MiB
//! each, bound to the run, its position and whether it is the last), so a
//! body is never held whole in memory, never readable without the key and
//! never silently truncated. History records what happened; nothing in it
//! can be sent again (no replay). A run still recorded as running when the
//! desktop starts is marked interrupted.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::admission::CredentialClass;
use super::profile::{self, PrivateFailure};
use crate::vault::Vault;

pub const MAX_RUNS: usize = 200;
pub const RETAIN_SECS: u64 = 30 * 24 * 60 * 60;
pub const BODY_QUOTA: u64 = 1024 * 1024 * 1024;
/// The response body shown in the app (v1: 256 KiB).
pub const PREVIEW_BYTES: usize = 256 * 1024;
/// Largest response body kept (the rest is not read).
pub const MAX_RESPONSE: u64 = 512 * 1024 * 1024;
const FRAME: usize = 1024 * 1024;
/// Response headers kept per run.
pub const MAX_HEADERS: usize = 64;
pub const PAGE: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Running,
    /// A response was received (any status).
    Completed,
    Failed,
    Cancelled,
    /// The desktop stopped while the run was in flight (not replayed).
    Interrupted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RunFailure {
    /// No response within the idle limit.
    Timeout,
    Connect,
    Tls,
    /// The connection failed after the request may have been sent.
    Request,
    /// The OAuth token could not be obtained (nothing else was sent).
    Token,
    /// The response was larger than 512 MiB (the rest was not read).
    TooLarge,
    /// The response could not be stored on this Mac.
    Storage,
    /// A redirect went somewhere this build does not send to.
    Redirect,
    /// The tenant context changed (switch, credential replacement or
    /// reset) before it went: nothing was sent.
    Retired,
}

/// One run as history keeps it (no secret value; URL and headers
/// redacted).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    /// The request document's identity and path when sent.
    pub request: String,
    pub relative: String,
    /// Seconds since the Unix epoch.
    #[ts(type = "number")]
    pub at: u64,
    pub method: String,
    pub url: String,
    pub class: CredentialClass,
    /// The catalogue operation, for a tenant-credential request.
    pub operation: Option<String>,
    pub state: RunState,
    pub failure: Option<RunFailure>,
    /// A change may have reached the server although no response was
    /// seen. It is never sent again by itself.
    pub uncertain: bool,
    pub status: Option<u16>,
    pub headers: Vec<(String, String)>,
    /// The Location of a redirect that was not followed.
    pub redirect: Option<String>,
    #[ts(type = "number")]
    pub elapsed_ms: u64,
    #[ts(type = "number")]
    pub bytes: u64,
    /// The stored body is complete (sealed and closed).
    pub stored: bool,
}

pub fn new_run_id() -> Option<String> {
    let mut n = [0u8; 8];
    getrandom::fill(&mut n).ok()?;
    Some(format!("run-{}", hex::encode(n)))
}

pub fn valid_run_id(id: &str) -> bool {
    id.strip_prefix("run-").is_some_and(|h| {
        h.len() == 16
            && h.bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    })
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn bodies(private: &Path, scope: &str) -> PathBuf {
    private.join("responses").join(scope)
}

pub fn body_path(private: &Path, scope: &str, run: &str) -> PathBuf {
    bodies(private, scope).join(format!("{run}.body"))
}

fn frame_aad(run: &str, index: u64, last: bool) -> Vec<u8> {
    profile::aad(&format!("body:{run}:{index}:{}", u8::from(last)))
}

/// Writes a body as sealed frames; `finish` closes it.
pub struct BodyWriter {
    file: std::fs::File,
    path: PathBuf,
    key: [u8; 32],
    run: String,
    index: u64,
    buffer: Vec<u8>,
}

impl BodyWriter {
    pub fn create(
        private: &Path,
        keychain: &dyn Vault,
        scope: &str,
        run: &str,
    ) -> Result<Self, PrivateFailure> {
        let key = profile::key(keychain, true)?.ok_or(PrivateFailure::Unavailable)?;
        let dir = bodies(private, scope);
        std::fs::create_dir_all(&dir).map_err(|_| PrivateFailure::Unavailable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for d in [private.join("responses"), dir.clone()] {
                std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| PrivateFailure::Unavailable)?;
            }
        }
        let path = body_path(private, scope, run);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|_| PrivateFailure::Unavailable)?;
        Ok(Self {
            file,
            path,
            key,
            run: run.to_owned(),
            index: 0,
            buffer: Vec::with_capacity(FRAME),
        })
    }

    fn frame(&mut self, last: bool) -> std::io::Result<()> {
        let sealed = crate::sessions::seal(
            &self.key,
            &frame_aad(&self.run, self.index, last),
            &self.buffer,
        )
        .ok_or_else(|| std::io::Error::other("seal"))?;
        let len = u32::try_from(sealed.len()).map_err(|_| std::io::Error::other("frame"))?;
        self.file.write_all(&len.to_be_bytes())?;
        self.file.write_all(&sealed)?;
        self.buffer.clear();
        self.index += 1;
        Ok(())
    }

    pub fn write(&mut self, mut bytes: &[u8]) -> std::io::Result<()> {
        while !bytes.is_empty() {
            let take = (FRAME - self.buffer.len()).min(bytes.len());
            self.buffer.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.buffer.len() == FRAME {
                self.frame(false)?;
            }
        }
        Ok(())
    }

    /// Seals the last frame and syncs the file.
    pub fn finish(mut self) -> std::io::Result<()> {
        self.frame(true)?;
        self.file.sync_all()
    }

    /// Removes a body that will not be completed.
    pub fn discard(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reads a stored body frame by frame; any missing, reordered, altered or
/// unterminated frame is an error.
pub struct BodyReader {
    file: std::io::BufReader<std::fs::File>,
    key: [u8; 32],
    run: String,
    index: u64,
    done: bool,
}

impl BodyReader {
    pub fn open(
        private: &Path,
        keychain: &dyn Vault,
        scope: &str,
        run: &str,
    ) -> Result<Self, PrivateFailure> {
        let key = profile::key(keychain, false)?.ok_or(PrivateFailure::Unreadable)?;
        let path = body_path(private, scope, run);
        let meta = std::fs::symlink_metadata(&path).map_err(|_| PrivateFailure::Unavailable)?;
        if !meta.is_file() {
            return Err(PrivateFailure::Unavailable);
        }
        let file = std::fs::File::open(&path).map_err(|_| PrivateFailure::Unavailable)?;
        Ok(Self {
            file: std::io::BufReader::new(file),
            key,
            run: run.to_owned(),
            index: 0,
            done: false,
        })
    }

    /// The next frame's bytes; `None` after the last frame.
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, PrivateFailure> {
        if self.done {
            return Ok(None);
        }
        let mut len = [0u8; 4];
        self.file
            .read_exact(&mut len)
            .map_err(|_| PrivateFailure::Unreadable)?;
        let len = u32::from_be_bytes(len) as usize;
        if len > FRAME + 64 {
            return Err(PrivateFailure::Unreadable);
        }
        let mut sealed = vec![0u8; len];
        self.file
            .read_exact(&mut sealed)
            .map_err(|_| PrivateFailure::Unreadable)?;
        for last in [false, true] {
            if let Some(plain) =
                crate::sessions::open(&self.key, &frame_aad(&self.run, self.index, last), &sealed)
            {
                self.index += 1;
                if last {
                    self.done = true;
                    // Nothing may follow the last frame.
                    let mut extra = [0u8; 1];
                    if self
                        .file
                        .read(&mut extra)
                        .map_err(|_| PrivateFailure::Unreadable)?
                        != 0
                    {
                        return Err(PrivateFailure::Unreadable);
                    }
                }
                return Ok(Some(plain));
            }
        }
        Err(PrivateFailure::Unreadable)
    }
}

/// The first `limit` bytes of a stored body.
pub fn preview(
    private: &Path,
    keychain: &dyn Vault,
    scope: &str,
    run: &str,
    limit: usize,
) -> Result<Vec<u8>, PrivateFailure> {
    let mut reader = BodyReader::open(private, keychain, scope, run)?;
    let mut out = Vec::new();
    while out.len() < limit {
        match reader.next_frame()? {
            Some(frame) => out.extend_from_slice(&frame[..frame.len().min(limit - out.len())]),
            None => break,
        }
    }
    Ok(out)
}

/// Copies a stored body to `destination` (written beside it and renamed,
/// never through a symbolic link, never inside `.git` or the private
/// folder). Returns the bytes written.
pub fn export(
    private: &Path,
    keychain: &dyn Vault,
    scope: &str,
    run: &str,
    destination: &Path,
) -> Result<u64, ExportFailure> {
    let parent = destination.parent().ok_or(ExportFailure::Destination)?;
    let name = destination.file_name().ok_or(ExportFailure::Destination)?;
    let parent = parent
        .canonicalize()
        .map_err(|_| ExportFailure::Destination)?;
    let private = private
        .canonicalize()
        .unwrap_or_else(|_| private.to_path_buf());
    if parent.starts_with(&private) || parent.components().any(|c| c.as_os_str() == ".git") {
        return Err(ExportFailure::Destination);
    }
    let target = parent.join(name);
    if let Ok(meta) = std::fs::symlink_metadata(&target)
        && !meta.is_file()
    {
        return Err(ExportFailure::Destination);
    }
    let mut reader =
        BodyReader::open(&private, keychain, scope, run).map_err(|_| ExportFailure::Body)?;
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix).map_err(|_| ExportFailure::Io)?;
    let temp = parent.join(format!(
        ".{}.airs-{}",
        name.to_string_lossy(),
        hex::encode(suffix)
    ));
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|_| ExportFailure::Io)?;
        let mut total = 0u64;
        while let Some(frame) = reader.next_frame().map_err(|_| ExportFailure::Body)? {
            file.write_all(&frame).map_err(|_| ExportFailure::Io)?;
            total += frame.len() as u64;
        }
        file.sync_all().map_err(|_| ExportFailure::Io)?;
        std::fs::rename(&temp, &target).map_err(|_| ExportFailure::Io)?;
        Ok(total)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ExportFailure {
    /// Not a writable place outside the private folder and `.git`.
    Destination,
    /// The stored body is missing or did not open.
    Body,
    Io,
}

/// Applies retention to `runs` (newest first): at most [`MAX_RUNS`], none
/// older than 30 days, bodies within the quota; running runs and `keep`
/// are never dropped. Returns the dropped runs' IDs.
pub fn prune(runs: &mut Vec<RunRecord>, keep: Option<&str>, now: u64) -> Vec<String> {
    let mut total = 0u64;
    let mut kept = 0usize;
    let mut dropped = Vec::new();
    runs.retain(|r| {
        let protected = r.state == RunState::Running || Some(r.id.as_str()) == keep;
        let fits = kept < MAX_RUNS
            && now.saturating_sub(r.at) <= RETAIN_SECS
            && total.saturating_add(r.bytes) <= BODY_QUOTA;
        if protected || fits {
            kept += 1;
            total = total.saturating_add(r.bytes);
            true
        } else {
            dropped.push(r.id.clone());
            false
        }
    });
    dropped
}

/// At startup: runs still marked running are interrupted (never resent);
/// their partial bodies and any body without a record are removed.
pub fn recover(private: &Path, scope: &str, runs: &mut [RunRecord]) -> bool {
    let mut changed = false;
    for r in runs.iter_mut().filter(|r| r.state == RunState::Running) {
        r.state = RunState::Interrupted;
        r.uncertain = !matches!(r.method.as_str(), "GET" | "HEAD" | "OPTIONS");
        r.stored = false;
        let _ = std::fs::remove_file(body_path(private, scope, &r.id));
        changed = true;
    }
    if let Ok(entries) = std::fs::read_dir(bodies(private, scope)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let known = name
                .strip_suffix(".body")
                .is_some_and(|id| runs.iter().any(|r| r.id == id && r.stored));
            if !known {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    changed
}

pub fn remove_body(private: &Path, scope: &str, run: &str) {
    let _ = std::fs::remove_file(body_path(private, scope, run));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::SessionVault;
    use crate::workbench::testdir::Dir;

    #[test]
    fn bodies_are_sealed_frames_that_cannot_be_cut_or_moved() {
        let dir = Dir::new();
        let vault = SessionVault::default();
        let mut w =
            BodyWriter::create(dir.path(), &vault, "s", "run-0000000000000001").expect("fixture");
        let data: Vec<u8> = (0..(FRAME * 2 + 10)).map(|i| (i % 251) as u8).collect();
        w.write(&data).expect("fixture");
        w.finish().expect("fixture");
        let path = body_path(dir.path(), "s", "run-0000000000000001");
        let raw = std::fs::read(&path).expect("fixture");
        assert!(!raw.windows(64).any(|w| w == &data[..64]), "sealed on disk");
        let mut out = Vec::new();
        let mut r =
            BodyReader::open(dir.path(), &vault, "s", "run-0000000000000001").expect("fixture");
        while let Some(f) = r.next_frame().expect("fixture") {
            out.extend(f);
        }
        assert_eq!(out, data);
        // Cut after the first frame: never accepted as complete.
        let first = 4 + u32::from_be_bytes(raw[..4].try_into().expect("fixture")) as usize;
        std::fs::write(&path, &raw[..first]).expect("fixture");
        let mut r =
            BodyReader::open(dir.path(), &vault, "s", "run-0000000000000001").expect("fixture");
        assert!(r.next_frame().expect("first").is_some());
        assert_eq!(r.next_frame(), Err(PrivateFailure::Unreadable));
        // Another run's body under this run's name does not open.
        std::fs::write(&path, &raw).expect("fixture");
        std::fs::rename(&path, body_path(dir.path(), "s", "run-0000000000000002"))
            .expect("fixture");
        let mut r =
            BodyReader::open(dir.path(), &vault, "s", "run-0000000000000002").expect("fixture");
        assert_eq!(r.next_frame(), Err(PrivateFailure::Unreadable));
    }

    fn record(id: &str, at: u64, bytes: u64, state: RunState) -> RunRecord {
        RunRecord {
            id: id.into(),
            request: "r".into(),
            relative: "r.request.toml".into(),
            at,
            method: "POST".into(),
            url: "http://127.0.0.1/".into(),
            class: CredentialClass::Anonymous,
            operation: None,
            state,
            failure: None,
            uncertain: false,
            status: None,
            headers: Vec::new(),
            redirect: None,
            elapsed_ms: 0,
            bytes,
            stored: false,
        }
    }

    #[test]
    fn retention_keeps_running_and_viewed_runs() {
        let now = 100 * 24 * 3600;
        let mut runs = vec![
            record("a", now, 10, RunState::Running),
            record("b", now, BODY_QUOTA - 10, RunState::Completed),
            record("c", now, 10, RunState::Completed),
            record("old", 0, 1, RunState::Completed),
            record("viewed", 0, 1, RunState::Completed),
        ];
        let dropped = prune(&mut runs, Some("viewed"), now);
        assert_eq!(dropped, ["c", "old"]);
        assert_eq!(
            runs.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["a", "b", "viewed"]
        );
    }

    #[test]
    fn interrupted_runs_are_marked_and_never_resent() {
        let dir = Dir::new();
        let mut runs = vec![record("run-0000000000000003", 1, 0, RunState::Running)];
        std::fs::create_dir_all(dir.path().join("responses/s")).expect("fixture");
        std::fs::write(
            body_path(dir.path(), "s", "run-0000000000000003"),
            b"partial",
        )
        .expect("fixture");
        std::fs::write(
            body_path(dir.path(), "s", "run-00000000000000ff"),
            b"orphan",
        )
        .expect("fixture");
        assert!(recover(dir.path(), "s", &mut runs));
        assert_eq!(runs[0].state, RunState::Interrupted);
        assert!(runs[0].uncertain, "a POST may have reached the server");
        assert_eq!(
            std::fs::read_dir(dir.path().join("responses/s"))
                .expect("fixture")
                .count(),
            0
        );
    }
}
