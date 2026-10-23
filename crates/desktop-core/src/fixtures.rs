//! Deterministic adapters for tests and the fixture build. Never compiled
//! into shipping builds.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};

use futures_util::future::BoxFuture;
use zeroize::Zeroizing;

use crate::authority::{Adapters, CredentialStore, ImportFile, ImportPicker};
use crate::import::ImportError;
use crate::store::{AppStore, MemoryStore, PersistedState, StoreError};
use crate::vault::{RecordId, Vault, VaultError};

/// What a [`ScriptedVault`] was asked to do (no secret values).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VaultCall {
    Put(RecordId),
    Get(RecordId),
    Delete(RecordId),
    Purge,
}

#[derive(Default)]
struct VaultState {
    records: HashMap<RecordId, Zeroizing<Vec<u8>>>,
    failure: Option<VaultError>,
    /// Purge deletes this many records, then fails with this error.
    partial_purge: Option<(usize, VaultError)>,
    calls: Vec<VaultCall>,
    held: bool,
    held_reads: bool,
    waiting: usize,
}

/// A Keychain stand-in with scriptable failures and a write gate.
#[derive(Default)]
pub struct ScriptedVault {
    state: Mutex<VaultState>,
    gate: Condvar,
}

impl ScriptedVault {
    /// Every following call fails with `failure` until cleared.
    pub fn fail_with(&self, failure: Option<VaultError>) {
        if let Ok(mut s) = self.state.lock() {
            s.failure = failure;
        }
    }

    /// Purges delete `deleted` records and then fail, until cleared (`None`).
    pub fn fail_purge_after(&self, partial: Option<(usize, VaultError)>) {
        if let Ok(mut s) = self.state.lock() {
            s.partial_purge = partial;
        }
    }

    /// Writes block until [`ScriptedVault::release`].
    pub fn hold_writes(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.held = true;
        }
    }

    /// Reads block until [`ScriptedVault::release`] (a scan stays claimed
    /// before anything is sent).
    pub fn hold_reads(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.held_reads = true;
        }
    }

    pub fn release(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.held = false;
            s.held_reads = false;
        }
        self.gate.notify_all();
    }

    /// Writes currently waiting at the gate.
    pub fn waiting(&self) -> usize {
        self.state.lock().map(|s| s.waiting).unwrap_or(0)
    }

    pub fn calls(&self) -> Vec<VaultCall> {
        self.state
            .lock()
            .map(|s| s.calls.clone())
            .unwrap_or_default()
    }

    pub fn records(&self) -> Vec<RecordId> {
        let mut ids: Vec<RecordId> = self
            .state
            .lock()
            .map(|s| s.records.keys().cloned().collect())
            .unwrap_or_default();
        ids.sort();
        ids
    }

    pub fn secret(&self, record: &RecordId) -> Option<Vec<u8>> {
        self.state
            .lock()
            .ok()?
            .records
            .get(record)
            .map(|s| s.to_vec())
    }

    /// Places a record directly, as if left by an earlier run.
    pub fn plant(&self, record: &RecordId, secret: &[u8]) {
        if let Ok(mut s) = self.state.lock() {
            s.records
                .insert(record.clone(), Zeroizing::new(secret.to_vec()));
        }
    }
}

impl Vault for ScriptedVault {
    fn put(&self, record: &RecordId, secret: &[u8]) -> Result<(), VaultError> {
        let mut s = self.state.lock().map_err(|_| VaultError::Unavailable)?;
        s.calls.push(VaultCall::Put(record.clone()));
        s.waiting += 1;
        while s.held {
            s = self.gate.wait(s).map_err(|_| VaultError::Unavailable)?;
        }
        s.waiting -= 1;
        if let Some(failure) = s.failure {
            return Err(failure);
        }
        s.records
            .insert(record.clone(), Zeroizing::new(secret.to_vec()));
        Ok(())
    }

