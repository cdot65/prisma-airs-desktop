//! Account-scoped encrypted Academy offline work: downloaded course bundles,
//! cached attempts and drafts kept on this Mac (C07).
//!
//! Each learner account has its own directory (named by a digest of the
//! account id, so the id itself is not on disk) and its own random 32-byte
//! key in the learner Keychain namespace. Every record is sealed with
//! XChaCha20-Poly1305 under a fresh random nonce; the associated data binds
//! the schema, the account digest and the record's file name, so a record
//! cannot be read as another account's or another record. Nothing is ever
//! written in plaintext: a store that cannot open (no key, wrong key, locked
//! Keychain, another instance) stays closed, and a wrong key is refused
//! before any record is read, set aside or written (a sealed key check). Writes replace a file
//! atomically (temporary file, sync, rename, directory sync) with owner-only
//! permissions. Writes, deletions and the quota check they make run one at
//! a time per store. A record that does not authenticate is renamed aside
//! and kept (the newest [`UNREADABLE_KEPT`]; they count toward the account's
//! budget). Sizes and counts are bounded.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Sealed record header.
const MAGIC: &[u8; 8] = b"AIRSOFF\x01";
const NONCE: usize = 24;
const TAG: usize = 16;
/// Associated-data schema.
const SCHEMA: &str = "prisma-airs-desktop-v2/offline/1";
/// One record (a downloaded course is at most Academy's 7 MiB bundle, or its
/// assembled unit bundles).
pub const RECORD_LIMIT: u64 = 16 * 1024 * 1024;
/// All of one account's offline work.
pub const ACCOUNT_LIMIT: u64 = 512 * 1024 * 1024;
/// Records per account.
pub const RECORD_COUNT: usize = 1024;
/// Records set aside as unreadable that are kept for recovery (older ones
/// are removed when another is set aside).
pub const UNREADABLE_KEPT: usize = 8;
const UNREADABLE: &str = ".unreadable-";

/// Whether an account holding `used` bytes in `count` records can take a
/// `sealed`-byte record replacing one of `existing` bytes (0: a new record).
fn within_budget(used: u64, count: usize, existing: u64, sealed: u64) -> bool {
    let after = used.saturating_sub(existing).saturating_add(sealed);
    let after_count = count + usize::from(existing == 0);
    after <= ACCOUNT_LIMIT && after_count <= RECORD_COUNT
}
const LOCK: &str = ".lock";
/// A sealed known value proving the store's key before anything is read,
/// set aside or written.
const KEY_CHECK: &str = "key.check";
const KEY_CHECK_VALUE: &[u8] = b"prisma-airs-desktop-v2 offline key check";

/// Why offline work is unavailable. Sanitized: never the account id, paths or
/// content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OfflineError {
    /// Another instance of the app holds this account's offline work.
    #[error("offline work is open in another instance of the app")]
    Busy,
    /// The directory or a file could not be created, read or replaced.
    #[error("offline work storage is unavailable")]
    Unavailable,
    /// A record did not authenticate; it was kept aside, not deleted.
    #[error("saved offline work could not be authenticated and was kept aside")]
    Corrupt,
    /// The record is larger than one record may be.
    #[error("this item is too large to keep offline")]
    TooLarge,
    /// The account's offline storage is full (size or record count).
    #[error("offline storage is full")]
    Full,
    /// The key is not a 32-byte key.
    #[error("the offline work key is invalid; saved work was kept")]
    BadKey,
}

/// What a record holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordKind {
    /// A downloaded course bundle and its lease/revision state.
    Course,
    /// A cached `desktop.attempt.get` reply.
    Attempt,
    /// The cached `desktop.attempt.list` reply.
    AttemptList,
    /// Lab work kept on this Mac that Academy has not confirmed.
    Draft,
}

impl RecordKind {
    fn prefix(self) -> &'static str {
        match self {
            Self::Course => "course",
            Self::Attempt => "attempt",
            Self::AttemptList => "attempts",
            Self::Draft => "draft",
        }
    }
}

/// Fault points for interrupted-write tests (fixture builds only).
#[cfg(any(test, feature = "test-fixtures"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteFault {
    /// Stop after writing the temporary file, before the rename.
    BeforeRename,
    /// Rename, then fail the directory sync (visible but not known durable).
    DirectorySync,
}

/// The hex SHA-256 of a value.
pub fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// The directory holding one account's offline work.
pub fn account_dir(root: &Path, account: &str) -> PathBuf {
    root.join(digest(account))
}

/// A fresh random key for an account's offline work.
pub fn new_key() -> Option<Zeroizing<Vec<u8>>> {
    let mut key = Zeroizing::new(vec![0u8; 32]);
    getrandom::fill(&mut key).ok()?;
    Some(key)
}

