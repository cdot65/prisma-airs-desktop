//! Academy offline work inside the authority (C07): one handle per signed-in
//! learner generation over that account's encrypted store
//! ([`crate::offline`]), downloads under Academy's lease and revision,
//! offline answers for the few reads a downloaded course or cached attempt
//! can serve, lab work kept on this Mac until the learner saves it to
//! Academy, and revalidation when Academy is reachable again.
//!
//! Nothing here acts on Academy by itself: offline answers are reads, local
//! drafts are never sent automatically, and reconnecting only re-checks
//! access, leases and revisions. A retired generation's handle is dropped
//! with it, so sign-out, an account change and reset lock the store. Every
//! store operation enters the generation's write ledger first
//! ([`OfflineGen::enter`]): a retired generation or a reset in progress
//! admits none, and reset waits for those already admitted.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use ts_rs::TS;

use super::learner_domain::LearnerSession;
use super::{CredentialStore, Inner, StorageHealth, lock};
use crate::academy::{AcademyClient, CallFailure, CallReply};
use crate::learner::ProcedureKind;
use crate::offline::{OfflineError, OfflineStore, RecordKind};
use crate::vault::{RecordId, VaultError};

/// Reads a downloaded course or cached attempt can answer while Academy is
/// unreachable.
pub(super) const OFFLINE_READS: [&str; 4] = [
    "units.get",
    "labs.get",
    "desktop.attempt.get",
    "desktop.attempt.list",
];

/// A course, unit or attempt id as records and the snapshot name them.
pub(super) fn plain_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Cached attempts re-read on reconnect (Academy lists at most 100 per page).
const MAX_CACHED_ATTEMPTS: usize = 100;

/// Academy's draft cap (`draftSchema`, 3 MiB).
const DRAFT_LIMIT: usize = 3 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Units fetched one by one when a course exceeds Academy's bundle cap.
const MAX_UNITS: usize = 400;

/// Seconds since the Unix epoch on this Mac's clock.
pub(super) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Seconds since the epoch of an RFC 3339 UTC time (`YYYY-MM-DDTHH:MM:SS[.fff]Z`).
pub(super) fn parse_iso(text: &str) -> Option<u64> {
    let b = text.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    if !text.ends_with('Z') {
        return None;
    }
    let num = |s: &str| s.parse::<u64>().ok();
    let (y, mo, d) = (num(&text[0..4])?, num(&text[5..7])?, num(&text[8..10])?);
    let (h, mi, s) = (
        num(&text[11..13])?,
        num(&text[14..16])?,
        num(&text[17..19])?,
    );
    let rest = &text[19..text.len() - 1];
    if !(rest.is_empty()
        || (rest.starts_with('.') && rest[1..].bytes().all(|c| c.is_ascii_digit())))
    {
        return None;
    }
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 || y < 1970
    {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let (y, m) = if mo <= 2 {
        (y - 1, mo + 9)
    } else {
        (y, mo - 3)
    };
    let era = y / 400;
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + s)
}

/// An RFC 3339 UTC time for seconds since the epoch.
pub(super) fn iso(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// What a downloaded course is to the learner now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    /// Readable offline: the lease holds and Academy last confirmed it.
    Current,
    /// Readable offline, but Academy has a newer revision to download.
    Outdated,
    /// Not readable until Academy renews the lease (online).
    Expired,
    /// Academy no longer gives this account the course; not readable.
    Withdrawn,
}

/// One downloaded course in the snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DownloadView {
    pub course_id: String,
    pub title: String,
    pub revision: String,
    pub downloaded_at: String,
    pub lease_expires_at: String,
    pub status: DownloadStatus,
    pub units: u32,
    /// Where reading the course starts (a unit id from the bundle; its
    /// lessons link on to the rest), if a plain id.
    pub first_unit: Option<String>,
}

/// Lab work kept on this Mac that Academy has not confirmed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LocalDraftView {
    pub attempt_id: String,
    pub base_version: u32,
    pub saved_at: String,
}

/// Whether offline work can be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OfflineState {
    /// No signed-in account (offline work stays locked on disk).
    SignedOut,
    /// Opening the account's store.
    Opening,
    /// Nothing kept on this Mac for this account yet.
    Empty,
    /// The account's store is open.
    Ready,
    /// The store could not be opened (locked Keychain, another instance,
    /// unreadable key); nothing was written in plaintext and saved work was
    /// kept.
    Unavailable,
}

/// Offline work in the snapshot (never content, only what it is).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OfflineView {
    pub state: OfflineState,
    /// False after a learner call found Academy unreachable, until one gets
    /// a reply again.
    pub reachable: bool,
    pub downloads: Vec<DownloadView>,
    pub local_drafts: Vec<LocalDraftView>,
}

