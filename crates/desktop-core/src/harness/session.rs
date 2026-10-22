//! One conversation's lifecycle, run as an operation: verify the engine,
//! make its runtime home, read the key, launch, bootstrap, initialize and
//! start a read-only thread; then turns until it is ended, retired or its
//! deadline passes; then a bounded stop (close input, wait 2 s, kill the
//! process group, wait 2 s, reap) and the home removed.
//!
//! Pipe IO runs on two threads of its own: the writer owns stdin (dropping
//! its channel closes it) and the reader owns stdout, bounding every line
//! (64 KiB until the receipt, 4 MiB after) and the stream's lifetime.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc as std_mpsc};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{Notify, mpsc};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

use super::engine::{self, Engine, Pin};
use super::history::HistoryFailure;
use super::protocol::{
    self, Binding, EVENT_LIMIT, FRAME_LIMIT, Incoming, Method, Outbound, Receipt, RequestId,
};
use super::sidecar::{LaunchSpec, Sidecar, SidecarLauncher};
use super::{
    ApprovalKind, Cleanup, EntryState, HarnessApproval, HarnessEntry, HarnessFailure, HarnessPhase,
    HarnessRole, HarnessUsage, MAX_ANSWER_BYTES, MAX_ENTRIES, MAX_TRANSCRIPT_BYTES,
    McpServerStatus, McpStartup,
};
use crate::runtime::Outcome;

/// The receipt must arrive within this (the engine's own start is about
/// 70 ms warm, a few seconds on the first launch of new bytes).
pub const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(15);
/// Initialize and thread/start, each.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// DESIGN §3: 2 s graceful, then 2 s forced and reaped.
pub const GRACE: Duration = Duration::from_secs(2);
pub const FORCED: Duration = Duration::from_secs(2);
/// Everything one engine may write over its life.
pub const MAX_STREAM_BYTES: u64 = 256 * 1024 * 1024;
/// A conversation's operation budget.
pub const SESSION_DEADLINE: Duration = Duration::from_secs(8 * 60 * 60);
/// The largest token count published (JavaScript's `Number.MAX_SAFE_INTEGER`).
pub const MAX_SAFE_COUNT: u64 = (1 << 53) - 1;
/// The engine environment names the core may set; nothing is inherited.
pub const ENV_NAMES: [&str; 8] = [
    "PATH",
    "LANG",
    "TMPDIR",
    "AIRS_HARNESS_HOME",
    "CODEX_CA_CERTIFICATE",
    "NO_PROXY",
    "HTTPS_PROXY",
    "HTTP_PROXY",
];
const PATH: &str = "/usr/bin:/bin:/usr/sbin:/sbin";

/// What the operator asked of a running conversation.
pub enum Command {
    Send {
        id: String,
        prompt: Zeroizing<String>,
    },
    Interrupt,
    /// A credential the engine holds was renewed: the Gateway credential
    /// (C20), the workspace MCP tokens (C22) or both. The engine is
    /// restarted with them (what is `None` stays as it was), the
    /// conversation restored from its last export, and then `prompt` is
    /// sent.
    Renew {
        key: Option<Zeroizing<String>>,
        mcp: Option<Vec<Option<Zeroizing<String>>>>,
        id: String,
        prompt: Zeroizing<String>,
    },
    /// The operator's decision on a pending approval, by the desktop's own
    /// approval id (C21).
    Decide {
        approval: String,
        allow: bool,
    },
    /// A typed tool's result, for the engine request that asked for it.
    ToolResult {
        request: RequestId,
        lease: ToolLease,
        success: bool,
        text: String,
    },
}

/// A typed tool the engine called, for the authority to run (C21).
pub struct ToolRequest {
    pub request: RequestId,
    pub call: super::tools::ToolCall,
    /// The engine run and turn it belongs to: its result counts only for
    /// them (P06, C21 review F1).
    pub lease: ToolLease,
    /// Cancelled when the turn is interrupted or ends, or the engine
    /// stops: nothing more is admitted for the call after that.
    pub cancel: CancellationToken,
}

/// Which engine run and turn a tool call belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolLease {
    pub run: u64,
    pub turn: String,
}

/// Hands a tool call to the authority; its result comes back as
/// [`Command::ToolResult`].
pub type ToolSink = Arc<dyn Fn(ToolRequest) + Send + Sync>;

/// Pending engine requests a turn may hold (approvals, tool calls).
const MAX_PENDING_DECISIONS: usize = 8;
/// What a command entry shows of a command and of its output.
const MAX_COMMAND_TEXT: usize = 2 * 1024;
const MAX_COMMAND_OUTPUT: usize = 8 * 1024;
/// The longest command an approval shows whole; a longer one is declined,
/// never shown cut (C21 F3).
const MAX_APPROVAL_COMMAND: usize = 8 * 1024;
/// MCP calls tracked in one turn (the engine's own bound).
const MAX_MCP_ITEMS: usize = 64;
const DECLINED_MCP: &str = "An MCP request did not name a running tool call of a signed-in server, or asked for other input; it was declined.";

/// One workspace MCP call the engine started this turn (C22).
#[derive(Clone)]
struct McpItem {
    id: String,
    server: String,
    tool: String,
    arguments: String,
    /// Its arguments fit the approval whole (else it is never asked).
    reviewable: bool,
    /// Its approval was asked (an item is asked once).
    asked: bool,
    done: bool,
}

/// What the engine exported after a turn, with the engine context it was
/// exported under (user work: kept in memory and in the sealed store only).
#[derive(Clone)]
pub struct Replay {
    pub context: Value,
    pub replay: Value,
}

/// Saves the conversation after a turn (blocking; returns once durable).
pub type HistorySink = Arc<
    dyn Fn(Vec<HarnessEntry>, Option<HarnessUsage>, &Replay) -> Result<u64, HistoryFailure>
        + Send
        + Sync,
>;

/// How the engine reaches the Gateway, from the app's transport policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProxyEnv {
    /// No proxy (`NO_PROXY=*`).
    Direct,
    /// The engine's own system and environment proxy discovery.
    System,
    /// An operator-reviewed proxy URL without credentials.
    Explicit(String),
}

/// How the last turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum TurnEnd {
    Completed,
    Interrupted,
    Truncated,
    Failed,
    Refused,
}

/// A conversation as it stands, shared with the authority's view.
#[derive(Clone, Debug, Default)]
pub struct Progress {
    pub phase: HarnessPhase,
    pub failure: Option<HarnessFailure>,
    pub entries: Vec<HarnessEntry>,
    pub last_turn: Option<TurnEnd>,
    pub usage: Option<HarnessUsage>,
    pub cleanup: Option<Cleanup>,
    pub runtime_version: Option<String>,
    /// A turn was handed to the engine (it may have reached the Gateway).
    pub turn_handed: bool,
    /// The engine process, while it is owned (for diagnostics and tests).
    pub pid: Option<u32>,
    /// The runtime home, while it exists.
    pub home: Option<PathBuf>,
    /// When the conversation was last saved (wall-clock seconds).
    pub saved_at: Option<u64>,
    /// Why the last save did not happen.
    pub save_failure: Option<HistoryFailure>,
    /// Approvals the engine is waiting for (C21).
    pub approvals: Vec<HarnessApproval>,
    /// The scope servers' startup in the running engine (C22).
    pub mcp: Vec<McpServerStatus>,
}

pub type ProgressSlot = Arc<Mutex<Progress>>;
pub type KeyReader = Box<dyn FnOnce() -> Result<Zeroizing<String>, HarnessFailure> + Send>;

pub struct Inputs {
    pub engine: Engine,
    pub pin: Pin,
    pub launcher: Arc<dyn SidecarLauncher>,
    pub runtime_root: PathBuf,
    /// Additional PEM trust roots of the app's transport policy.
    pub roots: Vec<Vec<u8>>,
    pub proxy: ProxyEnv,
    pub binding: Binding,
    pub key: KeyReader,
    /// Each scope server's token, or `None` for the signed-out
    /// placeholder (C22).
    pub mcp: Vec<Option<Zeroizing<String>>>,
    pub commands: mpsc::Receiver<Command>,
    pub progress: ProgressSlot,
    pub pulse: Arc<Notify>,
    pub client_version: String,
    pub bootstrap_timeout: Duration,
    /// Saves after every turn; `None` keeps the conversation in memory.
    pub history: Option<HistorySink>,
    /// A saved conversation to continue (its transcript is already in
    /// `progress`).
    pub restore: Option<Replay>,
    /// Runs typed tool calls (C21); `None` refuses them.
    pub tools: Option<ToolSink>,
}

fn update(slot: &ProgressSlot, pulse: &Notify, f: impl FnOnce(&mut Progress)) {
    if let Ok(mut p) = slot.lock() {
        f(&mut p);
    }
    pulse.notify_one();
}

/// The conversation's operation.
pub async fn run(inputs: Inputs, cancel: CancellationToken, deadline: Instant) -> Outcome {
    let slot = Arc::clone(&inputs.progress);
    let pulse = Arc::clone(&inputs.pulse);
    let fail = |f: HarnessFailure| {
        update(&slot, &pulse, |p| {
            p.phase = HarnessPhase::Failed;
            p.failure = Some(f);
        })
    };
    let (engine, pin) = (inputs.engine.clone(), inputs.pin.clone());
    let verified = tokio::task::spawn_blocking(move || engine::verify(&engine, &pin))
        .await
        .unwrap_or(Err(HarnessFailure::EngineUnavailable));
    if let Err(f) = verified {
        fail(f);
        return Outcome::NotSent;
    }
    if cancel.is_cancelled() {
        update(&slot, &pulse, |p| p.phase = HarnessPhase::Ended);
        return Outcome::NotSent;
    }
    let Inputs {
        engine,
        pin,
        launcher,
        runtime_root,
        roots,
        proxy,
        binding,
        key,
        mcp,
        commands,
        progress,
        pulse: notify,
        client_version,
        bootstrap_timeout,
        history,
        restore,
        tools,
    } = inputs;
    let mut shared = Shared {
        engine,
        pin,
        launcher,
        proxy,
        binding,
        commands,
        progress,
        pulse: notify,
        client_version,
        bootstrap_timeout,
        history,
        tools,
    };
    let mut next = Launch {
        key: Key::Reader(key),
        mcp,
        restore,
        then: None,
    };
    let mut handed_any = false;
    loop {
        let home = match Home::make(&runtime_root, &roots) {
            Ok(home) => home,
            Err(_) => {
                fail(HarnessFailure::StartFailed);
                return Outcome::NotSent;
            }
        };
        update(&slot, &pulse, |p| p.home = Some(home.path.clone()));
        let (outcome, renewal) = launched(&mut shared, next, home.clone(), &cancel, deadline).await;
        handed_any |= slot.lock().map(|p| p.turn_handed).unwrap_or(true);
        let removed = home.remove();
        update(&slot, &pulse, |p| {
            p.home = None;
            if !removed && p.failure.is_none() {
                p.failure = Some(HarnessFailure::CleanupFailed);
                p.phase = HarnessPhase::Failed;
            }
        });
        match renewal {
            Some(launch) if removed => next = launch,
            _ if handed_any && outcome == Outcome::NotSent => return Outcome::Unknown,
            _ => return outcome,
        }
    }
}

