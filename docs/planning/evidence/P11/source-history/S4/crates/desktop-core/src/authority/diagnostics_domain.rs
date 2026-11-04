//! Diagnostics inside the authority (C24, D051): the view Settings shows,
//! and the explicit export (through the native save sheet) and clearing.

use std::sync::Arc;

use serde::Serialize;
use ts_rs::TS;

use super::{Effect, Inner, Refusal, State, lock};

/// How the last diagnostics export ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticsExport {
    Saving,
    Saved,
    Cancelled,
    Failed,
}

/// How the last clearing ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticsClear {
    Cleared,
    /// A file could not be removed: its events are still kept and counted.
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsView {
    /// Whether this build keeps diagnostics (a logs folder).
    pub available: bool,
    /// Events kept on this Mac.
    pub events: u32,
    /// Days events are kept.
    pub retention_days: u32,
    pub export: Option<DiagnosticsExport>,
    pub clear: Option<DiagnosticsClear>,
}

#[derive(Default)]
pub(super) struct DiagnosticsState {
    lease: u64,
    export: Option<DiagnosticsExport>,
    clear: Option<DiagnosticsClear>,
}

pub(super) const EXPORT_NAME: &str = "prisma-airs-desktop-diagnostics.json";

impl State {
    pub(super) fn diagnostics_view(&self, inner_events: u32, available: bool) -> DiagnosticsView {
        DiagnosticsView {
            available,
            events: inner_events,
            retention_days: u32::try_from(crate::diagnostics::RETENTION_SECS / 86_400)
                .unwrap_or(u32::MAX),
            export: self.diagnostics.export,
            clear: self.diagnostics.clear,
        }
    }

    /// Clears the kept diagnostics and keeps how that ended for the view.
    pub(super) fn clear_diagnostics(&mut self) {
        self.diagnostics.clear = Some(if self.diagnostics_log.clear() {
            DiagnosticsClear::Cleared
        } else {
            DiagnosticsClear::Failed
        });
    }

    pub(super) fn begin_diagnostics_export(&mut self) -> Result<u64, Refusal> {
        if self.diagnostics.export == Some(DiagnosticsExport::Saving) {
            return Err(Refusal::Busy);
        }
        self.diagnostics.lease += 1;
        self.diagnostics.export = Some(DiagnosticsExport::Saving);
        Ok(self.diagnostics.lease)
    }

    fn settle_diagnostics_export(&mut self, lease: u64, saved: Result<Option<String>, ()>) {
        if self.diagnostics.lease != lease {
            return;
        }
        self.diagnostics.export = Some(match saved {
            Ok(Some(_)) => DiagnosticsExport::Saved,
            Ok(None) => DiagnosticsExport::Cancelled,
            Err(()) => DiagnosticsExport::Failed,
        });
    }
}

/// Builds the export (off the lock) and asks where to save it.
pub(super) fn export(inner: &Arc<Inner>, lease: u64) {
    let inner = Arc::clone(inner);
    tokio::spawn(async move {
        let worker = Arc::clone(&inner);
        let bytes = tokio::task::spawn_blocking(move || {
            worker
                .diagnostics
                .export(env!("CARGO_PKG_VERSION"), cfg!(feature = "test-fixtures"))
        })
        .await
        .unwrap_or_default();
        let saved = inner
            .adapters
            .saver
            .save(EXPORT_NAME.to_owned(), bytes)
            .await;
        if let Ok(mut state) = lock(&inner) {
            state.settle_diagnostics_export(lease, saved);
            state.changed();
        }
    });
}

pub(super) fn admit_export(state: &mut State, effects: &mut Vec<Effect>) -> Result<(), Refusal> {
    let lease = state.begin_diagnostics_export()?;
    effects.push(Effect::ExportDiagnostics { lease });
    state.changed();
    Ok(())
}
