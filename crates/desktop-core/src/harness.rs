//! The native Harness (C19, D046): the pinned Rust engine (9331a58,
//! `airs-desktop/1`) run directly as a desktop-owned sidecar.
//!
//! One conversation at a time. It is bound at its start to the tenant
//! context, the chosen Gateway workspace and the workspace inference key
//! stored for it (sent only to the runtime endpoint stored with it); a
//! change to any of them, ending it, sleep or quitting stops it. The
//! engine is verified against the compiled pin before it is started, gets
//! no inherited environment, no argument but `desktop-server` and no
//! credential except in the one bootstrap frame on stdin. It runs in its
//! own process group with its own ephemeral runtime home; stopping it
//! closes its input, waits a bounded grace period, then kills the group
//! and reaps it, and removes the home. Nothing of the conversation is
//! written to disk (protected history is C20); tools, approvals and
//! project access are C21, workspace MCP servers C22.

pub mod callback;
pub mod engine;
pub mod history;
pub mod identity;
pub mod protocol;
pub mod session;
pub mod sidecar;
pub mod tools;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Why a conversation failed or could not start (sanitized: never the
/// engine's own text, which may echo model or tool output).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum HarnessFailure {
    /// The bundled engine is missing, is not the pinned one or was changed.
    #[error("engine unavailable")]
    EngineUnavailable,
    #[error("credential missing")]
    CredentialMissing,
    #[error("credential locked")]
    CredentialLocked,
    #[error("credential denied")]
    CredentialDenied,
    /// The process could not be started, or its runtime home created.
    #[error("start failed")]
    StartFailed,
    /// The engine refused the bootstrap (or its receipt named another
    /// binding).
    #[error("bootstrap rejected")]
    BootstrapRejected,
    /// No receipt within the bootstrap bound.
    #[error("timed out")]
    Timeout,
    /// A malformed, unexpected or duplicate frame.
    #[error("protocol")]
    Protocol,
    /// A frame, the output or the stream exceeded its bound.
    #[error("bounds")]
    Bounds,
    /// The engine ended or stopped answering on its own.
    #[error("engine exited")]
    Exited,
    /// The engine refused a request (a thread or turn could not start).
    #[error("refused")]
    Refused,
    /// The Gateway or model ended the answer with an error.
    #[error("turn failed")]
    TurnFailed,
    /// The process could not be stopped and reaped within its bound.
    #[error("cleanup failed")]
    CleanupFailed,
    /// A saved conversation could not be continued: the engine refused its
    /// replay, or there was none to restore after a renewal (C20).
    #[error("restore failed")]
    RestoreFailed,
    /// Company sign-in must be renewed before anything is sent (C20).
    #[error("sign-in needed")]
    SignInNeeded,
}

/// Where a conversation stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HarnessPhase {
    /// Verifying and starting the engine.
    #[default]
    Starting,
    Ready,
    /// A turn is being answered.
    Answering,
    /// Stopping the answer.
    Interrupting,
    /// Renewing the Gateway sign-in, then restarting the engine and
    /// restoring the conversation before the message is sent (C20).
    Renewing,
    /// The sign-in must be renewed, and the engine restarted: the operator
    /// is asked to allow changes in the project again first (C21).
    ConfirmWrite,
    /// Stopping the engine.
    Ending,
    Ended,
    Failed,
}

/// Who wrote an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HarnessRole {
    Operator,
    Assistant,
    /// A command the engine ran in the project's sandbox (C21).
    Command,
    /// Files the engine changed in the project (C21).
    FileChange,
    /// A typed desktop tool the engine called (C21): its text is the
    /// desktop's own description, never the model's.
    Tool,
}

/// How an entry stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryState {
    Streaming,
    Complete,
    /// The answer was stopped before it was finished.
    Interrupted,
    /// The answer reached its bound; the rest was not kept.
    Truncated,
    Failed,
    /// The operator declined it, or it was refused (C21).
    Declined,
}

/// What an engine request for approval asks (C21).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalKind {
    /// Run one command in the project's sandbox.
    Command,
    /// Change files in the project.
    FileChange,
}

/// A pending approval, as the operator sees it (C21). Its id is the
/// desktop's own, issued for this run and turn; it never outlives them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessApproval {
    pub id: String,
    pub kind: ApprovalKind,
    /// The command to run (commands).
    pub command: Option<String>,
    /// Where it runs, inside the project.
    pub cwd: Option<String>,
    /// The engine's stated reason, if any.
    pub reason: Option<String>,
    /// The files it changes (file changes).
    pub files: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessEntry {
    pub id: String,
    pub role: HarnessRole,
    pub text: String,
    pub state: EntryState,
}

/// How the engine was stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Cleanup {
    /// It exited after its input was closed.
    Graceful,
    /// It was killed after the grace period, and reaped.
    Forced,
    /// It could not be reaped within the bound.
    Failed,
}

/// Tokens the engine reported for the conversation so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, TS, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessUsage {
    #[ts(type = "number")]
    pub input: u64,
    #[ts(type = "number")]
    pub output: u64,
    #[ts(type = "number")]
    pub total: u64,
}

/// Entries kept for one conversation (memory only).
pub const MAX_ENTRIES: usize = 200;
/// Text kept for one conversation, all entries together.
pub const MAX_TRANSCRIPT_BYTES: usize = 1024 * 1024;
/// One answer's text; past it the answer is stopped and marked truncated.
pub const MAX_ANSWER_BYTES: usize = 256 * 1024;
