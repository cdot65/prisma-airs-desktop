//! Gateway files (C18, D045): the chosen workspace's files read, one file
//! uploaded and one deleted through the SDK's `RuntimeClient`, with the
//! workspace inference key and only to the runtime endpoint stored with it
//! (the same key, endpoint and binding as a Playground run). An upload is
//! a file the operator chose in a native sheet, read once (at most
//! [`MAX_UPLOAD_BYTES`]) and held only in memory until it is sent; the
//! presentation sees its name and size, never its path or bytes. It is
//! uploaded for the `assistants` purpose, the one a Responses run can
//! attach. Every request is sent once and never retried, and a failed one
//! keeps what the SDK established about it. Files stay in the Gateway until
//! they are deleted: nothing here deletes one by itself or reads one back.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use scm_sdk::airs::gateway::models::{
    BinaryFile, GatewayInferenceInputCreateFileRequest,
    GatewayInferenceInputCreateFileRequestPurpose, GatewayInferenceInputListFilesQuery,
};
use scm_sdk::transport::Transport;
use scm_sdk::{Limits, RequestOptions, Session};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::gateway_inference::{InferenceFailure, Target, client, short};
use crate::runtime::Outcome;

/// Largest file uploaded (well inside the SDK's 8 MiB request bound).
pub const MAX_UPLOAD_BYTES: usize = 4 * 1024 * 1024;
/// Files shown from one read (the rest are reported as more).
pub const MAX_FILES: usize = 200;
/// Longest one file request may take.
pub const FILE_DEADLINE: Duration = Duration::from_secs(60);
/// The purpose every upload is made for.
pub const PURPOSE: &str = "assistants";
/// Latest creation time shown, in seconds: the end of the range a
/// presentation date can render (8.64e15 ms). A later one is unknown.
pub const MAX_CREATED_AT: u64 = 8_640_000_000_000;
const MAX_NAME: usize = 255;

/// One file the Gateway holds for the workspace (metadata only).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayFile {
    pub id: String,
    pub filename: String,
    #[ts(type = "number")]
    pub bytes: u64,
    /// When the Gateway created it (seconds since 1970), or `None` when
    /// the reply named a time past [`MAX_CREATED_AT`].
    #[ts(type = "number | null")]
    pub created_at: Option<u64>,
    pub purpose: String,
    pub status: String,
}

fn count(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n >= 0.0 && *n <= 9.0e15)
        .map(|n| n as u64)
}

/// A creation time: absent, negative or not a number is malformed; one
/// too late to render is kept as unknown, so the file stays usable.
fn created(value: Option<&Value>) -> Option<Option<u64>> {
    let n = value?.as_f64().filter(|n| n.is_finite() && *n >= 0.0)?;
    Some((n <= MAX_CREATED_AT as f64).then_some(n as u64))
}

/// A file as the SDK returned it, within this view's bounds.
fn file_of(value: &Value) -> Option<GatewayFile> {
    Some(GatewayFile {
        id: short(value.get("id"), 128).filter(|i| crate::gateway::is_id(i))?,
        filename: short(value.get("filename"), MAX_NAME)?,
        bytes: count(value.get("bytes"))?,
        created_at: created(value.get("created_at"))?,
        purpose: short(value.get("purpose"), 64)?,
        status: short(value.get("status"), 32)?,
    })
}

/// The name an upload is sent with: the chosen file's name with the
/// characters a multipart file name may not carry replaced, bounded.
pub fn upload_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | '"') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let mut cut = clean.len().min(MAX_NAME);
    while !clean.is_char_boundary(cut) {
        cut -= 1;
    }
    match clean[..cut].trim() {
        "" | "." | ".." => "upload".to_owned(),
        name => name.to_owned(),
    }
}

/// Which file request this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FileAction {
    Read,
    Upload,
    Delete,
}

/// What one request does.
pub enum FileRequest {
    Read,
    Upload { name: String, bytes: Vec<u8> },
    Delete { id: String },
}

impl FileRequest {
    pub fn action(&self) -> FileAction {
        match self {
            Self::Read => FileAction::Read,
            Self::Upload { .. } => FileAction::Upload,
            Self::Delete { .. } => FileAction::Delete,
        }
    }
}

