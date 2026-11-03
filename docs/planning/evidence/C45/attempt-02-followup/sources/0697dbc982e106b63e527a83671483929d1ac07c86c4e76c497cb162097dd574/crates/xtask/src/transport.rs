//! Codex review transport (D026).
//!
//! Preferred: queue the review message to the owner-designated thread by its
//! exact ID. Queue success is only submission; the reviewer-authored verdict
//! file is the only completion signal. Fallback: a fresh ephemeral read-only
//! `codex exec` whose JSONL events are captured and checked for tool use the
//! review must not perform.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::env::Environment;
use crate::manifest::Reviewer;
use crate::{Result, invalid, io_err, now_utc, random_hex, read_json, sha256_hex, write_json};

/// A resolved and flag-verified Codex executable.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Codex {
    pub path: String,
    pub version: String,
    pub sha256: String,
}

#[derive(Debug)]
pub struct Output {
    pub status: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

/// Runs a process with optional stdin and a hard timeout (killed on expiry).
pub fn run_with_timeout(
    mut command: Command,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<Output> {
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let program = PathBuf::from(command.get_program());
    let mut child = command.spawn().map_err(io_err(&program))?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes).map_err(io_err(&program))?;
    }
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_thread = std::thread::spawn(move || read_all(stdout));
    let err_thread = std::thread::spawn(move || read_all(stderr));
    let started = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(io_err(&program))? {
            break status.code();
        }
        if started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    Ok(Output {
        status,
        stdout: out_thread.join().unwrap_or_default(),
        stderr: err_thread.join().unwrap_or_default(),
        timed_out,
    })
}

fn read_all<R: std::io::Read>(reader: Option<R>) -> Vec<u8> {
    let mut buf = Vec::new();
    if let Some(mut r) = reader {
        let _ = r.read_to_end(&mut buf);
    }
    buf
}

impl Codex {
    /// Resolves `codex` in the selected toolchain PATH and verifies that every
    /// flag the manifest uses is advertised by this exact executable.
    pub fn resolve(env: &Environment, reviewer: &Reviewer, cwd: &Path) -> Result<Self> {
        let path = env.resolve("codex")?;
        let canonical = std::fs::canonicalize(&path).map_err(io_err(&path))?;
        let bytes = std::fs::read(&canonical).map_err(io_err(&canonical))?;
        let version = Self::stdout(env, cwd, &["codex", "--version"])?;
        let codex = Self {
            path: path.display().to_string(),
            version: version.trim().to_owned(),
            sha256: sha256_hex(&bytes),
        };
        let queue_help = Self::stdout(env, cwd, &["codex", "queue", "--help"])?;
        let exec_help = Self::stdout(env, cwd, &["codex", "exec", "--help"])?;
        verify_flags(&reviewer.preferred_transport.argv, &queue_help)?;
        verify_flags(&reviewer.fallback_transport.argv, &exec_help)?;
        Ok(codex)
    }

