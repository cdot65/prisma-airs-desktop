//! The case Library inside the authority (C12). Each tenant's cases are
//! sealed on this Mac under a key in the credential Keychain namespace and
//! opened off the lock; every change is written newest-generation-first by
//! the local-data writer, which tenant deletion and reset also take, so a
//! deleted tenant's library never reappears. Imports and catalog imports
//! are explicit, bounded and validated like v1; nothing is executed or
//! uploaded. Only Runtime Security cases fill the scan form, and that
//! happens in the presentation without scanning.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Refusal, State};
use crate::context::ContextRef;
use crate::library::{
    CaseDraft, CaseLibrary, CaseProblem, CaseView, LibraryFailure, TestCase, new_examples,
};
use crate::tenants::TenantId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LibraryState {
    Idle,
    Loading,
    Ready,
    Failed,
}

/// Where a catalog import comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CatalogSource {
    /// v1's eleven synthetic examples, compiled in.
    Bundled,
    /// The public example catalog (an explicit HTTPS read of public data;
    /// no tenant credential is involved).
    Public,
}

/// What the last explicit library operation did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LibraryNotice {
    Imported {
        added: u32,
    },
    ExamplesAdded {
        added: u32,
    },
    /// A case was copied into another tenant's library (as a new case).
    Copied {
        tenant: String,
    },
    /// The public catalog could not be read (sanitized).
    CatalogUnavailable,
    /// The chosen file could not be read.
    FileUnreadable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub state: LibraryState,
    pub failure: Option<LibraryFailure>,
    pub cases: Vec<CaseView>,
    /// Why the last change was refused.
    pub problem: Option<CaseProblem>,
    /// The last change could not be saved on this Mac.
    pub unsaved: Option<LibraryFailure>,
    pub notice: Option<LibraryNotice>,
    /// A case file sheet is open.
    pub choosing: bool,
    /// The public catalog is being read.
    pub fetching: bool,
    pub export: Option<super::ExportView>,
}

pub(super) struct Library {
    tenant: TenantId,
    lease: u64,
    state: LibraryState,
    failure: Option<LibraryFailure>,
    library: CaseLibrary,
    pub(super) generation: u64,
    unsaved: Option<LibraryFailure>,
    problem: Option<CaseProblem>,
    notice: Option<LibraryNotice>,
    /// Copies made into this tenant before its library opened.
    pending: Vec<TestCase>,
    choosing: Option<u64>,
    fetching: Option<u64>,
}

impl Library {
    fn new(tenant: TenantId, lease: u64) -> Self {
        Self {
            tenant,
            lease,
            state: LibraryState::Loading,
            failure: None,
            library: CaseLibrary::default(),
            generation: 0,
            unsaved: None,
            problem: None,
            notice: None,
            pending: Vec::new(),
            choosing: None,
            fetching: None,
        }
    }
}

/// A copy's follow-up: the destination's write and, if it was not open,
/// its load lease.
pub(super) type CopyEffects = (Option<(TenantId, u64)>, Option<u64>);

/// What a write needs: the tenant, its generation and its cases.
pub(super) struct LibraryWrite {
    pub root: std::path::PathBuf,
    pub tenant: String,
    pub library: CaseLibrary,
}

impl State {
    fn library_mut(&mut self, tenant: &TenantId) -> Result<&mut Library, Refusal> {
        let library = self
            .libraries
            .iter_mut()
            .find(|l| &l.tenant == tenant)
            .ok_or(Refusal::CapabilityMissing)?;
        match library.state {
            LibraryState::Ready => Ok(library),
            LibraryState::Loading => Err(Refusal::Busy),
            _ => Err(Refusal::CapabilityMissing),
        }
    }

    /// Starts opening `tenant`'s library unless it is open or opening;
    /// returns the load lease.
    fn start_library(&mut self, tenant: &TenantId) -> Option<u64> {
        if self.libraries.iter().any(|l| {
            &l.tenant == tenant && matches!(l.state, LibraryState::Loading | LibraryState::Ready)
        }) {
            return None;
        }
        let lease = self.lease();
        let pending = self
            .libraries
            .iter_mut()
            .find(|l| &l.tenant == tenant)
            .map(|l| std::mem::take(&mut l.pending))
            .unwrap_or_default();
        self.libraries.retain(|l| &l.tenant != tenant);
        let mut library = Library::new(tenant.clone(), lease);
        library.pending = pending;
        self.libraries.push(library);
        Some(lease)
    }