    fn get(&self, record: &RecordId) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        let mut s = self.state.lock().map_err(|_| VaultError::Unavailable)?;
        s.calls.push(VaultCall::Get(record.clone()));
        s.waiting += 1;
        while s.held_reads {
            s = self.gate.wait(s).map_err(|_| VaultError::Unavailable)?;
        }
        s.waiting -= 1;
        if let Some(failure) = s.failure {
            return Err(failure);
        }
        s.records
            .get(record)
            .map(|v| Zeroizing::new(v.to_vec()))
            .ok_or(VaultError::NotFound)
    }

    fn delete(&self, record: &RecordId) -> Result<(), VaultError> {
        let mut s = self.state.lock().map_err(|_| VaultError::Unavailable)?;
        s.calls.push(VaultCall::Delete(record.clone()));
        if let Some(failure) = s.failure {
            return Err(failure);
        }
        s.records
            .remove(record)
            .map(drop)
            .ok_or(VaultError::NotFound)
    }
}

impl CredentialStore for ScriptedVault {
    fn purge(&self) -> Result<(), VaultError> {
        let mut s = self.state.lock().map_err(|_| VaultError::Unavailable)?;
        s.calls.push(VaultCall::Purge);
        if let Some(failure) = s.failure {
            return Err(failure);
        }
        if let Some((deleted, failure)) = s.partial_purge {
            let mut ids: Vec<RecordId> = s.records.keys().cloned().collect();
            ids.sort();
            for id in ids.into_iter().take(deleted) {
                s.records.remove(&id);
            }
            return Err(failure);
        }
        s.records.clear();
        Ok(())
    }
}

/// An in-memory store whose saves and resets can be made to fail.
#[derive(Default)]
pub struct ScriptedStore {
    inner: MemoryStore,
    failing: Mutex<bool>,
    /// The next this-many saves are applied (visible) but report an
    /// unconfirmed directory sync; `usize::MAX` means all of them.
    unconfirmed: Mutex<usize>,
    /// The last document whose durability was confirmed: what a crash
    /// would leave on disk.
    durable: Mutex<Option<PersistedState>>,
    failing_resets: Mutex<bool>,
    load_error: Mutex<Option<StoreError>>,
    saves: Mutex<usize>,
}

impl ScriptedStore {
    pub fn with_state(state: PersistedState) -> Self {
        let store = Self::default();
        let _ = store.inner.save(&state);
        if let Ok(mut d) = store.durable.lock() {
            *d = Some(state);
        }
        store
    }

    /// The last confirmed-durable document (what survives a crash).
    pub fn durable(&self) -> Option<PersistedState> {
        self.durable.lock().ok().and_then(|d| d.clone())
    }

    pub fn corrupt() -> Self {
        let store = Self::default();
        if let Ok(mut e) = store.load_error.lock() {
            *e = Some(StoreError::Corrupt);
        }
        store
    }

    pub fn unavailable() -> Self {
        let store = Self::default();
        if let Ok(mut e) = store.load_error.lock() {
            *e = Some(StoreError::Unavailable);
        }
        store
    }

    pub fn unconfirmed_saves(&self, unconfirmed: bool) {
        self.unconfirm_next(if unconfirmed { usize::MAX } else { 0 });
    }

    /// The next `count` saves are visible but unconfirmed.
    pub fn unconfirm_next(&self, count: usize) {
        if let Ok(mut u) = self.unconfirmed.lock() {
            *u = count;
        }
    }

    pub fn fail_resets(&self, failing: bool) {
        if let Ok(mut f) = self.failing_resets.lock() {
            *f = failing;
        }
    }

    pub fn fail_saves(&self, failing: bool) {
        if let Ok(mut f) = self.failing.lock() {
            *f = failing;
        }
    }

    pub fn saved(&self) -> Option<PersistedState> {
        self.inner.load().ok().flatten()
    }

    pub fn saves(&self) -> usize {
        self.saves.lock().map(|s| *s).unwrap_or(0)
    }
}

