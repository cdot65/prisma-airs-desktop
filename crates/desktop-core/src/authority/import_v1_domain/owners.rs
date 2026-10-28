//! Each category's v2 owner as the import uses it: reading what v2 holds,
//! writing new entries (each noted in the receipt inside the section that
//! writes it) and reverting one only while v2 holds exactly what was
//! written. Library and report writes take the local-data writer (as every
//! library and report write and read, tenant deletion and reset do) and the
//! placement lock; an open library or book is changed in memory and written
//! as its newest generation, and one still loading reads again.

use std::sync::Arc;

use serde_json::{Value, json};

use super::super::{Inner, Intent, State, StorageHealth, lock, run_effects};
use super::{Journal, Revert, conversation_hash, draft_value, imported_record, rebound, stage};
use crate::import_v1::plan::Payload;
use crate::import_v1::receipt::Written;
use crate::import_v1::{Problem, content_hash, parse};
use crate::tenants::{ProfileDraft, TenantId, TenantProfile};

async fn blocking<T: Send + 'static>(fallback: T, f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.unwrap_or(fallback)
}

fn writer(inner: &Inner) -> Option<Arc<tokio::sync::Mutex<()>>> {
    lock(inner).ok().map(|s| Arc::clone(&s.report_writer))
}

fn ready(state: &State) -> Result<(), Problem> {
    (state.storage == StorageHealth::Ready)
        .then_some(())
        .ok_or(Problem::AccessDenied)
}

// ------------------------------------------------------------------ cases

/// A tenant's cases as v2 holds them now (memory when open, else disk,
/// read without side effects).
pub(super) async fn cases(
    inner: &Arc<Inner>,
    tenant: &TenantId,
) -> Result<Vec<crate::library::TestCase>, Problem> {
    let (open, root) = {
        let s = lock(inner).map_err(|_| Problem::AccessDenied)?;
        if s.tenant(tenant).is_none() {
            return Ok(Vec::new());
        }
        (s.open_library_cases(tenant), s.library_root.clone())
    };
    if let Some(cases) = open {
        return Ok(cases);
    }
    let root = root.ok_or(Problem::NoV2Equivalent)?;
    let worker = Arc::clone(inner);
    let tenant = tenant.as_str().to_owned();
    blocking(Err(Problem::AccessDenied), move || {
        crate::library::read(&root, worker.adapters.keychain.as_ref(), &tenant)
            .map(|l| l.cases)
            .map_err(|_| Problem::Corrupt)
    })
    .await
}

/// Changes a tenant's library under the writer: in memory (then written as
/// its newest generation) when open, otherwise on disk; a library loading
/// meanwhile reads again. `change` returns what it did and whether it
/// changed anything.
async fn change_library<R: Send + 'static>(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    change: impl Fn(&mut crate::library::CaseLibrary) -> (R, bool) + Send + Sync + 'static,
) -> Result<R, Problem> {
    let writer = writer(inner).ok_or(Problem::AccessDenied)?;
    let _write = writer.lock().await;
    let change = Arc::new(change);
    let open = {
        let mut s = lock(inner).map_err(|_| Problem::AccessDenied)?;
        ready(&s)?;
        if s.tenant(tenant).is_none() {
            return Err(Problem::NoTarget);
        }
        let c = Arc::clone(&change);
        s.change_open_library(tenant, move |lib| c(lib))
    };
    if let Some((result, generation)) = open {
        let Some(generation) = generation else {
            return Ok(result);
        };
        let worker = Arc::clone(inner);
        let check = tenant.clone();
        let stored = blocking(
            Err(crate::library::LibraryFailure::Unavailable),
            move || {
                let _placement = crate::sessions::placement();
                let write = lock(&worker)
                    .ok()
                    .and_then(|s| s.library_write(&check, generation))
                    .ok_or(crate::library::LibraryFailure::Unavailable)?;
                crate::library::store(
                    &write.root,
                    worker.adapters.keychain.as_ref(),
                    &write.tenant,
                    &write.library,
                )
            },
        )
        .await;
        let ok = stored.is_ok();
        if let Ok(mut s) = lock(inner) {
            s.settle_library_write(tenant, generation, stored);
            s.changed();
        }
        return if ok {
            Ok(result)
        } else {
            Err(Problem::AccessDenied)
        };
    }
    let worker = Arc::clone(inner);
    let check = tenant.clone();
    let again = Arc::clone(&change);
    let done = blocking(Err(Problem::AccessDenied), move || {
        let _placement = crate::sessions::placement();
        let root = {
            let s = lock(&worker).map_err(|_| Problem::AccessDenied)?;
            if s.tenant(&check).is_none() {
                return Err(Problem::NoTarget);
            }
            s.library_root.clone().ok_or(Problem::NoV2Equivalent)?
        };
        let keychain = worker.adapters.keychain.as_ref();
        let mut library =
            crate::library::read(&root, keychain, check.as_str()).map_err(|_| Problem::Corrupt)?;
        let (result, changed) = change(&mut library);
        if changed {
            crate::library::store(&root, keychain, check.as_str(), &library)
                .map_err(|_| Problem::AccessDenied)?;
        }
        Ok(result)
    })
    .await;
    catch_up_library(inner, tenant, |library| again(library).1);
    done
}