/// What one request has established (shared with its job).
#[derive(Clone, Debug, Default)]
pub struct FileProgress {
    /// The request was handed to the transport.
    pub sent: bool,
    /// A read's files (the first [`MAX_FILES`]) and whether more exist.
    pub listed: Option<(Vec<GatewayFile>, bool)>,
    /// The file an upload created.
    pub created: Option<GatewayFile>,
    pub failure: Option<InferenceFailure>,
}

pub type FileSlot = Arc<Mutex<FileProgress>>;

/// Everything a request needs, resolved at admission (the key is read in
/// the job, off the lock).
pub struct FileInputs {
    pub endpoint: String,
    pub trace: String,
    pub request: FileRequest,
    pub key: Box<dyn FnOnce() -> Result<Zeroizing<String>, InferenceFailure> + Send>,
    pub transport: Arc<dyn Transport>,
    pub slot: FileSlot,
}

fn file_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_retries = 0;
    limits.operation_timeout = FILE_DEADLINE;
    limits
}

fn outcome_of(error: &scm_sdk::Error) -> Outcome {
    match error.remote_outcome() {
        scm_sdk::RemoteOutcome::NotSent => Outcome::NotSent,
        scm_sdk::RemoteOutcome::Rejected => Outcome::Rejected,
        _ => Outcome::Unknown,
    }
}

enum Done {
    Listed(Vec<GatewayFile>, bool),
    Created(GatewayFile),
    Deleted(bool),
    Malformed,
}

fn listed(value: &Value) -> Done {
    let Some(rows) = value.get("data").and_then(Value::as_array) else {
        return Done::Malformed;
    };
    let files: Option<Vec<GatewayFile>> = rows.iter().take(MAX_FILES).map(file_of).collect();
    files.map_or(Done::Malformed, |files| {
        Done::Listed(files, rows.len() > MAX_FILES)
    })
}