impl OfflineView {
    pub(super) fn signed_out() -> Self {
        Self {
            state: OfflineState::SignedOut,
            reachable: true,
            downloads: Vec::new(),
            local_drafts: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Index {
    downloads: Vec<(DownloadView, u64, u64, String)>,
    drafts: Vec<LocalDraftView>,
    /// unit id -> course id
    units: Vec<(String, String)>,
}

/// An admitted offline store operation (see [`OfflineGen::enter`]).
pub(super) struct Writer(Arc<Inner>);

impl Drop for Writer {
    fn drop(&mut self) {
        if let Ok(mut state) = lock(&self.0) {
            state.offline_writers = state.offline_writers.saturating_sub(1);
        }
    }
}

/// One signed-in generation's offline work.
pub(super) struct OfflineGen {
    account: String,
    root: Option<PathBuf>,
    keychain: Arc<dyn CredentialStore>,
    store: Mutex<Option<Arc<OfflineStore>>>,
    state: Mutex<OfflineState>,
    index: Mutex<Index>,
    reachable: AtomicBool,
    notify: Mutex<Option<Weak<Inner>>>,
    /// Serializes opening (the instance lock admits one open per account).
    opening: Mutex<()>,
    /// Serializes index rebuilds, so an older snapshot is never published
    /// after a newer one.
    rebuilding: Mutex<()>,
    /// The last observation stamp handed out (see [`OfflineGen::observe`]).
    observed: std::sync::atomic::AtomicU64,
    /// Whether opening has finished (so persisted stamps are known), and its
    /// signal.
    seeded: Mutex<bool>,
    seeded_signal: std::sync::Condvar,
}

impl std::fmt::Debug for OfflineGen {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OfflineGen")
    }
}

/// The learner-namespace Keychain record holding an account's offline key.
/// Derived from the account, so no persisted reference is needed; reset
/// removes it with the namespace.
pub(super) fn key_record(account: &str) -> RecordId {
    let digest = crate::offline::digest(&format!("academy-offline-key:{account}"));
    RecordId::parse(&format!("cr-{}", &digest[..16])).expect("16 hex digits")
}

impl OfflineGen {
    pub(super) fn new(
        account: String,
        root: Option<PathBuf>,
        keychain: Arc<dyn CredentialStore>,
        inner: Weak<Inner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            account,
            root,
            keychain,
            store: Mutex::new(None),
            state: Mutex::new(OfflineState::Opening),
            index: Mutex::new(Index::default()),
            reachable: AtomicBool::new(true),
            notify: Mutex::new(Some(inner)),
            opening: Mutex::new(()),
            rebuilding: Mutex::new(()),
            observed: std::sync::atomic::AtomicU64::new(0),
            seeded: Mutex::new(false),
            seeded_signal: std::sync::Condvar::new(),
        })
    }

    /// Records that opening finished (whatever its outcome): stamps handed
    /// out from now on follow every persisted one.
    fn set_seeded(&self) {
        if let Ok(mut seeded) = self.seeded.lock() {
            *seeded = true;
        }
        self.seeded_signal.notify_all();
    }

    /// An observation stamp for a read whose result may be kept: taken only
    /// once opening finished, so it follows every persisted stamp (blocking;
    /// gives up after a minute, when the result will not be kept over newer
    /// state anyway).
    pub(super) fn observe_seeded(&self) -> u64 {
        if let Ok(guard) = self.seeded.lock() {
            let _ =
                self.seeded_signal
                    .wait_timeout_while(guard, Duration::from_secs(60), |seeded| !*seeded);
        }
        self.observe()
    }

    /// A strictly increasing stamp taken when a read of Academy starts (a
    /// foreground attempt read, a download, a revalidation). A kept record
    /// is replaced only by a result observed after it, so a reply that was
    /// asked earlier and answers later never undoes newer state. Stamps are
    /// wall-clock nanoseconds but never below the last one handed out or
    /// persisted (opening continues after every kept stamp), so ordering does
    /// not depend on the clock: a clock set back still yields later stamps.
    pub(super) fn observe(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        let mut last = self.observed.load(Ordering::SeqCst);
        loop {
            let next = now.max(last + 1);
            match self
                .observed
                .compare_exchange(last, next, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return next,
                Err(seen) => last = seen,
            }
        }
    }

    /// Admits one store operation of this generation: none once it is
    /// retired (sign-out, account change, a new generation) or while local
    /// data is being reset. Reset waits for every admitted operation to
    /// drop its [`Writer`] before it purges keys and files.
    pub(super) fn enter(&self) -> Option<Writer> {
        let inner = self.notify.lock().ok()?.as_ref()?.upgrade()?;
        let mut state = lock(&inner).ok()?;
        let current = state
            .offline
            .as_ref()
            .is_some_and(|o| std::ptr::eq(Arc::as_ptr(o), self));
        if !current || state.storage == StorageHealth::Resetting {
            return None;
        }
        state.offline_writers += 1;
        drop(state);
        Some(Writer(inner))
    }

    pub(super) fn account(&self) -> &str {
        &self.account
    }

    pub(super) fn set_notifier(&self, inner: Weak<Inner>) {
        if let Ok(mut n) = self.notify.lock() {
            *n = Some(inner);
        }
    }

    /// Republishes the snapshot (offline state changed off the state lock).
    fn notify(&self) {
        let inner = self
            .notify
            .lock()
            .ok()
            .and_then(|n| n.as_ref().and_then(Weak::upgrade));
        if let Some(inner) = inner
            && let Ok(mut state) = lock(&inner)
        {
            state.changed();
        }
    }

    fn set_state(&self, state: OfflineState) {
        if let Ok(mut s) = self.state.lock() {
            *s = state;
        }
    }

    pub(super) fn store(&self) -> Option<Arc<OfflineStore>> {
        self.store.lock().ok().and_then(|s| s.clone())
    }

    /// Records a learner call's reachability; true when Academy just became
    /// reachable again (revalidation is due).
    pub(super) fn mark_reachable(&self, reachable: bool) -> bool {
        let was = self.reachable.swap(reachable, Ordering::SeqCst);
        if was != reachable {
            self.notify();
        }
        reachable && !was
    }

    pub(super) fn view(&self) -> OfflineView {
        let now = now_secs();
        let index = self.index.lock();
        let (downloads, local_drafts) = match &index {
            Ok(index) => (
                index
                    .downloads
                    .iter()
                    .map(|(view, downloaded, expires, _)| {
                        let mut view = view.clone();
                        view.status = effective_status(view.status, *downloaded, *expires, now);
                        view
                    })
                    .collect(),
                index.drafts.clone(),
            ),
            Err(_) => (Vec::new(), Vec::new()),
        };
        OfflineView {
            state: self.state.lock().map_or(OfflineState::Unavailable, |s| *s),
            reachable: self.reachable.load(Ordering::SeqCst),
            downloads,
            local_drafts,
        }
    }