impl AppStore for ScriptedStore {
    fn load(&self) -> Result<Option<PersistedState>, StoreError> {
        if let Some(e) = self
            .load_error
            .lock()
            .map_err(|_| StoreError::Unavailable)?
            .as_ref()
        {
            return Err(*e);
        }
        self.inner.load()
    }

    fn save(&self, state: &PersistedState) -> Result<(), StoreError> {
        if *self.failing.lock().map_err(|_| StoreError::Unavailable)? {
            return Err(StoreError::Unavailable);
        }
        *self.saves.lock().map_err(|_| StoreError::Unavailable)? += 1;
        self.inner.save(state)?;
        let mut unconfirmed = self
            .unconfirmed
            .lock()
            .map_err(|_| StoreError::Unavailable)?;
        if *unconfirmed > 0 {
            if *unconfirmed != usize::MAX {
                *unconfirmed -= 1;
            }
            return Err(StoreError::Unconfirmed);
        }
        *self.durable.lock().map_err(|_| StoreError::Unavailable)? = Some(state.clone());
        Ok(())
    }

    fn reset(&self) -> Result<(), StoreError> {
        if *self.failing.lock().map_err(|_| StoreError::Unavailable)?
            || *self
                .failing_resets
                .lock()
                .map_err(|_| StoreError::Unavailable)?
        {
            return Err(StoreError::Unavailable);
        }
        if let Ok(mut e) = self.load_error.lock() {
            *e = None;
        }
        if let Ok(mut d) = self.durable.lock() {
            *d = None;
        }
        self.inner.reset()
    }
}

type PickerReply = Option<Result<(String, Vec<u8>), ImportError>>;

/// A native-sheet stand-in returning queued replies (or cancelling).
#[derive(Default)]
pub struct ScriptedPicker {
    replies: Mutex<VecDeque<PickerReply>>,
    opened: Mutex<usize>,
    /// While set, sheets stay open until [`ScriptedPicker::release`].
    gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

impl ScriptedPicker {
    pub fn reply(&self, reply: PickerReply) {
        if let Ok(mut r) = self.replies.lock() {
            r.push_back(reply);
        }
    }

    pub fn opened(&self) -> usize {
        self.opened.lock().map(|o| *o).unwrap_or(0)
    }
}

impl ScriptedPicker {
    /// Keeps later sheets open (their replies wait) until released.
    pub fn hold(&self) {
        if let Ok(mut g) = self.gate.lock() {
            *g = Some(Arc::new(tokio::sync::Notify::new()));
        }
    }

    /// Lets held sheets answer.
    pub fn release(&self) {
        if let Some(gate) = self.gate.lock().ok().and_then(|mut g| g.take()) {
            gate.notify_waiters();
        }
    }
}

impl ImportPicker for ScriptedPicker {
    fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        if let Ok(mut o) = self.opened.lock() {
            *o += 1;
        }
        let reply = self
            .replies
            .lock()
            .ok()
            .and_then(|mut r| r.pop_front())
            .flatten();
        let gate = self.gate.lock().ok().and_then(|g| g.clone());
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            reply.map(|r| {
                r.map(|(name, bytes)| ImportFile {
                    name,
                    bytes: Zeroizing::new(bytes),
                })
            })
        })
    }
}

/// Folder sheets returning queued folders (or cancelling).
#[derive(Default)]
pub struct ScriptedFolders {
    folders: Mutex<VecDeque<Option<std::path::PathBuf>>>,
    created: Mutex<VecDeque<Option<std::path::PathBuf>>>,
    saves: Mutex<VecDeque<Option<std::path::PathBuf>>>,
    suggested: Mutex<Vec<String>>,
    opened: Mutex<usize>,
}

impl ScriptedFolders {
    /// The next existing-folder sheet answers `folder` (None: cancel).
    pub fn reply(&self, folder: Option<std::path::PathBuf>) {
        if let Ok(mut f) = self.folders.lock() {
            f.push_back(folder);
        }
    }

    /// The next new-folder sheet answers `folder` (None: cancel).
    pub fn reply_new(&self, folder: Option<std::path::PathBuf>) {
        if let Ok(mut f) = self.created.lock() {
            f.push_back(folder);
        }
    }