    fn stdout(env: &Environment, cwd: &Path, argv: &[&str]) -> Result<String> {
        let argv: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
        let output = run_with_timeout(env.command(&argv, cwd)?, None, Duration::from_secs(30))?;
        if output.status != Some(0) {
            return Err(invalid(format!("{} failed", argv.join(" "))));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// Every `--flag` in the argv template must appear in the CLI help text.
pub fn verify_flags(argv: &[String], help: &str) -> Result<()> {
    for arg in argv.iter().filter(|a| a.starts_with('-') && *a != "-") {
        let flag = arg.split('=').next().unwrap_or(arg);
        let advertised = help
            .split(|c: char| c.is_whitespace() || c == ',')
            .any(|token| token == flag);
        if !advertised {
            return Err(invalid(format!(
                "codex does not advertise flag {flag}; refusing to weaken the review"
            )));
        }
    }
    Ok(())
}

/// Expands the manifest argv template with validated values only. `{codex}`
/// becomes the bare program name, which the environment resolves through the
/// same selected PATH that produced [`Codex::path`].
pub fn render_argv(template: &[String], values: &[(&str, &str)]) -> Result<Vec<String>> {
    template
        .iter()
        .map(|arg| {
            if arg == "{codex}" {
                return Ok("codex".to_owned());
            }
            if let Some((_, value)) = values.iter().find(|(key, _)| arg == &format!("{{{key}}}")) {
                return Ok((*value).to_owned());
            }
            if arg.contains('{') {
                return Err(invalid(format!("unresolved transport placeholder {arg}")));
            }
            Ok(arg.clone())
        })
        .collect()
}

/// Queue acknowledgement: submission only.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QueueAck {
    pub submitted_utc: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
}

pub fn queue(
    env: &Environment,
    reviewer: &Reviewer,
    cwd: &Path,
    message: &str,
) -> Result<QueueAck> {
    let argv = render_argv(
        &reviewer.preferred_transport.argv,
        &[("transport_message", message)],
    )?;
    let command = env.command(&argv, cwd)?;
    let submitted_utc = now_utc();
    let output = run_with_timeout(
        command,
        None,
        Duration::from_secs(reviewer.preferred_transport.submit_timeout_seconds),
    )?;
    if output.timed_out || output.status != Some(0) {
        return Err(invalid(format!(
            "codex queue did not acknowledge submission: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(QueueAck {
        submitted_utc,
        exit_code: output.status,
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    })
}

/// Handshake: the thread must author a matching route-ready.json.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteReady {
    pub request_id: String,
    pub thread: String,
    pub acknowledged: bool,
    pub reviewer: String,
    pub written_utc: String,
}

pub fn handshake(
    env: &Environment,
    reviewer: &Reviewer,
    cwd: &Path,
    transport_dir: &Path,
    wait: Duration,
) -> Result<PathBuf> {
    let thread = &reviewer.preferred_transport.thread;
    let request_id = format!(
        "hs-{}-{}",
        now_utc().replace([':', '-'], ""),
        random_hex(4)?
    );
    let dir = transport_dir.join(&request_id);
    let route = dir.join("route-ready.json");
    write_json(
        &dir.join("request.json"),
        &serde_json::json!({
            "request_id": request_id,
            "kind": "transport-handshake",
            "created_utc": now_utc(),
            "thread": thread,
            "expected_output": route.display().to_string(),
            "implementer": "Claude Code via xtask",
        }),
    )?;
    let message = format!(
        "Prisma AIRS Desktop review-transport handshake. Request ID: {request_id}. This is a harmless routing check, not a review or implementation request. Do not change application files, run product endpoints or access credentials. Only write {} containing {{\"request_id\": \"{request_id}\", \"thread\": \"{thread}\", \"acknowledged\": true, \"reviewer\": \"codex\", \"written_utc\": <current UTC ISO time>}} atomically (temp file then rename) and reply with the same JSON.",
        route.display()
    );
    queue(env, reviewer, cwd, &message)?;
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if route.is_file() {
            let ready: RouteReady = read_json(&route)?;
            if ready.request_id == request_id && &ready.thread == thread && ready.acknowledged {
                return Ok(route);
            }
            return Err(invalid(
                "route-ready.json does not match the handshake request",
            ));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    Err(invalid(
        "handshake not acknowledged within the bounded wait",
    ))
}

/// Waits for a reviewer-authored verdict with bounded progress reports.
pub fn wait_for_file(path: &Path, reviewer: &Reviewer, label: &str) -> Option<Vec<u8>> {
    let started = Instant::now();
    let mut last_progress = Instant::now();
    let max = Duration::from_secs(reviewer.wait.max_wait_seconds);
    loop {
        if let Ok(bytes) = std::fs::read(path) {
            // A writer that does not rename atomically may leave partial JSON.
            if serde_json::from_slice::<serde_json::Value>(&bytes).is_ok() {
                return Some(bytes);
            }
        }
        if started.elapsed() > max {
            return None;
        }
        if last_progress.elapsed() > Duration::from_secs(reviewer.wait.progress_every_seconds) {
            eprintln!(
                "[{}] waiting for {label}: {}s elapsed (max {}s)",
                now_utc(),
                started.elapsed().as_secs(),
                max.as_secs()
            );
            last_progress = Instant::now();
        }
        std::thread::sleep(Duration::from_secs(reviewer.wait.poll_seconds));
    }
}

/// Result of a fresh `codex exec` review.
pub struct ExecResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub forbidden_events: Vec<String>,
    pub model: Option<String>,
}

pub fn exec_fresh(
    env: &Environment,
    reviewer: &Reviewer,
    repo: &Path,
    schema: &Path,
    verdict: &Path,
    attempt_dir: &Path,
    prompt: &str,
) -> Result<ExecResult> {
    let argv = render_argv(
        &reviewer.fallback_transport.argv,
        &[
            ("repo", &repo.display().to_string()),
            ("schema_path", &schema.display().to_string()),
            ("verdict_path", &verdict.display().to_string()),
        ],
    )?;
    let command = env.command(&argv, repo)?;
    let output = run_with_timeout(
        command,
        Some(prompt.as_bytes()),
        Duration::from_secs(reviewer.wait.max_wait_seconds),
    )?;
    let stdout_path = attempt_dir.join("stdout.jsonl");
    let stderr_path = attempt_dir.join("stderr.log");
    crate::write_new(&stdout_path, &output.stdout)?;
    crate::write_new(&stderr_path, &output.stderr)?;
    let (forbidden_events, model) = inspect_events(&output.stdout);
    Ok(ExecResult {
        exit_code: if output.timed_out {
            None
        } else {
            output.status
        },
        timed_out: output.timed_out,
        stdout_path,
        stderr_path,
        forbidden_events,
        model,
    })
}

/// Flags JSONL events showing tool use a read-only review must not perform.
pub fn inspect_events(stdout: &[u8]) -> (Vec<String>, Option<String>) {
    let mut forbidden = Vec::new();
    let mut model = None;
    for line in String::from_utf8_lossy(stdout).lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if model.is_none() {
            model = find_string(&event, "model");
        }
        let item_type = event
            .pointer("/item/type")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if matches!(item_type, "mcp_tool_call" | "web_search" | "file_change") {
            forbidden.push(format!("reviewer used {item_type}"));
        }
    }
    forbidden.dedup();
    (forbidden, model)
}

fn find_string(value: &serde_json::Value, key: &str) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => map
            .get(key)
            .and_then(|v| v.as_str().map(str::to_owned))
            .or_else(|| map.values().find_map(|v| find_string(v, key))),
        serde_json::Value::Array(items) => items.iter().find_map(|v| find_string(v, key)),
        _ => None,
    }
}

