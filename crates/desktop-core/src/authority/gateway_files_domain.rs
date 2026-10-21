//! Gateway files inside the authority (C18, D045): the chosen workspace's
//! files, read, uploaded and deleted with the workspace inference key under
//! the same binding as a Playground run (the current context, the current
//! workspace reference and the key stored for that workspace, sent only to
//! its endpoint). One file request at a time runs as an operation in the
//! reference's domain, so a context change or another choice of workspace
//! stops it; replacing or removing the key stops it too, and from that
//! moment the view is withdrawn. A file to upload is chosen in the native
//! sheet, read once and held here (never in the presentation) until one
//! upload takes it; a new choice replaces it. An upload or delete that may
//! have taken effect without a known result is kept on this Mac with its
//! trace, and is never sent again.

use std::sync::Arc;

use serde::Serialize;
use tokio::time::Instant;
use ts_rs::TS;

use super::inference_domain::{KeyAdmission, trace_id};
use super::{Refusal, StagedFileFailure, State};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway::ListedWorkspace;
use crate::gateway_files::{
    FILE_DEADLINE, FileAction, FileInputs, FileProgress, FileRequest, FileSlot, GatewayFile,
    MAX_FILES, MAX_UPLOAD_BYTES, upload_name,
};
use crate::gateway_inference::InferenceFailure;
use crate::runtime::{OperationId, OperationState, Outcome, StopReason};
use crate::tenants::TenantId;
use crate::vault::{RecordId, SessionVault};

struct Chosen {
    token: String,
    name: String,
    bytes: Vec<u8>,
}

struct Pending {
    operation: OperationId,
    action: FileAction,
    filename: Option<String>,
    file: Option<String>,
    bytes: Option<u64>,
    trace: String,
    slot: FileSlot,
    settled: Option<Outcome>,
}

/// The files view of one workspace, bound like a Playground run.
pub(super) struct Files {
    context: ContextRef,
    tenant: TenantId,
    reference: WorkspaceRef,
    workspace: ListedWorkspace,
    record: RecordId,
    retired: bool,
    listed: Option<(Vec<GatewayFile>, bool)>,
    read_failure: Option<InferenceFailure>,
    chosen: Option<Chosen>,
    choosing: Option<u64>,
    choose_failure: Option<StagedFileFailure>,
    pending: Option<Pending>,
    last: Option<FileActionView>,
}

/// The file chosen to upload, as the presentation sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChosenFile {
    /// Names this choice in an upload (a new choice issues a new one).
    pub token: String,
    pub name: String,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// An upload or delete: going (no outcome yet) or how it settled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct FileActionView {
    pub action: FileAction,
    pub filename: Option<String>,
    /// The file deleted, or the one an upload created.
    pub file: Option<String>,
    #[ts(type = "number | null")]
    pub bytes: Option<u64>,
    /// Sent as `x-portkey-trace-id`.
    pub trace: String,
    pub outcome: Option<Outcome>,
    pub failure: Option<InferenceFailure>,
    /// The request was handed to the Gateway.
    pub sent: bool,
}

/// The chosen workspace's files while they are bound to the active
/// context, that workspace and the key they were read with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayFilesView {
    pub workspace: ListedWorkspace,
    /// The files last read (`None`: not read yet).
    pub files: Option<Vec<GatewayFile>>,
    /// The Gateway listed more files than are shown.
    pub more: bool,
    pub reading: bool,
    pub read_failure: Option<InferenceFailure>,
    pub chosen: Option<ChosenFile>,
    pub choosing: bool,
    pub choose_failure: Option<StagedFileFailure>,
    /// The upload or delete going now, or the last one.
    pub action: Option<FileActionView>,
}

fn action_view(p: &Pending) -> FileActionView {
    let progress = p.slot.lock().map(|g| g.clone()).unwrap_or_default();
    FileActionView {
        action: p.action,
        filename: p.filename.clone(),
        file: progress
            .created
            .as_ref()
            .map(|f| f.id.clone())
            .or_else(|| p.file.clone()),
        bytes: p.bytes,
        trace: p.trace.clone(),
        outcome: p.settled,
        failure: progress.failure,
        sent: progress.sent,
    }
}