/// What does not change when the engine is restarted.
struct Shared {
    engine: Engine,
    pin: Pin,
    launcher: Arc<dyn SidecarLauncher>,
    proxy: ProxyEnv,
    binding: Binding,
    commands: mpsc::Receiver<Command>,
    progress: ProgressSlot,
    pulse: Arc<Notify>,
    client_version: String,
    bootstrap_timeout: Duration,
    history: Option<HistorySink>,
    tools: Option<ToolSink>,
}

enum Key {
    Reader(KeyReader),
    /// A renewed Gateway credential (C20).
    Renewed(Zeroizing<String>),
}

/// One engine process: its key, what it restores, and what it sends once
/// ready.
pub(crate) struct Launch {
    key: Key,
    mcp: Vec<Option<Zeroizing<String>>>,
    restore: Option<Replay>,
    then: Option<(String, Zeroizing<String>)>,
}

/// From the key read to the reaped process. Returns the next launch when
/// the conversation continues in a new engine (a renewed credential).
async fn launched(
    shared: &mut Shared,
    launch: Launch,
    home: Home,
    cancel: &CancellationToken,
    deadline: Instant,
) -> (Outcome, Option<Launch>) {
    let progress = Arc::clone(&shared.progress);
    let pulse = Arc::clone(&shared.pulse);
    let fail = |f: HarnessFailure| {
        update(&progress, &pulse, |p| {
            p.phase = HarnessPhase::Failed;
            p.failure = Some(f);
        })
    };
    let Launch {
        key,
        mcp,
        restore,
        then,
    } = launch;
    let key = match key {
        Key::Renewed(key) => key,
        Key::Reader(read) => match tokio::task::spawn_blocking(read)
            .await
            .unwrap_or(Err(HarnessFailure::CredentialMissing))
        {
            Ok(key) => key,
            Err(f) => {
                fail(f);
                return (Outcome::NotSent, None);
            }
        },
    };
    let frame = match protocol::bootstrap(&shared.binding, &key, &mcp) {
        Ok(frame) => frame,
        Err(f) => {
            fail(f);
            return (Outcome::NotSent, None);
        }
    };
    let spec = LaunchSpec {
        executable: shared.engine.executable.clone(),
        args: vec!["desktop-server".into()],
        env: environment(&home, &shared.proxy),
        cwd: home.path.clone(),
    };
    let launcher = Arc::clone(&shared.launcher);
    let launch = tokio::task::spawn_blocking(move || launcher.launch(&spec)).await;
    let Ok(Ok(mut child)) = launch else {
        fail(HarnessFailure::StartFailed);
        return (Outcome::NotSent, None);
    };
    update(&progress, &pulse, |p| p.pid = Some(child.pid()));
    let (Some(stdin), Some(stdout)) = (child.take_stdin(), child.take_stdout()) else {
        let cleanup = stop(&mut *child, None).await;
        update(&progress, &pulse, |p| {
            p.cleanup = Some(cleanup);
            p.pid = None;
        });
        fail(HarnessFailure::StartFailed);
        return (Outcome::NotSent, None);
    };
    let (events_tx, events) = mpsc::channel::<Event>(64);
    let reader = std::thread::Builder::new()
        .name("airs-harness-reader".into())
        .spawn({
            let events_tx = events_tx.clone();
            move || read_frames(stdout, events_tx)
        })
        .ok();
    let (writer_tx, writer_rx) = std_mpsc::sync_channel::<Outbound>(8);
    let writer = std::thread::Builder::new()
        .name("airs-harness-writer".into())
        .spawn(move || write_frames(stdin, frame, writer_rx, events_tx))
        .ok();
    let mut conversation = Conversation {
        binding: shared.binding.clone(),
        runtime_version: shared.pin.runtime_version.clone(),
        client_version: shared.client_version.clone(),
        secret: Secrets { key, mcp },
        writer: Some(writer_tx),
        events,
        commands: std::mem::replace(&mut shared.commands, mpsc::channel(1).1),
        progress: Arc::clone(&progress),
        pulse: Arc::clone(&pulse),
        next_id: 1,
        pending: Vec::new(),
        thread: None,
        turn: None,
        interrupt_wanted: false,
        answer_bytes: 0,
        truncating: false,
        retry_refused: false,
        history: shared.history.clone(),
        latest: restore.clone(),
        restore,
        reading: None,
        deferred: None,
        exported: None,
        renewal: None,
        tools: shared.tools.clone(),
        approvals: Vec::new(),
        tool_calls: Vec::new(),
        run: {
            let mut b = [0u8; 8];
            let _ = getrandom::fill(&mut b);
            u64::from_le_bytes(b)
        },
        turn_cancel: CancellationToken::new(),
        seen: Vec::new(),
        files: Vec::new(),
        mcp_items: Vec::new(),
        elicitations_declined: 0,
    };
    let ended = if reader.is_none() || writer.is_none() {
        Err(HarnessFailure::StartFailed)
    } else {
        conversation
            .drive(cancel, deadline, shared.bootstrap_timeout, then)
            .await
    };
    shared.commands = std::mem::replace(&mut conversation.commands, mpsc::channel(1).1);
    // This engine's tool calls end with it.
    conversation.turn_cancel.cancel();
    let renewal = conversation.renewal.take();
    let latest = conversation.latest.take();
    // What the next engine keeps of this one's credentials.
    let held = Secrets {
        key: conversation.secret.key.clone(),
        mcp: conversation.secret.mcp.clone(),
    };
    let handed = progress.lock().map(|p| p.turn_handed).unwrap_or(true);
    update(&progress, &pulse, |p| {
        p.phase = if renewal.is_some() {
            HarnessPhase::Renewing
        } else {
            HarnessPhase::Ending
        };
        if let Err(f) = ended {
            p.failure = Some(f);
        }
    });
    let cleanup = stop(&mut *child, conversation.writer.take()).await;
    let sink = conversation.history.clone();
    drop(conversation);
    if cleanup != Cleanup::Failed {
        // The group is gone, so the reader has its end of file; detached if
        // it still has not returned.
        let joined = tokio::task::spawn_blocking(move || {
            if let Some(r) = reader {
                let _ = r.join();
            }
            if let Some(w) = writer {
                let _ = w.join();
            }
        });
        let _ = tokio::time::timeout(Duration::from_secs(1), joined).await;
    }
    // A renewed credential continues in a new engine, restoring what this
    // one last exported; a conversation that had turns but nothing to
    // restore cannot continue (it would silently lose its context).
    // What a stop may need to save with (a renewal restores it instead).
    let kept = latest.clone();
    let next = match (renewal, ended.is_ok() && cleanup != Cleanup::Failed) {
        (Some((key, mcp, id, prompt)), true) => {
            let had_turns = progress
                .lock()
                .map(|p| !p.entries.is_empty())
                .unwrap_or(true);
            if latest.is_none() && had_turns {
                update(&progress, &pulse, |p| {
                    p.failure = Some(HarnessFailure::RestoreFailed)
                });
                None
            } else {
                Some(Launch {
                    key: Key::Renewed(key.unwrap_or(held.key)),
                    mcp: mcp.unwrap_or(held.mcp),
                    restore: latest,
                    then: Some((id, prompt)),
                })
            }
        }
        _ => None,
    };
    // A turn the stop cut short is saved as it stood, its unfinished
    // entries interrupted, with the last export (whose context does not
    // hold that turn): a call that may have run is never left unrecorded,
    // and continuing restores only what the engine had (P06, C22 review
    // G1). Before the phase ends, so quitting waits for it.
    let cut = next.is_none()
        && progress
            .lock()
            .is_ok_and(|p| p.entries.iter().any(|e| e.state == EntryState::Streaming));
    if cut && let (Some(sink), Some(export)) = (sink, kept) {
        let mut taken = None;
        update(&progress, &pulse, |p| {
            for e in p
                .entries
                .iter_mut()
                .filter(|e| e.state == EntryState::Streaming)
            {
                e.state = EntryState::Interrupted;
            }
            taken = Some((p.entries.clone(), p.usage));
        });
        if let Some((entries, usage)) = taken {
            let saved = tokio::task::spawn_blocking(move || sink(entries, usage, &export))
                .await
                .unwrap_or(Err(HistoryFailure::Unavailable));
            update(&progress, &pulse, |p| match saved {
                Ok(at) => {
                    p.saved_at = Some(at);
                    p.save_failure = None;
                }
                Err(f) => p.save_failure = Some(f),
            });
        }
    }
    update(&progress, &pulse, |p| {
        p.cleanup = Some(cleanup);
        p.pid = None;
        if cleanup == Cleanup::Failed {
            p.failure = Some(HarnessFailure::CleanupFailed);
        }
        if next.is_some() {
            return;
        }
        p.phase = if p.failure.is_some() {
            HarnessPhase::Failed
        } else {
            HarnessPhase::Ended
        };
        for e in p
            .entries
            .iter_mut()
            .filter(|e| e.state == EntryState::Streaming)
        {
            e.state = EntryState::Interrupted;
        }
    });
    let outcome = match ended {
        Ok(()) => Outcome::Completed,
        Err(_) if handed => Outcome::Unknown,
        Err(_) => Outcome::NotSent,
    };
    (outcome, next)
}

/// The bounded stop: close input, wait, kill the group, wait, reap.
async fn stop(child: &mut dyn Sidecar, writer: Option<std_mpsc::SyncSender<Outbound>>) -> Cleanup {
    drop(writer);
    let started = Instant::now();
    let mut how = Cleanup::Graceful;
    loop {
        if child.exited().unwrap_or(false) {
            break;
        }
        if how == Cleanup::Graceful && started.elapsed() >= GRACE {
            child.kill_group();
            how = Cleanup::Forced;
        }
        if started.elapsed() >= GRACE + FORCED {
            return Cleanup::Failed;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The leader has exited but is not reaped: its group is still its own,
    // so whatever it left behind goes with it.
    child.kill_group();
    match child.reap() {
        Ok(Some(_)) => how,
        _ => Cleanup::Failed,
    }
}

/// The engine's environment: nothing inherited, no credential.
fn environment(home: &Home, proxy: &ProxyEnv) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), PATH.into()),
        ("LANG".into(), "en_US.UTF-8".into()),
        ("TMPDIR".into(), home.tmp().into_os_string()),
        (
            "AIRS_HARNESS_HOME".into(),
            home.path.clone().into_os_string(),
        ),
    ];
    if let Some(roots) = &home.roots {
        env.push((
            "CODEX_CA_CERTIFICATE".into(),
            roots.clone().into_os_string(),
        ));
    }
    match proxy {
        ProxyEnv::Direct => env.push(("NO_PROXY".into(), "*".into())),
        ProxyEnv::System => {}
        ProxyEnv::Explicit(url) => {
            env.push(("HTTPS_PROXY".into(), url.into()));
            env.push(("HTTP_PROXY".into(), url.into()));
        }
    }
    debug_assert!(env.iter().all(|(k, _)| ENV_NAMES.iter().any(|n| k == n)));
    env
}