/// A library that opened while its file was written here may have read it
/// before the write: the same change is made to it in memory and its newest
/// generation written (the change finds nothing to do if it read after).
/// One still loading reads again.
fn catch_up_library(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    change: impl FnOnce(&mut crate::library::CaseLibrary) -> bool,
) {
    let Ok(mut s) = lock(inner) else {
        return;
    };
    let effect = match s.reread_library(tenant) {
        Some(lease) => Some(super::super::Effect::LoadLibrary { lease }),
        None => match s.change_open_library(tenant, |library| ((), change(library))) {
            Some(((), Some(generation))) => Some(super::super::Effect::StoreLibrary {
                tenant: tenant.clone(),
                generation,
            }),
            _ => None,
        },
    };
    if let Some(effect) = effect {
        s.changed();
        drop(s);
        run_effects(inner, vec![effect]);
    }
}

// ---------------------------------------------------------------- reports

pub(super) async fn reports(
    inner: &Arc<Inner>,
    tenant: &TenantId,
) -> Result<Vec<crate::sessions::Report>, Problem> {
    let (open, root) = {
        let s = lock(inner).map_err(|_| Problem::AccessDenied)?;
        if s.tenant(tenant).is_none() {
            return Ok(Vec::new());
        }
        (s.open_report_book(tenant), s.sessions_root.clone())
    };
    if let Some(book) = open {
        return Ok(book);
    }
    let path =
        crate::sessions::reports_path(&root.ok_or(Problem::NoV2Equivalent)?, tenant.as_str());
    blocking(Err(Problem::AccessDenied), move || {
        crate::sessions::load_reports(&path)
            .map(|b| b.reports)
            .map_err(|_| Problem::Corrupt)
    })
    .await
}

async fn change_reports<R: Send + 'static>(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    change: impl Fn(&mut crate::sessions::ReportBook) -> (R, bool) + Send + Sync + 'static,
) -> Result<R, Problem> {
    let writer = writer(inner).ok_or(Problem::AccessDenied)?;
    let _write = writer.lock().await;
    let change = Arc::new(change);
    let open = {
        let mut s = lock(inner).map_err(|_| Problem::AccessDenied)?;
        ready(&s)?;
        if s.tenant(tenant).is_none() {
            return Err(Problem::NoTarget);
        }
        let c = Arc::clone(&change);
        s.change_open_reports(tenant, move |b| c(b))
    };
    if let Some((result, generation)) = open {
        let Some(generation) = generation else {
            return Ok(result);
        };
        let Some((path, book)) = lock(inner)
            .ok()
            .and_then(|s| s.report_write(tenant, generation))
        else {
            return Err(Problem::AccessDenied);
        };
        let stored = blocking(false, move || {
            crate::sessions::store_reports(&path, &book).is_ok()
        })
        .await;
        if let Ok(mut s) = lock(inner) {
            s.settle_report_write(tenant, generation, stored);
            s.changed();
        }
        return if stored {
            Ok(result)
        } else {
            Err(Problem::AccessDenied)
        };
    }
    let worker = Arc::clone(inner);
    let check = tenant.clone();
    let again = Arc::clone(&change);
    let done = blocking(Err(Problem::AccessDenied), move || {
        let path = {
            let s = lock(&worker).map_err(|_| Problem::AccessDenied)?;
            if s.tenant(&check).is_none() {
                return Err(Problem::NoTarget);
            }
            crate::sessions::reports_path(
                s.sessions_root.as_ref().ok_or(Problem::NoV2Equivalent)?,
                check.as_str(),
            )
        };
        let mut book = crate::sessions::load_reports(&path).map_err(|_| Problem::Corrupt)?;
        let (result, changed) = change(&mut book);
        if changed {
            crate::sessions::store_reports(&path, &book).map_err(|_| Problem::AccessDenied)?;
        }
        Ok(result)
    })
    .await;
    catch_up_reports(inner, tenant, |book| again(book).1);
    done
}

/// [`catch_up_library`] for a tenant's report book.
fn catch_up_reports(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    change: impl FnOnce(&mut crate::sessions::ReportBook) -> bool,
) {
    let Ok(mut s) = lock(inner) else {
        return;
    };
    let effect = match s.reread_reports(tenant) {
        Some(lease) => Some(super::super::Effect::LoadReports { lease }),
        None => match s.change_open_reports(tenant, |book| ((), change(book))) {
            Some(((), Some(generation))) => Some(super::super::Effect::StoreReports {
                tenant: tenant.clone(),
                generation,
            }),
            _ => None,
        },
    };
    if let Some(effect) = effect {
        s.changed();
        drop(s);
        run_effects(inner, vec![effect]);
    }
}

// ----------------------------------------------------------------- drafts

/// The signed-in learner's offline store, if it is the same account.
fn learner_store(
    inner: &Inner,
) -> Result<(Arc<super::super::offline_domain::OfflineGen>, String), Problem> {
    let s = lock(inner).map_err(|_| Problem::AccessDenied)?;
    let account = s
        .data
        .learner
        .as_ref()
        .map(|l| l.account_id.clone())
        .ok_or(Problem::SignInRequired)?;
    let generation = s.offline.clone().ok_or(Problem::SignInRequired)?;
    if generation.account() != account {
        return Err(Problem::SignInRequired);
    }
    Ok((generation, crate::import_v1::plan::learner_digest(&account)))
}

pub(super) async fn draft(inner: &Arc<Inner>, attempt: &str) -> Result<Option<Value>, Problem> {
    let (generation, _) = learner_store(inner)?;
    let attempt = attempt.to_owned();
    blocking(Err(Problem::AccessDenied), move || {
        let Some(store) = generation.store() else {
            // Nothing kept on this Mac yet for this account.
            return Ok(None);
        };
        store
            .get(crate::offline::RecordKind::Draft, &attempt)
            .map_err(|_| Problem::Corrupt)
    })
    .await
}

