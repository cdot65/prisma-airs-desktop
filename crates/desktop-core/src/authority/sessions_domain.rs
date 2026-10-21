//! Sessions investigations and metadata-only FP/FN reports inside the
//! authority (C11). Reports belong to the active tenant: a report is built in
//! Rust from the scan's metadata in this credential's investigation (the
//! operator supplies only its kind, category and reason), kept in a 0600
//! file per tenant, and exported as v1's CSV through the native save sheet.
//! Nothing here writes to the service: a report suppresses nothing and
//! changes no policy. The investigation cache can be cleared, and its
//! metadata (never prompt or response) exported.

use serde::Serialize;
use ts_rs::TS;

use super::{Refusal, State};
use crate::context::ContextRef;
use crate::sessions::{
    Displaced, InvestigationCache, InvestigationStorage, Report, ReportBook, ReportInput,
    ReportProblem, ReportStoreFailure,
};
use crate::tenants::TenantId;

/// What a report is saved for: a scan in a session this investigation read
/// (a new report), or an already-reported scan (an edit).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReportTarget {
    pub scan_id: String,
    pub sub_request: u32,
    /// The scan's session, as read (empty when editing an existing report).
    pub session_id: String,
    pub app_id: String,
    pub app_name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReportsState {
    /// Not opened for the active tenant yet.
    Idle,
    Loading,
    Ready,
    Failed,
}

