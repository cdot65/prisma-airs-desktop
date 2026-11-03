//! The read-only Git view of an open collection (C14, v1: branch, HEAD,
//! upstream and changes). Local editing, saving and previews work without
//! Git. `git` runs from an absolute path only (never a PATH lookup or the
//! `/usr/bin` shim that offers to install developer tools), with an empty
//! environment, no system or global configuration, optional locks off and
//! every repository-configurable program disabled (fsmonitor, hooks), for
//! at most 5 s and 1 MiB of output.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use ts_rs::TS;

pub const TIMEOUT: Duration = Duration::from_secs(5);
pub const OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_CHANGES: usize = 500;

/// Absolute locations tried in order.
pub const CANDIDATES: [&str; 4] = [
    "/opt/homebrew/bin/git",
    "/usr/local/bin/git",
    "/Library/Developer/CommandLineTools/usr/bin/git",
    "/Applications/Xcode.app/Contents/Developer/usr/bin/git",
];

pub fn locate() -> Option<PathBuf> {
    CANDIDATES
        .iter()
        .map(PathBuf::from)
        .find(|p| std::fs::metadata(p).is_ok_and(|m| m.is_file()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum GitState {
    /// No Git installation was found at a known location.
    Unavailable,
    /// The collection is not inside a Git working tree.
    NotRepository,
    Ready,
    /// Git failed or took too long.
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitChange {
    /// Repository-relative.
    pub path: String,
    /// Porcelain v2 `XY` (or `??` untracked, `!!` ignored, `u` unmerged).
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GitView {
    pub state: GitState,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub changes: Vec<GitChange>,
    pub truncated: bool,
}

impl GitView {
    fn of(state: GitState) -> Self {
        Self {
            state,
            branch: None,
            head: None,
            upstream: None,
            ahead: None,
            behind: None,
            changes: Vec::new(),
            truncated: false,
        }
    }
}

/// Runs one hardened, bounded, read-only git command in `root`.
fn run(git: &Path, root: &Path, args: &[&str]) -> Option<(bool, Vec<u8>)> {
    let mut child = Command::new(git)
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "status.showUntrackedFiles=normal",
            "--no-optional-locks",
        ])
        .args(args)
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = (&mut stdout)
            .take(OUTPUT_BYTES as u64 + 1)
            .read_to_end(&mut out);
        out
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() < TIMEOUT => {
                std::thread::sleep(Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let out = reader.join().ok()?;
    let status = status?;
    if out.len() > OUTPUT_BYTES {
        return None;
    }
    Some((status.success(), out))
}

/// Parses `git status --porcelain=v2 --branch -z` output.
pub fn parse(out: &[u8]) -> GitView {
    let mut view = GitView::of(GitState::Ready);
    let text = String::from_utf8_lossy(out);
    let mut records = text.split('\0').peekable();
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            let (key, rest) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if rest != "(initial)" => {
                    view.head = Some(rest.chars().take(12).collect());
                }
                "branch.head" if rest != "(detached)" => view.branch = Some(rest.to_owned()),
                "branch.upstream" => view.upstream = Some(rest.to_owned()),
                "branch.ab" => {
                    let mut parts = rest.split(' ');
                    view.ahead = parts
                        .next()
                        .and_then(|a| a.trim_start_matches('+').parse().ok());
                    view.behind = parts
                        .next()
                        .and_then(|b| b.trim_start_matches('-').parse().ok());
                }
                _ => {}
            }
            continue;
        }
        let (status, path) = match record.chars().next() {
            Some('1') => {
                let fields: Vec<&str> = record.splitn(9, ' ').collect();
                (fields.get(1).copied(), fields.get(8).copied())
            }
            Some('2') => {
                let fields: Vec<&str> = record.splitn(10, ' ').collect();
                // The original path follows as its own record.
                records.next();
                (fields.get(1).copied(), fields.get(9).copied())
            }
            Some('u') => {
                let fields: Vec<&str> = record.splitn(11, ' ').collect();
                (Some("u"), fields.get(10).copied())
            }
            Some('?') => (Some("??"), record.get(2..)),
            Some('!') => (Some("!!"), record.get(2..)),
            _ => (None, None),
        };
        if let (Some(status), Some(path)) = (status, path) {
            if view.changes.len() >= MAX_CHANGES {
                view.truncated = true;
                continue;
            }
            view.changes.push(GitChange {
                path: path.chars().take(1024).collect(),
                status: status.to_owned(),
            });
        }
    }
    view
}

/// Reads the view for the collection at `root`.
pub fn read(git: Option<&Path>, root: &Path) -> GitView {
    let Some(git) = git else {
        return GitView::of(GitState::Unavailable);
    };
    match run(git, root, &["rev-parse", "--is-inside-work-tree"]) {
        Some((true, out)) if out.starts_with(b"true") => {}
        Some(_) => return GitView::of(GitState::NotRepository),
        None => return GitView::of(GitState::Failed),
    }
    match run(
        git,
        root,
        &["status", "--porcelain=v2", "--branch", "-z", "--", "."],
    ) {
        Some((true, out)) => parse(&out),
        _ => GitView::of(GitState::Failed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_v2_records_are_parsed() {
        let out = b"# branch.oid 0123456789abcdef0123\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\x001 .M N... 100644 100644 100644 aaa bbb a.request.toml\x002 R. N... 100644 100644 100644 aaa bbb R100 new.toml\0old.toml\0? untracked.txt\0";
        let view = parse(out);
        assert_eq!(view.branch.as_deref(), Some("main"));
        assert_eq!(view.head.as_deref(), Some("0123456789ab"));
        assert_eq!((view.ahead, view.behind), (Some(2), Some(1)));
        assert_eq!(
            view.changes,
            vec![
                GitChange {
                    path: "a.request.toml".into(),
                    status: ".M".into()
                },
                GitChange {
                    path: "new.toml".into(),
                    status: "R.".into()
                },
                GitChange {
                    path: "untracked.txt".into(),
                    status: "??".into()
                },
            ]
        );
    }

    #[test]
    fn repository_hooks_and_fsmonitor_never_run() {
        let Some(git) = locate() else { return };
        let d = crate::workbench::testdir::Dir::new();
        let ok = |args: &[&str]| {
            Command::new(&git)
                .arg("-C")
                .arg(d.path())
                .args(args)
                .env_clear()
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("fixture")
                .success()
        };
        assert!(ok(&["init", "-q"]));
        let marker = d.path().join("ran");
        let script = d.path().join("monitor.sh");
        std::fs::write(&script, format!("#!/bin/sh\ntouch {}\n", marker.display()))
            .expect("fixture");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("fixture");
        assert!(ok(&[
            "config",
            "core.fsmonitor",
            script.to_str().expect("fixture")
        ]));
        std::fs::write(d.path().join("a.txt"), "x").expect("fixture");
        let view = read(Some(&git), d.path());
        assert_eq!(view.state, GitState::Ready);
        assert!(view.changes.iter().any(|c| c.path == "a.txt"));
        assert!(!marker.exists(), "a repository-configured program ran");
        let elsewhere = crate::workbench::testdir::Dir::new();
        assert_eq!(
            read(Some(&git), elsewhere.path()).state,
            GitState::NotRepository
        );
        assert_eq!(read(None, d.path()).state, GitState::Unavailable);
    }
}
