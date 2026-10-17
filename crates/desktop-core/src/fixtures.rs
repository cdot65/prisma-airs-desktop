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

    pub fn release(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.held = false;
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

/// Handles to the scripted adapters behind an [`Adapters`] value.
pub struct Handles {
    pub vault: Arc<ScriptedVault>,
    pub store: Arc<ScriptedStore>,
    pub picker: Arc<ScriptedPicker>,
}

/// Fresh scripted adapters with empty storage.
pub fn adapters() -> (Adapters, Handles) {
    adapters_with(ScriptedStore::default())
}

pub fn adapters_with(store: ScriptedStore) -> (Adapters, Handles) {
    let handles = Handles {
        vault: Arc::new(ScriptedVault::default()),
        store: Arc::new(store),
        picker: Arc::new(ScriptedPicker::default()),
    };
    (
        Adapters {
            keychain: handles.vault.clone(),
            store: handles.store.clone(),
            picker: handles.picker.clone(),
        },
        handles,
    )
}