    pub(super) fn open_library(&mut self, context: &ContextRef) -> Result<Option<u64>, Refusal> {
        let tenant = self.check_context(context)?;
        self.library_root
            .as_ref()
            .ok_or(Refusal::CapabilityMissing)?;
        Ok(self.start_library(&tenant))
    }

    pub(super) fn library_load_target(&self, lease: u64) -> Option<(std::path::PathBuf, String)> {
        let library = self.libraries.iter().find(|l| l.lease == lease)?;
        Some((
            self.library_root.clone()?,
            library.tenant.as_str().to_owned(),
        ))
    }

    /// Settles a load; copies made meanwhile are merged and need a write.
    pub(super) fn settle_library(
        &mut self,
        lease: u64,
        loaded: Result<CaseLibrary, LibraryFailure>,
    ) -> Option<(TenantId, u64)> {
        let library = self
            .libraries
            .iter_mut()
            .find(|l| l.lease == lease && l.state == LibraryState::Loading)?;
        match loaded {
            Ok(cases) => {
                library.library = cases;
                library.state = LibraryState::Ready;
                let pending = std::mem::take(&mut library.pending);
                if pending.is_empty() {
                    return None;
                }
                match library.library.merge(pending) {
                    Ok(_) => {
                        library.generation += 1;
                        Some((library.tenant.clone(), library.generation))
                    }
                    Err(problem) => {
                        library.problem = Some(problem);
                        None
                    }
                }
            }
            Err(failure) => {
                library.state = LibraryState::Failed;
                library.failure = Some(failure);
                None
            }
        }
    }

    fn changed_library(library: &mut Library) -> (TenantId, u64) {
        library.problem = None;
        library.generation += 1;
        (library.tenant.clone(), library.generation)
    }

    /// Saves a new case authored from `draft`. A library that is not open
    /// yet opens, and the case is added when it does (its write follows).
    pub(super) fn save_case(
        &mut self,
        context: &ContextRef,
        draft: CaseDraft,
    ) -> Result<CopyEffects, Refusal> {
        let tenant = self.check_context(context)?;
        self.library_root
            .as_ref()
            .ok_or(Refusal::CapabilityMissing)?;
        let case = draft.into_case(tenant.as_str());
        let open = self
            .libraries
            .iter()
            .any(|l| l.tenant == tenant && l.state == LibraryState::Ready);
        if !open {
            let case = case.map_err(|_| Refusal::InvalidInput)?;
            let load = self.start_library(&tenant);
            let library = self
                .libraries
                .iter_mut()
                .find(|l| l.tenant == tenant)
                .ok_or(Refusal::Busy)?;
            if library.pending.len() >= crate::library::CASE_LIMIT {
                return Err(Refusal::Busy);
            }
            library.pending.push(case);
            return Ok((None, load));
        }
        let library = self.library_mut(&tenant)?;
        library.notice = None;
        match case.and_then(|case| library.library.add(case, tenant.as_str())) {
            Ok(()) => Ok((Some(Self::changed_library(library)), None)),
            Err(problem) => {
                library.problem = Some(problem);
                Err(Refusal::InvalidInput)
            }
        }
    }