/// A conversation's ephemeral runtime home: `<root>/run-<16 hex>`, 0700.
#[derive(Clone, Debug)]
pub struct Home {
    pub path: PathBuf,
    root: PathBuf,
    roots: Option<PathBuf>,
}

fn is_run_dir(name: &str) -> bool {
    name.strip_prefix("run-")
        .is_some_and(|h| h.len() == 16 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

impl Home {
    fn make(root: &Path, roots: &[Vec<u8>]) -> std::io::Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
        if !root.is_absolute() {
            return Err(std::io::ErrorKind::InvalidInput.into());
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(root)?;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))?;
        let root = root.canonicalize()?;
        // Only one conversation runs at a time: any other home is left from
        // an earlier run that did not finish removing it.
        for entry in std::fs::read_dir(&root)?.flatten() {
            if entry.file_name().to_str().is_some_and(is_run_dir) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).map_err(|_| std::io::Error::other("entropy"))?;
        let path = root.join(format!("run-{}", hex::encode(entropy)));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(path.join("tmp"))?;
        let roots = if roots.is_empty() {
            None
        } else {
            let file = path.join("roots.pem");
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&file)?;
            for pem in roots {
                f.write_all(pem)?;
                f.write_all(b"\n")?;
            }
            Some(file)
        };
        Ok(Self { path, root, roots })
    }

    fn tmp(&self) -> PathBuf {
        self.path.join("tmp")
    }

    /// Removes this home, and only it.
    fn remove(&self) -> bool {
        let ours = self.path.parent() == Some(self.root.as_path())
            && self
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(is_run_dir);
        ours && (std::fs::remove_dir_all(&self.path).is_ok() || !self.path.exists())
    }
}

enum Event {
    Frame(Incoming),
    Eof,
    Failed(HarnessFailure),
}

fn read_frames(mut stdout: Box<dyn Read + Send>, events: mpsc::Sender<Event>) {
    let mut line = Zeroizing::new(Vec::new());
    let mut chunk = [0u8; 8192];
    let mut total = 0u64;
    let mut first = true;
    let failed = loop {
        let n = match stdout.read(&mut chunk) {
            Ok(0) => {
                break if line.is_empty() {
                    Event::Eof
                } else {
                    Event::Failed(HarnessFailure::Protocol)
                };
            }
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break Event::Eof,
        };
        total += n as u64;
        if total > MAX_STREAM_BYTES {
            break Event::Failed(HarnessFailure::Bounds);
        }
        let mut bad = None;
        for &byte in &chunk[..n] {
            if byte != b'\n' {
                if line.len() >= if first { FRAME_LIMIT } else { EVENT_LIMIT } {
                    bad = Some(HarnessFailure::Bounds);
                    break;
                }
                line.push(byte);
                continue;
            }
            let decoded = Incoming::decode(&line);
            line.zeroize();
            line.clear();
            first = false;
            match decoded {
                Ok(frame) => {
                    if events.blocking_send(Event::Frame(frame)).is_err() {
                        return;
                    }
                }
                Err(f) => {
                    bad = Some(f);
                    break;
                }
            }
        }
        chunk.zeroize();
        if let Some(f) = bad {
            break Event::Failed(f);
        }
    };
    let _ = events.blocking_send(failed);
}

fn write_frames(
    mut stdin: Box<dyn Write + Send>,
    bootstrap: Zeroizing<Vec<u8>>,
    frames: std_mpsc::Receiver<Outbound>,
    events: mpsc::Sender<Event>,
) {
    let wrote = stdin.write_all(&bootstrap).and_then(|_| stdin.flush());
    drop(bootstrap);
    if wrote.is_err() {
        let _ = events.blocking_send(Event::Failed(HarnessFailure::Exited));
        return;
    }
    // Ends (closing stdin) when the conversation drops its sender.
    while let Ok(frame) = frames.recv() {
        if stdin
            .write_all(&frame.0)
            .and_then(|_| stdin.flush())
            .is_err()
        {
            let _ = events.blocking_send(Event::Failed(HarnessFailure::Exited));
            return;
        }
    }
}

struct Conversation {
    binding: Binding,
    runtime_version: String,
    client_version: String,
    /// Kept only to keep it out of anything shown.
    secret: Secrets,
    writer: Option<std_mpsc::SyncSender<Outbound>>,
    events: mpsc::Receiver<Event>,
    commands: mpsc::Receiver<Command>,
    history: Option<HistorySink>,
    /// The export of the conversation as it stands (or the restored one),
    /// for a renewal; none once a turn ends until that turn is exported.
    latest: Option<Replay>,
    /// Restored instead of starting a new thread.
    restore: Option<Replay>,
    /// A history read is pending until this: commands wait for it.
    reading: Option<Instant>,
    deferred: Option<Command>,
    /// An export to save once the frame is handled.
    exported: Option<Replay>,
    /// The conversation continues in a new engine with this credential.
    renewal: Option<Renewal>,
    progress: ProgressSlot,
    pulse: Arc<Notify>,
    next_id: u64,
    /// Requests sent and not yet answered.
    pending: Vec<(u64, Method)>,
    thread: Option<String>,
    /// The turn being answered: its native entry id, and the engine's turn
    /// id once known.
    turn: Option<(String, Option<String>)>,
    interrupt_wanted: bool,
    answer_bytes: usize,
    truncating: bool,
    /// The engine said it would retry the Gateway: the turn is stopped
    /// instead (D046), and ends as failed.
    retry_refused: bool,
    tools: Option<ToolSink>,
    /// Approvals waiting for the operator: the desktop's id and the
    /// engine's request (C21).
    approvals: Vec<(String, RequestId, ApprovalKind)>,
    /// This turn's workspace MCP calls (C22), as the engine started them.
    mcp_items: Vec<McpItem>,
    /// Elicitations declined without asking (numbers their notices).
    elicitations_declined: u32,
    /// Tool calls being run: the engine's request and the entry.
    tool_calls: Vec<(RequestId, String, ToolLease)>,
    /// This engine run, as a tool lease names it.
    run: u64,
    /// Cancelled when the turn is interrupted or ends (tool calls).
    turn_cancel: CancellationToken,
    /// Call and item ids seen in this turn (a repeat is a protocol failure).
    seen: Vec<String>,
    /// Files each file-change item of this turn changes.
    files: Vec<(String, Vec<String>)>,
}

impl Conversation {
    fn set(&self, f: impl FnOnce(&mut Progress)) {
        update(&self.progress, &self.pulse, f);
    }

    fn send(&mut self, frame: Outbound) -> Result<(), HarnessFailure> {
        let writer = self.writer.as_ref().ok_or(HarnessFailure::Exited)?;
        writer.try_send(frame).map_err(|e| match e {
            std_mpsc::TrySendError::Full(_) => HarnessFailure::Bounds,
            std_mpsc::TrySendError::Disconnected(_) => HarnessFailure::Exited,
        })
    }

