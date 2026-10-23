//! Local, redacted diagnostics (C24, D051; D021).
//!
//! The log holds typed events only: what kind of request was admitted or
//! refused and why, storage and reset outcomes, and where the app panicked.
//! Every field is a fixed name, a code or a number, so no prompt, response,
//! credential, token, tenant name, file path or other customer text can be
//! written, whatever a caller passes. The log stays on this Mac in its own
//! folder, bounded (two files of at most 256 KiB; older than 14 days is
//! dropped); nothing sends it anywhere. Export is explicit, through the
//! native save sheet, and re-checks every line; reset removes the folder.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

pub const DIAGNOSTICS_SCHEMA: &str = "prisma-airs-desktop-v2/diagnostics/1";
const CURRENT: &str = "diagnostics.jsonl";
const PREVIOUS: &str = "diagnostics.1.jsonl";
/// A file is rotated once it holds this much.
pub const FILE_LIMIT: u64 = 256 * 1024;
/// Lines older than this are dropped when the log is opened.
pub const RETENTION_SECS: u64 = 14 * 24 * 60 * 60;
/// The longest name or code a field may hold.
const NAME_LIMIT: usize = 48;

/// One typed event. Names and codes are checked against [`name_ok`] when
/// recorded and when exported: anything else is never written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Event {
    /// The app started (its version and whether it is a fixture build).
    Started { version: String, fixture: bool },
    /// A request was admitted, or refused with its code.
    Intent {
        kind: String,
        refusal: Option<String>,
    },
    /// The saved document could not be read or written (its health).
    Storage { health: String },
    /// Local data was reset, or the reset failed at a stage.
    Reset { outcome: String },
    /// The app panicked (where, never what).
    Panicked { file: String, line: u32 },
    /// The operator cleared the diagnostics.
    Cleared,
}