/// Removes all offline work under `root` (a local-data reset). The keys are
/// removed with the learner Keychain namespace.
pub fn purge(root: &Path) -> Result<(), OfflineError> {
    match fs::symlink_metadata(root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(meta) if meta.is_dir() => {
            fs::remove_dir_all(root).map_err(|_| OfflineError::Unavailable)
        }
        _ => Err(OfflineError::Unavailable),
    }
}

/// One account's open offline work. Dropping it releases the instance lock.
pub struct OfflineStore {
    account_digest: String,
    dir: PathBuf,
    key: Zeroizing<Vec<u8>>,
    _lock: fs::File,
    /// Serializes quota checks, commits, deletions and set-asides.
    commit: std::sync::Mutex<()>,
    temp: std::sync::atomic::AtomicU64,
    /// Records renamed into place whose directory sync did not complete:
    /// readable, but not known durable until a later directory sync does.
    unconfirmed: std::sync::Mutex<std::collections::BTreeSet<String>>,
    #[cfg(any(test, feature = "test-fixtures"))]
    fault: std::sync::Mutex<Option<WriteFault>>,
}

impl std::fmt::Debug for OfflineStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OfflineStore")
    }
}

fn ordinary_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

fn create_dir(path: &Path) -> Result<(), OfflineError> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .map_err(|_| OfflineError::Unavailable)?;
    if ordinary_dir(path) {
        Ok(())
    } else {
        Err(OfflineError::Unavailable)
    }
}

impl OfflineStore {
    /// Opens (creating if needed) `account`'s offline work under `root`
    /// with its key. Only one instance may hold it.
    pub fn open(root: &Path, account: &str, key: &[u8]) -> Result<Self, OfflineError> {
        if key.len() != 32 {
            return Err(OfflineError::BadKey);
        }
        if account.is_empty() || account.len() > 256 || !root.is_absolute() {
            return Err(OfflineError::Unavailable);
        }
        create_dir(root)?;
        let account_digest = digest(account);
        let dir = root.join(&account_digest);
        create_dir(&dir)?;
        let lock_path = dir.join(LOCK);
        if fs::symlink_metadata(&lock_path).is_ok_and(|m| !m.is_file()) {
            return Err(OfflineError::Unavailable);
        }
        let mut options = fs::OpenOptions::new();
        options.create(true).read(true).write(true).truncate(false);
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(&lock_path)
            .map_err(|_| OfflineError::Unavailable)?;
        lock.try_lock().map_err(|_| OfflineError::Busy)?;
        let store = Self {
            account_digest,
            dir,
            key: Zeroizing::new(key.to_vec()),
            _lock: lock,
            commit: std::sync::Mutex::new(()),
            temp: std::sync::atomic::AtomicU64::new(0),
            unconfirmed: std::sync::Mutex::new(std::collections::BTreeSet::new()),
            #[cfg(any(test, feature = "test-fixtures"))]
            fault: std::sync::Mutex::new(None),
        };
        store.verify_key()?;
        let dir = &store.dir;
        // Holding the instance lock, no other writer exists: temporary files
        // are leftovers of an interrupted write.
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') && name.ends_with(".tmp") {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        // Whatever an earlier run renamed into place is made durable before
        // anything relies on it; a directory that cannot be synced stays closed.
        store.sync_directory()?;
        Ok(store)
    }

    /// Proves the key against the store's sealed key check; a store without
    /// one gets it only if it is empty or one of its records authenticates
    /// under this key. A key that cannot be proven touches nothing.
    fn verify_key(&self) -> Result<(), OfflineError> {
        let path = self.dir.join(KEY_CHECK);
        match Self::read_bounded(&path) {
            Ok(Some(bytes)) => {
                return match self.open_sealed(KEY_CHECK, &bytes) {
                    Some(plain) if plain.as_slice() == KEY_CHECK_VALUE => Ok(()),
                    _ => Err(OfflineError::BadKey),
                };
            }
            Ok(None) => {}
            Err(_) => return Err(OfflineError::BadKey),
        }
        let mut records = 0usize;
        let mut proven = false;
        for entry in fs::read_dir(&self.dir)
            .map_err(|_| OfflineError::Unavailable)?
            .flatten()
        {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !(name.ends_with(".bin") || name.contains(UNREADABLE)) {
                continue;
            }
            records += 1;
            if name.ends_with(".bin")
                && let Ok(Some(bytes)) = Self::read_bounded(&entry.path())
                && self.open_sealed(&name, &bytes).is_some()
            {
                proven = true;
                break;
            }
        }
        if records > 0 && !proven {
            return Err(OfflineError::BadKey);
        }
        let sealed = self.seal(KEY_CHECK, KEY_CHECK_VALUE)?;
        let temp = self.dir.join(format!(".{KEY_CHECK}.tmp"));
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options
            .open(&temp)
            .and_then(|mut f| f.write_all(&sealed).and_then(|()| f.sync_all()))
            .and_then(|()| fs::rename(&temp, &path))
            .and_then(|()| fs::File::open(&self.dir).and_then(|d| d.sync_all()))
            .map_err(|_| OfflineError::Unavailable)
    }