    fn request(&mut self, method: Method, params: Value) -> Result<u64, HarnessFailure> {
        if self.pending.len() >= 8 {
            return Err(HarnessFailure::Bounds);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.send(protocol::request(id, method, params)?)?;
        self.pending.push((id, method));
        Ok(id)
    }

    /// The next frame, or why there will be none.
    async fn next(&mut self) -> Result<Incoming, HarnessFailure> {
        frame_of(self.events.recv().await)
    }

    /// Runs the conversation. `Ok` when it was ended (by the operator, a
    /// retirement or its deadline); `Err` when it failed.
    async fn drive(
        &mut self,
        cancel: &CancellationToken,
        deadline: Instant,
        bootstrap: Duration,
        then: Option<(String, Zeroizing<String>)>,
    ) -> Result<(), HarnessFailure> {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(()),
            _ = tokio::time::sleep_until(deadline) => return Ok(()),
            started = self.start(bootstrap) => started?,
        }
        self.set(|p| p.phase = HarnessPhase::Ready);
        if let Some((id, prompt)) = then {
            self.on_command(Command::Send { id, prompt })?;
        }
        loop {
            let read_by = self.reading.unwrap_or(deadline);
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                _ = tokio::time::sleep_until(deadline) => return Ok(()),
                _ = tokio::time::sleep_until(read_by), if self.reading.is_some() => {
                    return Err(HarnessFailure::Timeout);
                }
                event = self.events.recv() => self.on_frame(frame_of(event)?)?,
                command = self.commands.recv() => match command {
                    Some(command) => self.on_command(command)?,
                    None => return Ok(()),
                },
            }
            if let Some(export) = self.exported.take() {
                self.save(export).await;
                if let Some(command) = self.deferred.take() {
                    self.on_command(command)?;
                }
            }
            if self.renewal.is_some() {
                return Ok(());
            }
        }
    }

    /// Keeps the export for a renewal and saves it (off this task).
    async fn save(&mut self, export: Replay) {
        self.latest = Some(export.clone());
        let Some(sink) = self.history.clone() else {
            return;
        };
        let (entries, usage) = match self.progress.lock() {
            Ok(p) => (p.entries.clone(), p.usage),
            Err(_) => return,
        };
        let saved = tokio::task::spawn_blocking(move || sink(entries, usage, &export))
            .await
            .unwrap_or(Err(HistoryFailure::Unavailable));
        self.set(|p| match saved {
            Ok(at) => {
                p.saved_at = Some(at);
                p.save_failure = None;
            }
            Err(f) => p.save_failure = Some(f),
        });
    }

    /// Receipt, initialize, initialized and the thread.
    async fn start(&mut self, bootstrap: Duration) -> Result<(), HarnessFailure> {
        let receipt = tokio::time::timeout(bootstrap, self.next())
            .await
            .map_err(|_| HarnessFailure::Timeout)??;
        match receipt {
            Incoming::Response {
                id: RequestId::Number(0),
                result,
            } => {
                let receipt: Receipt = serde_json::from_value(result)
                    .map_err(|_| HarnessFailure::BootstrapRejected)?;
                if !receipt.matches(&self.binding, &self.runtime_version) {
                    return Err(HarnessFailure::BootstrapRejected);
                }
                let version = receipt.runtime_version;
                self.set(|p| p.runtime_version = Some(version));
            }
            Incoming::Failure {
                id: RequestId::Number(0),
                ..
            } => return Err(HarnessFailure::BootstrapRejected),
            _ => return Err(HarnessFailure::Protocol),
        }
        let params = protocol::initialize_params(&self.client_version);
        self.request(Method::Initialize, params)?;
        self.answer(Method::Initialize).await?;
        self.send(protocol::initialized())?;
        let result = match self.restore.take() {
            // The engine accepts it only under exactly the context it was
            // exported for; a refusal names the conversation as not
            // continuable, never a new thread in its place.
            Some(saved) => {
                let params = protocol::restore_params(&saved.context, &saved.replay);
                self.request(Method::HistoryRestore, params)?;
                self.answer(Method::HistoryRestore)
                    .await
                    .map_err(|f| match f {
                        HarnessFailure::Refused => HarnessFailure::RestoreFailed,
                        other => other,
                    })?
            }
            None => {
                self.request(Method::ThreadStart, protocol::thread_params(&self.binding))?;
                self.answer(Method::ThreadStart).await?
            }
        };
        let thread = result["thread"]["id"]
            .as_str()
            .filter(|t| bounded_id(t))
            .ok_or(HarnessFailure::Protocol)?;
        self.thread = Some(thread.to_owned());
        Ok(())
    }

    /// Waits for the answer to the one pending `method`, taking care of
    /// notifications and engine requests meanwhile.
    async fn answer(&mut self, method: Method) -> Result<Value, HarnessFailure> {
        let until = Instant::now() + REQUEST_TIMEOUT;
        loop {
            let frame = tokio::time::timeout_at(until, self.next())
                .await
                .map_err(|_| HarnessFailure::Timeout)??;
            match frame {
                Incoming::Response { id, result } => {
                    if self.take_pending(&id)? != method {
                        return Err(HarnessFailure::Protocol);
                    }
                    return Ok(result);
                }
                Incoming::Failure { id, .. } => {
                    self.take_pending(&id)?;
                    return Err(HarnessFailure::Refused);
                }
                other => self.on_frame(other)?,
            }
        }
    }

    /// The method of a pending request; an answer to anything else (an
    /// unknown or repeated id) is a protocol failure.
    fn take_pending(&mut self, id: &RequestId) -> Result<Method, HarnessFailure> {
        let RequestId::Number(n) = id else {
            return Err(HarnessFailure::Protocol);
        };
        let at = self
            .pending
            .iter()
            .position(|(p, _)| p == n)
            .ok_or(HarnessFailure::Protocol)?;
        Ok(self.pending.remove(at).1)
    }

    fn on_command(&mut self, command: Command) -> Result<(), HarnessFailure> {
        if self.reading.is_some() && matches!(command, Command::Send { .. } | Command::Renew { .. })
        {
            // The engine exports only an idle thread: the next turn (or a
            // restart) waits for the read.
            self.deferred = Some(command);
            return Ok(());
        }
        match command {
            Command::Renew {
                key,
                mcp,
                id,
                prompt,
            } => {
                if self.turn.is_none() {
                    self.renewal = Some((key, mcp, id, prompt));
                }
            }
            Command::Decide { approval, allow } => {
                // Only an approval still pending in this run and turn: the
                // engine forgets them when a turn ends or is interrupted.
                if let Some(at) = self.approvals.iter().position(|(a, _, _)| *a == approval) {
                    let (_, request, kind) = self.approvals.remove(at);
                    let decision = if allow { "accept" } else { "decline" };
                    // MCP approvals are elicitations: `action`, nothing else.
                    let reply = if kind == ApprovalKind::McpTool {
                        json!({"action": decision})
                    } else {
                        json!({"decision": decision})
                    };
                    self.send(protocol::reply(&request, reply)?)?;
                    self.set(|p| p.approvals.retain(|a| a.id != approval));
                }
            }
            Command::ToolResult {
                request,
                lease,
                success,
                text,
            } => {
                // Only the call this run and turn still waits for: a late
                // result from an earlier run (whose request id may be
                // reused) or turn is dropped.
                if let Some(at) = self
                    .tool_calls
                    .iter()
                    .position(|(r, _, l)| *r == request && *l == lease)
                {
                    let (_, entry, _) = self.tool_calls.remove(at);
                    let text = redact(&text, &self.secret);
                    self.send(protocol::reply(
                        &request,
                        json!({"contentItems":[{"type":"inputText","text":text}],"success":success}),
                    )?)?;
                    let state = if success {
                        EntryState::Complete
                    } else {
                        EntryState::Declined
                    };
                    self.set(|p| {
                        if let Some(e) = p.entries.iter_mut().find(|e| e.id == entry) {
                            e.state = state;
                        }
                    });
                }
            }
            Command::Send { id, prompt } => {
                let thread = self.thread.clone().ok_or(HarnessFailure::Protocol)?;
                if self.turn.is_some() {
                    return Ok(());
                }
                let params = protocol::turn_params(&thread, &prompt)?;
                let text = redact(&prompt, &self.secret);
                self.set(|p| {
                    push(
                        p,
                        HarnessEntry {
                            id: id.clone(),
                            role: HarnessRole::Operator,
                            text,
                            state: EntryState::Complete,
                        },
                    );
                    p.phase = HarnessPhase::Answering;
                    p.last_turn = None;
                    p.turn_handed = true;
                });
                self.turn = Some((id, None));
                self.turn_cancel = CancellationToken::new();
                self.interrupt_wanted = false;
                self.answer_bytes = 0;
                self.truncating = false;
                self.retry_refused = false;
                self.request(Method::TurnStart, params)?;
            }
            Command::Interrupt => {
                if self.turn.is_some() && !self.interrupt_wanted {
                    self.interrupt_wanted = true;
                    self.set(|p| p.phase = HarnessPhase::Interrupting);
                    self.interrupt()?;
                }
            }
        }
        Ok(())
    }

    /// Sends turn/interrupt once the engine's turn id is known.
    fn interrupt(&mut self) -> Result<(), HarnessFailure> {
        if let (Some(thread), Some((_, Some(turn)))) = (self.thread.clone(), self.turn.clone())
            && !self
                .pending
                .iter()
                .any(|(_, m)| *m == Method::TurnInterrupt)
        {
            self.request(
                Method::TurnInterrupt,
                json!({"threadId":thread,"turnId":turn}),
            )?;
            // The engine drops the turn's pending approvals and tool calls
            // when it is interrupted; a later answer to one would be stale.
            self.turn_cancel.cancel();
            self.approvals.clear();
            self.tool_calls.clear();
            self.mcp_items.clear();
            self.set(|p| p.approvals.clear());
        }
        Ok(())
    }

    fn on_frame(&mut self, frame: Incoming) -> Result<(), HarnessFailure> {
        match frame {
            Incoming::Response { id, result } => match self.take_pending(&id)? {
                Method::TurnStart => {
                    let turn = result["turn"]["id"]
                        .as_str()
                        .filter(|t| bounded_id(t))
                        .ok_or(HarnessFailure::Protocol)?;
                    if let Some((_, known @ None)) = self.turn.as_mut() {
                        *known = Some(turn.to_owned());
                    }
                    if self.interrupt_wanted {
                        self.interrupt()?;
                    }
                }
                Method::TurnInterrupt => {}
                Method::HistoryRead => {
                    self.reading = None;
                    let (Some(context), Some(replay)) = (
                        result.get("context").filter(|v| v.is_object()),
                        result.get("replay").filter(|v| v.is_object()),
                    ) else {
                        return Err(HarnessFailure::Protocol);
                    };
                    self.exported = Some(Replay {
                        context: context.clone(),
                        replay: replay.clone(),
                    });
                }
                Method::Initialize | Method::ThreadStart | Method::HistoryRestore => {
                    return Err(HarnessFailure::Protocol);
                }
            },
            Incoming::Failure { id, .. } => match self.take_pending(&id)? {
                Method::TurnStart => self.end_turn(TurnEnd::Refused),
                // The turn may have ended already.
                Method::TurnInterrupt => {}
                // Not saved this time; the conversation goes on.
                Method::HistoryRead => {
                    self.reading = None;
                    self.set(|p| p.save_failure = Some(HistoryFailure::Unavailable));
                    if let Some(command) = self.deferred.take() {
                        self.on_command(command)?;
                    }
                }
                Method::Initialize | Method::ThreadStart | Method::HistoryRestore => {
                    return Err(HarnessFailure::Refused);
                }
            },
            Incoming::Request { id, method, params } => match method.as_str() {
                "item/tool/call" => self.on_tool_call(id, &params)?,
                "item/commandExecution/requestApproval" => {
                    self.on_approval(id, ApprovalKind::Command, &params)?;
                }
                "item/fileChange/requestApproval" => {
                    self.on_approval(id, ApprovalKind::FileChange, &params)?;
                }
                "mcpServer/elicitation/request" => self.on_elicitation(id, &params)?,
                _ => self.send(protocol::refuse(&id, -32601)?)?,
            },
            Incoming::Notification { method, params } => self.on_notification(&method, &params)?,
        }
        Ok(())
    }

    /// A typed tool call: only for the turn being answered, once per call
    /// id, parsed into a closed set; anything else is answered as
    /// unavailable without running anything.
    fn on_tool_call(&mut self, id: RequestId, params: &Value) -> Result<(), HarnessFailure> {
        let call_id = params["callId"]
            .as_str()
            .filter(|c| bounded_id(c))
            .ok_or(HarnessFailure::Protocol)?
            .to_owned();
        if self.seen.contains(&call_id) {
            return Err(HarnessFailure::Protocol);
        }
        let call = self.ours(params).then(|| {
            super::tools::ToolCall::parse(
                params["namespace"].as_str(),
                params["tool"].as_str().unwrap_or_default(),
                &params["arguments"],
            )
        });
        // Once the turn is being stopped, nothing more is admitted.
        let call = call.filter(|_| !self.interrupt_wanted);
        let turn = self.turn.as_ref().and_then(|(_, t)| t.clone());
        let (Some(Some(call)), Some(sink), Some(turn)) = (call, self.tools.clone(), turn) else {
            self.seen.push(call_id);
            let text = super::tools::UNAVAILABLE;
            return self.send(protocol::reply(
                &id,
                json!({"contentItems":[{"type":"inputText","text":text}],"success":false}),
            )?);
        };
        if self.tool_calls.len() >= MAX_PENDING_DECISIONS {
            return Err(HarnessFailure::Bounds);
        }
        self.seen.push(call_id.clone());
        let entry = self.assistant_id(&call_id);
        let title = call.title();
        self.set(|p| {
            push(
                p,
                HarnessEntry {
                    id: entry.clone(),
                    role: HarnessRole::Tool,
                    text: title,
                    state: EntryState::Streaming,
                },
            );
        });
        let lease = ToolLease {
            run: self.run,
            turn,
        };
        self.tool_calls.push((id.clone(), entry, lease.clone()));
        sink(ToolRequest {
            request: id,
            call,
            lease,
            cancel: self.turn_cancel.clone(),
        });
        Ok(())
    }

    /// An engine request to run a command or change files: held for the
    /// operator under an id of the desktop's own, unless it cannot be
    /// granted at all (no project, changes in a read-only conversation,
    /// added permissions, network or roots), which is declined at once.
    fn on_approval(
        &mut self,
        id: RequestId,
        kind: ApprovalKind,
        params: &Value,
    ) -> Result<(), HarnessFailure> {
        let item = params["itemId"]
            .as_str()
            .filter(|i| bounded_id(i))
            .ok_or(HarnessFailure::Protocol)?
            .to_owned();
        // What would be approved must be shown whole (C21 F3): a command up
        // to its display bound, and a file change only with the files the
        // engine named for it; anything else is declined, never shown cut.
        let command = params["command"].as_str().map(|c| redact(c, &self.secret));
        let files = self
            .files
            .iter()
            .find(|(i, _)| *i == item)
            .map(|(_, f)| f.clone())
            .unwrap_or_default();
        let reviewable = match kind {
            ApprovalKind::Command => command
                .as_deref()
                .is_some_and(|c| !c.is_empty() && c.len() <= MAX_APPROVAL_COMMAND),
            ApprovalKind::FileChange => !files.is_empty(),
            ApprovalKind::McpTool => false,
        };
        let grantable = self.ours(params)
            && self.binding.project.is_some()
            && (kind == ApprovalKind::Command || self.binding.write)
            && params["additionalPermissions"].is_null()
            && params["networkApprovalContext"].is_null()
            && params["grantRoot"].is_null()
            && reviewable
            && self.approvals.len() < MAX_PENDING_DECISIONS;
        if !grantable {
            return self.send(protocol::reply(&id, json!({"decision":"decline"}))?);
        }
        let mut token = [0u8; 12];
        getrandom::fill(&mut token).map_err(|_| HarnessFailure::Protocol)?;
        let token = hex::encode(token);
        let text = |v: &Value, max: usize| {
            v.as_str().map(|t| {
                let mut t = redact(t, &self.secret);
                if t.len() > max {
                    let mut end = max;
                    while !t.is_char_boundary(end) {
                        end -= 1;
                    }
                    t.truncate(end);
                    t.push('…');
                }
                t
            })
        };
        let approval = HarnessApproval {
            id: token.clone(),
            kind,
            command,
            cwd: text(&params["cwd"], 1024),
            reason: text(&params["reason"], 1024),
            files,
            server: None,
            tool: None,
            arguments: None,
        };
        self.approvals.push((token, id, kind));
        self.set(|p| p.approvals.push(approval));
        Ok(())
    }

    /// An elicitation from the engine (C22). Only an MCP tool approval
    /// naming this turn's running call, on the in-scope server that started
    /// it, not yet asked, whose server holds a current sign-in, is held for
    /// the operator, with the server, tool and arguments of the call as
    /// the engine started it; anything else is declined at once.
    fn on_elicitation(&mut self, id: RequestId, params: &Value) -> Result<(), HarnessFailure> {
        let meta = &params["_meta"];
        let server = params["serverName"].as_str();
        let at = meta["airs_desktop_item_id"].as_str().and_then(|item| {
            self.mcp_items.iter().position(|m| {
                m.id == item && Some(m.server.as_str()) == server && !m.asked && !m.done
            })
        });
        let signed_in = |name: &str| {
            self.binding
                .mcp
                .iter()
                .position(|s| s.name == name)
                .and_then(|i| self.secret.mcp.get(i))
                .is_some_and(Option::is_some)
        };
        let held = self.ours(params)
            && params["mode"] == "form"
            && meta["codex_approval_kind"] == "mcp_tool_call"
            && self.approvals.len() < MAX_PENDING_DECISIONS;
        let Some(at) = at.filter(|&i| {
            held && self.mcp_items[i].reviewable && signed_in(&self.mcp_items[i].server)
        }) else {
            self.elicitations_declined += 1;
            let entry = self.assistant_id(&format!("declined-{}", self.elicitations_declined));
            self.set(|p| {
                push(
                    p,
                    HarnessEntry {
                        id: entry,
                        role: HarnessRole::Mcp,
                        text: DECLINED_MCP.into(),
                        state: EntryState::Declined,
                    },
                );
            });
            return self.send(protocol::reply(&id, json!({"action":"decline"}))?);
        };
        self.mcp_items[at].asked = true;
        let item = self.mcp_items[at].clone();
        let mut token = [0u8; 12];
        getrandom::fill(&mut token).map_err(|_| HarnessFailure::Protocol)?;
        let token = hex::encode(token);
        let approval = HarnessApproval {
            id: token.clone(),
            kind: ApprovalKind::McpTool,
            command: None,
            cwd: None,
            reason: None,
            files: Vec::new(),
            server: Some(item.server),
            tool: Some(item.tool),
            arguments: Some(item.arguments),
        };
        self.approvals.push((token, id, ApprovalKind::McpTool));
        self.set(|p| p.approvals.push(approval));
        Ok(())
    }

    /// A workspace MCP server's startup (C22): only scope servers, and
    /// never the engine's error text.
    fn on_mcp_startup(&mut self, params: &Value) {
        let Some(name) = params["name"].as_str() else {
            return;
        };
        let Some(i) = self.binding.mcp.iter().position(|s| s.name == name) else {
            return;
        };
        let status = match params["status"].as_str() {
            Some("starting") => McpStartup::Starting,
            Some("ready") => McpStartup::Ready,
            Some("cancelled") => McpStartup::Cancelled,
            _ => McpStartup::Failed,
        };
        let signed_out = self.secret.mcp.get(i).is_none_or(Option::is_none);
        let name = name.to_owned();
        self.set(|p| match p.mcp.iter_mut().find(|s| s.name == name) {
            Some(s) => s.status = status,
            None => p.mcp.push(McpServerStatus {
                name,
                status,
                signed_out,
            }),
        });
    }

    /// A command, file-change or tool item, as an entry of its turn.
    fn on_item(&mut self, item: &Value, done: bool) -> Result<(), HarnessFailure> {
        let Some(item_id) = item["id"].as_str().filter(|i| bounded_id(i)) else {
            return Err(HarnessFailure::Protocol);
        };
        let id = self.assistant_id(item_id);
        let state = match (done, item["status"].as_str()) {
            (false, _) => EntryState::Streaming,
            (true, Some("completed")) => EntryState::Complete,
            (true, Some("declined")) => EntryState::Declined,
            _ => EntryState::Failed,
        };
        let cut = |t: &str, max: usize| {
            let mut t = redact(t, &self.secret);
            if t.len() > max {
                let mut end = max;
                while !t.is_char_boundary(end) {
                    end -= 1;
                }
                t.truncate(end);
                t.push('…');
            }
            t
        };
        let (role, text) = match item["type"].as_str() {
            Some("commandExecution") => {
                let mut text = cut(
                    item["command"].as_str().unwrap_or_default(),
                    MAX_COMMAND_TEXT,
                );
                if done {
                    if let Some(output) =
                        item["aggregatedOutput"].as_str().filter(|o| !o.is_empty())
                    {
                        text.push('\n');
                        text.push_str(&cut(output, MAX_COMMAND_OUTPUT));
                    }
                    if let Some(code) = item["exitCode"].as_i64() {
                        text.push_str(&format!("\n(exit code {code})"));
                    }
                }
                (HarnessRole::Command, text)
            }
            Some("fileChange") => {
                let paths: Vec<String> = item["changes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["path"].as_str())
                    .take(64)
                    .map(|p| cut(p, 1024))
                    .collect();
                if !done && !self.files.iter().any(|(i, _)| i == item_id) {
                    self.files.push((item_id.to_owned(), paths.clone()));
                }
                (HarnessRole::FileChange, paths.join("\n"))
            }
            // The tool's own entry was made from its typed call.
            Some("dynamicToolCall") => return Ok(()),
            Some("mcpToolCall") => {
                let server = cut(item["server"].as_str().unwrap_or_default(), 64);
                let tool = cut(item["tool"].as_str().unwrap_or_default(), 128);
                let whole = redact(&item["arguments"].to_string(), &self.secret);
                let reviewable = whole.len() <= MAX_APPROVAL_COMMAND;
                let arguments = if reviewable {
                    whole
                } else {
                    cut(&whole, MAX_COMMAND_TEXT)
                };
                // Only calls of this conversation's servers are this turn's.
                if !self.binding.mcp.iter().any(|s| s.name == server) {
                    return Ok(());
                }
                let known = self.mcp_items.iter().position(|m| m.id == item_id);
                match known {
                    Some(i) => self.mcp_items[i].done |= done,
                    None if self.mcp_items.len() < MAX_MCP_ITEMS => self.mcp_items.push(McpItem {
                        id: item_id.to_owned(),
                        server: server.clone(),
                        tool: tool.clone(),
                        arguments: arguments.clone(),
                        reviewable,
                        asked: false,
                        done,
                    }),
                    None => return Err(HarnessFailure::Bounds),
                }
                let mut text = format!("{server} · {tool}\n{arguments}");
                if done {
                    let outcome = if item["status"] == "completed" {
                        item["result"]["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|c| c["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        item["error"]["message"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned()
                    };
                    if !outcome.is_empty() {
                        text.push('\n');
                        text.push_str(&cut(&outcome, MAX_COMMAND_OUTPUT));
                    }
                }
                let state = match state {
                    EntryState::Failed
                        if item["error"]["message"] == "user rejected MCP tool call" =>
                    {
                        EntryState::Declined
                    }
                    s => s,
                };
                self.set(|p| {
                    upsert(
                        p,
                        HarnessEntry {
                            id,
                            role: HarnessRole::Mcp,
                            text,
                            state,
                        },
                    )
                });
                return Ok(());
            }
            _ => return Ok(()),
        };
        self.set(|p| {
            upsert(
                p,
                HarnessEntry {
                    id,
                    role,
                    text,
                    state,
                },
            )
        });
        Ok(())
    }

    /// Whether `params` belong to the turn being answered.
    fn ours(&self, params: &Value) -> bool {
        let thread_ok = params["threadId"].as_str() == self.thread.as_deref();
        let turn = params["turnId"]
            .as_str()
            .or_else(|| params["turn"]["id"].as_str());
        thread_ok && matches!((&self.turn, turn), (Some((_, Some(t))), Some(n)) if t == n)
    }

    fn on_notification(&mut self, method: &str, params: &Value) -> Result<(), HarnessFailure> {
        match method {
            "mcpServer/startupStatus/updated" => self.on_mcp_startup(params),
            "turn/started" if params["threadId"].as_str() == self.thread.as_deref() => {
                // The engine's turn id, if its response has not come yet.
                if let (Some((_, known @ None)), Some(t)) = (
                    self.turn.as_mut(),
                    params["turn"]["id"].as_str().filter(|t| bounded_id(t)),
                ) {
                    *known = Some(t.to_owned());
                    if self.interrupt_wanted {
                        self.interrupt()?;
                    }
                }
            }
            "item/started" if self.ours(params) && params["item"]["type"] == "agentMessage" => {
                let item = params["item"]["id"]
                    .as_str()
                    .filter(|i| bounded_id(i))
                    .ok_or(HarnessFailure::Protocol)?;
                let id = self.assistant_id(item);
                self.set(|p| {
                    if !p.entries.iter().any(|e| e.id == id) {
                        push(
                            p,
                            HarnessEntry {
                                id,
                                role: HarnessRole::Assistant,
                                text: String::new(),
                                state: EntryState::Streaming,
                            },
                        );
                    }
                });
            }
            "item/agentMessage/delta" if self.ours(params) => {
                // Item ids are bounded wherever they can name an entry.
                let (Some(item), Some(delta)) = (
                    params["itemId"].as_str().filter(|i| bounded_id(i)),
                    params["delta"].as_str(),
                ) else {
                    return Err(HarnessFailure::Protocol);
                };
                let id = self.assistant_id(item);
                self.append(&id, delta)?;
            }
            "item/completed" if self.ours(params) && params["item"]["type"] == "agentMessage" => {
                let (Some(item), Some(text)) = (
                    params["item"]["id"].as_str().filter(|i| bounded_id(i)),
                    params["item"]["text"].as_str(),
                ) else {
                    return Err(HarnessFailure::Protocol);
                };
                if !self.truncating {
                    let id = self.assistant_id(item);
                    let text = redact(text, &self.secret);
                    self.complete(&id, text)?;
                }
            }
            "item/started" | "item/completed"
                if self.ours(params)
                    && matches!(
                        params["item"]["type"].as_str(),
                        Some("commandExecution" | "fileChange" | "dynamicToolCall" | "mcpToolCall")
                    ) =>
            {
                self.on_item(&params["item"], method == "item/completed")?;
            }
            "thread/tokenUsage/updated"
                if params["threadId"].as_str() == self.thread.as_deref() =>
            {
                let total = &params["tokenUsage"]["total"];
                // Counters the presentation can hold exactly (JavaScript's
                // safe integers); a larger one is left out, never published.
                let count = |v: &Value| v.as_u64().filter(|n| *n <= MAX_SAFE_COUNT);
                if let (Some(input), Some(output), Some(all)) = (
                    count(&total["inputTokens"]),
                    count(&total["outputTokens"]),
                    count(&total["totalTokens"]),
                ) {
                    self.set(|p| {
                        p.usage = Some(HarnessUsage {
                            input,
                            output,
                            total: all,
                        })
                    });
                }
            }
            // The engine is about to retry the Gateway on its own (its stream
            // reconnects). No answer is resent unasked: the turn is stopped.
            "error" if self.ours(params) && params["willRetry"] == true && !self.retry_refused => {
                self.retry_refused = true;
                self.interrupt_wanted = true;
                self.interrupt()?;
            }
            "turn/completed" if self.ours(params) => {
                let end = match params["turn"]["status"].as_str() {
                    _ if self.retry_refused => TurnEnd::Failed,
                    _ if self.truncating => TurnEnd::Truncated,
                    Some("completed") => TurnEnd::Completed,
                    Some("interrupted") => TurnEnd::Interrupted,
                    _ => TurnEnd::Failed,
                };
                self.end_turn(end);
            }
            _ => {}
        }
        Ok(())
    }

    /// An answer's entry: its turn's entry and the engine's item id (item
    /// ids are the provider's, and need not differ between turns).
    fn assistant_id(&self, item: &str) -> String {
        let turn = self.turn.as_ref().map_or("", |(entry, _)| entry.as_str());
        format!("{turn}/{item}")
    }

    /// An answer's completed text, within the same bounds as its deltas: the
    /// turn's answer bytes and the conversation's transcript. Text past them
    /// is cut at the bound, the entry marked truncated and the answer
    /// stopped; it is never shown as complete.
    fn complete(&mut self, id: &str, text: String) -> Result<(), HarnessFailure> {
        let Ok(mut p) = self.progress.lock() else {
            return Err(HarnessFailure::Protocol);
        };
        let before = p
            .entries
            .iter()
            .find(|e| e.id == id)
            .map_or(0, |e| e.text.len());
        let room = MAX_TRANSCRIPT_BYTES.saturating_sub(transcript_bytes(&p)) + before;
        let turn_room = MAX_ANSWER_BYTES.saturating_sub(self.answer_bytes.saturating_sub(before));
        let allowed = room.min(turn_room);
        let fits = text.len() <= allowed;
        let mut kept = text;
        if !fits {
            let mut end = allowed;
            while !kept.is_char_boundary(end) {
                end -= 1;
            }
            kept.truncate(end);
        }
        self.answer_bytes = self.answer_bytes.saturating_sub(before) + kept.len();
        let state = if fits {
            EntryState::Complete
        } else {
            EntryState::Truncated
        };
        match p.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => {
                e.text = kept;
                e.state = state;
            }
            None => push(
                &mut p,
                HarnessEntry {
                    id: id.to_owned(),
                    role: HarnessRole::Assistant,
                    text: kept,
                    state,
                },
            ),
        }
        drop(p);
        self.pulse.notify_one();
        if !fits {
            self.truncating = true;
            self.interrupt_wanted = true;
            return self.interrupt();
        }
        Ok(())
    }

    fn append(&mut self, id: &str, delta: &str) -> Result<(), HarnessFailure> {
        if self.truncating {
            return Ok(());
        }
        let delta = redact(delta, &self.secret);
        let over = self.answer_bytes + delta.len() > MAX_ANSWER_BYTES
            || self
                .progress
                .lock()
                .map(|p| transcript_bytes(&p) + delta.len() > MAX_TRANSCRIPT_BYTES)
                .unwrap_or(true);
        if over {
            // Keep what fits, stop the answer and say so.
            self.truncating = true;
            self.set(|p| {
                if let Some(e) = p.entries.iter_mut().find(|e| e.id == id) {
                    e.state = EntryState::Truncated;
                }
            });
            self.interrupt_wanted = true;
            return self.interrupt();
        }
        self.answer_bytes += delta.len();
        let id = id.to_owned();
        let secrets = &self.secret;
        update(&self.progress, &self.pulse, |p| {
            if let Some(e) = p.entries.iter_mut().find(|e| e.id == id) {
                e.text.push_str(&delta);
                // A key split across deltas is whole only once joined.
                if secrets.all().any(|s| e.text.contains(s)) {
                    e.text = redact(&e.text, secrets);
                }
            }
        });
        Ok(())
    }

    fn end_turn(&mut self, end: TurnEnd) {
        self.turn = None;
        // The transcript now holds this turn: an earlier export no longer
        // matches it, so a renewal restores only this turn's own export
        // and, if it is refused, fails rather than restoring an older one
        // (C20 review F1).
        self.latest = None;
        self.turn_cancel.cancel();
        self.interrupt_wanted = false;
        // The engine forgets a turn's approvals and tool calls with it; so
        // does the desktop (a late decision or result is never sent).
        self.approvals.clear();
        self.tool_calls.clear();
        self.seen.clear();
        self.files.clear();
        self.mcp_items.clear();
        let state = match end {
            TurnEnd::Completed => EntryState::Complete,
            TurnEnd::Interrupted => EntryState::Interrupted,
            TurnEnd::Truncated => EntryState::Truncated,
            TurnEnd::Failed | TurnEnd::Refused => EntryState::Failed,
        };
        self.set(|p| {
            for e in p
                .entries
                .iter_mut()
                .filter(|e| e.role == HarnessRole::Assistant && e.state == EntryState::Streaming)
            {
                e.state = state;
            }
            for e in p
                .entries
                .iter_mut()
                .filter(|e| e.role != HarnessRole::Assistant && e.state == EntryState::Streaming)
            {
                e.state = EntryState::Interrupted;
            }
            p.approvals.clear();
            p.last_turn = Some(end);
            p.phase = HarnessPhase::Ready;
        });
        // Export the idle thread for protected history and renewal.
        if let Some(thread) = self.thread.clone()
            && self
                .request(Method::HistoryRead, json!({"threadId": thread}))
                .is_ok()
        {
            self.reading = Some(Instant::now() + REQUEST_TIMEOUT);
        }
    }
}

fn frame_of(event: Option<Event>) -> Result<Incoming, HarnessFailure> {
    match event {
        Some(Event::Frame(frame)) => Ok(frame),
        Some(Event::Failed(f)) => Err(f),
        Some(Event::Eof) | None => Err(HarnessFailure::Exited),
    }
}

fn bounded_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

fn transcript_bytes(p: &Progress) -> usize {
    p.entries.iter().map(|e| e.text.len()).sum()
}

/// Adds an entry, dropping the oldest past the bound.
fn push(p: &mut Progress, entry: HarnessEntry) {
    p.entries.push(entry);
    while p.entries.len() > MAX_ENTRIES || transcript_bytes(p) > MAX_TRANSCRIPT_BYTES {
        p.entries.remove(0);
    }
}

/// Replaces an entry's text and state (or adds it) under the same bounds
/// as a new entry: the transcript's oldest entries give way, never the
/// bound (C21 F4: a command's completion replaces its shorter start).
fn upsert(p: &mut Progress, entry: HarnessEntry) {
    match p.entries.iter().position(|e| e.id == entry.id) {
        Some(at) => {
            p.entries[at].text = entry.text;
            p.entries[at].state = entry.state;
            while transcript_bytes(p) > MAX_TRANSCRIPT_BYTES && p.entries.len() > 1 {
                let oldest = usize::from(p.entries[0].id == entry.id);
                p.entries.remove(oldest);
            }
        }
        None => push(p, entry),
    }
}

/// A renewal's new credentials (`None`: kept), entry ID and prompt.
type Renewal = (
    Option<Zeroizing<String>>,
    Option<Vec<Option<Zeroizing<String>>>>,
    String,
    Zeroizing<String>,
);

/// What the engine was given that must never be shown: the Gateway key
/// and the workspace MCP tokens (C22).
pub(crate) struct Secrets {
    pub key: Zeroizing<String>,
    pub mcp: Vec<Option<Zeroizing<String>>>,
}

impl Secrets {
    fn all(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.key.as_str())
            .chain(self.mcp.iter().flatten().map(|t| t.as_str()))
            .filter(|s| !s.is_empty())
    }
}