// ---------------------------------------------------------- conversations

fn history_scope(
    inner: &Inner,
    tenant: &TenantId,
    workspace: &str,
) -> Result<crate::harness::history::Scope, Problem> {
    let s = lock(inner).map_err(|_| Problem::AccessDenied)?;
    let root = s.history_root().ok_or(Problem::NoV2Equivalent)?;
    Ok(crate::harness::history::Scope::new(
        &root,
        tenant.as_str(),
        workspace,
    ))
}

/// A record in its scope (if present) and how many the scope holds.
pub(super) async fn conversation(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    workspace: &str,
    id: &str,
) -> Result<(Option<crate::harness::history::Record>, usize), Problem> {
    let scope = history_scope(inner, tenant, workspace)?;
    let worker = Arc::clone(inner);
    let id = id.to_owned();
    blocking(Err(Problem::AccessDenied), move || {
        use crate::harness::history::{self, HistoryFailure};
        let held = history::list(&*worker.adapters.keychain, &scope)
            .map(|l| l.len())
            .map_err(|_| Problem::Corrupt)?;
        match history::read(&*worker.adapters.keychain, &scope, &id) {
            Ok(record) => Ok((Some(record), held)),
            Err(HistoryFailure::Missing) => Ok((None, held)),
            Err(HistoryFailure::Unreadable) if held == 0 => Ok((None, held)),
            Err(_) => Err(Problem::Corrupt),
        }
    })
    .await
}

// ------------------------------------------------------------- API Client

pub(super) async fn api_client_present(
    inner: &Arc<Inner>,
    candidate: &crate::workbench::adoption::Candidate,
) -> Result<bool, Problem> {
    let recent = recents(inner).await?;
    Ok(recent.contains(&candidate.root))
}

async fn recents(inner: &Arc<Inner>) -> Result<Vec<std::path::PathBuf>, Problem> {
    super::super::workbench_domain::v1_recent_roots(inner)
        .await
        .ok_or(Problem::AccessDenied)
}

// ------------------------------------------------------------ the writes

/// What an owner wrote, and why it stopped (`None`: a fixture interruption).
type Failed = (Vec<Written>, Option<Problem>);

fn fail(p: Problem) -> Failed {
    (Vec::new(), Some(p))
}

fn case_entry(tenant: &TenantId, case: &crate::library::TestCase) -> Written {
    Written::Case {
        tenant: tenant.as_str().to_owned(),
        id: case.id.clone(),
        hash: content_hash(case),
    }
}

fn report_entry(tenant: &TenantId, report: &crate::sessions::Report) -> Written {
    Written::Report {
        tenant: tenant.as_str().to_owned(),
        scan_id: report.scan_id.clone(),
        sub: report.scan_sub_req_id,
        hash: content_hash(report),
    }
}

/// A kept draft's content as an import wrote it.
fn draft_hash(v: &Value) -> String {
    content_hash(&json!({
        "attemptId": v["attemptId"], "baseVersion": v["baseVersion"], "draft": v["draft"]
    }))
}