    fn committing(&self) -> Result<std::sync::MutexGuard<'_, ()>, OfflineError> {
        self.commit.lock().map_err(|_| OfflineError::Unavailable)
    }

    /// Makes the next write stop at `fault`; a `DirectorySync` fault also
    /// keeps failing later directory syncs until another fault replaces it
    /// or [`OfflineStore::clear_fault`] (fixture builds only).
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn inject(&self, fault: WriteFault) {
        if let Ok(mut f) = self.fault.lock() {
            *f = Some(fault);
        }
    }

    /// Ends an injected fault (fixture builds only).
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn clear_fault(&self) {
        if let Ok(mut f) = self.fault.lock() {
            *f = None;
        }
    }

    fn name(kind: RecordKind, id: &str) -> Result<String, OfflineError> {
        if id.is_empty() || id.len() > 512 {
            return Err(OfflineError::Unavailable);
        }
        Ok(format!("{}-{}.bin", kind.prefix(), digest(id)))
    }

    fn aad(&self, name: &str) -> Vec<u8> {
        format!("{SCHEMA}:{}:{name}", self.account_digest).into_bytes()
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(chacha20poly1305::Key::from_slice(&self.key))
    }

    fn seal(&self, name: &str, plain: &[u8]) -> Result<Vec<u8>, OfflineError> {
        let mut nonce = [0u8; NONCE];
        getrandom::fill(&mut nonce).map_err(|_| OfflineError::Unavailable)?;
        let sealed = self
            .cipher()
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plain,
                    aad: &self.aad(name),
                },
            )
            .map_err(|_| OfflineError::Unavailable)?;
        let mut out = Vec::with_capacity(MAGIC.len() + NONCE + sealed.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    fn open_sealed(&self, name: &str, bytes: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
        if bytes.len() < MAGIC.len() + NONCE + TAG || &bytes[..MAGIC.len()] != MAGIC {
            return None;
        }
        let nonce = &bytes[MAGIC.len()..MAGIC.len() + NONCE];
        self.cipher()
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: &bytes[MAGIC.len() + NONCE..],
                    aad: &self.aad(name),
                },
            )
            .ok()
            .map(Zeroizing::new)
    }

    fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, OfflineError> {
        let meta = match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(OfflineError::Unavailable),
            Ok(meta) => meta,
        };
        if !meta.is_file() {
            return Err(OfflineError::Unavailable);
        }
        if meta.len() > RECORD_LIMIT + (MAGIC.len() + NONCE + TAG) as u64 {
            return Err(OfflineError::Corrupt);
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .and_then(|f| f.take(RECORD_LIMIT + 64).read_to_end(&mut bytes))
            .map_err(|_| OfflineError::Unavailable)?;
        Ok(Some(bytes))
    }

    /// Keeps a record that did not authenticate aside, if the file at
    /// `path` still does not authenticate (a writer may have replaced it
    /// since it was read). Only the newest [`UNREADABLE_KEPT`] are kept.
    /// Returns the record when it now reads.
    fn set_aside(
        &self,
        path: &Path,
        name: &str,
        decode: impl Fn(&[u8]) -> Option<Value>,
    ) -> Option<Value> {
        let _commit = self.committing().ok()?;
        self.set_aside_committing(path, name, decode)
    }

    fn set_aside_committing(
        &self,
        path: &Path,
        name: &str,
        decode: impl Fn(&[u8]) -> Option<Value>,
    ) -> Option<Value> {
        match Self::read_bounded(path) {
            Ok(Some(bytes)) => {
                if let Some(value) = decode(&bytes) {
                    return Some(value);
                }
            }
            Ok(None) => return None,
            Err(_) => {}
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let _ = fs::rename(
            path,
            self.dir.join(format!("{name}{UNREADABLE}{stamp:024}")),
        );
        let mut kept: Vec<(String, PathBuf)> = fs::read_dir(&self.dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| {
                        let file = e.file_name().to_string_lossy().into_owned();
                        let stamp = file.split(UNREADABLE).nth(1)?.to_owned();
                        Some((stamp, e.path()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        kept.sort();
        let excess = kept.len().saturating_sub(UNREADABLE_KEPT);
        for (_, old) in kept.into_iter().take(excess) {
            let _ = fs::remove_file(old);
        }
        let _ = fs::File::open(&self.dir).and_then(|d| d.sync_all());
        None
    }

    fn decode(&self, kind: RecordKind, id: &str, name: &str, bytes: &[u8]) -> Option<Value> {
        let plain = self.open_sealed(name, bytes)?;
        let envelope: Value = serde_json::from_slice(&plain).ok()?;
        (envelope["kind"] == kind.prefix() && envelope["id"] == id)
            .then(|| envelope["value"].clone())
    }

    /// Reads a record; `None` when there is none.
    pub fn get(&self, kind: RecordKind, id: &str) -> Result<Option<Value>, OfflineError> {
        let name = Self::name(kind, id)?;
        let path = self.dir.join(&name);
        let read = match Self::read_bounded(&path) {
            Ok(None) => return Ok(None),
            Ok(Some(bytes)) => self.decode(kind, id, &name, &bytes),
            Err(OfflineError::Corrupt) => None,
            Err(e) => return Err(e),
        };
        match read {
            Some(value) => Ok(Some(value)),
            None => self
                .set_aside(&path, &name, |b| self.decode(kind, id, &name, b))
                .map(Some)
                .ok_or(OfflineError::Corrupt),
        }
    }

    /// Reads, changes and writes back one record with no other write in
    /// between: `change` sees the current value (`None` when there is none
    /// or it did not authenticate) and returns what to write, or `None` to
    /// leave it as it is.
    pub fn update(
        &self,
        kind: RecordKind,
        id: &str,
        change: impl FnOnce(Option<Value>) -> Option<Value>,
    ) -> Result<(), OfflineError> {
        let name = Self::name(kind, id)?;
        let path = self.dir.join(&name);
        let _commit = self.committing()?;
        let current = match Self::read_bounded(&path) {
            Ok(None) => None,
            Ok(Some(bytes)) => match self.decode(kind, id, &name, &bytes) {
                Some(value) => Some(value),
                None => {
                    self.set_aside_committing(&path, &name, |b| self.decode(kind, id, &name, b))
                }
            },
            Err(OfflineError::Corrupt) => {
                self.set_aside_committing(&path, &name, |b| self.decode(kind, id, &name, b))
            }
            Err(e) => return Err(e),
        };
        match change(current) {
            Some(value) => self.put_committing(kind, id, &value),
            None => Ok(()),
        }
    }

    /// Every readable record of `kind` as (id, value). Records that do not
    /// authenticate are kept aside and skipped.
    pub fn list(&self, kind: RecordKind) -> Result<Vec<(String, Value)>, OfflineError> {
        let prefix = format!("{}-", kind.prefix());
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(|_| OfflineError::Unavailable)? {
            let entry = entry.map_err(|_| OfflineError::Unavailable)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&prefix) || !name.ends_with(".bin") {
                continue;
            }
            let path = entry.path();
            let bytes = match Self::read_bounded(&path) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => continue,
                Err(OfflineError::Corrupt) => Vec::new(),
                Err(e) => return Err(e),
            };
            let envelope = self
                .open_sealed(&name, &bytes)
                .and_then(|plain| serde_json::from_slice::<Value>(&plain).ok())
                .filter(|e| e["kind"] == kind.prefix() && e["id"].is_string());
            match envelope {
                Some(e)
                    if Self::name(kind, e["id"].as_str().unwrap_or_default())
                        .ok()
                        .as_deref()
                        == Some(name.as_str()) =>
                {
                    let id = e["id"].as_str().unwrap_or_default().to_owned();
                    out.push((id, e["value"].clone()));
                }
                _ => {
                    // Re-read under the commit lock: a writer may have
                    // replaced the file since; only a still-unreadable one
                    // is set aside.
                    let fresh = self.set_aside(&path, &name, |b| {
                        let plain = self.open_sealed(&name, b)?;
                        let e: Value = serde_json::from_slice(&plain).ok()?;
                        let id = e["id"].as_str()?;
                        (e["kind"] == kind.prefix()
                            && Self::name(kind, id).ok().as_deref() == Some(name.as_str()))
                        .then(|| json!([id, e["value"]]))
                    });
                    if let Some(pair) = fresh {
                        out.push((
                            pair[0].as_str().unwrap_or_default().to_owned(),
                            pair[1].clone(),
                        ));
                    }
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    /// Bytes and records the account's offline work occupies (sealed files
    /// and records kept aside as unreadable).
    pub fn usage(&self) -> Result<(u64, usize), OfflineError> {
        let mut bytes = 0u64;
        let mut count = 0usize;
        for entry in fs::read_dir(&self.dir).map_err(|_| OfflineError::Unavailable)? {
            let entry = entry.map_err(|_| OfflineError::Unavailable)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with('.') && (name.ends_with(".bin") || name.contains(UNREADABLE)) {
                count += 1;
                bytes = bytes.saturating_add(entry.metadata().map_or(0, |m| m.len()));
            }
        }
        Ok((bytes, count))
    }

    /// Writes (or replaces) a record atomically.
    pub fn put(&self, kind: RecordKind, id: &str, value: &Value) -> Result<(), OfflineError> {
        let _commit = self.committing()?;
        self.put_committing(kind, id, value)
    }

    fn put_committing(
        &self,
        kind: RecordKind,
        id: &str,
        value: &Value,
    ) -> Result<(), OfflineError> {
        let name = Self::name(kind, id)?;
        let plain = Zeroizing::new(
            serde_json::to_vec(&json!({ "kind": kind.prefix(), "id": id, "value": value }))
                .map_err(|_| OfflineError::Unavailable)?,
        );
        if plain.len() as u64 > RECORD_LIMIT {
            return Err(OfflineError::TooLarge);
        }
        let path = self.dir.join(&name);
        let existing = fs::symlink_metadata(&path).map_or(0, |m| m.len());
        let (used, count) = self.usage()?;
        let sealed_len = (plain.len() + MAGIC.len() + NONCE + TAG) as u64;
        if !within_budget(used, count, existing, sealed_len) {
            return Err(OfflineError::Full);
        }
        let sealed = self.seal(&name, &plain)?;
        let serial = self.temp.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temp = self.dir.join(format!(".{name}.{serial}.tmp"));
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let written = options
            .open(&temp)
            .and_then(|mut f| f.write_all(&sealed).and_then(|()| f.sync_all()));
        if written.is_err() {
            let _ = fs::remove_file(&temp);
            return Err(OfflineError::Unavailable);
        }
        #[cfg(any(test, feature = "test-fixtures"))]
        if self.fault.lock().ok().is_some_and(|mut f| {
            let before = *f == Some(WriteFault::BeforeRename);
            if before {
                *f = None;
            }
            before
        }) {
            return Err(OfflineError::Unavailable);
        }
        if fs::rename(&temp, &path).is_err() {
            let _ = fs::remove_file(&temp);
            return Err(OfflineError::Unavailable);
        }
        // Until the directory sync completes, the record is readable but not
        // known durable.
        if let Ok(mut pending) = self.unconfirmed.lock() {
            pending.insert(name);
        }
        self.sync_directory()
    }

    /// Syncs the account directory; on success every earlier rename in it
    /// is durable.
    fn sync_directory(&self) -> Result<(), OfflineError> {
        #[cfg(any(test, feature = "test-fixtures"))]
        if self
            .fault
            .lock()
            .ok()
            .is_some_and(|f| *f == Some(WriteFault::DirectorySync))
        {
            return Err(OfflineError::Unavailable);
        }
        fs::File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(|_| OfflineError::Unavailable)?;
        if let Ok(mut pending) = self.unconfirmed.lock() {
            pending.clear();
        }
        Ok(())
    }

    /// Whether a record's last commit is known durable; when it is not, the
    /// directory sync is tried again (a sync that completes confirms it).
    pub fn durable(&self, kind: RecordKind, id: &str) -> bool {
        let Ok(name) = Self::name(kind, id) else {
            return false;
        };
        let Ok(_commit) = self.committing() else {
            return false;
        };
        let pending = self.unconfirmed.lock().is_ok_and(|p| p.contains(&name));
        !pending || self.sync_directory().is_ok()
    }

    /// Removes a record (a missing record is not an error).
    pub fn delete(&self, kind: RecordKind, id: &str) -> Result<(), OfflineError> {
        let name = Self::name(kind, id)?;
        let _commit = self.committing()?;
        match fs::remove_file(self.dir.join(name)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(OfflineError::Unavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .canonicalize()
            .expect("tmp")
            .join(format!("airs-v2-offline-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn records_are_sealed_and_never_on_disk_in_plaintext() {
        let root = root("sealed");
        let store = OfflineStore::open(&root, "user_2abc", &KEY).expect("open");
        store
            .put(
                RecordKind::Course,
                "course-1",
                &json!({"title": "SYNTHETIC-PLAINTEXT-MARKER"}),
            )
            .expect("put");
        assert_eq!(
            store.get(RecordKind::Course, "course-1").expect("get"),
            Some(json!({"title": "SYNTHETIC-PLAINTEXT-MARKER"}))
        );
        let dir = account_dir(&root, "user_2abc");
        for entry in fs::read_dir(&dir).expect("dir") {
            let bytes = fs::read(entry.expect("entry").path()).expect("read");
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("SYNTHETIC-PLAINTEXT-MARKER"));
            assert!(!text.contains("course-1"));
            assert!(!text.contains("user_2abc"));
        }
        assert!(
            !dir.to_string_lossy().contains("user_2abc"),
            "the account id is not a path"
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&dir).expect("meta").permissions().mode() & 0o777,
            0o700
        );
        let file = fs::read_dir(&dir)
            .expect("dir")
            .map(|e| e.expect("e").path())
            .find(|p| p.extension().is_some_and(|x| x == "bin"))
            .expect("file");
        assert_eq!(
            fs::metadata(file).expect("meta").permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn another_account_or_key_cannot_read_it_and_unreadable_records_are_kept() {
        let root = root("accounts");
        {
            let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
            store
                .put(RecordKind::Draft, "att-1", &json!({"debrief": "A"}))
                .expect("put");
        }
        // A different key for the same account is refused before anything
        // is read or touched.
        assert_eq!(
            OfflineStore::open(&root, "user_2a", &[9; 32]).err(),
            Some(OfflineError::BadKey)
        );
        // A record that stops authenticating under the right key is set aside.
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        let name = OfflineStore::name(RecordKind::Draft, "att-1").expect("name");
        fs::write(
            account_dir(&root, "user_2a").join(&name),
            b"AIRSOFF\x01 tampered",
        )
        .expect("tamper");
        assert_eq!(
            store.get(RecordKind::Draft, "att-1"),
            Err(OfflineError::Corrupt)
        );
        let kept = fs::read_dir(account_dir(&root, "user_2a"))
            .expect("dir")
            .filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.file_name().to_string_lossy().contains(".unreadable-"))
            })
            .count();
        assert_eq!(kept, 1, "set aside, not deleted");
        drop(store);
        // Another account sees its own empty directory.
        let other = OfflineStore::open(&root, "user_2b", &KEY).expect("open");
        assert_eq!(other.get(RecordKind::Draft, "att-1").expect("get"), None);
        assert!(other.list(RecordKind::Draft).expect("list").is_empty());
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_record_moved_to_another_name_does_not_authenticate() {
        let root = root("swap");
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        store.put(RecordKind::Course, "c1", &json!(1)).expect("put");
        store.put(RecordKind::Course, "c2", &json!(2)).expect("put");
        let dir = account_dir(&root, "user_2a");
        let one = dir.join(OfflineStore::name(RecordKind::Course, "c1").expect("name"));
        let two = dir.join(OfflineStore::name(RecordKind::Course, "c2").expect("name"));
        fs::copy(&one, &two).expect("copy");
        assert_eq!(
            store.get(RecordKind::Course, "c2"),
            Err(OfflineError::Corrupt)
        );
        assert_eq!(
            store.get(RecordKind::Course, "c1").expect("get"),
            Some(json!(1))
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn one_instance_holds_an_account_and_keys_must_be_32_bytes() {
        let root = root("lock");
        let first = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        assert_eq!(
            OfflineStore::open(&root, "user_2a", &KEY).err(),
            Some(OfflineError::Busy)
        );
        drop(first);
        assert!(OfflineStore::open(&root, "user_2a", &KEY).is_ok());
        assert_eq!(
            OfflineStore::open(&root, "user_2c", &[1; 16]).err(),
            Some(OfflineError::BadKey)
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn records_are_bounded_and_listed_by_kind() {
        let root = root("bounds");
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        let big = "x".repeat(RECORD_LIMIT as usize);
        assert_eq!(
            store.put(RecordKind::Course, "big", &json!(big)),
            Err(OfflineError::TooLarge)
        );
        store.put(RecordKind::Draft, "b", &json!("B")).expect("put");
        store.put(RecordKind::Draft, "a", &json!("A")).expect("put");
        store
            .put(RecordKind::Course, "c", &json!("C"))
            .expect("put");
        assert_eq!(
            store.list(RecordKind::Draft).expect("list"),
            vec![("a".into(), json!("A")), ("b".into(), json!("B"))]
        );
        store.delete(RecordKind::Draft, "a").expect("delete");
        store
            .delete(RecordKind::Draft, "missing")
            .expect("missing is fine");
        assert_eq!(store.list(RecordKind::Draft).expect("list").len(), 1);
        assert_eq!(store.usage().expect("usage").1, 2);
        purge(&root).expect("purge");
        assert!(!root.exists());
    }

    #[test]
    fn an_account_is_bounded_in_bytes_and_records() {
        // Bytes: up to the account budget, a replacement counting only its growth.
        assert!(within_budget(ACCOUNT_LIMIT - 100, 1, 0, 100));
        assert!(!within_budget(ACCOUNT_LIMIT - 100, 1, 0, 101));
        assert!(within_budget(ACCOUNT_LIMIT, 1, 500, 500));
        assert!(!within_budget(ACCOUNT_LIMIT, 1, 500, 501));
        // Records: a new one past the cap is refused, a replacement is not.
        assert!(within_budget(0, RECORD_COUNT - 1, 0, 1));
        assert!(!within_budget(0, RECORD_COUNT, 0, 1));
        assert!(within_budget(0, RECORD_COUNT, 10, 10));
        // Through the store: the record cap refuses the next new record and
        // keeps every existing one replaceable.
        let root = root("count");
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        for n in 0..RECORD_COUNT {
            store
                .put(RecordKind::Draft, &format!("d{n}"), &json!(n))
                .expect("within the cap");
        }
        assert_eq!(
            store.put(RecordKind::Draft, "one-more", &json!(0)),
            Err(OfflineError::Full)
        );
        store
            .put(RecordKind::Draft, "d0", &json!("again"))
            .expect("replacing is within the cap");
        assert_eq!(store.usage().expect("usage").1, RECORD_COUNT);
        purge(&root).expect("purge");
    }

    #[test]
    fn an_interrupted_write_leaves_the_previous_version() {
        let root = root("fault");
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        store
            .put(RecordKind::Draft, "att", &json!({"v": 1}))
            .expect("put");
        store.inject(WriteFault::BeforeRename);
        assert_eq!(
            store.put(RecordKind::Draft, "att", &json!({"v": 2})),
            Err(OfflineError::Unavailable)
        );
        let dir = account_dir(&root, "user_2a");
        let temps = |dir: &Path| {
            fs::read_dir(dir)
                .expect("dir")
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                .count()
        };
        assert_eq!(temps(&dir), 1, "the interrupted write's temporary file");
        drop(store);
        let reopened = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        assert_eq!(temps(&dir), 0, "leftovers are cleared on open");
        assert_eq!(
            reopened.get(RecordKind::Draft, "att").expect("get"),
            Some(json!({"v": 1}))
        );
        reopened
            .put(RecordKind::Draft, "att", &json!({"v": 3}))
            .expect("put after restart");
        assert_eq!(
            reopened.get(RecordKind::Draft, "att").expect("get"),
            Some(json!({"v": 3}))
        );
        fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn concurrent_writers_commit_whole_records_within_the_budget() {
        let root = root("concurrent");
        let store = std::sync::Arc::new(OfflineStore::open(&root, "user_2a", &KEY).expect("open"));
        let start = std::sync::Arc::new(std::sync::Barrier::new(8));
        let writers: Vec<_> = (0..8)
            .map(|n| {
                let (store, start) = (store.clone(), start.clone());
                std::thread::spawn(move || {
                    start.wait();
                    for round in 0..20 {
                        // Every writer replaces the same record and its own.
                        let body = "x".repeat(1000 + n * 10 + round);
                        store
                            .put(RecordKind::Course, "shared", &json!({"n": n, "body": body}))
                            .expect("shared");
                        store
                            .put(RecordKind::Draft, &format!("own-{n}"), &json!(round))
                            .expect("own");
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().expect("writer");
        }
        // Every record authenticates and holds one writer's whole value.
        let shared = store
            .get(RecordKind::Course, "shared")
            .expect("get")
            .expect("kept");
        let n = shared["n"].as_u64().expect("n") as usize;
        assert!(shared["body"].as_str().expect("body").len() >= 1000 + n * 10);
        for n in 0..8 {
            assert_eq!(
                store
                    .get(RecordKind::Draft, &format!("own-{n}"))
                    .expect("get"),
                Some(json!(19))
            );
        }
        assert_eq!(store.usage().expect("usage").1, 9);
        let leftovers = fs::read_dir(account_dir(&root, "user_2a"))
            .expect("dir")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .count();
        assert_eq!(leftovers, 0);
        purge(&root).expect("purge");
    }

    #[test]
    fn records_kept_aside_are_bounded_and_count_toward_the_budget() {
        let root = root("aside");
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        let dir = account_dir(&root, "user_2a");
        for n in 0..(UNREADABLE_KEPT + 3) {
            let id = format!("c{n}");
            store.put(RecordKind::Course, &id, &json!(n)).expect("put");
            let name = OfflineStore::name(RecordKind::Course, &id).expect("name");
            fs::write(dir.join(&name), b"AIRSOFF\x01 not a sealed record at all").expect("corrupt");
            assert_eq!(
                store.get(RecordKind::Course, &id),
                Err(OfflineError::Corrupt)
            );
        }
        let aside: Vec<String> = fs::read_dir(&dir)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(UNREADABLE))
            .collect();
        assert_eq!(aside.len(), UNREADABLE_KEPT, "only the newest are kept");
        // The oldest went first: the last one set aside is still kept.
        let newest = OfflineStore::name(RecordKind::Course, &format!("c{}", UNREADABLE_KEPT + 2))
            .expect("name");
        assert!(aside.iter().any(|n| n.starts_with(&newest)));
        assert_eq!(store.usage().expect("usage").1, UNREADABLE_KEPT);
        purge(&root).expect("purge");
    }

    #[test]
    fn an_update_sees_the_current_record_and_can_leave_it() {
        let root = root("update");
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        // Nothing there: the change may decline to create it.
        store
            .update(RecordKind::Course, "c", |current| {
                assert_eq!(current, None);
                None
            })
            .expect("update");
        assert_eq!(store.get(RecordKind::Course, "c").expect("get"), None);
        store
            .put(RecordKind::Course, "c", &json!({"revision": "r1"}))
            .expect("put");
        store
            .update(RecordKind::Course, "c", |current| {
                let mut v = current.expect("current");
                v["status"] = json!("outdated");
                Some(v)
            })
            .expect("update");
        assert_eq!(
            store.get(RecordKind::Course, "c").expect("get"),
            Some(json!({"revision": "r1", "status": "outdated"}))
        );
        purge(&root).expect("purge");
    }

    #[test]
    fn a_wrong_key_opens_nothing_and_loses_nothing() {
        let root = root("wrongkey");
        let dir = account_dir(&root, "user_2a");
        {
            let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
            for n in 0..(UNREADABLE_KEPT + 4) {
                store
                    .put(RecordKind::Draft, &format!("d{n}"), &json!({"n": n}))
                    .expect("put");
            }
            store
                .put(RecordKind::Course, "c", &json!("course"))
                .expect("put");
        }
        let snapshot = |dir: &Path| {
            let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
                .expect("dir")
                .flatten()
                .filter(|e| e.file_name() != LOCK)
                .map(|e| {
                    (
                        e.file_name().to_string_lossy().into_owned(),
                        fs::read(e.path()).expect("read"),
                    )
                })
                .collect();
            files.sort();
            files
        };
        let before = snapshot(&dir);
        assert_eq!(
            OfflineStore::open(&root, "user_2a", &[9; 32]).err(),
            Some(OfflineError::BadKey)
        );
        assert_eq!(
            snapshot(&dir),
            before,
            "nothing read aside, deleted or rewritten"
        );
        // Even a store from before the key check (records only): proven by a
        // record, refused when none authenticates.
        fs::remove_file(dir.join(KEY_CHECK)).expect("remove check");
        let without_check = snapshot(&dir);
        assert_eq!(
            OfflineStore::open(&root, "user_2a", &[9; 32]).err(),
            Some(OfflineError::BadKey)
        );
        assert_eq!(snapshot(&dir), without_check);
        // The right key recovers every record.
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        assert_eq!(
            store.list(RecordKind::Draft).expect("list").len(),
            UNREADABLE_KEPT + 4
        );
        assert_eq!(
            store.get(RecordKind::Course, "c").expect("get"),
            Some(json!("course"))
        );
        purge(&root).expect("purge");
    }

    #[test]
    fn a_commit_whose_directory_sync_failed_is_not_durable_until_one_completes() {
        let root = root("dirsync");
        let store = OfflineStore::open(&root, "user_2a", &KEY).expect("open");
        store
            .put(RecordKind::Attempt, "a", &json!({"v": 1}))
            .expect("put");
        assert!(store.durable(RecordKind::Attempt, "a"));
        store.inject(WriteFault::DirectorySync);
        assert_eq!(
            store.put(RecordKind::Attempt, "a", &json!({"v": 2})),
            Err(OfflineError::Unavailable)
        );
        // Visible, but not known durable, and a retry finding the same bytes
        // does not confirm it while syncs keep failing.
        assert_eq!(
            store.get(RecordKind::Attempt, "a").expect("get"),
            Some(json!({"v": 2}))
        );
        assert!(!store.durable(RecordKind::Attempt, "a"));
        assert!(!store.durable(RecordKind::Attempt, "a"));
        assert!(
            store.durable(RecordKind::Draft, "other"),
            "untouched records are durable"
        );
        // A directory sync that completes confirms it.
        store.clear_fault();
        assert!(store.durable(RecordKind::Attempt, "a"));
        purge(&root).expect("purge");
    }
}