    /// The next save sheet answers `file` (None: cancel).
    pub fn reply_save(&self, file: Option<std::path::PathBuf>) {
        if let Ok(mut f) = self.saves.lock() {
            f.push_back(file);
        }
    }

    /// The names the save sheet was opened with.
    pub fn suggested(&self) -> Vec<String> {
        self.suggested.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn opened(&self) -> usize {
        self.opened.lock().map(|o| *o).unwrap_or(0)
    }

    fn next(
        &self,
        queue: &Mutex<VecDeque<Option<std::path::PathBuf>>>,
    ) -> BoxFuture<'static, Option<std::path::PathBuf>> {
        if let Ok(mut o) = self.opened.lock() {
            *o += 1;
        }
        let reply = queue.lock().ok().and_then(|mut q| q.pop_front()).flatten();
        Box::pin(async move { reply })
    }
}

impl crate::FolderPicker for ScriptedFolders {
    fn choose_folder(&self) -> BoxFuture<'static, Option<std::path::PathBuf>> {
        self.next(&self.folders)
    }

    fn choose_new(&self) -> BoxFuture<'static, Option<std::path::PathBuf>> {
        self.next(&self.created)
    }

    fn choose_project(&self) -> BoxFuture<'static, Option<std::path::PathBuf>> {
        self.next(&self.folders)
    }

    fn choose_save(&self, suggested: String) -> BoxFuture<'static, Option<std::path::PathBuf>> {
        if let Ok(mut s) = self.suggested.lock() {
            s.push(suggested);
        }
        self.next(&self.saves)
    }
}

/// Records the pages the core asked to open; opens nothing.
#[derive(Default)]
pub struct ScriptedOpener {
    opened: Mutex<Vec<String>>,
}

impl ScriptedOpener {
    pub fn opened(&self) -> Vec<String> {
        self.opened.lock().map(|o| o.clone()).unwrap_or_default()
    }
}

impl crate::UrlOpener for ScriptedOpener {
    fn open(&self, url: &url::Url) -> bool {
        self.opened
            .lock()
            .map(|mut o| o.push(url.to_string()))
            .is_ok()
    }
}

/// Counts native print sheets requested; a sheet that cannot open can be
/// scripted.
#[derive(Default)]
pub struct ScriptedPrinter {
    printed: std::sync::atomic::AtomicUsize,
    unavailable: std::sync::atomic::AtomicBool,
}

impl ScriptedPrinter {
    pub fn printed(&self) -> usize {
        self.printed.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Makes later sheets fail to open.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.unavailable
            .store(unavailable, std::sync::atomic::Ordering::SeqCst);
    }
}

impl crate::PagePrinter for ScriptedPrinter {
    fn print(&self) -> bool {
        if self.unavailable.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        self.printed
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    }
}

/// Answers native image-picker sheets from a script (`None`: cancelled).
#[derive(Default)]
pub struct ScriptedImagePicker {
    replies: Mutex<VecDeque<PickerReply>>,
    opened: Mutex<usize>,
}

impl ScriptedImagePicker {
    pub fn reply(&self, reply: PickerReply) {
        if let Ok(mut r) = self.replies.lock() {
            r.push_back(reply);
        }
    }

    pub fn opened(&self) -> usize {
        self.opened.lock().map(|o| *o).unwrap_or(0)
    }
}

impl crate::ImagePicker for ScriptedImagePicker {
    fn choose_image(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        if let Ok(mut o) = self.opened.lock() {
            *o += 1;
        }
        let reply = self
            .replies
            .lock()
            .ok()
            .and_then(|mut r| r.pop_front())
            .flatten();
        Box::pin(async move {
            reply.map(|r| {
                r.map(|(name, bytes)| ImportFile {
                    name,
                    bytes: Zeroizing::new(bytes),
                })
            })
        })
    }
}