/// Writes one item's new entries: only what v2 does not hold, each noted
/// in the receipt (inside the section that writes it) before it is
/// written. What it wrote, or what it wrote before it stopped.
pub(super) async fn write(
    inner: &Arc<Inner>,
    journal: &Journal,
    payload: Payload,
    target: Option<&TenantId>,
) -> Result<Vec<Written>, Failed> {
    match payload {
        Payload::Tenant {
            label,
            tsg_id,
            destination,
            ..
        } => {
            let id = target.ok_or(fail(Problem::NoTarget))?.clone();
            let profile = ProfileDraft {
                label,
                tsg_id,
                destination,
                environment: Default::default(),
            }
            .validate()
            .ok_or(fail(Problem::Corrupt))?;
            let entry = Written::Tenant {
                id: id.as_str().to_owned(),
                hash: content_hash(&profile),
            };
            let room = |s: &State| -> Result<bool, Failed> {
                ready(s).map_err(fail)?;
                // Its identity was issued at review: present only when an
                // earlier attempt's tenant was kept.
                if s.tenant(&id).is_some() {
                    return Ok(false);
                }
                if s.data.tenants.len() >= super::super::MAX_TENANTS {
                    return Err(fail(Problem::DestinationFull));
                }
                Ok(true)
            };
            if !room(&*lock(inner).map_err(|_| fail(Problem::AccessDenied))?)? {
                return Ok(Vec::new());
            }
            // Noted first: the identity is this receipt's alone.
            journal
                .writing(inner, std::slice::from_ref(&entry))
                .map_err(|stop| (Vec::new(), stop))?;
            let mut s = lock(inner).map_err(|_| fail(Problem::AccessDenied))?;
            if !room(&s)? {
                return Ok(Vec::new());
            }
            let before = s.data.clone();
            s.data
                .tenants
                .push(super::super::Tenant::new(TenantProfile {
                    id,
                    label: profile.label,
                    tsg_id: profile.tsg_id,
                    destination: profile.destination,
                    environment: profile.environment,
                }));
            s.v1_import.staged.push(entry.clone());
            // A first-run draft is over once a tenant exists (C29); its
            // Keychain records join the cleanup queue (session records of a
            // draft are not expected during an import).
            let _ = s.end_first_run_setup();
            s.save_with(&before)
                .map_err(|_| fail(Problem::AccessDenied))?;
            s.changed();
            Ok(vec![entry])
        }
        Payload::Appearance(theme) => {
            let before = {
                let s = lock(inner).map_err(|_| fail(Problem::AccessDenied))?;
                ready(&s).map_err(fail)?;
                s.appearance()
            };
            if before == theme {
                return Ok(Vec::new());
            }
            let entry = Written::Appearance {
                before,
                after: theme,
            };
            journal
                .writing(inner, std::slice::from_ref(&entry))
                .map_err(|stop| (Vec::new(), stop))?;
            let mut s = lock(inner).map_err(|_| fail(Problem::AccessDenied))?;
            ready(&s).map_err(fail)?;
            if s.appearance() != before {
                return Ok(Vec::new());
            }
            let previous = s.data.clone();
            s.data.appearance = Some(theme);
            s.v1_import.staged.push(entry.clone());
            s.save_with(&previous)
                .map_err(|_| fail(Problem::AccessDenied))?;
            s.changed();
            Ok(vec![entry])
        }
        Payload::Cases(incoming) => {
            let tenant = target.ok_or(fail(Problem::NoTarget))?.clone();
            let wanted = incoming.iter().map(|c| rebound(c, Some(&tenant))).collect();
            insert_cases(inner, journal, &tenant, wanted).await
        }
        Payload::Reports { reports, .. } => {
            let tenant = target.ok_or(fail(Problem::NoTarget))?.clone();
            insert_reports(inner, journal, &tenant, reports).await
        }
        Payload::Drafts {
            owner_digest,
            drafts,
            ..
        } => insert_drafts(inner, journal, &owner_digest, drafts).await,
        Payload::Conversations(snapshots) => {
            let tenant = target.ok_or(fail(Problem::NoTarget))?.clone();
            let mut fresh = Vec::new();
            for s in &snapshots {
                let Some(record) = imported_record(&tenant, s) else {
                    continue;
                };
                let (existing, _) = conversation(inner, &tenant, &s.context.workspace, &record.id)
                    .await
                    .map_err(fail)?;
                if existing.is_none() {
                    let entry = Written::Conversation {
                        tenant: tenant.as_str().to_owned(),
                        workspace: s.context.workspace.clone(),
                        id: record.id.clone(),
                        hash: conversation_hash(&record),
                    };
                    fresh.push((s.context.workspace.clone(), record, entry));
                }
            }
            if fresh.is_empty() {
                return Ok(Vec::new());
            }
            let entries: Vec<Written> = fresh.iter().map(|(_, _, e)| e.clone()).collect();
            stage(inner, &entries);
            journal
                .writing(inner, &entries)
                .map_err(|stop| (Vec::new(), stop))?;
            let mut done = Vec::new();
            for (workspace, record, entry) in fresh {
                match save_conversation(inner, &tenant, &workspace, record).await {
                    Ok(true) => done.push(entry),
                    // Saved meanwhile by something else: not this import's.
                    Ok(false) => {}
                    Err(p) => return Err((done, Some(p))),
                }
            }
            Ok(done)
        }
        Payload::ApiClient(candidates) => {
            let noted = Arc::new(std::sync::Mutex::new(None));
            let (worker, journal, slot) = (Arc::clone(inner), journal.clone(), Arc::clone(&noted));
            let added = super::super::workbench_domain::add_v1_recents(
                inner,
                candidates,
                move |added: &[(std::path::PathBuf, String)]| {
                    let entries: Vec<Written> = added
                        .iter()
                        .map(|(root, id)| Written::Recent {
                            root: root.clone(),
                            id: id.clone(),
                        })
                        .collect();
                    stage(&worker, &entries);
                    let noted = journal.writing(&worker, &entries);
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some(entries);
                    }
                    noted
                },
            )
            .await;
            let entries = noted
                .lock()
                .ok()
                .and_then(|mut slot| slot.take())
                .unwrap_or_default();
            match added {
                Ok(_) => Ok(entries),
                Err(Some(stop)) => Err((Vec::new(), stop)),
                Err(None) => Err((entries, Some(Problem::AccessDenied))),
            }
        }
    }
}