    /// Opens the account's store if its key already exists (blocking).
    /// Nothing is created: an account that never kept work offline has no
    /// key and no store.
    pub(super) fn open_existing(&self) -> OfflineState {
        let Some(_writer) = self.enter() else {
            return OfflineState::Unavailable;
        };
        let _opening = self.opening.lock();
        if self.store().is_some() {
            return OfflineState::Ready;
        }
        let state = match self.keychain.get(&key_record(&self.account)) {
            Err(VaultError::NotFound) => OfflineState::Empty,
            Err(_) => OfflineState::Unavailable,
            Ok(key) => self.open_with(&key),
        };
        self.set_state(state);
        self.set_seeded();
        state
    }

    /// Opens the store, creating the account's key on first use (blocking).
    pub(super) fn ensure_open(&self) -> Result<(Arc<OfflineStore>, Writer), CallFailure> {
        let writer = self.enter().ok_or(CallFailure::OfflineUnavailable)?;
        let _opening = self.opening.lock();
        if let Some(store) = self.store() {
            return Ok((store, writer));
        }
        let record = key_record(&self.account);
        let key = match self.keychain.get(&record) {
            Ok(key) => key,
            Err(VaultError::NotFound) => {
                let key = crate::offline::new_key().ok_or(CallFailure::OfflineUnavailable)?;
                self.keychain
                    .put(&record, &key)
                    .map_err(|_| CallFailure::OfflineUnavailable)?;
                key
            }
            Err(_) => {
                self.set_state(OfflineState::Unavailable);
                return Err(CallFailure::OfflineUnavailable);
            }
        };
        let state = self.open_with(&key);
        self.set_state(state);
        self.set_seeded();
        let store = self.store().ok_or(CallFailure::OfflineUnavailable)?;
        Ok((store, writer))
    }

    fn open_with(&self, key: &[u8]) -> OfflineState {
        let Some(root) = &self.root else {
            return OfflineState::Unavailable;
        };
        match OfflineStore::open(root, &self.account, key) {
            Ok(store) => {
                let store = Arc::new(store);
                self.rebuild(&store);
                if let Ok(mut s) = self.store.lock() {
                    *s = Some(store);
                }
                OfflineState::Ready
            }
            Err(_) => OfflineState::Unavailable,
        }
    }