/// A scripted native operator confirmation. By default every request is
/// held until [`ScriptedConfirmer::complete`] (or a dismissal) resolves it.
#[derive(Default)]
pub struct ScriptedConfirmer {
    state: Mutex<ConfirmerState>,
}

#[derive(Default)]
struct ConfirmerState {
    auto: Option<bool>,
    pending: Vec<(u64, tokio::sync::oneshot::Sender<bool>)>,
    requests: Vec<(u64, crate::authority::ConfirmationRequest)>,
    dismissed: Vec<u64>,
}

impl ScriptedConfirmer {
    /// Answer every later request at once (`None`: hold them).
    pub fn auto(&self, answer: Option<bool>) {
        if let Ok(mut s) = self.state.lock() {
            s.auto = answer;
        }
    }

    /// Resolves the oldest held confirmation; false when none is held.
    pub fn complete(&self, accepted: bool) -> bool {
        let sender = self
            .state
            .lock()
            .ok()
            .and_then(|mut s| (!s.pending.is_empty()).then(|| s.pending.remove(0).1));
        sender.is_some_and(|tx| tx.send(accepted).is_ok())
    }

    pub fn requests(&self) -> Vec<(u64, crate::authority::ConfirmationRequest)> {
        self.state
            .lock()
            .map(|s| s.requests.clone())
            .unwrap_or_default()
    }

    pub fn held(&self) -> usize {
        self.state.lock().map(|s| s.pending.len()).unwrap_or(0)
    }

    pub fn dismissed(&self) -> Vec<u64> {
        self.state
            .lock()
            .map(|s| s.dismissed.clone())
            .unwrap_or_default()
    }
}

impl crate::authority::OperatorConfirmation for ScriptedConfirmer {
    fn confirm(
        &self,
        nonce: u64,
        request: crate::authority::ConfirmationRequest,
    ) -> futures_util::future::BoxFuture<'static, bool> {
        let Ok(mut s) = self.state.lock() else {
            return Box::pin(async { false });
        };
        s.requests.push((nonce, request));
        if let Some(answer) = s.auto {
            return Box::pin(async move { answer });
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        s.pending.push((nonce, tx));
        Box::pin(async move { rx.await.unwrap_or(false) })
    }

    fn dismiss(&self, nonce: u64) {
        if let Ok(mut s) = self.state.lock() {
            s.dismissed.push(nonce);
            if let Some(i) = s.pending.iter().position(|(n, _)| *n == nonce) {
                let (_, tx) = s.pending.remove(i);
                let _ = tx.send(false);
            }
        }
    }
}

/// A scripted one-time secret sheet: records what it was asked to show.
#[derive(Default)]
pub struct ScriptedSecrets {
    shown: Mutex<Vec<(String, String)>>,
    dismissals: Mutex<usize>,
}

impl ScriptedSecrets {
    pub fn shown(&self) -> Vec<(String, String)> {
        self.shown.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn dismissals(&self) -> usize {
        self.dismissals.lock().map(|d| *d).unwrap_or(0)
    }
}

impl crate::authority::SecretPresenter for ScriptedSecrets {
    fn reveal(&self, label: String, secret: Zeroizing<String>) {
        if let Ok(mut s) = self.shown.lock() {
            s.push((label, secret.as_str().to_owned()));
        }
    }

    fn dismiss(&self) {
        if let Ok(mut d) = self.dismissals.lock() {
            *d += 1;
        }
    }
}

/// What a scripted save sheet does with the next export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SaveScript {
    #[default]
    Save,
    Cancel,
    Fail,
}

/// A scripted save sheet: records each export it was handed.
#[derive(Default)]
pub struct ScriptedSaver {
    saved: Mutex<Vec<(String, Vec<u8>)>>,
    next: Mutex<SaveScript>,
}

