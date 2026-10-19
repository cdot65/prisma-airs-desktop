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