/// Adds the cases a tenant's library does not hold, under the local-data
/// writer. An open library is changed in memory (and staged) in one step,
/// then noted, then written as its newest generation; nothing of it reaches
/// disk before the note, since the writer is held. A library not open is
/// read, noted, and written under the placement lock.
async fn insert_cases(
    inner: &Arc<Inner>,
    journal: &Journal,
    tenant: &TenantId,
    wanted: Vec<crate::library::TestCase>,
) -> Result<Vec<Written>, Failed> {
    let writer = writer(inner).ok_or(fail(Problem::AccessDenied))?;
    let _write = writer.lock().await;
    let mut wanted = Some(wanted);
    let open = {
        let mut s = lock(inner).map_err(|_| fail(Problem::AccessDenied))?;
        ready(&s).map_err(fail)?;
        if s.tenant(tenant).is_none() {
            return Err(fail(Problem::NoTarget));
        }
        let added = s.change_open_library(tenant, |library| {
            let fresh: Vec<_> = wanted
                .take()
                .unwrap_or_default()
                .into_iter()
                .filter(|c| !library.cases.iter().any(|e| e.id == c.id))
                .collect();
            match library.merge(fresh.clone()) {
                Ok(n) => (Ok(fresh), n > 0),
                Err(_) => (Err(Problem::DestinationFull), false),
            }
        });
        if let Some((Ok(fresh), _)) = &added {
            let entries: Vec<Written> = fresh.iter().map(|c| case_entry(tenant, c)).collect();
            s.v1_import.staged.extend(entries);
        }
        added
    };
    if let Some((added, generation)) = open {
        let entries: Vec<Written> = added
            .map_err(fail)?
            .iter()
            .map(|c| case_entry(tenant, c))
            .collect();
        if generation.is_none() {
            return Ok(Vec::new());
        }
        if let Err(stop) = journal.writing(inner, &entries) {
            if stop.is_some() {
                // Not recorded: taken out of memory again, never written.
                let back = entries.clone();
                if let Ok(mut s) = lock(inner) {
                    s.change_open_library(tenant, move |library| {
                        let before = library.cases.len();
                        library.cases.retain(|c| {
                            !back.iter().any(|w| {
                                matches!(w, Written::Case { id, hash, .. }
                                    if id == &c.id && *hash == content_hash(c))
                            })
                        });
                        ((), library.cases.len() != before)
                    });
                }
            }
            return Err((Vec::new(), stop));
        }
        store_library(inner, tenant)
            .await
            .map_err(|p| (entries.clone(), Some(p)))?;
        return Ok(entries);
    }
    let wanted = wanted.unwrap_or_default();
    let again = wanted.clone();
    let worker = Arc::clone(inner);
    let check = tenant.clone();
    let journal = journal.clone();
    let done = blocking(Err(fail(Problem::AccessDenied)), move || {
        let _placement = crate::sessions::placement();
        let root = {
            let s = lock(&worker).map_err(|_| fail(Problem::AccessDenied))?;
            if s.tenant(&check).is_none() {
                return Err(fail(Problem::NoTarget));
            }
            s.library_root
                .clone()
                .ok_or(fail(Problem::NoV2Equivalent))?
        };
        let keychain = worker.adapters.keychain.as_ref();
        let mut library = crate::library::read(&root, keychain, check.as_str())
            .map_err(|_| fail(Problem::Corrupt))?;
        let fresh: Vec<_> = wanted
            .into_iter()
            .filter(|c| !library.cases.iter().any(|e| e.id == c.id))
            .collect();
        if fresh.is_empty() {
            return Ok(Vec::new());
        }
        library
            .merge(fresh.clone())
            .map_err(|_| fail(Problem::DestinationFull))?;
        let entries: Vec<Written> = fresh.iter().map(|c| case_entry(&check, c)).collect();
        stage(&worker, &entries);
        journal
            .writing(&worker, &entries)
            .map_err(|stop| (Vec::new(), stop))?;
        crate::library::store(&root, keychain, check.as_str(), &library)
            .map_err(|_| (entries.clone(), Some(Problem::AccessDenied)))?;
        Ok(entries)
    })
    .await;
    let written = done.as_ref().map_or_else(|(w, _)| w.clone(), Clone::clone);
    catch_up_library(inner, tenant, move |library| {
        let fresh: Vec<_> = again
            .into_iter()
            .filter(|c| written.contains(&case_entry(tenant, c)))
            .filter(|c| !library.cases.iter().any(|e| e.id == c.id))
            .collect();
        !fresh.is_empty() && library.merge(fresh).is_ok()
    });
    done
}

/// Writes a tenant's open library as its newest generation (the writer is
/// held by the caller); a change made meanwhile is written with it.
async fn store_library(inner: &Arc<Inner>, tenant: &TenantId) -> Result<(), Problem> {
    let Some((write, generation)) = lock(inner).ok().and_then(|s| {
        let generation = s.library_generation(tenant)?;
        Some((s.library_write(tenant, generation)?, generation))
    }) else {
        return Err(Problem::AccessDenied);
    };
    let worker = Arc::clone(inner);
    let stored = blocking(
        Err(crate::library::LibraryFailure::Unavailable),
        move || {
            let _placement = crate::sessions::placement();
            crate::library::store(
                &write.root,
                worker.adapters.keychain.as_ref(),
                &write.tenant,
                &write.library,
            )
        },
    )
    .await;
    let ok = stored.is_ok();
    if let Ok(mut s) = lock(inner) {
        s.settle_library_write(tenant, generation, stored);
        s.changed();
    }
    ok.then_some(()).ok_or(Problem::AccessDenied)
}