impl ScriptedSaver {
    /// Every (suggested name, bytes) the sheet was handed.
    pub fn saved(&self) -> Vec<(String, Vec<u8>)> {
        self.saved.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn script(&self, next: SaveScript) {
        if let Ok(mut n) = self.next.lock() {
            *n = next;
        }
    }
}

impl crate::authority::FileSaver for ScriptedSaver {
    fn save(
        &self,
        suggested: String,
        bytes: Vec<u8>,
    ) -> futures_util::future::BoxFuture<'static, Result<Option<String>, ()>> {
        let next = self.next.lock().map(|n| *n).unwrap_or_default();
        if let Ok(mut s) = self.saved.lock() {
            s.push((suggested.clone(), bytes));
        }
        Box::pin(async move {
            match next {
                SaveScript::Save => Ok(Some(suggested)),
                SaveScript::Cancel => Ok(None),
                SaveScript::Fail => Err(()),
            }
        })
    }
}

/// v1's Keychain service, scripted (C23): items planted by the test, reads
/// recorded. It has no write method, like the real source.
#[derive(Default)]
pub struct ScriptedV1Keychain {
    items: Mutex<HashMap<String, Zeroizing<Vec<u8>>>>,
    failure: Mutex<Option<VaultError>>,
    reads: Mutex<Vec<String>>,
}

impl ScriptedV1Keychain {
    /// Plants an item as v1 would have stored it.
    pub fn plant(&self, account: &str, bytes: &[u8]) {
        if let Ok(mut items) = self.items.lock() {
            items.insert(account.to_owned(), Zeroizing::new(bytes.to_vec()));
        }
    }

    /// Every read fails with `failure` (locked, denied...) until cleared.
    pub fn fail_with(&self, failure: Option<VaultError>) {
        if let Ok(mut f) = self.failure.lock() {
            *f = failure;
        }
    }

    /// The accounts read, in order.
    pub fn reads(&self) -> Vec<String> {
        self.reads.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// The items as planted (to check nothing changed them).
    pub fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut all: Vec<(String, Vec<u8>)> = self
            .items
            .lock()
            .map(|i| i.iter().map(|(k, v)| (k.clone(), v.to_vec())).collect())
            .unwrap_or_default();
        all.sort();
        all
    }
}

impl crate::import_v1::V1Keychain for ScriptedV1Keychain {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<Vec<u8>>>, VaultError> {
        if let Ok(mut reads) = self.reads.lock() {
            reads.push(account.to_owned());
        }
        if let Some(f) = self.failure.lock().ok().and_then(|f| *f) {
            return Err(f);
        }
        Ok(self.items.lock().ok().and_then(|i| i.get(account).cloned()))
    }
}

/// Handles to the scripted adapters behind an [`Adapters`] value.
pub struct Handles {
    pub vault: Arc<ScriptedVault>,
    /// The Academy learner namespace.
    pub learner_vault: Arc<ScriptedVault>,
    pub store: Arc<ScriptedStore>,
    pub picker: Arc<ScriptedPicker>,
    pub opener: Arc<ScriptedOpener>,
    pub printer: Arc<ScriptedPrinter>,
    pub images: Arc<ScriptedImagePicker>,
    pub confirmer: Arc<ScriptedConfirmer>,
    pub secrets: Arc<ScriptedSecrets>,
    pub text_files: Arc<ScriptedPicker>,
    /// This instance's offline work root (a fresh temporary directory).
    pub offline_root: std::path::PathBuf,
    pub saver: Arc<ScriptedSaver>,
    /// This instance's sealed investigations and report files.
    pub sessions_root: std::path::PathBuf,
    pub case_files: Arc<ScriptedPicker>,
    /// This instance's sealed case libraries.
    pub library_root: std::path::PathBuf,
    pub folders: Arc<ScriptedFolders>,
    pub workbench_files: Arc<ScriptedPicker>,
    pub gateway_files: Arc<ScriptedPicker>,
    /// This instance's private API Client folder.
    pub workbench_root: std::path::PathBuf,
    /// The v1 folder this instance imports from (absent until staged).
    pub v1_root: std::path::PathBuf,
    pub v1_keychain: Arc<ScriptedV1Keychain>,
    /// This instance's import receipts.
    pub import_root: std::path::PathBuf,
}