    /// Re-reads what is kept (decrypting each record) into the index.
    fn rebuild(&self, store: &OfflineStore) {
        let _rebuilding = self.rebuilding.lock();
        // Stamps continue after every persisted one, so a clock set back
        // after a restart never makes newer results lose to older ones.
        let mut persisted = 0u64;
        for kind in [RecordKind::Attempt, RecordKind::AttemptList] {
            for (_, record) in store.list(kind).unwrap_or_default() {
                persisted = persisted.max(stamp(&record));
            }
        }
        let mut index = Index::default();
        for (course_id, record) in store.list(RecordKind::Course).unwrap_or_default() {
            let units: Vec<String> = record["bundle"]["units"]
                .as_array()
                .map(|u| {
                    u.iter()
                        .filter_map(|u| u["id"].as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            for unit in &units {
                index.units.push((unit.clone(), course_id.clone()));
            }
            persisted = persisted
                .max(stamp(&record))
                .max(record["validated"].as_u64().unwrap_or(0));
            let downloaded = record["downloadedAt"].as_u64().unwrap_or(0);
            let expires = record["leaseExpiresAt"].as_u64().unwrap_or(0);
            let status = match record["status"].as_str() {
                Some("outdated") => DownloadStatus::Outdated,
                Some("withdrawn") => DownloadStatus::Withdrawn,
                _ => DownloadStatus::Current,
            };
            index.downloads.push((
                DownloadView {
                    course_id: course_id.clone(),
                    title: record["bundle"]["title"]
                        .as_str()
                        .unwrap_or("Course")
                        .to_owned(),
                    revision: record["bundle"]["revision"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned(),
                    downloaded_at: iso(downloaded),
                    lease_expires_at: iso(expires),
                    status,
                    units: u32::try_from(units.len()).unwrap_or(u32::MAX),
                    first_unit: units.first().filter(|u| plain_id(u)).cloned(),
                },
                downloaded,
                expires,
                course_id,
            ));
        }
        for (attempt_id, record) in store.list(RecordKind::Draft).unwrap_or_default() {
            index.drafts.push(LocalDraftView {
                attempt_id,
                base_version: record["baseVersion"].as_u64().map_or(0, |v| v as u32),
                saved_at: iso(record["savedAt"].as_u64().unwrap_or(0)),
            });
        }
        if let Ok(mut i) = self.index.lock() {
            *i = index;
        }
        self.observed.fetch_max(persisted, Ordering::SeqCst);
    }

    pub(super) fn refresh(&self) {
        if let Some(store) = self.store()
            && let Some(_writer) = self.enter()
        {
            self.rebuild(&store);
        }
        self.notify();
    }

    /// Whether the kept record of `course_id` says Academy withdrew it
    /// (read from the record itself, not the index).
    fn withdrawn_course(store: &OfflineStore, course_id: &str) -> bool {
        matches!(store.get(RecordKind::Course, course_id), Ok(Some(r)) if r["status"] == "withdrawn")
    }

    /// A cached attempt as it may be shown offline: Academy's own reply
    /// while its lease holds (clock not behind when it was kept) and its
    /// course is not withdrawn; otherwise locked, keeping only the
    /// learner's own work (no authored definition).
    fn offline_attempt(store: &OfflineStore, record: &Value, now: u64) -> Value {
        let attempt = &record["attempt"];
        if attempt["locked"] == true {
            return attempt.clone();
        }
        let cached = record["cachedAt"].as_u64().unwrap_or(u64::MAX);
        let lease = attempt["lease"]["expiresAt"].as_str().and_then(parse_iso);
        let reason = if attempt["courseId"]
            .as_str()
            .is_some_and(|c| Self::withdrawn_course(store, c))
        {
            Some("access_revoked")
        } else if !lease.is_some_and(|expires| now < expires && now >= cached) {
            Some("lease_expired")
        } else {
            None
        };
        match reason {
            None => attempt.clone(),
            Some(reason) => locked_attempt(attempt, reason),
        }
    }

    /// A kept course record as it may be read offline now: current or
    /// outdated, lease not ended, clock not behind its download (decided
    /// from the record itself, so a verdict written to it applies at once).
    fn readable_record(store: &OfflineStore, course_id: &str) -> Option<Value> {
        let record = store.get(RecordKind::Course, course_id).ok()??;
        let status = match record["status"].as_str() {
            Some("outdated") => DownloadStatus::Outdated,
            Some("current") => DownloadStatus::Current,
            _ => return None,
        };
        let downloaded = record["downloadedAt"].as_u64()?;
        let expires = record["leaseExpiresAt"].as_u64()?;
        matches!(
            effective_status(status, downloaded, expires, now_secs()),
            DownloadStatus::Current | DownloadStatus::Outdated
        )
        .then_some(record)
    }

    fn course_of_unit(&self, unit: &str) -> Option<String> {
        self.index
            .lock()
            .ok()?
            .units
            .iter()
            .find(|(u, _)| u == unit)
            .map(|(_, c)| c.clone())
    }

    /// An offline answer (`{json, …}` text) for a read Academy could not be
    /// reached for, from this account's downloads and cached attempts only
    /// (blocking: decrypts a record).
    pub(super) fn answer(&self, procedure: &str, input: Option<&str>) -> Option<String> {
        let _writer = self.enter()?;
        let store = self.store()?;
        let input: Value = input
            .and_then(|i| serde_json::from_str::<Value>(i).ok())
            .map(|v| v["json"].clone())
            .unwrap_or(Value::Null);
        let data = match procedure {
            "units.get" => {
                let unit = input["unitId"].as_str()?;
                let course = self.course_of_unit(unit)?;
                let record = Self::readable_record(&store, &course)?;
                record["bundle"]["units"]
                    .as_array()?
                    .iter()
                    .find(|u| u["id"] == unit)?
                    .clone()
            }
            "labs.get" => {
                let course = input["courseId"].as_str()?;
                let order = input["order"].as_u64()?;
                let record = Self::readable_record(&store, course)?;
                let bundle = &record["bundle"];
                let lab_id = bundle["units"].as_array()?.iter().find_map(|u| {
                    (u["placement"]["courseId"] == course
                        && u["placement"]["order"].as_u64() == Some(order))
                    .then(|| u["labId"].as_str().map(str::to_owned))
                    .flatten()
                })?;
                let lab = bundle["labs"]
                    .as_array()?
                    .iter()
                    .find(|l| l["id"] == lab_id.as_str())?;
                json!({ "lab": lab, "progress": {} })
            }
            "desktop.attempt.get" => {
                let attempt = input["attemptId"].as_str()?;
                let record = store.get(RecordKind::Attempt, attempt).ok()??;
                Self::offline_attempt(&store, &record, now_secs())
            }
            "desktop.attempt.list" => {
                store.get(RecordKind::AttemptList, "list").ok()??["list"].clone()
            }
            _ => return None,
        };
        Some(json!({ "json": data }).to_string())
    }

    /// Keeps an attempt read (observed at `observed`) for offline use, unless
    /// what is kept was observed later (blocking). A kept draft newer than
    /// the reply's is kept.
    pub(super) fn cache(&self, procedure: &str, input: Option<&str>, data: &str, observed: u64) {
        let Some(_writer) = self.enter() else { return };
        let Some(store) = self.store() else { return };
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };
        let json = value["json"].clone();
        match procedure {
            "desktop.attempt.get" => {
                let id = input
                    .and_then(|i| serde_json::from_str::<Value>(i).ok())
                    .and_then(|v| v["json"]["attemptId"].as_str().map(str::to_owned));
                if let Some(id) = id
                    && json["attemptId"] == id.as_str()
                    && json["accountId"] == self.account.as_str()
                {
                    let _ = store.update(RecordKind::Attempt, &id, |current| {
                        if current.as_ref().is_some_and(|c| stamp(c) >= observed) {
                            return None;
                        }
                        let attempt = keep_newer_draft(json, current.as_ref().map(|c| &c["attempt"]));
                        Some(json!({ "attempt": attempt, "cachedAt": now_secs(), "observed": observed }))
                    });
                }
            }
            "desktop.attempt.list" if json["accountId"] == self.account.as_str() => {
                let _ = store.update(RecordKind::AttemptList, "list", |current| {
                    if current.as_ref().is_some_and(|c| stamp(c) >= observed) {
                        return None;
                    }
                    Some(json!({ "list": json, "observed": observed }))
                });
            }
            _ => {}
        }
    }

    /// Records a draft Academy confirmed (`desktop.draft.save` or
    /// `desktop.draft.resolve` answered `saved`) on the kept attempt, so the
    /// confirmed work opens offline even if the attempt is not read again
    /// (blocking). Only the draft and its version move, and only forward:
    /// the lease and the authored definition are left as Academy gave them.
    pub(super) fn confirm_draft(&self, procedure: &str, input: Option<&str>, data: &str) {
        let Some(_writer) = self.enter() else { return };
        let Some(store) = self.store() else { return };
        // A failure here is safe: the copy kept on this Mac is forgotten
        // only once the kept attempt holds the confirmed work (`clear_draft`).
        let reply = serde_json::from_str::<Value>(data)
            .map(|v| v["json"].clone())
            .unwrap_or_default();
        let id = input
            .and_then(|i| serde_json::from_str::<Value>(i).ok())
            .and_then(|v| v["json"]["attemptId"].as_str().map(str::to_owned));
        let (Some(id), Some(version)) = (id, reply["draftVersion"].as_u64()) else {
            return;
        };
        if reply["status"] != "saved" || !reply["draft"].is_object() {
            return;
        }
        let resolved = procedure == "desktop.draft.resolve";
        let _ = store.update(RecordKind::Attempt, &id, |current| {
            let mut record = current?;
            let attempt = &mut record["attempt"];
            if attempt["draftVersion"]
                .as_u64()
                .is_some_and(|v| v >= version)
            {
                return None;
            }
            if attempt["draft"] != reply["draft"] {
                attempt["result"] = stale_results(attempt["result"].take());
            }
            attempt["draft"] = reply["draft"].clone();
            attempt["draftVersion"] = json!(version);
            if resolved {
                attempt["conflicts"] = json!([]);
            }
            Some(record)
        });
    }
}

/// When a kept record was observed (0 for records from before stamps).
fn stamp(record: &Value) -> u64 {
    record["observed"].as_u64().unwrap_or(0)
}

/// Academy's attempt reply, keeping newer learner work already kept (a
/// confirmed save or resolve the reply predates never goes back): the draft,
/// its version, the results projected for it and the conflict state move
/// together.
fn keep_newer_draft(mut incoming: Value, current: Option<&Value>) -> Value {
    if let Some(current) = current
        && current["draftVersion"].as_u64() > incoming["draftVersion"].as_u64()
    {
        for key in ["draft", "draftVersion", "result", "conflicts"] {
            incoming[key] = current[key].clone();
        }
    }
    incoming
}

/// Academy's results with every graded step marked stale, as Academy marks
/// a step whose draft changed after grading (`visibleResults`): a verdict
/// for earlier work is never shown as checking newer work.
fn stale_results(mut result: Value) -> Value {
    if let Some(steps) = result.get_mut("steps").and_then(Value::as_object_mut) {
        for entry in steps.values_mut() {
            if entry.is_object() {
                entry["stale"] = json!(true);
                entry["verdict"] = json!({
                    "pass": false,
                    "feedback": "This draft changed after grading. Check this step again.",
                });
            }
        }
    }
    result
}

/// An attempt locked for `reason`: Academy's identifying fields and the
/// learner's own work, without the authored definition or lease.
fn locked_attempt(attempt: &Value, reason: &str) -> Value {
    let mut out = serde_json::Map::new();
    for key in [
        "attemptId",
        "accountId",
        "labId",
        "courseId",
        "courseTitle",
        "revision",
        "status",
        "draftVersion",
        "draft",
        "result",
        "conflicts",
        "createdAt",
    ] {
        out.insert(key.to_owned(), attempt[key].clone());
    }
    out.insert("locked".to_owned(), json!(true));
    out.insert("lockReason".to_owned(), json!(reason));
    Value::Object(out)
}

/// A download's status at `now`: a lease past its expiry, or a clock set
/// before the download, is expired until Academy renews it.
fn effective_status(
    stored: DownloadStatus,
    downloaded: u64,
    expires: u64,
    now: u64,
) -> DownloadStatus {
    match stored {
        DownloadStatus::Withdrawn => DownloadStatus::Withdrawn,
        _ if now >= expires || now < downloaded => DownloadStatus::Expired,
        other => other,
    }
}

/// One account request outside the operations runtime (revalidation).
async fn request(
    session: &Arc<LearnerSession>,
    client: &AcademyClient,
    procedure: &str,
    input: &str,
) -> Result<CallReply, CallFailure> {
    let mut force = false;
    loop {
        let token = session.access_token(force).await?;
        let reply = client
            .call(
                procedure,
                ProcedureKind::Query,
                Some(input),
                Some(token.as_str()),
                REQUEST_TIMEOUT,
            )
            .await;
        if !force && matches!(reply, Ok(CallReply::Error { status: 401, .. })) {
            force = true;
            continue;
        }
        return reply;
    }
}

fn data_json(reply: &CallReply) -> Option<Value> {
    match reply {
        CallReply::Data(data) => serde_json::from_str::<Value>(data)
            .ok()
            .map(|v| v["json"].clone()),
        CallReply::Error { .. } => None,
    }
}

/// Re-checks every download with Academy: access (renewing the lease or
/// withdrawing the course) and revision (marking it outdated). Changes no
/// Academy state and replays nothing.
pub(super) async fn revalidate(generation: Arc<OfflineGen>, session: Arc<LearnerSession>) {
    let client = session.client().clone();
    let Some(store) = generation.store() else {
        return;
    };
    let checked = revalidate_courses(&generation, &session, &client, &store).await;
    // A withdrawal is published before the attempts are read again; when
    // Academy's access check could not be made, attempts wait for the next
    // revalidation too.
    generation.refresh();
    if checked {
        revalidate_attempts(&generation, &session, &client, &store).await;
        generation.refresh();
    }
}

/// Renews each download's lease, marks a course Academy no longer gives the
/// account withdrawn and a changed revision outdated; false when Academy's
/// access check could not be made. Each record is changed
/// only if it is still the download that was checked (a download or removal
/// meanwhile wins).
async fn revalidate_courses(
    generation: &Arc<OfflineGen>,
    session: &Arc<LearnerSession>,
    client: &AcademyClient,
    store: &Arc<OfflineStore>,
) -> bool {
    let kept: Vec<(String, String)> = generation
        .index
        .lock()
        .map(|i| {
            i.downloads
                .iter()
                .map(|d| (d.3.clone(), d.0.revision.clone()))
                .collect()
        })
        .unwrap_or_default();
    if kept.is_empty() {
        return true;
    }
    let ids: Vec<&str> = kept.iter().map(|(id, _)| id.as_str()).collect();
    let observed = generation.observe();
    let input = json!({ "json": { "courseIds": ids } }).to_string();
    let Ok(reply) = request(session, client, "desktop.access", &input).await else {
        return false;
    };
    let Some(access) = data_json(&reply) else {
        return false;
    };
    if access["accountId"] != generation.account() {
        return false;
    }
    for course in access["courses"].as_array().cloned().unwrap_or_default() {
        let Some(id) = course["courseId"].as_str() else {
            continue;
        };
        let Some((_, revision)) = kept.iter().find(|(k, _)| k == id) else {
            continue;
        };
        let allowed = course["allowed"] == true;
        let mut lease = None;
        let mut status = if allowed { None } else { Some("withdrawn") };
        if allowed {
            lease = course["expiresAt"].as_str().and_then(parse_iso);
            let manifest = json!({ "json": { "courseId": id } }).to_string();
            if let Ok(reply) = request(session, client, "desktop.manifest", &manifest).await
                && let Some(manifest) = data_json(&reply)
            {
                status = Some(if manifest["revision"] == revision.as_str() {
                    "current"
                } else {
                    "outdated"
                });
            }
        }
        let (store, id, revision) = (Arc::clone(store), id.to_owned(), revision.clone());
        let writer = Arc::clone(generation);
        let _ = tokio::task::spawn_blocking(move || {
            let Some(_writer) = writer.enter() else {
                return;
            };
            let _ = store.update(RecordKind::Course, &id, |current| {
                let mut record = current?;
                // Only the download that was checked, and only a verdict
                // newer than any it already has.
                if record["bundle"]["revision"] != revision.as_str()
                    || stamp(&record) >= observed
                    || record["validated"].as_u64().unwrap_or(0) >= observed
                {
                    return None;
                }
                if let Some(expires) = lease {
                    record["leaseExpiresAt"] = json!(expires);
                }
                if let Some(status) = status {
                    record["status"] = json!(status);
                }
                record["validatedAt"] = json!(now_secs());
                record["validated"] = json!(observed);
                Some(record)
            });
        })
        .await;
    }
    true
}

/// Reads each cached attempt again (a read, never a write to Academy), so
/// its lease is renewed or Academy's lock (access revoked, revision
/// withdrawn) replaces the authored definition kept for it. An attempt
/// Academy no longer returns for the account is kept locked.
async fn revalidate_attempts(
    generation: &Arc<OfflineGen>,
    session: &Arc<LearnerSession>,
    client: &AcademyClient,
    store: &Arc<OfflineStore>,
) {
    let lister = (Arc::clone(generation), Arc::clone(store));
    let cached: Vec<String> = tokio::task::spawn_blocking(move || {
        let Some(_writer) = lister.0.enter() else {
            return Vec::new();
        };
        lister
            .1
            .list(RecordKind::Attempt)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, _)| id)
            .take(MAX_CACHED_ATTEMPTS)
            .collect()
    })
    .await
    .unwrap_or_default();
    for id in cached {
        let observed = generation.observe();
        let input = json!({ "json": { "attemptId": id } }).to_string();
        let Ok(reply) = request(session, client, "desktop.attempt.get", &input).await else {
            continue;
        };
        let fresh = match &reply {
            CallReply::Data(_) => data_json(&reply).filter(|a| {
                a["attemptId"] == id.as_str() && a["accountId"] == generation.account()
            }),
            CallReply::Error {
                status: 401 | 403 | 404,
                ..
            } => None,
            _ => continue,
        };
        let (store, writer) = (Arc::clone(store), Arc::clone(generation));
        let _ = tokio::task::spawn_blocking(move || {
            let Some(_writer) = writer.enter() else {
                return;
            };
            let _ = store.update(RecordKind::Attempt, &id, |current| {
                let current = current?;
                // A read or save observed after this one already won.
                if stamp(&current) >= observed {
                    return None;
                }
                let (attempt, cached) = match fresh {
                    Some(fresh) => (
                        keep_newer_draft(fresh, Some(&current["attempt"])),
                        json!(now_secs()),
                    ),
                    None => (
                        locked_attempt(&current["attempt"], "access_revoked"),
                        current["cachedAt"].clone(),
                    ),
                };
                Some(json!({ "attempt": attempt, "cachedAt": cached, "observed": observed }))
            });
        })
        .await;
    }
}

/// Downloads a course for offline reading: Academy's access check, then its
/// bundle (or, beyond Academy's bundle cap, its manifest and unit bundles),
/// kept sealed under Academy's lease.
pub(super) async fn download(
    generation: &Arc<OfflineGen>,
    session: &Arc<LearnerSession>,
    course_id: &str,
) -> Result<CallReply, CallFailure> {
    let client = session.client().clone();
    let opener = Arc::clone(generation);
    // Opened (key created on first use) before anything is fetched; the
    // write ledger is left while Academy answers and entered again to save.
    let (store, writer) = tokio::task::spawn_blocking(move || opener.ensure_open())
        .await
        .unwrap_or(Err(CallFailure::OfflineUnavailable))?;
    drop(writer);
    let observed = generation.observe();
    let access_input = json!({ "json": { "courseIds": [course_id] } }).to_string();
    let reply = request(session, &client, "desktop.access", &access_input).await?;
    let Some(access) = data_json(&reply) else {
        return Ok(reply);
    };
    let allowed = access["courses"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c["courseId"] == course_id))
        .is_some_and(|c| c["allowed"] == true);
    if access["accountId"] != generation.account() || !allowed {
        return Err(CallFailure::NotDownloadable);
    }
    let input = json!({ "json": { "courseId": course_id } }).to_string();
    let bundle = match request(session, &client, "desktop.bundle", &input).await? {
        CallReply::Error { status: 413, .. } => {
            assemble(session, &client, &input, course_id, generation.account()).await?
        }
        error @ CallReply::Error { .. } => return Ok(error),
        reply => data_json(&reply).ok_or(CallFailure::InvalidResponse)?,
    };
    let expires = bundle["lease"]["expiresAt"].as_str().and_then(parse_iso);
    let valid = bundle["schemaVersion"] == 1
        && bundle["courseId"] == course_id
        && bundle["accountId"] == generation.account()
        && bundle["revision"].is_string()
        && bundle["units"].is_array();
    let (true, Some(expires)) = (valid, expires) else {
        return Err(CallFailure::InvalidResponse);
    };
    let now = now_secs();
    let record = json!({
        "bundle": bundle,
        "downloadedAt": now,
        "validatedAt": now,
        "leaseExpiresAt": expires,
        "status": "current",
        "observed": observed,
        "validated": observed,
    });
    let id = course_id.to_owned();
    let saver = Arc::clone(generation);
    let saved = tokio::task::spawn_blocking(move || {
        let _writer = saver.enter().ok_or(OfflineError::Unavailable)?;
        // A download started later already saved its own copy: it stays.
        let mut superseded = false;
        store.update(RecordKind::Course, &id, |current| {
            // A download or verdict observed after this download's access
            // check (a newer download, a withdrawal, a renewal) stands.
            if current.as_ref().is_some_and(|c| {
                stamp(c) > observed || c["validated"].as_u64().unwrap_or(0) > observed
            }) {
                superseded = true;
                return None;
            }
            Some(record)
        })?;
        Ok(superseded)
    })
    .await
    .unwrap_or(Err(OfflineError::Unavailable));
    match saved {
        Ok(false) => {}
        // Academy's newer word on the course (a withdrawal meanwhile) stands.
        Ok(true) => {
            generation.refresh();
            return Err(CallFailure::NotDownloadable);
        }
        Err(OfflineError::TooLarge | OfflineError::Full) => return Err(CallFailure::OfflineFull),
        Err(_) => return Err(CallFailure::OfflineUnavailable),
    }
    generation.refresh();
    let units = bundle["units"].as_array().map_or(0, Vec::len);
    Ok(CallReply::Data(
        json!({ "json": { "courseId": course_id, "title": bundle["title"], "revision": bundle["revision"], "units": units } })
            .to_string(),
    ))
}

