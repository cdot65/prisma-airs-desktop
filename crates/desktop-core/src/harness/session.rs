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
    Cleanup, EntryState, HarnessEntry, HarnessFailure, HarnessPhase, HarnessRole, HarnessUsage,
    MAX_ANSWER_BYTES, MAX_ENTRIES, MAX_TRANSCRIPT_BYTES,
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
    /// The Gateway credential was renewed (C20): the engine is restarted
    /// with it, the conversation restored from its last export, and then
    /// `prompt` is sent.
    Renew {
        key: Zeroizing<String>,
        id: String,
        prompt: Zeroizing<String>,
    },
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
        commands,
        progress,
        pulse: notify,
        client_version,
        bootstrap_timeout,
        history,
        restore,
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
    };
    let mut next = Launch {
        key: Key::Reader(key),
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
    let Launch { key, restore, then } = launch;
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
    let frame = match protocol::bootstrap(&shared.binding, &key) {
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
        secret: key,
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
    };
    let ended = if reader.is_none() || writer.is_none() {
        Err(HarnessFailure::StartFailed)
    } else {
        conversation
            .drive(cancel, deadline, shared.bootstrap_timeout, then)
            .await
    };
    shared.commands = std::mem::replace(&mut conversation.commands, mpsc::channel(1).1);
    let renewal = conversation.renewal.take();
    let latest = conversation.latest.take();
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
    let next = match (renewal, ended.is_ok() && cleanup != Cleanup::Failed) {
        (Some((key, id, prompt)), true) => {
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
                    key: Key::Renewed(key),
                    restore: latest,
                    then: Some((id, prompt)),
                })
            }
        }
        _ => None,
    };
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
    secret: Zeroizing<String>,
    writer: Option<std_mpsc::SyncSender<Outbound>>,
    events: mpsc::Receiver<Event>,
    commands: mpsc::Receiver<Command>,
    history: Option<HistorySink>,
    /// The last export (or the restored one), for a renewal.
    latest: Option<Replay>,
    /// Restored instead of starting a new thread.
    restore: Option<Replay>,
    /// A history read is pending until this: commands wait for it.
    reading: Option<Instant>,
    deferred: Option<Command>,
    /// An export to save once the frame is handled.
    exported: Option<Replay>,
    /// The conversation continues in a new engine with this credential.
    renewal: Option<(Zeroizing<String>, String, Zeroizing<String>)>,
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
                self.request(Method::ThreadStart, protocol::thread_params())?;
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
        if self.reading.is_some() && !matches!(command, Command::Interrupt) {
            // The engine exports only an idle thread: the next turn (or a
            // restart) waits for the read.
            self.deferred = Some(command);
            return Ok(());
        }
        match command {
            Command::Renew { key, id, prompt } => {
                if self.turn.is_none() {
                    self.renewal = Some((key, id, prompt));
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
            Incoming::Request { id, method, .. } => {
                // Nothing that needs a decision is offered before C21: every
                // approval or elicitation is declined, anything else refused.
                let reply = match method.as_str() {
                    "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                        protocol::reply(&id, json!({"decision":"decline"}))
                    }
                    "mcpServer/elicitation/request" => {
                        protocol::reply(&id, json!({"action":"decline"}))
                    }
                    _ => protocol::refuse(&id, -32601),
                }?;
                self.send(reply)?;
            }
            Incoming::Notification { method, params } => self.on_notification(&method, &params)?,
        }
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
        let secret: &str = &self.secret;
        update(&self.progress, &self.pulse, |p| {
            if let Some(e) = p.entries.iter_mut().find(|e| e.id == id) {
                e.text.push_str(&delta);
                // A key split across deltas is whole only once joined.
                if !secret.is_empty() && e.text.contains(secret) {
                    e.text = redact(&e.text, secret);
                }
            }
        });
        Ok(())
    }

    fn end_turn(&mut self, end: TurnEnd) {
        self.turn = None;
        self.interrupt_wanted = false;
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

/// Text with any copy of the key replaced.
fn redact(text: &str, secret: &str) -> String {
    if secret.is_empty() || !text.contains(secret) {
        return text.to_owned();
    }
    text.replace(secret, "[redacted]")
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
            },
            runtime_version: String::new(),
            client_version: String::new(),
            secret: Zeroizing::new("k".into()),
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