/// Fresh scripted adapters with empty storage.
pub fn adapters() -> (Adapters, Handles) {
    adapters_with(ScriptedStore::default())
}

pub fn adapters_with(store: ScriptedStore) -> (Adapters, Handles) {
    let handles = Handles {
        vault: Arc::new(ScriptedVault::default()),
        learner_vault: Arc::new(ScriptedVault::default()),
        store: Arc::new(store),
        picker: Arc::new(ScriptedPicker::default()),
        opener: Arc::new(ScriptedOpener::default()),
        printer: Arc::new(ScriptedPrinter::default()),
        images: Arc::new(ScriptedImagePicker::default()),
        confirmer: Arc::new(ScriptedConfirmer::default()),
        secrets: Arc::new(ScriptedSecrets::default()),
        text_files: Arc::new(ScriptedPicker::default()),
        offline_root: fresh_offline_root(),
        saver: Arc::new(ScriptedSaver::default()),
        sessions_root: fresh_offline_root().with_extension("sessions"),
        case_files: Arc::new(ScriptedPicker::default()),
        library_root: fresh_offline_root().with_extension("library"),
        folders: Arc::new(ScriptedFolders::default()),
        workbench_files: Arc::new(ScriptedPicker::default()),
        gateway_files: Arc::new(ScriptedPicker::default()),
        workbench_root: fresh_offline_root().with_extension("workbench"),
        v1_root: fresh_offline_root().with_extension("v1"),
        v1_keychain: Arc::new(ScriptedV1Keychain::default()),
        import_root: fresh_offline_root().with_extension("imports"),
    };
    (
        Adapters {
            keychain: handles.vault.clone(),
            learner_keychain: handles.learner_vault.clone(),
            store: handles.store.clone(),
            picker: handles.picker.clone(),
            opener: handles.opener.clone(),
            printer: handles.printer.clone(),
            images: handles.images.clone(),
            confirmer: handles.confirmer.clone(),
            secrets: handles.secrets.clone(),
            text_files: handles.text_files.clone(),
            offline_root: Some(handles.offline_root.clone()),
            saver: handles.saver.clone(),
            sessions_root: Some(handles.sessions_root.clone()),
            case_files: handles.case_files.clone(),
            library_root: Some(handles.library_root.clone()),
            folders: handles.folders.clone(),
            workbench_files: handles.workbench_files.clone(),
            gateway_files: handles.gateway_files.clone(),
            workbench_root: Some(handles.workbench_root.clone()),
            harness: crate::harness::sidecar::HarnessAdapters::unavailable(),
            v1_source: Some(crate::import_v1::V1Source {
                root: handles.v1_root.clone(),
                keychain: handles.v1_keychain.clone(),
            }),
            import_root: Some(handles.import_root.clone()),
        },
        handles,
    )
}

/// A fresh, empty offline work root under the system temporary directory
/// (resolved, so no symlinked ancestor).
fn fresh_offline_root() -> std::path::PathBuf {
    let mut entropy = [0u8; 8];
    let _ = getrandom::fill(&mut entropy);
    let base = std::env::temp_dir()
        .canonicalize()
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join(format!("airs-v2-offline-{}", hex::encode(entropy)))
}

/// The fixture sidecar launcher (C19): the host adapter's semantics
/// (`desktop-host` `sidecar::OsLauncher`) for core tests, which cannot
/// depend on the host. No inherited environment, piped stdin and stdout, no
/// stderr, its own process group; exit observed without reaping; the group
/// killed, never signalled once reaped; killed and reaped on drop.
#[cfg(feature = "test-fixtures")]
#[allow(unsafe_code)]
pub mod sidecar {
    use std::io::{Read, Write};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::{Arc, Mutex};

    use crate::harness::sidecar::{LaunchSpec, Sidecar, SidecarLauncher};

    /// Runtime roots whose next homes cannot be made (P06 review R4-F1).
    static REFUSED_HOMES: Mutex<Vec<std::path::PathBuf>> = Mutex::new(Vec::new());