/// [`insert_cases`] for a tenant's FP/FN reports.
async fn insert_reports(
    inner: &Arc<Inner>,
    journal: &Journal,
    tenant: &TenantId,
    wanted: Vec<crate::sessions::Report>,
) -> Result<Vec<Written>, Failed> {
    let writer = writer(inner).ok_or(fail(Problem::AccessDenied))?;
    let _write = writer.lock().await;
    // Reports keep v1's order: newest first, as v1 showed them.
    let add = move |book: &mut crate::sessions::ReportBook| {
        let mut added = Vec::new();
        for r in wanted {
            if book
                .reports
                .iter()
                .any(|e| e.scan_id == r.scan_id && e.scan_sub_req_id == r.scan_sub_req_id)
            {
                continue;
            }
            if book.save(r.clone()).is_err() {
                return Err(Problem::DestinationFull);
            }
            added.push(r);
        }
        book.reports
            .sort_by(|a, b| b.reported_at.cmp(&a.reported_at));
        Ok(added)
    };
    let open = {
        let mut s = lock(inner).map_err(|_| fail(Problem::AccessDenied))?;
        ready(&s).map_err(fail)?;
        if s.tenant(tenant).is_none() {
            return Err(fail(Problem::NoTarget));
        }
        let mut add = Some(add);
        let added = s.change_open_reports(tenant, |book| {
            let Some(add) = add.take() else {
                return (Ok(Vec::new()), false);
            };
            let mut trial = book.clone();
            match add(&mut trial) {
                Ok(added) if !added.is_empty() => {
                    *book = trial;
                    (Ok(added), true)
                }
                other => (other, false),
            }
        });
        if let Some((Ok(added), _)) = &added {
            let entries: Vec<Written> = added.iter().map(|r| report_entry(tenant, r)).collect();
            s.v1_import.staged.extend(entries);
        }
        match added {
            Some(added) => Ok(added),
            None => Err(add),
        }
    };
    match open {
        Ok((added, generation)) => {
            let entries: Vec<Written> = added
                .map_err(fail)?
                .iter()
                .map(|r| report_entry(tenant, r))
                .collect();
            if generation.is_none() {
                return Ok(Vec::new());
            }
            if let Err(stop) = journal.writing(inner, &entries) {
                if stop.is_some() {
                    let back = entries.clone();
                    if let Ok(mut s) = lock(inner) {
                        s.change_open_reports(tenant, move |book| {
                            let before = book.reports.len();
                            book.reports.retain(|r| {
                                !back.iter().any(|w| {
                                    matches!(w, Written::Report { scan_id, sub, hash, .. }
                                        if scan_id == &r.scan_id && *sub == r.scan_sub_req_id
                                            && *hash == content_hash(r))
                                })
                            });
                            ((), book.reports.len() != before)
                        });
                    }
                }
                return Err((Vec::new(), stop));
            }
            store_reports(inner, tenant)
                .await
                .map_err(|p| (entries.clone(), Some(p)))?;
            Ok(entries)
        }
        Err(add) => {
            let Some(add) = add else {
                return Err(fail(Problem::AccessDenied));
            };
            let worker = Arc::clone(inner);
            let check = tenant.clone();
            let journal = journal.clone();
            let done = blocking(Err(fail(Problem::AccessDenied)), move || {
                let path = {
                    let s = lock(&worker).map_err(|_| fail(Problem::AccessDenied))?;
                    if s.tenant(&check).is_none() {
                        return Err(fail(Problem::NoTarget));
                    }
                    crate::sessions::reports_path(
                        s.sessions_root
                            .as_ref()
                            .ok_or(fail(Problem::NoV2Equivalent))?,
                        check.as_str(),
                    )
                };
                let mut book =
                    crate::sessions::load_reports(&path).map_err(|_| fail(Problem::Corrupt))?;
                let added = add(&mut book).map_err(fail)?;
                if added.is_empty() {
                    return Ok((Vec::new(), added));
                }
                let entries: Vec<Written> = added.iter().map(|r| report_entry(&check, r)).collect();
                stage(&worker, &entries);
                journal
                    .writing(&worker, &entries)
                    .map_err(|stop| (Vec::new(), stop))?;
                crate::sessions::store_reports(&path, &book)
                    .map_err(|_| (entries.clone(), Some(Problem::AccessDenied)))?;
                Ok((entries, added))
            })
            .await;
            let added = done.as_ref().map(|(_, a)| a.clone()).unwrap_or_default();
            catch_up_reports(inner, tenant, move |book| {
                let mut changed = false;
                for r in added {
                    if !book
                        .reports
                        .iter()
                        .any(|e| e.scan_id == r.scan_id && e.scan_sub_req_id == r.scan_sub_req_id)
                        && book.save(r).is_ok()
                    {
                        changed = true;
                    }
                }
                book.reports
                    .sort_by(|a, b| b.reported_at.cmp(&a.reported_at));
                changed
            });
            done.map(|(entries, _)| entries)
        }
    }
}

/// Writes a tenant's open report book as its newest generation (the writer
/// is held by the caller).
async fn store_reports(inner: &Arc<Inner>, tenant: &TenantId) -> Result<(), Problem> {
    let Some((path, book, generation)) = lock(inner).ok().and_then(|s| {
        let generation = s.report_generation(tenant)?;
        let (path, book) = s.report_write(tenant, generation)?;
        Some((path, book, generation))
    }) else {
        return Err(Problem::AccessDenied);
    };
    let stored = blocking(false, move || {
        crate::sessions::store_reports(&path, &book).is_ok()
    })
    .await;
    if let Ok(mut s) = lock(inner) {
        s.settle_report_write(tenant, generation, stored);
        s.changed();
    }
    stored.then_some(()).ok_or(Problem::AccessDenied)
}