/// A course beyond Academy's bundle cap: its manifest, then each unit's
/// bundle, combined into one bundle. Fetching stops (`offlineFull`) as soon
/// as what was received could no longer fit one record.
async fn assemble(
    session: &Arc<LearnerSession>,
    client: &AcademyClient,
    course_input: &str,
    course_id: &str,
    account: &str,
) -> Result<Value, CallFailure> {
    let reply = request(session, client, "desktop.manifest", course_input).await?;
    let mut received = reply_bytes(&reply);
    let mut manifest = data_json(&reply).ok_or(CallFailure::InvalidResponse)?;
    let entries = manifest["units"]
        .as_array()
        .cloned()
        .ok_or(CallFailure::InvalidResponse)?;
    if entries.len() > MAX_UNITS {
        return Err(CallFailure::OfflineFull);
    }
    let mut units = Vec::new();
    let mut labs: Vec<Value> = Vec::new();
    for entry in entries {
        let unit_id = entry["id"].as_str().ok_or(CallFailure::InvalidResponse)?;
        let input = json!({ "json": { "unitId": unit_id } }).to_string();
        let reply = request(session, client, "desktop.unitBundle", &input).await?;
        received = received.saturating_add(reply_bytes(&reply));
        if received > crate::offline::RECORD_LIMIT as usize {
            return Err(CallFailure::OfflineFull);
        }
        let part = data_json(&reply).ok_or(CallFailure::InvalidResponse)?;
        // Each part is Academy's bundle projection of one unit, for this
        // course, account and revision.
        if part["schemaVersion"] != 1
            || part["courseId"] != course_id
            || part["accountId"] != account
            || part["revision"] != manifest["revision"]
        {
            return Err(CallFailure::InvalidResponse);
        }
        units.extend(part["units"].as_array().cloned().unwrap_or_default());
        for lab in part["labs"].as_array().cloned().unwrap_or_default() {
            if !labs.iter().any(|l| l["id"] == lab["id"]) {
                labs.push(lab);
            }
        }
    }
    manifest["units"] = Value::Array(units);
    manifest["labs"] = Value::Array(labs);
    manifest["manifest"] = json!(false);
    Ok(manifest)
}