    /// Refuses (or allows again) making homes under `root`.
    pub fn refuse_homes(root: &std::path::Path, refuse: bool) {
        if let Ok(mut r) = REFUSED_HOMES.lock() {
            r.retain(|p| p != root);
            if refuse {
                r.push(root.to_owned());
            }
        }
    }

    pub(crate) fn home_refused(root: &std::path::Path) -> bool {
        REFUSED_HOMES
            .lock()
            .is_ok_and(|r| r.iter().any(|p| p == root))
    }

    /// How later launches fail (P06 review R4-F1).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum LaunchFault {
        /// The process cannot be spawned.
        Refused,
        /// It runs without its output pipe.
        Pipeless,
    }

    /// Launches processes and records every spec it was given.
    #[derive(Default)]
    pub struct ProcessLauncher {
        pub launched: Mutex<Vec<LaunchSpec>>,
        /// From the `n`th launch on (counting from 0), each fails this way.
        pub fault: Mutex<Option<(usize, LaunchFault)>>,
    }

    impl ProcessLauncher {
        pub fn new() -> Arc<Self> {
            Arc::new(Self::default())
        }
        pub fn launches(&self) -> Vec<LaunchSpec> {
            self.launched.lock().map(|l| l.clone()).unwrap_or_default()
        }
    }

    impl SidecarLauncher for ProcessLauncher {
        fn launch(&self, spec: &LaunchSpec) -> std::io::Result<Box<dyn Sidecar>> {
            let count = self
                .launched
                .lock()
                .map(|mut l| {
                    l.push(spec.clone());
                    l.len() - 1
                })
                .unwrap_or(0);
            let fault = self
                .fault
                .lock()
                .ok()
                .and_then(|f| *f)
                .filter(|(from, _)| count >= *from)
                .map(|(_, fault)| fault);
            if fault == Some(LaunchFault::Refused) {
                return Err(std::io::Error::other("fixture"));
            }
            let child = Command::new(&spec.executable)
                .args(&spec.args)
                .env_clear()
                .envs(spec.env.iter().map(|(k, v)| (k, v)))
                .current_dir(&spec.cwd)
                .stdin(Stdio::piped())
                .stdout(if fault == Some(LaunchFault::Pipeless) {
                    Stdio::null()
                } else {
                    Stdio::piped()
                })
                .stderr(Stdio::null())
                .process_group(0)
                .spawn()?;
            Ok(Box::new(Process {
                child,
                reaped: None,
            }))
        }
    }

    struct Process {
        child: Child,
        reaped: Option<i32>,
    }

    impl Sidecar for Process {
        fn pid(&self) -> u32 {
            self.child.id()
        }
        fn take_stdin(&mut self) -> Option<Box<dyn Write + Send>> {
            self.child
                .stdin
                .take()
                .map(|s| Box::new(s) as Box<dyn Write + Send>)
        }
        fn take_stdout(&mut self) -> Option<Box<dyn Read + Send>> {
            self.child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>)
        }
        fn exited(&mut self) -> std::io::Result<bool> {
            if self.reaped.is_some() {
                return Ok(true);
            }
            // SAFETY: a zeroed siginfo_t for our own unreaped child.
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let rc = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id() as libc::id_t,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if rc != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(info.si_pid != 0)
        }
        fn kill_group(&mut self) {
            if self.reaped.is_none()
                && let Ok(pid) = libc::pid_t::try_from(self.child.id())
                && pid > 1
            {
                // SAFETY: our unreaped child leads its own group.
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
        fn reap(&mut self) -> std::io::Result<Option<i32>> {
            if let Some(code) = self.reaped {
                return Ok(Some(code));
            }
            Ok(self.child.try_wait()?.map(|status| {
                let code = status.code().unwrap_or(-1);
                self.reaped = Some(code);
                code
            }))
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            if self.reaped.is_none() {
                self.kill_group();
                let _ = self.child.wait();
            }
        }
    }
}