    pub(super) fn delete_case(
        &mut self,
        context: &ContextRef,
        id: &str,
    ) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let library = self.library_mut(&tenant)?;
        library.notice = None;
        if library.library.remove(id).is_none() {
            library.problem = Some(CaseProblem::Missing);
            return Err(Refusal::InvalidInput);
        }
        Ok(Self::changed_library(library))
    }

    /// Adds catalog examples (new identifiers, this tenant) not already
    /// present by product and name.
    pub(super) fn add_examples(
        &mut self,
        tenant: &TenantId,
        parsed: Result<Vec<TestCase>, CaseProblem>,
    ) -> Result<(TenantId, u64), Refusal> {
        let library = self.library_mut(tenant)?;
        let examples = match parsed {
            Ok(examples) => new_examples(&library.library, examples),
            Err(problem) => {
                library.problem = Some(problem);
                return Err(Refusal::InvalidInput);
            }
        };
        if examples.is_empty() {
            library.problem = Some(CaseProblem::AllPresent);
            return Err(Refusal::InvalidInput);
        }
        match library.library.merge(examples) {
            Ok(added) => {
                library.notice = Some(LibraryNotice::ExamplesAdded {
                    added: u32::try_from(added).unwrap_or(u32::MAX),
                });
                Ok(Self::changed_library(library))
            }
            Err(problem) => {
                library.problem = Some(problem);
                Err(Refusal::InvalidInput)
            }
        }
    }

    pub(super) fn import_bundled(
        &mut self,
        context: &ContextRef,
    ) -> Result<(TenantId, u64), Refusal> {
        let tenant = self.check_context(context)?;
        let parsed =
            crate::library::parse_catalog(crate::library::BUNDLED_CATALOG, tenant.as_str());
        self.add_examples(&tenant, parsed)
    }

    /// Starts reading the public catalog; returns the lease and its URL.
    pub(super) fn begin_catalog(
        &mut self,
        context: &ContextRef,
    ) -> Result<(u64, url::Url), Refusal> {
        let tenant = self.check_context(context)?;
        let url = self.case_catalog.clone();
        if !crate::library::catalog_url_ok(&url, &url) {
            return Err(Refusal::CapabilityMissing);
        }
        let lease = self.lease();
        let library = self.library_mut(&tenant)?;
        if library.fetching.is_some() {
            return Err(Refusal::Busy);
        }
        library.fetching = Some(lease);
        library.notice = None;
        library.problem = None;
        Ok((lease, url))
    }

    pub(super) fn settle_catalog(
        &mut self,
        lease: u64,
        fetched: Option<Vec<u8>>,
    ) -> Option<(TenantId, u64)> {
        let library = self
            .libraries
            .iter_mut()
            .find(|l| l.fetching == Some(lease))?;
        library.fetching = None;
        let tenant = library.tenant.clone();
        let Some(bytes) = fetched else {
            library.notice = Some(LibraryNotice::CatalogUnavailable);
            return None;
        };
        let parsed = crate::library::parse_catalog(&bytes, tenant.as_str());
        self.add_examples(&tenant, parsed).ok()
    }

    pub(super) fn begin_case_import(&mut self, context: &ContextRef) -> Result<u64, Refusal> {
        let tenant = self.check_context(context)?;
        let lease = self.lease();
        let library = self.library_mut(&tenant)?;
        if library.choosing.is_some() {
            return Err(Refusal::Busy);
        }
        library.choosing = Some(lease);
        library.notice = None;
        library.problem = None;
        Ok(lease)
    }

    /// Settles a chosen case file (None: cancelled).
    pub(super) fn settle_case_import(
        &mut self,
        lease: u64,
        chosen: Option<Option<Vec<u8>>>,
    ) -> Option<(TenantId, u64)> {
        let library = self
            .libraries
            .iter_mut()
            .find(|l| l.choosing == Some(lease))?;
        library.choosing = None;
        let bytes = match chosen? {
            Some(bytes) => bytes,
            None => {
                library.notice = Some(LibraryNotice::FileUnreadable);
                return None;
            }
        };
        let merged = crate::library::parse_import(&bytes, library.tenant.as_str())
            .and_then(|cases| library.library.merge(cases));
        match merged {
            Ok(added) => {
                library.notice = Some(LibraryNotice::Imported {
                    added: u32::try_from(added).unwrap_or(u32::MAX),
                });
                Some(Self::changed_library(library))
            }
            Err(problem) => {
                library.problem = Some(problem);
                None
            }
        }
    }

    /// Copies one case into another tenant's library as a new case (v1
    /// explicit copy). The destination opens if it is not open yet.
    pub(super) fn copy_case(
        &mut self,
        context: &ContextRef,
        id: &str,
        to: &TenantId,
    ) -> Result<CopyEffects, Refusal> {
        let tenant = self.check_context(context)?;
        if to == &tenant || self.tenant(to).is_none() {
            return Err(Refusal::InvalidInput);
        }
        let source = self.library_mut(&tenant)?;
        let Some(mut copy) = source.library.cases.iter().find(|c| c.id == id).cloned() else {
            source.problem = Some(CaseProblem::Missing);
            return Err(Refusal::InvalidInput);
        };
        copy.id = crate::library::new_id().ok_or(Refusal::Busy)?;
        copy.tenant = to.as_str().to_owned();
        let label = self
            .tenant(to)
            .map(|t| t.profile.label.clone())
            .unwrap_or_default();
        let load = self.start_library(to);
        let destination = self
            .libraries
            .iter_mut()
            .find(|l| &l.tenant == to)
            .ok_or(Refusal::Busy)?;
        let write = if destination.state == LibraryState::Ready {
            match destination.library.add(copy, to.as_str()) {
                Ok(()) => Some(Self::changed_library(destination)),
                Err(problem) => {
                    if let Some(source) = self.libraries.iter_mut().find(|l| l.tenant == tenant) {
                        source.problem = Some(problem);
                    }
                    return Err(Refusal::InvalidInput);
                }
            }
        } else {
            if destination.pending.len() >= crate::library::CASE_LIMIT {
                return Err(Refusal::Busy);
            }
            destination.pending.push(copy);
            None
        };
        if let Some(source) = self.libraries.iter_mut().find(|l| l.tenant == tenant) {
            source.problem = None;
            source.notice = Some(LibraryNotice::Copied { tenant: label });
        }
        Ok((write, load))
    }

    /// The export of the active tenant's cases, for the save sheet.
    pub(super) fn export_cases(
        &mut self,
        context: &ContextRef,
    ) -> Result<(u64, String, Vec<u8>), Refusal> {
        let tenant = self.check_context(context)?;
        let library = self.library_mut(&tenant)?;
        let bytes = match crate::library::export(tenant.as_str(), &library.library.cases) {
            Ok(bytes) => bytes,
            Err(problem) => {
                library.problem = Some(problem);
                return Err(Refusal::InvalidInput);
            }
        };
        let lease = self.begin_export(context, super::ExportKind::Cases)?;
        Ok((lease, crate::library::EXPORT_NAME.to_owned(), bytes))
    }

    /// A tenant's newest library to write, if `generation` is still newest.
    pub(super) fn library_write(&self, tenant: &TenantId, generation: u64) -> Option<LibraryWrite> {
        if self.storage == super::StorageHealth::Resetting {
            return None;
        }
        let library = self.libraries.iter().find(|l| {
            &l.tenant == tenant && l.generation == generation && l.state == LibraryState::Ready
        })?;
        Some(LibraryWrite {
            root: self.library_root.clone()?,
            tenant: tenant.as_str().to_owned(),
            library: library.library.clone(),
        })
    }

    pub(super) fn settle_library_write(
        &mut self,
        tenant: &TenantId,
        generation: u64,
        stored: Result<(), LibraryFailure>,
    ) {
        if let Some(library) = self
            .libraries
            .iter_mut()
            .find(|l| &l.tenant == tenant && l.generation == generation)
        {
            library.unsaved = stored.err();
        }
    }

    /// Drops a deleted tenant's library from memory.
    pub(super) fn forget_library(&mut self, tenant: &TenantId) {
        self.libraries.retain(|l| &l.tenant != tenant);
    }

    pub(super) fn library_view(&self) -> LibraryView {
        let active = self.active_ref.as_ref();
        let tenant = self.data.active.as_ref();
        let export = self
            .export
            .as_ref()
            .filter(|e| Some(&e.context) == active && e.is_kind(super::ExportKind::Cases))
            .map(super::sessions_domain::Export::view);
        match self
            .libraries
            .iter()
            .find(|l| Some(&l.tenant) == tenant && active.is_some())
        {
            Some(l) => LibraryView {
                state: l.state,
                failure: l.failure,
                cases: l.library.cases.iter().map(CaseView::from).collect(),
                problem: l.problem,
                unsaved: l.unsaved,
                notice: l.notice.clone(),
                choosing: l.choosing.is_some(),
                fetching: l.fetching.is_some(),
                export,
            },
            None => LibraryView {
                state: LibraryState::Idle,
                failure: None,
                cases: Vec::new(),
                problem: None,
                unsaved: None,
                notice: None,
                choosing: false,
                fetching: false,
                export,
            },
        }
    }
}

/// One explicit read of the public example catalog (v1: 15 s, at most 256
/// KiB, no redirects, JSON only, no credential). `None` on any failure.
pub(super) async fn fetch_catalog(
    transport: &crate::transport::NativeTransport,
    url: &url::Url,
) -> Option<Vec<u8>> {
    use futures_util::StreamExt;
    let response = transport
        .http()
        .get(url.clone())
        .header("accept", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::OK {
        return None;
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk.ok()?);
        if body.len() > crate::library::LIBRARY_BYTES {
            return None;
        }
    }
    Some(body)
}