/// Adds the drafts the planned account does not hold, into that account's
/// own store only: refused when another account (or none) is signed in.
/// Each is added only while its attempt has no draft (another written
/// meanwhile is never replaced) and is marked with the receipt, so a
/// rollback removes only the draft this import wrote.
async fn insert_drafts(
    inner: &Arc<Inner>,
    journal: &Journal,
    owner: &str,
    drafts: Vec<parse::V1Draft>,
) -> Result<Vec<Written>, Failed> {
    let (generation, digest) = learner_store(inner).map_err(fail)?;
    if digest != owner {
        return Err(fail(Problem::AccountMismatch));
    }
    let worker = Arc::clone(inner);
    let journal = journal.clone();
    blocking(Err(fail(Problem::AccessDenied)), move || {
        let (store, _writer) = generation
            .ensure_open()
            .map_err(|_| fail(Problem::AccessDenied))?;
        // The store is the planned account's (its generation never changes
        // account).
        if crate::import_v1::plan::learner_digest(generation.account()) != digest {
            return Err(fail(Problem::AccountMismatch));
        }
        let mut fresh = Vec::new();
        for d in drafts {
            let held = store
                .get(crate::offline::RecordKind::Draft, &d.attempt_id)
                .map_err(|_| fail(Problem::Corrupt))?;
            if held.is_none() {
                let entry = Written::Draft {
                    account: digest.clone(),
                    attempt: d.attempt_id.clone(),
                    hash: content_hash(&draft_value(&d)),
                };
                fresh.push((d, entry));
            }
        }
        if fresh.is_empty() {
            return Ok(Vec::new());
        }
        let entries: Vec<Written> = fresh.iter().map(|(_, e)| e.clone()).collect();
        stage(&worker, &entries);
        journal
            .writing(&worker, &entries)
            .map_err(|stop| (Vec::new(), stop))?;
        let id = journal.id();
        let mut done = Vec::new();
        for (d, entry) in fresh {
            let mut value = draft_value(&d);
            value["savedAt"] = json!(super::now_secs());
            value["importedBy"] = json!(id);
            match store.put_if_absent(crate::offline::RecordKind::Draft, &d.attempt_id, &value) {
                Ok(true) => done.push(entry),
                // Written meanwhile through Academy: kept, not this import's.
                Ok(false) => {}
                Err(e) => {
                    generation.refresh();
                    let p = match e {
                        crate::offline::OfflineError::TooLarge
                        | crate::offline::OfflineError::Full => Problem::DestinationFull,
                        _ => Problem::AccessDenied,
                    };
                    return Err((done, Some(p)));
                }
            }
        }
        generation.refresh();
        Ok(done)
    })
    .await
}

/// Saves one imported conversation like any history write (admitted with
/// its scope noted first, so tenant removal and reset wait for it), and
/// only while its scope holds no record of that ID. `false`: one is held.
async fn save_conversation(
    inner: &Arc<Inner>,
    tenant: &TenantId,
    workspace: &str,
    record: crate::harness::history::Record,
) -> Result<bool, Problem> {
    let scope = history_scope(inner, tenant, workspace)?;
    {
        let mut s = lock(inner).map_err(|_| Problem::AccessDenied)?;
        if !s.admit_history_write(tenant, workspace, &scope, &record.id) {
            return Err(Problem::AccessDenied);
        }
    }
    let worker = Arc::clone(inner);
    let saving = scope.clone();
    let saved = blocking(
        Err(crate::harness::history::HistoryFailure::Unavailable),
        move || {
            if saving.ids()?.contains(&record.id) {
                return Ok(false);
            }
            crate::harness::history::save(&*worker.adapters.keychain, &saving, &record)
                .map(|()| true)
        },
    )
    .await;
    if let Ok(mut s) = lock(inner) {
        s.history_written(tenant, &scope);
        s.changed();
    }
    saved.map_err(|f| match f {
        crate::harness::history::HistoryFailure::Full => Problem::DestinationFull,
        crate::harness::history::HistoryFailure::TooLarge => Problem::TooLarge,
        _ => Problem::AccessDenied,
    })
}