/// A name or code: ASCII letters, digits, `-`, `_`, `.` and `/` only, short.
pub fn name_ok(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= NAME_LIMIT
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

impl Event {
    fn fields_ok(&self) -> bool {
        match self {
            Event::Started { version, .. } => name_ok(version),
            Event::Intent { kind, refusal } => {
                name_ok(kind) && refusal.as_deref().is_none_or(name_ok)
            }
            Event::Storage { health } => name_ok(health),
            Event::Reset { outcome } => name_ok(outcome),
            Event::Panicked { file, .. } => {
                name_ok(file) && !file.starts_with('/') && !file.split('/').any(|c| c == "..")
            }
            Event::Cleared => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Line {
    /// Seconds since 1970.
    pub at: u64,
    pub event: Event,
}

/// The diagnostics log of one data root (None: kept nowhere).
pub struct Diagnostics {
    root: Option<PathBuf>,
    lock: Mutex<()>,
    /// Lines in the current and previous file (kept in memory: the view
    /// shows the count without reading the files).
    current: AtomicU32,
    previous: AtomicU32,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn open_private(path: &Path, append: bool) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .create(true)
        .append(append)
        .write(true)
        .truncate(!append)
        .mode(0o600)
        .custom_flags(0x0100) // O_NOFOLLOW
        .open(path)
}

/// Every well-formed line of a file (others are dropped, never repaired).
fn read_lines(path: &Path) -> Vec<Line> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Vec::new();
    };
    if !meta.is_file() || meta.len() > 2 * FILE_LIMIT {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Line>(l).ok())
        .filter(|l| l.event.fields_ok())
        .collect()
}

impl Diagnostics {
    /// Opens the log under `root`, dropping lines past retention.
    pub fn open(root: Option<PathBuf>) -> Self {
        let diagnostics = Self {
            root,
            lock: Mutex::new(()),
            current: AtomicU32::new(0),
            previous: AtomicU32::new(0),
        };
        diagnostics.retain(now());
        if let Some(dir) = diagnostics.dir() {
            let count = |n: &str| u32::try_from(read_lines(&dir.join(n)).len()).unwrap_or(u32::MAX);
            diagnostics.current.store(count(CURRENT), Ordering::Relaxed);
            diagnostics
                .previous
                .store(count(PREVIOUS), Ordering::Relaxed);
        }
        diagnostics
    }

    fn dir(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    fn ensure_dir(dir: &Path) -> bool {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .and_then(|()| std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)))
            .is_ok()
            && std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir())
    }

    /// Drops lines older than retention (rewriting what remains).
    fn retain(&self, at: u64) {
        let Some(dir) = self.dir() else {
            return;
        };
        let _guard = self.lock.lock();
        for name in [PREVIOUS, CURRENT] {
            let path = dir.join(name);
            if !path.exists() {
                continue;
            }
            let lines = read_lines(&path);
            let kept: Vec<_> = lines
                .iter()
                .filter(|l| at.saturating_sub(l.at) <= RETENTION_SECS)
                .collect();
            if kept.len() == lines.len() {
                continue;
            }
            if kept.is_empty() {
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if let Ok(mut f) = open_private(&path, false) {
                for l in kept {
                    if let Ok(text) = serde_json::to_string(l) {
                        let _ = writeln!(f, "{text}");
                    }
                }
            }
        }
    }

    /// Appends one event. An event with a field that is not a plain name
    /// or code is not written; a failure to write is ignored (diagnostics
    /// never stop the app).
    pub fn record(&self, event: Event) {
        self.record_at(now(), event);
    }

    pub fn record_at(&self, at: u64, event: Event) {
        let Some(dir) = self.dir() else {
            return;
        };
        if !event.fields_ok() {
            return;
        }
        let Ok(text) = serde_json::to_string(&Line { at, event }) else {
            return;
        };
        let _guard = self.lock.lock();
        if !Self::ensure_dir(dir) {
            return;
        }
        let current = dir.join(CURRENT);
        if std::fs::symlink_metadata(&current).is_ok_and(|m| m.len() >= FILE_LIMIT)
            && std::fs::rename(&current, dir.join(PREVIOUS)).is_ok()
        {
            self.previous
                .store(self.current.swap(0, Ordering::Relaxed), Ordering::Relaxed);
        }
        if let Ok(mut f) = open_private(&current, true)
            && writeln!(f, "{text}").is_ok()
        {
            self.current.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// The export: a header and every kept line, oldest first, each checked
    /// again (an edited or foreign line is not exported).
    pub fn export(&self, version: &str, fixture: bool) -> Vec<u8> {
        let mut lines = Vec::new();
        if let Some(dir) = self.dir() {
            let _guard = self.lock.lock();
            lines.extend(read_lines(&dir.join(PREVIOUS)));
            lines.extend(read_lines(&dir.join(CURRENT)));
        }
        let document = serde_json::json!({
            "schema": DIAGNOSTICS_SCHEMA,
            "app": if name_ok(version) { version } else { "unknown" },
            "fixture": fixture,
            "exported": now(),
            "events": lines,
        });
        serde_json::to_vec_pretty(&document).unwrap_or_default()
    }

    /// Removes every kept line; what remains is the record of clearing.
    pub fn clear(&self) {
        if let Some(dir) = self.dir() {
            let _guard = self.lock.lock();
            let _ = std::fs::remove_file(dir.join(PREVIOUS));
            let _ = std::fs::remove_file(dir.join(CURRENT));
            self.current.store(0, Ordering::Relaxed);
            self.previous.store(0, Ordering::Relaxed);
        }
        self.record(Event::Cleared);
    }

    /// Removes the folder (local-data reset).
    pub fn purge(&self) -> bool {
        let Some(dir) = self.dir() else {
            return true;
        };
        let _guard = self.lock.lock();
        self.current.store(0, Ordering::Relaxed);
        self.previous.store(0, Ordering::Relaxed);
        match std::fs::remove_dir_all(dir) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        }
    }

    /// How many lines are kept (shown in Settings).
    pub fn count(&self) -> u32 {
        self.current
            .load(Ordering::Relaxed)
            .saturating_add(self.previous.load(Ordering::Relaxed))
    }

    /// Whether diagnostics are kept anywhere on this build.
    pub fn available(&self) -> bool {
        self.root.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).expect("entropy");
        std::env::temp_dir()
            .canonicalize()
            .expect("tmp")
            .join(format!("airs-diagnostics-{}", hex::encode(entropy)))
    }

    #[test]
    fn only_names_and_codes_are_written() {
        let dir = root();
        let d = Diagnostics::open(Some(dir.clone()));
        d.record(Event::Intent {
            kind: "scan".into(),
            refusal: None,
        });
        // Free text never reaches the file, whatever a caller passes.
        d.record(Event::Intent {
            kind: "scan".into(),
            refusal: Some("prompt: ignore previous instructions".into()),
        });
        d.record(Event::Storage {
            health: "sk-live-CANARY secret".into(),
        });
        let text = std::fs::read_to_string(dir.join(CURRENT)).expect("log");
        assert_eq!(text.lines().count(), 1, "{text}");
        assert!(!text.contains("CANARY") && !text.contains("ignore"));
        // A line edited on disk is not exported.
        std::fs::write(
            dir.join(CURRENT),
            format!(
                "{text}{}\n",
                r#"{"at":1,"event":{"type":"storage","health":"has space"}}"#
            ),
        )
        .expect("write");
        let exported = String::from_utf8(d.export("0.1.0", true)).expect("utf8");
        assert!(!exported.contains("has space"));
        assert_eq!(d.count(), 1);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_log_is_bounded_kept_fourteen_days_and_purged() {
        let dir = root();
        let d = Diagnostics::open(Some(dir.clone()));
        let old = now() - RETENTION_SECS - 10;
        d.record_at(
            old,
            Event::Reset {
                outcome: "done".into(),
            },
        );
        d.record(Event::Cleared);
        // Reopened: the line past retention is gone.
        let d = Diagnostics::open(Some(dir.clone()));
        let exported = String::from_utf8(d.export("0.1.0", false)).expect("utf8");
        assert!(!exported.contains("\"done\""), "{exported}");
        assert_eq!(d.count(), 1);
        for _ in 0..12_000 {
            d.record(Event::Intent {
                kind: "observeShell".into(),
                refusal: Some("busy".into()),
            });
        }
        let size = |n: &str| std::fs::metadata(dir.join(n)).map_or(0, |m| m.len());
        assert!(size(CURRENT) <= FILE_LIMIT + 256 && size(PREVIOUS) <= FILE_LIMIT + 256);
        assert!(d.count() < 12_000);
        d.clear();
        assert_eq!(d.count(), 1, "only the record of clearing");
        assert!(d.purge());
        assert!(!dir.exists());
    }
}