fn token() -> Option<String> {
    let mut bytes = [0u8; 12];
    getrandom::fill(&mut bytes).ok()?;
    Some(format!("gf-{}", hex::encode(bytes)))
}

impl State {
    /// Whether `files` may still be shown and used: not retired, under the
    /// active context and the chosen workspace, with the key it was bound
    /// to.
    fn files_bound(&self, files: &Files) -> bool {
        !files.retired
            && Some(&files.context) == self.active_ref.as_ref()
            && self.workspace_current(Some(&files.reference))
            && self.tenant(&files.tenant).is_some_and(|t| {
                matches!(&t.inference, super::ScanCapability::Ready { record, .. } if record == &files.record)
            })
    }

    /// Whether the files last read for `reference` with `record` list
    /// `file` (a run may attach only such a file).
    pub(super) fn file_listed(
        &self,
        reference: &WorkspaceRef,
        record: &RecordId,
        file: &str,
    ) -> bool {
        self.files.as_ref().is_some_and(|f| {
            self.files_bound(f)
                && &f.reference == reference
                && &f.record == record
                && f.listed
                    .as_ref()
                    .is_some_and(|(files, _)| files.iter().any(|x| x.id == file))
        })
    }

    fn file_going(&self) -> bool {
        self.files.as_ref().is_some_and(|f| {
            f.pending.as_ref().is_some_and(|p| {
                p.settled.is_none() && self.operations.state(&p.operation).is_some()
            })
        })
    }

    /// The files view for this admission: the bound one, or a new one (the
    /// previous view's request must have ended).
    fn files_for(
        &mut self,
        context: &ContextRef,
        reference: &WorkspaceRef,
        admission: &KeyAdmission,
    ) -> Result<&mut Files, Refusal> {
        let same = self.files.as_ref().is_some_and(|f| {
            self.files_bound(f)
                && &f.context == context
                && &f.reference == reference
                && f.record == admission.record
        });
        if !same {
            if self.file_going() {
                return Err(Refusal::Busy);
            }
            self.files = Some(Files {
                context: context.clone(),
                tenant: admission.tenant.clone(),
                reference: reference.clone(),
                workspace: admission.workspace.clone(),
                record: admission.record.clone(),
                retired: false,
                listed: None,
                read_failure: None,
                chosen: None,
                choosing: None,
                choose_failure: None,
                pending: None,
                last: None,
            });
        }
        self.files.as_mut().ok_or(Refusal::Busy)
    }

    fn admit_file(
        &mut self,
        reference: &WorkspaceRef,
        admission: KeyAdmission,
        request: FileRequest,
        filename: Option<String>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), Refusal> {
        if self.file_going() {
            return Err(Refusal::Busy);
        }
        let trace = trace_id().ok_or(Refusal::Busy)?;
        let slot = FileSlot::default();
        let action = request.action();
        let (file, bytes) = match &request {
            FileRequest::Delete { id } => (Some(id.clone()), None),
            FileRequest::Upload { bytes, .. } => (None, Some(bytes.len() as u64)),
            FileRequest::Read => (None, None),
        };
        let size = bytes.unwrap_or(0) as usize;
        let inputs = FileInputs {
            endpoint: admission.endpoint.clone(),
            trace: trace.clone(),
            request,
            key: admission.key,
            transport: Arc::clone(&admission.transport),
            slot: Arc::clone(&slot),
        };
        let work: crate::runtime::Work = Box::new(move |claim| {
            Box::pin(crate::gateway_files::perform(
                inputs,
                claim.cancel,
                claim.deadline,
            ))
        });
        let (operation, starts) = self.operations.admit_for(
            reference.as_str(),
            size,
            work,
            Instant::now(),
            FILE_DEADLINE,
        )?;
        let files = self.files.as_mut().ok_or(Refusal::Busy)?;
        files.pending = Some(Pending {
            operation: operation.clone(),
            action,
            filename,
            file,
            bytes,
            trace,
            slot,
            settled: None,
        });
        Ok((operation, starts))
    }