/// Reverts one written entry while v2 still holds exactly what the receipt
/// `receipt` wrote, checked inside the owner's own section.
pub(super) async fn revert(inner: &Arc<Inner>, receipt: &str, w: &Written) -> Revert {
    let outcome = |o: Result<Revert, Problem>| o.unwrap_or(Revert::Failed);
    match w {
        Written::Tenant { id, hash } => {
            let Some(tenant) = TenantId::parse(id) else {
                return Revert::Failed;
            };
            let held = lock(inner).ok().map(|s| {
                s.tenant(&tenant).map(|t| {
                    content_hash(&ProfileDraft {
                        label: t.profile.label.clone(),
                        tsg_id: t.profile.tsg_id.clone(),
                        destination: t.profile.destination.clone(),
                        environment: t.profile.environment,
                    }) == *hash
                })
            });
            match held {
                None => Revert::Failed,
                Some(None) => Revert::Gone,
                Some(Some(false)) => Revert::Kept,
                Some(Some(true)) => match revert_tenant(inner, &tenant).await {
                    Ok(true) => Revert::Removed,
                    Ok(false) => Revert::Kept,
                    Err(_) => Revert::Failed,
                },
            }
        }
        Written::Appearance { before, after } => outcome((|| {
            let mut s = lock(inner).map_err(|_| Problem::AccessDenied)?;
            ready(&s)?;
            if s.appearance() != *after {
                return Ok(Revert::Kept);
            }
            let previous = s.data.clone();
            s.data.appearance = Some(*before);
            s.save_with(&previous).map_err(|_| Problem::AccessDenied)?;
            s.changed();
            Ok(Revert::Removed)
        })()),
        Written::Case { tenant, id, hash } => {
            let Some(tenant) = TenantId::parse(tenant) else {
                return Revert::Failed;
            };
            let (id, hash) = (id.clone(), hash.clone());
            outcome(
                change_library(inner, &tenant, move |library| {
                    match library.cases.iter().find(|c| c.id == id) {
                        None => (Revert::Gone, false),
                        Some(c) if content_hash(c) != hash => (Revert::Kept, false),
                        Some(_) => {
                            library.remove(&id);
                            (Revert::Removed, true)
                        }
                    }
                })
                .await,
            )
        }
        Written::Report {
            tenant,
            scan_id,
            sub,
            hash,
        } => {
            let Some(tenant) = TenantId::parse(tenant) else {
                return Revert::Failed;
            };
            let (scan_id, sub, hash) = (scan_id.clone(), *sub, hash.clone());
            outcome(
                change_reports(inner, &tenant, move |book| {
                    match book
                        .reports
                        .iter()
                        .find(|r| r.scan_id == scan_id && r.scan_sub_req_id == sub)
                    {
                        None => (Revert::Gone, false),
                        Some(r) if content_hash(r) != hash => (Revert::Kept, false),
                        Some(_) => {
                            book.remove(&scan_id, sub);
                            (Revert::Removed, true)
                        }
                    }
                })
                .await,
            )
        }
        Written::Draft {
            account,
            attempt,
            hash,
        } => {
            // Only the account it was written for: another account's
            // drafts are never read or removed.
            let Ok((generation, digest)) = learner_store(inner) else {
                return Revert::Failed;
            };
            if &digest != account {
                return Revert::Failed;
            }
            let (attempt, hash, receipt) = (attempt.clone(), hash.clone(), receipt.to_owned());
            outcome(
                blocking(Err(Problem::AccessDenied), move || {
                    let (store, _writer) = generation
                        .ensure_open()
                        .map_err(|_| Problem::AccessDenied)?;
                    if crate::import_v1::plan::learner_digest(generation.account()) != digest {
                        return Err(Problem::AccountMismatch);
                    }
                    let ours = |v: &Value| {
                        draft_hash(v) == hash && v["importedBy"] == Value::String(receipt.clone())
                    };
                    let removed = store
                        .delete_if(crate::offline::RecordKind::Draft, &attempt, ours)
                        .map_err(|_| Problem::AccessDenied)?;
                    generation.refresh();
                    if removed {
                        return Ok(Revert::Removed);
                    }
                    let held = store
                        .get(crate::offline::RecordKind::Draft, &attempt)
                        .map_err(|_| Problem::AccessDenied)?;
                    Ok(if held.is_some() {
                        Revert::Kept
                    } else {
                        Revert::Gone
                    })
                })
                .await,
            )
        }
        Written::Conversation {
            tenant,
            workspace,
            id,
            hash,
        } => {
            let Some(tenant) = TenantId::parse(tenant) else {
                return Revert::Failed;
            };
            let scope = match history_scope(inner, &tenant, workspace) {
                Ok(scope) => scope,
                Err(_) => return Revert::Failed,
            };
            let worker = Arc::clone(inner);
            let (id, hash) = (id.clone(), hash.clone());
            outcome(
                blocking(Err(Problem::AccessDenied), move || {
                    use crate::harness::history::{self, HistoryFailure, Origin};
                    if !scope
                        .ids()
                        .map_err(|_| Problem::AccessDenied)?
                        .contains(&id)
                    {
                        return Ok(Revert::Gone);
                    }
                    // Only an imported record with what this import wrote.
                    match history::read(&*worker.adapters.keychain, &scope, &id) {
                        Ok(r)
                            if r.origin == Some(Origin::ImportedV1)
                                && conversation_hash(&r) == hash => {}
                        Ok(_) | Err(HistoryFailure::Unreadable) => return Ok(Revert::Kept),
                        Err(HistoryFailure::Missing) => return Ok(Revert::Gone),
                        Err(_) => return Err(Problem::AccessDenied),
                    }
                    match history::delete(&scope, &id) {
                        Ok(()) => Ok(Revert::Removed),
                        Err(HistoryFailure::Missing) => Ok(Revert::Gone),
                        Err(_) => Err(Problem::AccessDenied),
                    }
                })
                .await,
            )
        }
        Written::Recent { root, id } => {
            match super::super::workbench_domain::remove_v1_recent(inner, root, id).await {
                Ok(true) => Revert::Removed,
                Ok(false) => Revert::Gone,
                Err(_) => Revert::Failed,
            }
        }
    }
}

/// An imported tenant goes through the same removal as Delete Tenant, but
/// only while nothing else of it exists: no credential, sign-in, project,
/// server, case or report added since.
async fn revert_tenant(inner: &Arc<Inner>, tenant: &TenantId) -> Result<bool, Problem> {
    let tenant = tenant.clone();
    if !cases(inner, &tenant).await?.is_empty() || !reports(inner, &tenant).await?.is_empty() {
        return Ok(false);
    }
    // No conversation is kept for it (any added since would go with it).
    if let Some(root) = lock(inner).ok().and_then(|s| s.history_root()) {
        let dir = crate::harness::history::tenant_dir(&root, tenant.as_str());
        let held = blocking(true, move || sealed_under(&dir)).await;
        if held {
            return Ok(false);
        }
    }
    let mut effects = Vec::new();
    {
        let mut s = lock(inner).map_err(|_| Problem::AccessDenied)?;
        if !s.v1_tenant_unused(&tenant) {
            return Ok(false);
        }
        super::super::admit(
            &mut s,
            &inner.session,
            &inner.adapters,
            Intent::DeleteTenant { tenant },
            &mut effects,
        )
        .map_err(|_| Problem::AccessDenied)?;
    }
    run_effects(inner, effects);
    Ok(true)
}

/// Whether any sealed record is kept below `dir`.
fn sealed_under(dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let path = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => sealed_under(&path),
            Ok(_) => path.extension().is_some_and(|x| x == "sealed"),
            Err(_) => true,
        }
    })
}