fn reply_bytes(reply: &CallReply) -> usize {
    match reply {
        CallReply::Data(text) => text.len(),
        _ => 0,
    }
}

/// Removes a downloaded course (blocking).
pub(super) fn remove(generation: &OfflineGen, course_id: &str) -> Result<CallReply, CallFailure> {
    let _writer = generation.enter().ok_or(CallFailure::OfflineUnavailable)?;
    let store = generation.store().ok_or(CallFailure::OfflineUnavailable)?;
    store
        .delete(RecordKind::Course, course_id)
        .map_err(|_| CallFailure::OfflineUnavailable)?;
    generation.refresh();
    Ok(CallReply::Data(
        json!({ "json": { "removed": course_id } }).to_string(),
    ))
}

/// Keeps lab work on this Mac (blocking). Never sent anywhere.
pub(super) fn save_draft(
    generation: &OfflineGen,
    attempt_id: &str,
    base_version: u32,
    draft: &str,
) -> Result<CallReply, CallFailure> {
    if draft.len() > DRAFT_LIMIT {
        return Err(CallFailure::OfflineFull);
    }
    let draft: Value = serde_json::from_str(draft).map_err(|_| CallFailure::InvalidResponse)?;
    if !draft.is_object() {
        return Err(CallFailure::InvalidResponse);
    }
    let (store, _writer) = generation.ensure_open()?;
    let saved_at = now_secs();
    store
        .put(
            RecordKind::Draft,
            attempt_id,
            &json!({ "attemptId": attempt_id, "baseVersion": base_version, "draft": draft, "savedAt": saved_at }),
        )
        .map_err(|e| match e {
            OfflineError::TooLarge | OfflineError::Full => CallFailure::OfflineFull,
            _ => CallFailure::OfflineUnavailable,
        })?;
    generation.refresh();
    Ok(CallReply::Data(
        json!({ "json": { "attemptId": attempt_id, "baseVersion": base_version, "savedAt": iso(saved_at) } }).to_string(),
    ))
}