/// Text with any copy of the key or a token replaced.
fn redact(text: &str, secrets: &Secrets) -> String {
    let mut out = text.to_owned();
    for secret in secrets.all() {
        if out.contains(secret) {
            out = out.replace(secret, "[redacted]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sidecar whose leader never exits and cannot be killed.
    struct Stuck {
        kills: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Sidecar for Stuck {
        fn pid(&self) -> u32 {
            999_999
        }
        fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>> {
            None
        }
        fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
            None
        }
        fn exited(&mut self) -> std::io::Result<bool> {
            Ok(false)
        }
        fn kill_group(&mut self) {
            self.kills.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        fn reap(&mut self) -> std::io::Result<Option<i32>> {
            Ok(None)
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_process_that_cannot_be_reaped_is_a_cleanup_failure_never_success() {
        let kills = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut stuck = Stuck {
            kills: Arc::clone(&kills),
        };
        let started = Instant::now();
        assert_eq!(stop(&mut stuck, None).await, Cleanup::Failed);
        // Killed once the grace period passed, given up after the forced one.
        assert_eq!(kills.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(started.elapsed() >= GRACE + FORCED);
        assert!(started.elapsed() < GRACE + FORCED + Duration::from_millis(100));
    }

    fn conversation(writer: std_mpsc::SyncSender<Outbound>) -> Conversation {
        let (_, events) = mpsc::channel(1);
        let (_, commands) = mpsc::channel(1);
        Conversation {
            binding: Binding {
                tenant: String::new(),
                workspace: String::new(),
                credential: String::new(),
                credential_epoch: 1,
                endpoint: String::new(),
                project: None,
                write: false,
                mcp: Vec::new(),
            },
            runtime_version: String::new(),
            client_version: String::new(),
            secret: Secrets {
                key: Zeroizing::new("k".into()),
                mcp: Vec::new(),
            },
            writer: Some(writer),
            events,
            commands,
            progress: ProgressSlot::default(),
            pulse: Arc::default(),
            next_id: 1,
            pending: Vec::new(),
            thread: Some("t".into()),
            turn: None,
            interrupt_wanted: false,
            answer_bytes: 0,
            truncating: false,
            retry_refused: false,
            history: None,
            latest: None,
            restore: None,
            reading: None,
            deferred: None,
            exported: None,
            renewal: None,
            tools: None,
            approvals: Vec::new(),
            tool_calls: Vec::new(),
            run: 1,
            turn_cancel: CancellationToken::new(),
            seen: Vec::new(),
            files: Vec::new(),
            mcp_items: Vec::new(),
            elicitations_declined: 0,
        }
    }

    #[test]
    fn a_full_queue_or_too_many_pending_requests_is_a_bound_never_a_wait() {
        // The writer's queue is full (the engine is not reading): refused at
        // once, never waited on.
        let (writer, _reader) = std_mpsc::sync_channel(1);
        let mut c = conversation(writer);
        assert!(c.request(Method::TurnInterrupt, json!({})).is_ok());
        assert_eq!(
            c.request(Method::TurnInterrupt, json!({})),
            Err(HarnessFailure::Bounds)
        );
        // At most eight requests wait for answers.
        let (writer, reader) = std_mpsc::sync_channel(64);
        let mut c = conversation(writer);
        for _ in 0..8 {
            c.request(Method::TurnInterrupt, json!({}))
                .expect("bounded");
        }
        assert_eq!(
            c.request(Method::TurnInterrupt, json!({})),
            Err(HarnessFailure::Bounds)
        );
        // A closed writer is an exited engine.
        drop(reader);
        c.pending.clear();
        assert_eq!(
            c.request(Method::TurnInterrupt, json!({})),
            Err(HarnessFailure::Exited)
        );
        // An answer to nothing pending, or a repeated one, is a protocol failure.
        assert_eq!(
            c.take_pending(&RequestId::Number(99)),
            Err(HarnessFailure::Protocol)
        );
    }

    #[test]
    fn a_tool_call_runs_once_and_only_for_the_turn_being_answered() {
        let (writer, reader) = std_mpsc::sync_channel(64);
        let mut c = conversation(writer);
        c.turn = Some(("u-1".into(), Some("turn-1".into())));
        let ran = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&ran);
        c.tools = Some(Arc::new(move |r: ToolRequest| {
            seen.lock().expect("fixture").push(r.call);
        }));
        let call = |call: &str, turn: &str, tool: &str| json!({"threadId":"t","turnId":turn,"callId":call,"namespace":"airs_desktop","tool":tool,"arguments":{}});
        c.on_tool_call(
            RequestId::Number(1),
            &call("call-1", "turn-1", "supplyChain"),
        )
        .expect("runs");
        assert_eq!(ran.lock().expect("fixture").len(), 1);
        // The same call again is a protocol failure; nothing runs twice.
        assert_eq!(
            c.on_tool_call(
                RequestId::Number(2),
                &call("call-1", "turn-1", "supplyChain")
            ),
            Err(HarnessFailure::Protocol)
        );
        // Another turn's call, or an unknown tool, is answered as
        // unavailable without running anything, and cannot be repeated.
        c.on_tool_call(
            RequestId::Number(3),
            &call("call-2", "turn-0", "supplyChain"),
        )
        .expect("answered");
        c.on_tool_call(
            RequestId::Number(4),
            &call("call-3", "turn-1", "applyChange"),
        )
        .expect("answered");
        assert_eq!(
            c.on_tool_call(
                RequestId::Number(5),
                &call("call-3", "turn-1", "supplyChain")
            ),
            Err(HarnessFailure::Protocol)
        );
        assert_eq!(ran.lock().expect("fixture").len(), 1);
        let replies: Vec<String> = reader
            .try_iter()
            .map(|o| String::from_utf8_lossy(o.0.as_slice()).into_owned())
            .collect();
        assert_eq!(replies.len(), 2, "{replies:?}");
        assert!(replies.iter().all(
            |r| r.contains(super::super::tools::UNAVAILABLE) && r.contains("\"success\":false")
        ));
        // Only the one that ran waits for its result.
        assert_eq!(c.tool_calls.len(), 1);
    }

    #[test]
    fn a_stopped_turns_tool_calls_are_cancelled_refused_and_never_answered_late() {
        // C21 review F1: the run and turn lease of host tool calls.
        let (writer, reader) = std_mpsc::sync_channel(64);
        let mut c = conversation(writer);
        c.turn = Some(("u-1".into(), Some("turn-1".into())));
        let handed = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = Arc::clone(&handed);
        c.tools = Some(Arc::new(move |r: ToolRequest| {
            seen.lock().expect("fixture").push(r);
        }));
        let call = |call: &str, turn: &str| json!({"threadId":"t","turnId":turn,"callId":call,"namespace":"airs_desktop","tool":"supplyChain","arguments":{}});
        c.on_tool_call(RequestId::Number(1), &call("call-1", "turn-1"))
            .expect("runs");
        let first = handed.lock().expect("fixture").remove(0);
        assert_eq!(first.lease.turn, "turn-1");
        assert!(!first.cancel.is_cancelled());
        // Being stopped: nothing more is admitted, and the turn's end
        // cancels what is under way.
        c.interrupt_wanted = true;
        c.on_tool_call(RequestId::Number(2), &call("call-2", "turn-1"))
            .expect("answered");
        assert!(handed.lock().expect("fixture").is_empty());
        c.end_turn(TurnEnd::Interrupted);
        assert!(first.cancel.is_cancelled());
        // Replies to the engine (the turn's export request aside).
        let replies = |reader: &std_mpsc::Receiver<Outbound>| -> Vec<String> {
            reader
                .try_iter()
                .map(|o| String::from_utf8_lossy(o.0.as_slice()).into_owned())
                .filter(|r| !r.contains("\"method\""))
                .collect()
        };
        let refused = replies(&reader);
        assert_eq!(refused.len(), 1, "{refused:?}");
        assert!(refused[0].contains(super::super::tools::UNAVAILABLE));
        // Its late result is never sent.
        let result = |r: &ToolRequest, lease: &ToolLease| Command::ToolResult {
            request: r.request.clone(),
            lease: lease.clone(),
            success: true,
            text: "late".into(),
        };
        c.on_command(result(&first, &first.lease)).expect("dropped");
        assert!(replies(&reader).is_empty());
        // The next turn reuses request ids: only its own lease answers.
        c.turn = Some(("u-2".into(), Some("turn-2".into())));
        c.on_tool_call(RequestId::Number(1), &call("call-3", "turn-2"))
            .expect("runs");
        let next = handed.lock().expect("fixture").remove(0);
        assert_eq!(next.request, first.request);
        assert_ne!(next.lease, first.lease);
        c.on_command(result(&next, &first.lease)).expect("dropped");
        assert!(replies(&reader).is_empty());
        // Another run (a restarted engine) is another lease too.
        let other_run = ToolLease {
            run: next.lease.run.wrapping_add(1),
            turn: next.lease.turn.clone(),
        };
        c.on_command(result(&next, &other_run)).expect("dropped");
        assert!(replies(&reader).is_empty());
        c.on_command(result(&next, &next.lease)).expect("answered");
        let answered = replies(&reader);
        assert_eq!(answered.len(), 1, "{answered:?}");
        assert!(answered[0].contains("late") && answered[0].contains("\"success\":true"));
    }

    #[test]
    fn approvals_are_asked_only_with_a_folder_and_file_changes_only_with_changes_allowed() {
        let (writer, reader) = std_mpsc::sync_channel(64);
        let mut c = conversation(writer);
        c.turn = Some(("u-1".into(), Some("turn-1".into())));
        let ask =
            |item: &str| json!({"threadId":"t","turnId":"turn-1","itemId":item,"command":"ls"});
        let declined = |reader: &std_mpsc::Receiver<Outbound>| {
            reader
                .try_iter()
                .map(|o| String::from_utf8_lossy(o.0.as_slice()).into_owned())
                .filter(|r| r.contains("\"decline\""))
                .count()
        };
        // No folder: declined at once, nothing held.
        c.on_approval(RequestId::Number(1), ApprovalKind::Command, &ask("i-1"))
            .expect("declined");
        assert_eq!(declined(&reader), 1);
        assert!(c.approvals.is_empty());
        // A read-only folder: a command is held; a file change, added
        // permissions or another turn's request is declined at once.
        c.binding.project = Some("/Users/someone/work".into());
        c.on_approval(RequestId::Number(2), ApprovalKind::FileChange, &ask("i-2"))
            .expect("declined");
        let mut extra = ask("i-3");
        extra["additionalPermissions"] = json!({"network": true});
        c.on_approval(RequestId::Number(3), ApprovalKind::Command, &extra)
            .expect("declined");
        let mut other = ask("i-4");
        other["turnId"] = json!("turn-0");
        c.on_approval(RequestId::Number(4), ApprovalKind::Command, &other)
            .expect("declined");
        assert_eq!(declined(&reader), 3);
        assert!(c.approvals.is_empty());
        c.on_approval(RequestId::Number(5), ApprovalKind::Command, &ask("i-5"))
            .expect("held");
        assert_eq!(c.approvals.len(), 1);
        assert_eq!(declined(&reader), 0);
        // Changes allowed: a file change naming its files is held too.
        c.binding.write = true;
        c.on_item(
            &json!({"id":"i-6","type":"fileChange","status":"inProgress",
                "changes":[{"path":"/Users/someone/work/a.txt","kind":"add"}]}),
            false,
        )
        .expect("started");
        c.on_approval(RequestId::Number(6), ApprovalKind::FileChange, &ask("i-6"))
            .expect("held");
        assert_eq!(c.approvals.len(), 2);
        assert_eq!(declined(&reader), 0);
        // What cannot be shown whole is declined, never shown cut (C21 F3):
        // a file change whose files the engine did not name, a command past
        // the display bound, or none at all.
        c.on_approval(RequestId::Number(7), ApprovalKind::FileChange, &ask("i-7"))
            .expect("declined");
        let mut long = ask("i-8");
        long["command"] = json!(format!(
            "echo safe; {}rm -rf ~",
            " ".repeat(MAX_APPROVAL_COMMAND)
        ));
        c.on_approval(RequestId::Number(8), ApprovalKind::Command, &long)
            .expect("declined");
        let mut none = ask("i-9");
        none["command"] = Value::Null;
        c.on_approval(RequestId::Number(9), ApprovalKind::Command, &none)
            .expect("declined");
        assert_eq!(declined(&reader), 3);
        assert_eq!(c.approvals.len(), 2);
        // A held command is shown whole.
        let shown = c.progress.lock().expect("progress").approvals[0]
            .command
            .clone();
        assert_eq!(shown.as_deref(), Some("ls"));
    }

    #[test]
    fn only_an_mcp_approval_naming_this_turns_running_call_is_asked() {
        let (writer, reader) = std_mpsc::sync_channel(64);
        let mut c = conversation(writer);
        c.turn = Some(("u-1".into(), Some("turn-1".into())));
        let server = |name: &str| super::super::mcp::ScopeServer {
            name: name.into(),
            server_id: format!("id-{name}"),
            url: format!("https://gw.example/{name}/mcp"),
            revision: "r".into(),
            grant: Some("g".into()),
        };
        c.binding.mcp = vec![server("tools"), server("docs")];
        c.secret.key = Zeroizing::new("gw-key-of-a-realistic-length".into());
        c.secret.mcp = vec![Some(Zeroizing::new("t".repeat(32))), None];
        let item = |id: &str, server: &str| {
            json!({"id": id, "type": "mcpToolCall", "server": server, "tool": "lookup",
                "status": "inProgress", "arguments": {"query": "record-1"}})
        };
        c.on_item(&item("call-1", "tools"), false).expect("started");
        c.on_item(&item("call-2", "docs"), false).expect("started");
        // A server outside the scope is not this conversation's call.
        c.on_item(&item("call-3", "other"), false).expect("ignored");
        let ask = |item: &str, server: &str| {
            json!({"threadId": "t", "turnId": "turn-1", "serverName": server, "mode": "form",
                "message": "Allow?", "requestedSchema": {"type": "object", "properties": {}},
                "_meta": {"codex_approval_kind": "mcp_tool_call", "airs_desktop_item_id": item,
                    "tool_params": {"query": "forged"}}})
        };
        let declined = |reader: &std_mpsc::Receiver<Outbound>| {
            reader
                .try_iter()
                .map(|o| String::from_utf8_lossy(o.0.as_slice()).into_owned())
                .filter(|r| r.contains("\"action\":\"decline\""))
                .count()
        };
        let mut forged = vec![
            ask("call-9", "tools"),
            ask("call-1", "docs"),
            ask("call-3", "other"),
            ask("call-2", "docs"),
        ];
        let mut other_turn = ask("call-1", "tools");
        other_turn["turnId"] = json!("turn-0");
        forged.push(other_turn);
        let mut url_mode = ask("call-1", "tools");
        url_mode["mode"] = json!("url");
        forged.push(url_mode);
        let mut server_owned = ask("call-1", "tools");
        server_owned["_meta"] = json!({"note": "server-owned"});
        forged.push(server_owned);
        for (n, params) in forged.iter().enumerate() {
            c.on_elicitation(RequestId::Number(n as u64), params)
                .expect("declined");
        }
        // Unknown item, wrong server, outside the scope, a signed-out
        // server, another turn, another mode and a server's own request.
        assert_eq!(declined(&reader), forged.len());
        assert!(c.approvals.is_empty());
        // The running call on its signed-in server is asked, once, with the
        // call's own arguments (never the request's).
        c.on_elicitation(RequestId::Number(50), &ask("call-1", "tools"))
            .expect("asked");
        assert_eq!(c.approvals.len(), 1);
        let shown = c.progress.lock().expect("progress").approvals[0].clone();
        assert_eq!(shown.kind, ApprovalKind::McpTool);
        assert_eq!(shown.server.as_deref(), Some("tools"));
        assert_eq!(shown.tool.as_deref(), Some("lookup"));
        assert!(
            shown
                .arguments
                .as_deref()
                .is_some_and(|a| a.contains("record-1") && !a.contains("forged"))
        );
        c.on_elicitation(RequestId::Number(51), &ask("call-1", "tools"))
            .expect("declined");
        assert_eq!(declined(&reader), 1);
        // Allowed once: exactly `{"action":"accept"}`.
        c.on_command(Command::Decide {
            approval: shown.id.clone(),
            allow: true,
        })
        .expect("decided");
        let replies: Vec<String> = reader
            .try_iter()
            .map(|o| String::from_utf8_lossy(o.0.as_slice()).into_owned())
            .collect();
        assert_eq!(replies.len(), 1);
        assert!(
            replies[0].contains("\"id\":50") && replies[0].contains("{\"action\":\"accept\"}"),
            "{}",
            replies[0]
        );
        // A call whose arguments cannot be shown whole is never asked.
        let mut big = item("call-4", "tools");
        big["arguments"] = json!({"query": "q".repeat(MAX_APPROVAL_COMMAND)});
        c.on_item(&big, false).expect("started");
        c.on_elicitation(RequestId::Number(60), &ask("call-4", "tools"))
            .expect("declined");
        assert_eq!(declined(&reader), 1);
        assert!(
            c.approvals.is_empty(),
            "the decided one is gone; none was added"
        );
        // Startup is published for scope servers only, never the engine's text.
        c.on_mcp_startup(&json!({"name": "tools", "status": "ready", "error": "airs mcp login"}));
        c.on_mcp_startup(&json!({"name": "docs", "status": "failed"}));
        c.on_mcp_startup(&json!({"name": "other", "status": "ready"}));
        let mcp = c.progress.lock().expect("progress").mcp.clone();
        assert_eq!(mcp.len(), 2);
        assert_eq!(mcp[0].status, McpStartup::Ready);
        assert!(!mcp[0].signed_out && mcp[1].signed_out);
    }

    #[test]
    fn a_replaced_entry_keeps_the_transcript_within_its_bound() {
        let mut p = Progress::default();
        // Near the bound with short command starts.
        for n in 0..103 {
            push(
                &mut p,
                HarnessEntry {
                    id: format!("e-{n}"),
                    role: HarnessRole::Command,
                    text: "c".repeat(10 * 1024),
                    state: EntryState::Streaming,
                },
            );
        }
        let before = p.entries.len();
        assert!(transcript_bytes(&p) <= MAX_TRANSCRIPT_BYTES);
        // A completion with output past the bound: older entries give way,
        // the completed one stays, and the bound holds.
        upsert(
            &mut p,
            HarnessEntry {
                id: "e-102".into(),
                role: HarnessRole::Command,
                text: "o".repeat(64 * 1024),
                state: EntryState::Complete,
            },
        );
        assert!(transcript_bytes(&p) <= MAX_TRANSCRIPT_BYTES);
        assert!(p.entries.len() < before);
        assert_eq!(
            p.entries.last().map(|e| e.state),
            Some(EntryState::Complete)
        );
        assert_eq!(p.entries.last().map(|e| e.id.as_str()), Some("e-102"));
    }

    fn home(roots: &[Vec<u8>]) -> (Home, PathBuf) {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).expect("fixture");
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("fixture")
            .join(format!("airs-home-{}", hex::encode(entropy)));
        (Home::make(&root, roots).expect("fixture"), root)
    }

    #[test]
    fn the_engine_environment_names_no_credential_and_follows_the_transport_policy() {
        let (h, root) = home(&[b"-----BEGIN CERTIFICATE-----\n".to_vec()]);
        let names = |proxy| {
            environment(&h, &proxy)
                .into_iter()
                .map(|(k, _)| k.into_string().expect("fixture"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(ProxyEnv::Direct),
            [
                "PATH",
                "LANG",
                "TMPDIR",
                "AIRS_HARNESS_HOME",
                "CODEX_CA_CERTIFICATE",
                "NO_PROXY"
            ]
        );
        assert_eq!(
            names(ProxyEnv::System),
            [
                "PATH",
                "LANG",
                "TMPDIR",
                "AIRS_HARNESS_HOME",
                "CODEX_CA_CERTIFICATE"
            ]
        );
        let explicit = environment(&h, &ProxyEnv::Explicit("http://proxy.example:3128".into()));
        assert!(explicit.contains(&("HTTPS_PROXY".into(), "http://proxy.example:3128".into())));
        assert!(explicit.contains(&("HTTP_PROXY".into(), "http://proxy.example:3128".into())));
        let direct = environment(&h, &ProxyEnv::Direct);
        assert!(direct.contains(&("NO_PROXY".into(), "*".into())));
        assert!(direct.contains(&("PATH".into(), PATH.into())));
        let roots = PathBuf::from(
            &direct
                .iter()
                .find(|(k, _)| k == "CODEX_CA_CERTIFICATE")
                .expect("fixture")
                .1,
        );
        assert_eq!(roots.parent(), Some(h.path.as_path()));
        assert!(
            std::fs::read_to_string(&roots)
                .expect("fixture")
                .starts_with("-----BEGIN CERTIFICATE-----")
        );
        assert!(direct.len() <= ENV_NAMES.len());
        std::fs::remove_dir_all(root).expect("fixture");
        // Without additional roots the engine uses its own trust store.
        let (h, root) = home(&[]);
        assert!(
            !environment(&h, &ProxyEnv::Direct)
                .iter()
                .any(|(k, _)| k == "CODEX_CA_CERTIFICATE")
        );
        std::fs::remove_dir_all(root).expect("fixture");
    }

    #[test]
    fn runtime_homes_are_private_ephemeral_and_only_ours_are_removed() {
        use std::os::unix::fs::PermissionsExt;
        let (first, root) = home(&[]);
        let mode = |p: &Path| std::fs::metadata(p).expect("fixture").permissions().mode() & 0o777;
        assert_eq!(mode(&root), 0o700);
        assert_eq!(mode(&first.path), 0o700);
        assert_eq!(mode(&first.tmp()), 0o700);
        // Something else in the root, and a home left by an earlier run.
        std::fs::write(root.join("keep.txt"), b"not ours").expect("fixture");
        std::fs::create_dir(root.join("run-notours")).expect("fixture");
        let second = Home::make(&root, &[]).expect("fixture");
        assert!(!first.path.exists(), "an earlier run's home is removed");
        assert!(root.join("keep.txt").exists() && root.join("run-notours").exists());
        assert!(second.remove());
        assert!(!second.path.exists());
        assert!(root.join("keep.txt").exists());
        // A home that is not a run directory of its root is never removed.
        let forged = Home {
            path: root.join("run-notours"),
            root: root.clone(),
            roots: None,
        };
        assert!(!forged.remove());
        assert!(root.join("run-notours").exists());
        std::fs::remove_dir_all(root).expect("fixture");
        // A relative root is refused.
        assert!(Home::make(Path::new("relative"), &[]).is_err());
    }
}
