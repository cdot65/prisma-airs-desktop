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
pub mod mcp;
pub mod mcp_auth;
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
    /// The workspace MCP sign-ins do not fit in the engine's bootstrap
    /// together (C22): fewer servers must be on.
    #[error("MCP scope too large")]
    McpTooLarge,
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
    /// A workspace MCP server's tool call (C22): the server, tool,
    /// arguments and its result or error, bounded and redacted.
    Mcp,
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
    /// Call one tool of a workspace MCP server, once (C22).
    McpTool,
    /// Read the tenant through one of the desktop's own read tools, once
    /// (C32): read-only by construction.
    DesktopRead,
}

/// What a decision lets the engine do, as the core can establish it (C32).
/// Only a desktop read, or a command in a read-only project sandbox, is
/// read-only; a file change, a command with changes allowed and any remote
/// MCP tool (whose effect the desktop cannot know) may change things, and
/// allowing one needs the native confirmation, never in-page consent alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalEffect {
    ReadOnly,
    MayChange,
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
    /// The workspace MCP server (its namespace) and tool, and the call's
    /// arguments as JSON, from the engine's running call (MCP tools, C22).
    pub server: Option<String>,
    pub tool: Option<String>,
    pub arguments: Option<String>,
    /// What allowing it lets the engine do (C32). Always sent; optional in
    /// the presentation's type only so snapshots written before C32 still
    /// type-check (the validator and the presentation treat absent as
    /// `mayChange`).
    #[ts(as = "Option<ApprovalEffect>", optional)]
    pub effect: ApprovalEffect,
}

/// How a workspace MCP server started in the running engine (C22).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum McpStartup {
    Starting,
    Ready,
    Failed,
    Cancelled,
}

/// One scope server's startup in the running engine (C22).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatus {
    /// Its namespace.
    pub name: String,
    pub status: McpStartup,
    /// It was given the signed-out placeholder: its tools are unavailable.
    pub signed_out: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessEntry {
    pub id: String,
    pub role: HarnessRole,
    pub text: String,
    pub state: EntryState,
}

/// Where a conversation's message numbering (`u-{n}`) continues from a
/// restored transcript: past every message it shows. A transcript that
/// dropped its oldest entries shows fewer messages than its last number, so
/// counting them would reuse an ID it still shows, and the presentation
/// tells a new message by its ID (C32).
pub(crate) fn messages_after(entries: &[HarnessEntry]) -> u64 {
    let shown = entries
        .iter()
        .filter(|e| e.role == HarnessRole::Operator)
        .count() as u64;
    entries
        .iter()
        .filter_map(|e| e.id.strip_prefix("u-")?.parse::<u64>().ok())
        .fold(shown, u64::max)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, role: HarnessRole) -> HarnessEntry {
        HarnessEntry {
            id: id.to_owned(),
            role,
            text: String::new(),
            state: EntryState::Complete,
        }
    }

    #[test]
    fn a_restored_transcript_continues_past_every_message_it_shows() {
        // Its first messages gave way to the bounds: two shown, numbered 51 and 52.
        let evicted = [
            entry("a-50", HarnessRole::Assistant),
            entry("u-51", HarnessRole::Operator),
            entry("a-51", HarnessRole::Assistant),
            entry("u-52", HarnessRole::Operator),
        ];
        assert_eq!(messages_after(&evicted), 52);
        assert_eq!(messages_after(&[]), 0);
        // An imported transcript keeps its own IDs: counted, never reused.
        let imported = [
            entry("msg-a", HarnessRole::Operator),
            entry("u-2", HarnessRole::Operator),
            entry("msg-c", HarnessRole::Operator),
        ];
        assert_eq!(messages_after(&imported), 3);
        let numbered = [
            entry("u-1", HarnessRole::Operator),
            entry("u-7", HarnessRole::Tool),
        ];
        assert_eq!(messages_after(&numbered), 7);
    }
}