/// The lab work kept on this Mac for an attempt, or null (blocking).
pub(super) fn load_draft(
    generation: &OfflineGen,
    attempt_id: &str,
    hidden: bool,
) -> Result<CallReply, CallFailure> {
    if hidden {
        return Ok(CallReply::Data(json!({ "json": null }).to_string()));
    }
    let _writer = generation.enter().ok_or(CallFailure::OfflineUnavailable)?;
    // Joins an opening in progress: an account whose kept work is still
    // opening is never answered as having none.
    let store = match generation.store() {
        Some(store) => store,
        None => match generation.open_existing() {
            OfflineState::Ready => generation.store().ok_or(CallFailure::OfflineUnavailable)?,
            OfflineState::Empty => {
                return Ok(CallReply::Data(json!({ "json": null }).to_string()));
            }
            _ => return Err(CallFailure::OfflineUnavailable),
        },
    };
    let record = store
        .get(RecordKind::Draft, attempt_id)
        .map_err(|_| CallFailure::OfflineUnavailable)?;
    let value = record.map_or(Value::Null, |r| {
        json!({ "attemptId": attempt_id, "baseVersion": r["baseVersion"], "draft": r["draft"], "savedAt": iso(r["savedAt"].as_u64().unwrap_or(0)) })
    });
    Ok(CallReply::Data(json!({ "json": value }).to_string()))
}