    /// Reads the workspace's files.
    pub(super) fn read_files(
        &mut self,
        context: ContextRef,
        reference: WorkspaceRef,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), Refusal> {
        let admission = self.admit_key(&context, &reference, keychain, session_vault)?;
        if self.file_going() {
            return Err(Refusal::Busy);
        }
        self.files_for(&context, &reference, &admission)?;
        self.admit_file(&reference, admission, FileRequest::Read, None)
    }

    /// Opens the native sheet to choose a file to upload; returns its lease.
    pub(super) fn choose_file(
        &mut self,
        context: ContextRef,
        reference: WorkspaceRef,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<u64, Refusal> {
        let admission = self.admit_key(&context, &reference, keychain, session_vault)?;
        if self.files.as_ref().is_some_and(|f| f.choosing.is_some()) {
            return Err(Refusal::Busy);
        }
        let lease = self.lease();
        let files = self.files_for(&context, &reference, &admission)?;
        files.choosing = Some(lease);
        files.choose_failure = None;
        Ok(lease)
    }

    /// The sheet answered: the file is held for an upload (a cancelled
    /// sheet keeps the previous choice).
    pub(super) fn settle_file_choice(
        &mut self,
        lease: u64,
        chosen: Option<Result<(String, Vec<u8>), StagedFileFailure>>,
    ) {
        let Some(files) = self.files.as_mut().filter(|f| f.choosing == Some(lease)) else {
            return;
        };
        files.choosing = None;
        match chosen {
            None => {}
            Some(Ok((_, bytes))) if bytes.is_empty() => {
                files.choose_failure = Some(StagedFileFailure::Empty);
            }
            Some(Ok((_, bytes))) if bytes.len() > MAX_UPLOAD_BYTES => {
                files.choose_failure = Some(StagedFileFailure::TooLarge);
            }
            Some(Ok((name, bytes))) => match token() {
                Some(token) => {
                    files.chosen = Some(Chosen {
                        token,
                        name: upload_name(&name),
                        bytes,
                    });
                }
                None => files.choose_failure = Some(StagedFileFailure::Unreadable),
            },
            Some(Err(failure)) => files.choose_failure = Some(failure),
        }
    }

    /// Uploads the chosen file once. The choice is taken by this upload: a
    /// further upload needs the file chosen again.
    pub(super) fn upload_file(
        &mut self,
        context: ContextRef,
        reference: WorkspaceRef,
        file: &str,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), Refusal> {
        let admission = self.admit_key(&context, &reference, keychain, session_vault)?;
        if self.file_going() {
            return Err(Refusal::Busy);
        }
        let files = self.files_for(&context, &reference, &admission)?;
        if files.chosen.as_ref().is_none_or(|c| c.token != file) {
            return Err(Refusal::InvalidInput);
        }
        let Some(Chosen { name, bytes, .. }) = files.chosen.take() else {
            return Err(Refusal::InvalidInput);
        };
        let request = FileRequest::Upload {
            name: name.clone(),
            bytes,
        };
        self.admit_file(&reference, admission, request, Some(name))
    }

    /// Deletes one file the last read listed, once.
    pub(super) fn delete_file(
        &mut self,
        context: ContextRef,
        reference: WorkspaceRef,
        file: &str,
        keychain: Arc<dyn super::CredentialStore>,
        session_vault: Arc<SessionVault>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), Refusal> {
        let admission = self.admit_key(&context, &reference, keychain, session_vault)?;
        if !self.file_listed(&reference, &admission.record, file) {
            return Err(Refusal::InvalidInput);
        }
        let filename = self
            .files
            .as_ref()
            .and_then(|f| f.listed.as_ref())
            .and_then(|(files, _)| files.iter().find(|x| x.id == file))
            .map(|x| x.filename.clone());
        let request = FileRequest::Delete {
            id: file.to_owned(),
        };
        self.admit_file(&reference, admission, request, filename)
    }

    /// Withdraws the tenant's files view and stops its request (its key
    /// was replaced or removed).
    pub(super) fn retire_files(&mut self, tenant: &TenantId) -> Vec<crate::runtime::Start> {
        let Some(files) = self.files.as_mut().filter(|f| &f.tenant == tenant) else {
            return Vec::new();
        };
        files.retired = true;
        files.chosen = None;
        let Some(operation) = files
            .pending
            .as_ref()
            .filter(|p| p.settled.is_none())
            .map(|p| p.operation.clone())
        else {
            return Vec::new();
        };
        self.operations
            .cancel(&operation, StopReason::Cancelled, Instant::now())
            .1
    }

    /// Settles the files request once its operation has: a read replaces
    /// the list; an upload adds its file and a delete removes its file; an
    /// upload or delete that was sent without a known result is kept on
    /// this Mac. A view no longer bound goes once nothing of it is going.
    pub(super) fn sync_files(&mut self) {
        let bound = self.files.as_ref().is_some_and(|f| self.files_bound(f));
        let Some(files) = self.files.as_mut() else {
            return;
        };
        let mut kept = None;
        if let Some(p) = files.pending.as_mut().filter(|p| p.settled.is_none()) {
            let outcome = match self.operations.state(&p.operation) {
                Some(OperationState::Settled(s)) => Some(s.outcome),
                None => Some(Outcome::Unknown),
                Some(_) => None,
            };
            if let Some(outcome) = outcome {
                p.settled = Some(outcome);
                let progress: FileProgress = p.slot.lock().map(|g| g.clone()).unwrap_or_default();
                match (p.action, outcome) {
                    (FileAction::Read, Outcome::Completed) if progress.listed.is_some() => {
                        files.listed = progress.listed.clone();
                        files.read_failure = None;
                    }
                    (FileAction::Read, _) => {
                        files.read_failure =
                            Some(progress.failure.unwrap_or(InferenceFailure::Network));
                    }
                    (FileAction::Upload, Outcome::Completed) => {
                        if let (Some((list, more)), Some(created)) =
                            (files.listed.as_mut(), progress.created.clone())
                        {
                            list.retain(|f| f.id != created.id);
                            list.insert(0, created);
                            // The list keeps its bound; a file it no longer
                            // shows is reported as more, never dropped silently.
                            if list.len() > MAX_FILES {
                                list.truncate(MAX_FILES);
                                *more = true;
                            }
                        }
                    }
                    (FileAction::Delete, Outcome::Completed) => {
                        if let (Some((list, _)), Some(id)) = (files.listed.as_mut(), &p.file) {
                            list.retain(|f| &f.id != id);
                        }
                    }
                    _ => {}
                }
                if p.action != FileAction::Read {
                    let view = action_view(p);
                    if progress.sent && outcome == Outcome::Unknown {
                        kept = Some((files.tenant.clone(), files.workspace.clone(), view.clone()));
                    }
                    files.last = Some(view);
                }
            }
        }
        let idle = files.pending.as_ref().is_none_or(|p| p.settled.is_some());
        if !bound && idle {
            self.files = None;
        }
        if let Some((tenant, workspace, view)) = kept {
            self.file_settled(&tenant, workspace, &view);
        }
    }

    pub(super) fn files_view(&self) -> Option<GatewayFilesView> {
        let files = self.files.as_ref().filter(|f| self.files_bound(f))?;
        let going = files.pending.as_ref().filter(|p| p.settled.is_none());
        Some(GatewayFilesView {
            workspace: files.workspace.clone(),
            files: files.listed.as_ref().map(|(f, _)| f.clone()),
            more: files.listed.as_ref().is_some_and(|(_, more)| *more),
            reading: going.is_some_and(|p| p.action == FileAction::Read),
            read_failure: files.read_failure,
            chosen: files.chosen.as_ref().map(|c| ChosenFile {
                token: c.token.clone(),
                name: c.name.clone(),
                bytes: c.bytes.len() as u64,
            }),
            choosing: files.choosing.is_some(),
            choose_failure: files.choose_failure,
            action: going
                .filter(|p| p.action != FileAction::Read)
                .map(action_view)
                .or_else(|| files.last.clone()),
        })
    }
}