/// The removal Undo can restore.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReportUndoView {
    pub scan_id: String,
    pub sub_request: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReportsView {
    pub state: ReportsState,
    pub failure: Option<ReportStoreFailure>,
    /// Newest first.
    pub items: Vec<Report>,
    pub undo: Option<ReportUndoView>,
    /// Why the last save was refused.
    pub problem: Option<ReportProblem>,
    /// The last change could not be written to this Mac.
    pub unsaved: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InvestigationView {
    pub storage: InvestigationStorage,
    pub sessions: u32,
    pub transactions: u32,
    /// Revealed scan contents kept for this credential.
    pub contents: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ExportKind {
    Reports,
    Investigation,
    /// The case Library (C12).
    Cases,
    /// A saved Harness conversation's transcript (C20).
    HarnessConversation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ExportState {
    Saving,
    Saved,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExportView {
    pub kind: ExportKind,
    pub state: ExportState,
    /// The saved file's name (no directory).
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionsView {
    pub reports: ReportsView,
    /// The active context's investigation, once a sessions read made one.
    pub investigation: Option<InvestigationView>,
    pub export: Option<ExportView>,
}

/// The active tenant's reports.
pub(super) struct Reports {
    tenant: TenantId,
    lease: u64,
    state: ReportsState,
    failure: Option<ReportStoreFailure>,
    book: ReportBook,
    undo: Option<Displaced>,
    problem: Option<ReportProblem>,
    /// Bumped by every change; only the newest is written.
    pub(super) generation: u64,
    unsaved: bool,
}

pub(super) struct Export {
    pub(super) lease: u64,
    pub(super) context: ContextRef,
    kind: ExportKind,
    state: ExportState,
    name: Option<String>,
}

impl Export {
    pub(super) fn is_kind(&self, kind: ExportKind) -> bool {
        self.kind == kind
    }

    pub(super) fn view(&self) -> ExportView {
        ExportView {
            kind: self.kind,
            state: self.state,
            name: self.name.clone(),
        }
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl State {
    fn ready_reports(&mut self, tenant: &TenantId) -> Result<&mut Reports, Refusal> {
        let reports = self
            .reports
            .iter_mut()
            .find(|r| &r.tenant == tenant)
            .ok_or(Refusal::CapabilityMissing)?;
        match reports.state {
            ReportsState::Ready => Ok(reports),
            ReportsState::Loading => Err(Refusal::Busy),
            _ => Err(Refusal::CapabilityMissing),
        }
    }

    /// Opens the active tenant's reports (once; a failed open is retried);
    /// returns the load lease when a read must start. Every opened tenant
    /// keeps its own book, so a pending write of one tenant is never lost or
    /// confused with another's.
    pub(super) fn open_reports(&mut self, context: &ContextRef) -> Result<Option<u64>, Refusal> {
        let tenant = self.check_context(context)?;
        self.sessions_root
            .as_ref()
            .ok_or(Refusal::CapabilityMissing)?;
        if self.reports.iter().any(|r| {
            r.tenant == tenant && matches!(r.state, ReportsState::Loading | ReportsState::Ready)
        }) {
            return Ok(None);
        }
        let lease = self.lease();
        self.reports.retain(|r| r.tenant != tenant);
        self.reports.push(Reports {
            tenant,
            lease,
            state: ReportsState::Loading,
            failure: None,
            book: ReportBook::default(),
            undo: None,
            problem: None,
            generation: 0,
            unsaved: false,
        });
        Ok(Some(lease))
    }

    pub(super) fn report_load_target(&self, lease: u64) -> Option<std::path::PathBuf> {
        let reports = self.reports.iter().find(|r| r.lease == lease)?;
        let root = self.sessions_root.as_ref()?;
        Some(crate::sessions::reports_path(root, reports.tenant.as_str()))
    }

    pub(super) fn settle_reports(
        &mut self,
        lease: u64,
        loaded: Result<ReportBook, ReportStoreFailure>,
    ) {
        let Some(reports) = self
            .reports
            .iter_mut()
            .find(|r| r.lease == lease && r.state == ReportsState::Loading)
        else {
            return;
        };
        match loaded {
            Ok(book) => {
                reports.book = book;
                reports.state = ReportsState::Ready;
            }
            Err(failure) => {
                reports.state = ReportsState::Failed;
                reports.failure = Some(failure);
            }
        }
    }

    /// Saves a report: an already-reported scan is edited (its metadata and
    /// reported time kept); a new one is built from the scan's metadata in
    /// this credential's investigation. Returns the tenant and generation to
    /// write.
    pub(super) fn save_report(
        &mut self,
        context: &ContextRef,
        target: ReportTarget,
        input: ReportInput,
    ) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let investigation = self.investigation(context);
        let reports = self.ready_reports(&tenant)?;
        let existing = reports
            .book
            .reports
            .iter()
            .find(|r| r.scan_id == target.scan_id && r.scan_sub_req_id == target.sub_request)
            .cloned();
        let report = match existing {
            Some(mut report) => {
                report.kind = input.kind;
                report.category = input.category;
                report.reason = input.reason;
                Some(report)
            }
            None => investigation.and_then(|investigation| {
                let key = format!(
                    "{}\u{1f}{}\u{1f}{}",
                    target.session_id, target.app_id, target.app_name
                );
                investigation
                    .with(|c| {
                        let read = InvestigationCache::get(&c.sessions, &key)?;
                        let session: serde_json::Value =
                            serde_json::from_str(read.get("session")?.as_str()?).ok()?;
                        let row = session
                            .get("session_actions")?
                            .as_array()?
                            .iter()
                            .find(|row| {
                                row.get("scan_id").and_then(serde_json::Value::as_str)
                                    == Some(target.scan_id.as_str())
                                    && row
                                        .get("scan_sub_req_id")
                                        .and_then(serde_json::Value::as_u64)
                                        .unwrap_or(0)
                                        == u64::from(target.sub_request)
                            })?
                            .clone();
                        let application = session
                            .get("application_name")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or(&target.app_name)
                            .to_owned();
                        Some(crate::sessions::report_of(
                            &row,
                            &application,
                            input,
                            crate::sessions::rfc3339(now_secs()),
                        ))
                    })
                    .flatten()
            }),
        };
        let Some(report) = report else {
            reports.problem = Some(ReportProblem::Scan);
            return Err(Refusal::InvalidInput);
        };
        match reports.book.save(report) {
            Ok(_) => {
                reports.problem = None;
                reports.generation += 1;
                Ok((tenant, reports.generation))
            }
            Err(problem) => {
                reports.problem = Some(problem);
                Err(Refusal::InvalidInput)
            }
        }
    }

    pub(super) fn remove_report(
        &mut self,
        context: &ContextRef,
        scan_id: &str,
        sub_request: u32,
    ) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let reports = self.ready_reports(&tenant)?;
        let displaced = reports
            .book
            .remove(scan_id, sub_request)
            .ok_or(Refusal::InvalidInput)?;
        reports.undo = Some(displaced);
        reports.generation += 1;
        Ok((tenant, reports.generation))
    }

    /// Restores the last removal, never over a newer report of that scan.
    pub(super) fn undo_report(&mut self, context: &ContextRef) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let reports = self.ready_reports(&tenant)?;
        let displaced = reports.undo.take().ok_or(Refusal::InvalidInput)?;
        if !reports.book.undo(&displaced) {
            return Err(Refusal::InvalidInput);
        }
        reports.generation += 1;
        Ok((tenant, reports.generation))
    }

    /// A tenant's newest report book to write, if `generation` is still its
    /// newest (nothing once the tenant was deleted or the data reset).
    pub(super) fn report_write(
        &self,
        tenant: &TenantId,
        generation: u64,
    ) -> Option<(std::path::PathBuf, ReportBook)> {
        if self.storage == super::StorageHealth::Resetting {
            return None;
        }
        let reports = self.reports.iter().find(|r| {
            &r.tenant == tenant && r.generation == generation && r.state == ReportsState::Ready
        })?;
        let root = self.sessions_root.as_ref()?;
        Some((
            crate::sessions::reports_path(root, reports.tenant.as_str()),
            reports.book.clone(),
        ))
    }

    pub(super) fn settle_report_write(&mut self, tenant: &TenantId, generation: u64, stored: bool) {
        if let Some(reports) = self
            .reports
            .iter_mut()
            .find(|r| &r.tenant == tenant && r.generation == generation)
        {
            reports.unsaved = !stored;
        }
    }

    /// Drops a deleted tenant's reports from memory; returns the file to
    /// remove.
    pub(super) fn forget_reports(&mut self, tenant: &TenantId) -> Option<std::path::PathBuf> {
        self.reports.retain(|r| &r.tenant != tenant);
        self.sessions_root
            .as_ref()
            .map(|root| crate::sessions::reports_path(root, tenant.as_str()))
    }

    pub(super) fn begin_export(
        &mut self,
        context: &ContextRef,
        kind: ExportKind,
    ) -> Result<u64, Refusal> {
        if self
            .export
            .as_ref()
            .is_some_and(|e| e.state == ExportState::Saving)
        {
            return Err(Refusal::Busy);
        }
        let lease = self.lease();
        self.export = Some(Export {
            lease,
            context: context.clone(),
            kind,
            state: ExportState::Saving,
            name: None,
        });
        Ok(lease)
    }

    /// v1's CSV of the active tenant's reports, for the native save sheet.
    pub(super) fn export_reports(
        &mut self,
        context: &ContextRef,
    ) -> Result<(u64, String, Vec<u8>), Refusal> {
        let tenant = self.check_context(context)?;
        let profile = self
            .tenant(&tenant)
            .ok_or(Refusal::StaleContext)?
            .profile
            .clone();
        let reports = self.ready_reports(&tenant)?.book.reports.clone();
        let now = now_secs();
        let csv = crate::sessions::csv(
            &profile.label,
            profile.tsg_id.as_deref().unwrap_or_default(),
            &reports,
            &crate::sessions::rfc3339(now),
        );
        let name = format!(
            "fpfn-reports-{}-{}.csv",
            crate::sessions::slug(&profile.label),
            crate::sessions::file_stamp(now)
        );
        let lease = self.begin_export(context, ExportKind::Reports)?;
        Ok((lease, name, csv.into_bytes()))
    }

    /// The investigation's metadata (sessions and transactions, never a
    /// prompt or response) as JSON, for the native save sheet.
    pub(super) fn export_investigation(
        &mut self,
        context: &ContextRef,
    ) -> Result<(u64, String, Vec<u8>), Refusal> {
        let tenant = self.check_context(context)?;
        let profile = self
            .tenant(&tenant)
            .ok_or(Refusal::StaleContext)?
            .profile
            .clone();
        let investigation = self.investigation(context).ok_or(Refusal::InvalidInput)?;
        let metadata = investigation
            .with(|c| c.metadata())
            .ok_or(Refusal::InvalidInput)?;
        let now = now_secs();
        let document = serde_json::json!({
            "tenant": profile.label,
            "tsg_id": profile.tsg_id,
            "exported_at": crate::sessions::rfc3339(now),
            "sessions": metadata["sessions"],
            "transactions": metadata["transactions"],
        });
        let bytes = serde_json::to_vec_pretty(&document).map_err(|_| Refusal::InvalidInput)?;
        let name = format!(
            "investigation-{}-{}.json",
            crate::sessions::slug(&profile.label),
            crate::sessions::file_stamp(now)
        );
        let lease = self.begin_export(context, ExportKind::Investigation)?;
        Ok((lease, name, bytes))
    }

    pub(super) fn settle_export(&mut self, lease: u64, saved: Result<Option<String>, ()>) {
        if let Some(export) = self.export.as_mut().filter(|e| e.lease == lease) {
            (export.state, export.name) = match saved {
                Ok(Some(name)) => (ExportState::Saved, Some(name)),
                Ok(None) => (ExportState::Cancelled, None),
                Err(()) => (ExportState::Failed, None),
            };
        }
    }

    pub(super) fn sessions_view(&self) -> SessionsView {
        let active = self.active_ref.as_ref();
        let tenant = self.data.active.as_ref();
        let reports = match self
            .reports
            .iter()
            .find(|r| Some(&r.tenant) == tenant && active.is_some())
        {
            Some(r) => ReportsView {
                state: r.state,
                failure: r.failure,
                items: r.book.reports.clone(),
                undo: r.undo.as_ref().map(|d| ReportUndoView {
                    scan_id: d.report.scan_id.clone(),
                    sub_request: d.report.scan_sub_req_id,
                }),
                problem: r.problem,
                unsaved: r.unsaved,
            },
            None => ReportsView {
                state: ReportsState::Idle,
                failure: None,
                items: Vec::new(),
                undo: None,
                problem: None,
                unsaved: false,
            },
        };
        let investigation = active.and_then(|c| self.investigation(c)).map(|i| {
            let (sessions, transactions, contents) = i.counts();
            InvestigationView {
                storage: i.storage(),
                sessions,
                transactions,
                contents,
            }
        });
        let export = self
            .export
            .as_ref()
            .filter(|e| {
                Some(&e.context) == active
                    && !e.is_kind(ExportKind::Cases)
                    && !e.is_kind(ExportKind::HarnessConversation)
            })
            .map(Export::view);
        SessionsView {
            reports,
            investigation,
            export,
        }
    }
}