/// Performs one admitted file request: reads the key and sends the
/// request once. A read or a refused request changes nothing; an upload or
/// delete stopped after it was sent may have taken effect.
pub async fn perform(inputs: FileInputs, cancel: CancellationToken, deadline: Instant) -> Outcome {
    let FileInputs {
        endpoint,
        trace,
        request,
        key,
        transport,
        slot,
    } = inputs;
    let fail = |failure: InferenceFailure, outcome: Outcome| {
        if let Ok(mut p) = slot.lock() {
            p.failure = Some(failure);
        }
        outcome
    };
    let key = match tokio::task::spawn_blocking(key).await {
        Ok(Ok(key)) => key,
        Ok(Err(failure)) => return fail(failure, Outcome::NotSent),
        Err(_) => return fail(InferenceFailure::CredentialMissing, Outcome::NotSent),
    };
    let session = Session::new();
    let target = Target {
        endpoint: &endpoint,
        trace: &trace,
        route: None,
        transport,
    };
    let runtime = match client(&target, &key, session.clone(), file_limits()) {
        Ok(runtime) => runtime,
        Err(failure) => return fail(failure, Outcome::NotSent),
    };
    drop(key);
    let remaining = deadline.saturating_duration_since(Instant::now());
    let Some(options) = RequestOptions::with_timeout(remaining.max(Duration::from_millis(1)))
        .and_then(|o| o.with_retries(0))
        .ok()
    else {
        return fail(InferenceFailure::Configuration, Outcome::NotSent);
    };
    let body = match &request {
        FileRequest::Upload { name, bytes } => {
            let file = BinaryFile::new(bytes.clone())
                .and_then(|f| f.with_filename(name.clone()))
                .and_then(|f| {
                    GatewayInferenceInputCreateFileRequest::builder()
                        .file(f)
                        .purpose(GatewayInferenceInputCreateFileRequestPurpose::Assistants)
                        .build()
                });
            match file {
                Ok(body) => Some(body),
                Err(_) => return fail(InferenceFailure::Configuration, Outcome::NotSent),
            }
        }
        _ => None,
    };
    if cancel.is_cancelled() {
        return Outcome::NotSent;
    }
    if let Ok(mut p) = slot.lock() {
        p.sent = true;
    }
    let work = async {
        match (&request, &body) {
            (FileRequest::Upload { .. }, Some(body)) => runtime
                .create_file(body, Some(&options))
                .await
                .map(|f| serde_json::to_value(&f).ok().and_then(|v| file_of(&v)))
                .map(|f| f.map_or(Done::Malformed, Done::Created)),
            // A reply naming another file says nothing about this one.
            (FileRequest::Delete { id }, _) => {
                runtime.delete_file(id, Some(&options)).await.map(|r| {
                    if &r.id == id {
                        Done::Deleted(r.deleted)
                    } else {
                        Done::Malformed
                    }
                })
            }
            _ => {
                let query: GatewayInferenceInputListFilesQuery =
                    serde_json::from_value(serde_json::json!({}))
                        .map_err(|_| scm_sdk::Error::new(scm_sdk::ErrorKind::Validation))?;
                runtime
                    .list_files(&query, Some(&options))
                    .await
                    .map(|l| serde_json::to_value(&l).map_or(Done::Malformed, |v| listed(&v)))
            }
        }
    };
    let done = tokio::select! {
        done = work => done,
        () = cancel.cancelled() => {
            session.cancel();
            // Sent: an upload or delete may have taken effect.
            return Outcome::Unknown;
        }
    };
    match done {
        Ok(Done::Listed(files, more)) => {
            if let Ok(mut p) = slot.lock() {
                p.listed = Some((files, more));
            }
            Outcome::Completed
        }
        Ok(Done::Created(file)) => {
            if let Ok(mut p) = slot.lock() {
                p.created = Some(file);
            }
            Outcome::Completed
        }
        Ok(Done::Deleted(true)) => Outcome::Completed,
        // The Gateway answered that it did not delete the file.
        Ok(Done::Deleted(false)) => fail(InferenceFailure::Rejected, Outcome::Rejected),
        // It answered, but not with a file this view can show, or (for a
        // delete) not about the file asked for: the change may still have
        // taken effect.
        Ok(Done::Malformed) => fail(
            InferenceFailure::InvalidResponse,
            match request.action() {
                FileAction::Read => Outcome::Completed,
                _ => Outcome::Unknown,
            },
        ),
        Err(error) => fail(InferenceFailure::from_sdk(&error), outcome_of(&error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn files_are_read_within_their_bounds() {
        let row = |id: &str| {
            json!({"id": id, "object": "file", "bytes": 12, "created_at": 1_760_000_000,
                "filename": "notes.txt", "purpose": "assistants", "status": "processed"})
        };
        let Done::Listed(files, more) = listed(&json!({"data": [row("file-1")]})) else {
            panic!("listed");
        };
        assert_eq!(files[0].id, "file-1");
        assert_eq!(files[0].bytes, 12);
        assert!(!more);
        let many: Vec<Value> = (0..=MAX_FILES).map(|i| row(&format!("file-{i}"))).collect();
        let Done::Listed(files, more) = listed(&json!({"data": many})) else {
            panic!("listed");
        };
        assert_eq!(files.len(), MAX_FILES);
        assert!(more, "more than the bound is reported, never hidden");
        assert!(matches!(
            listed(&json!({"data": [row("../etc")]})),
            Done::Malformed
        ));
        let mut bad = row("file-2");
        bad["filename"] = json!("a\u{7}b");
        assert!(matches!(listed(&json!({"data": [bad]})), Done::Malformed));
        // A time too late to render is unknown, never a refused list.
        let mut late = row("file-3");
        late["created_at"] = json!(9_000_000_000_000_u64);
        let Done::Listed(files, _) = listed(&json!({"data": [late, row("file-4")]})) else {
            panic!("listed");
        };
        assert_eq!(files[0].created_at, None);
        assert_eq!(files[1].created_at, Some(1_760_000_000));
        let mut edge = row("file-5");
        edge["created_at"] = json!(MAX_CREATED_AT);
        assert!(
            matches!(listed(&json!({"data": [edge]})), Done::Listed(f, _) if f[0].created_at == Some(MAX_CREATED_AT))
        );
        for bad_time in [json!(-1), json!("2026-10-01"), json!(null)] {
            let mut bad = row("file-6");
            bad["created_at"] = bad_time;
            assert!(matches!(listed(&json!({"data": [bad]})), Done::Malformed));
        }
    }

    #[test]
    fn upload_names_are_ones_a_multipart_part_may_carry() {
        assert_eq!(upload_name("notes.txt"), "notes.txt");
        assert_eq!(upload_name("a/b\\c\"d\ne.txt"), "a_b_c_d_e.txt");
        assert_eq!(upload_name(".."), "upload");
        assert_eq!(upload_name(&"é".repeat(200)).len(), 254);
    }
}