/// Forgets lab work kept on this Mac once the kept attempt holds it
/// (blocking).
pub(super) fn clear_draft(
    generation: &OfflineGen,
    attempt_id: &str,
) -> Result<CallReply, CallFailure> {
    let _writer = generation.enter().ok_or(CallFailure::OfflineUnavailable)?;
    if let Some(store) = generation.store() {
        let local = store
            .get(RecordKind::Draft, attempt_id)
            .map_err(|_| CallFailure::OfflineUnavailable)?;
        if let Some(local) = local {
            // The only offline copy of this work goes only once the kept
            // attempt holds it (or newer work Academy confirmed after it);
            // otherwise it stays and the page asks again later.
            let held = store
                .get(RecordKind::Attempt, attempt_id)
                .ok()
                .flatten()
                .is_some_and(|record| {
                    let attempt = &record["attempt"];
                    attempt["draft"] == local["draft"]
                        || attempt["draftVersion"].as_u64() > local["baseVersion"].as_u64()
                });
            // ...and only once that attempt's commit is known durable.
            if !held || !store.durable(RecordKind::Attempt, attempt_id) {
                return Err(CallFailure::OfflineUnavailable);
            }
            store
                .delete(RecordKind::Draft, attempt_id)
                .map_err(|_| CallFailure::OfflineUnavailable)?;
            generation.refresh();
        }
    }
    Ok(CallReply::Data(
        json!({ "json": { "cleared": attempt_id } }).to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times_round_trip() {
        for secs in [0, 951_782_400, 1_792_281_600, 4_102_444_799] {
            assert_eq!(parse_iso(&iso(secs)), Some(secs), "{}", iso(secs));
        }
        assert_eq!(parse_iso("2026-10-25T12:00:00Z"), Some(1_792_929_600));
        assert_eq!(parse_iso("2026-10-25T12:00:00.123Z"), Some(1_792_929_600));
        for bad in [
            "2026-10-25 12:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-10-25T12:00:00+01:00",
            "soon",
        ] {
            assert_eq!(parse_iso(bad), None, "{bad}");
        }
    }

    #[test]
    fn expired_or_rolled_back_leases_are_not_readable() {
        assert_eq!(
            effective_status(DownloadStatus::Current, 100, 200, 150),
            DownloadStatus::Current
        );
        assert_eq!(
            effective_status(DownloadStatus::Outdated, 100, 200, 150),
            DownloadStatus::Outdated
        );
        assert_eq!(
            effective_status(DownloadStatus::Current, 100, 200, 200),
            DownloadStatus::Expired
        );
        assert_eq!(
            effective_status(DownloadStatus::Current, 100, 200, 50),
            DownloadStatus::Expired
        );
        assert_eq!(
            effective_status(DownloadStatus::Withdrawn, 100, 200, 150),
            DownloadStatus::Withdrawn
        );
    }
}