/// Finds the thread's rollout transcript under `$HOME/.codex/sessions`.
pub fn thread_transcript(thread: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let root = PathBuf::from(home).join(".codex/sessions");
    let suffix = format!("{thread}.jsonl");
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(&suffix))
            {
                return Some(path);
            }
        }
    }
    None
}

/// Collects the thread's reply events that name `request_id` at or after
/// `since` (RFC 3339 UTC). This corroborates that the verdict came from the
/// designated thread rather than from the implementer.
pub fn thread_reply(transcript: &Path, request_id: &str, since: &str) -> Vec<u8> {
    let Ok(text) = std::fs::read_to_string(transcript) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let timestamp = event
            .get("timestamp")
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        let kind = event
            .pointer("/payload/type")
            .and_then(|t| t.as_str())
            .unwrap_or_default();
        let is_reply = matches!(kind, "agent_message" | "task_complete")
            || (kind == "message"
                && event.pointer("/payload/role").and_then(|r| r.as_str()) == Some("assistant"));
        let completes_turn = kind == "task_complete";
        if timestamp >= since && (is_reply || completes_turn) && line.contains(request_id) {
            out.extend_from_slice(line.as_bytes());
            out.push(b'\n');
        }
    }
    out
}

/// True once the captured events include the thread's completed turn (a
/// progress message naming the request is not a final verdict).
pub fn has_completed_turn(reply: &[u8]) -> bool {
    String::from_utf8_lossy(reply).lines().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .is_ok_and(|event| event["payload"]["type"] == "task_complete")
    })
}

/// Waits (bounded) for the thread's completed turn naming the request.
pub fn wait_for_thread_reply(
    thread: &str,
    request_id: &str,
    since: &str,
    max: Duration,
) -> Vec<u8> {
    let deadline = Instant::now() + max;
    loop {
        if let Some(path) = thread_transcript(thread) {
            let reply = thread_reply(&path, request_id, since);
            if has_completed_turn(&reply) {
                return reply;
            }
        }
        if Instant::now() > deadline {
            return Vec::new();
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}
