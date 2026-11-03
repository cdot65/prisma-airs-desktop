//! Protected credential storage seam and the session-only store.
//!
//! Credentials are immutable records (`cr-` + 16 hex digits). A record is
//! written once, read for use and deleted on replacement, sign-out, endpoint
//! change or reset; a newer record never reuses an older ID, so a late
//! completion cannot overwrite current material. The native host supplies the
//! Keychain implementation; [`SessionVault`] keeps explicitly session-only
//! credentials in memory, zeroized on removal and when the app ends. Vault
//! calls block, so the core runs them off the transition lock on the runtime's
//! blocking pool.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

/// Largest secret the vault stores (the SDK's `Secret` limit is 16 KiB).
pub const MAX_SECRET_BYTES: usize = 16 * 1024;

/// Native-issued immutable credential record identifier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RecordId(String);

impl RecordId {
    pub(crate) fn issue() -> Option<Self> {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).ok()?;
        Some(Self(format!("cr-{}", hex::encode(entropy))))
    }

    pub fn parse(text: &str) -> Option<Self> {
        let hex_part = text.strip_prefix("cr-")?;
        (hex_part.len() == 16
            && hex_part
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for RecordId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("not a record id"))
    }
}

/// Where an operator chose to keep a credential.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StorageMode {
    /// The macOS login Keychain (a dedicated test keychain in fixture builds).
    Keychain,
    /// Memory only; cleared on sign-out, reset and app exit. Never persisted.
    Session,
}

/// Why a vault call failed. Sanitized: no OS message or secret material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum VaultError {
    #[error("the credential store is locked")]
    Locked,
    #[error("access to the credential store was denied")]
    Denied,
    #[error("the credential store is unavailable")]
    Unavailable,
    #[error("the credential was not found")]
    NotFound,
    #[error("the stored credential is unreadable")]
    Corrupt,
}

/// A blocking credential store for one storage mode.
pub trait Vault: Send + Sync + 'static {
    /// Stores a new record. Records are immutable: callers never rewrite one.
    fn put(&self, record: &RecordId, secret: &[u8]) -> Result<(), VaultError>;
    /// Reads a record's secret.
    fn get(&self, record: &RecordId) -> Result<Zeroizing<Vec<u8>>, VaultError>;
    /// Deletes a record; a missing record is `NotFound`.
    fn delete(&self, record: &RecordId) -> Result<(), VaultError>;
}

/// Session-only credentials, held in memory and zeroized when removed.
#[derive(Default)]
pub struct SessionVault {
    records: Mutex<HashMap<RecordId, Zeroizing<Vec<u8>>>>,
}

impl SessionVault {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clears every session credential (sign-out of all, reset, shutdown).
    pub fn clear(&self) {
        if let Ok(mut records) = self.records.lock() {
            records.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.records.lock().map(|r| r.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Vault for SessionVault {
    fn put(&self, record: &RecordId, secret: &[u8]) -> Result<(), VaultError> {
        let mut records = self.records.lock().map_err(|_| VaultError::Unavailable)?;
        records.insert(record.clone(), Zeroizing::new(secret.to_vec()));
        Ok(())
    }

    fn get(&self, record: &RecordId) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        let records = self.records.lock().map_err(|_| VaultError::Unavailable)?;
        records
            .get(record)
            .map(|s| Zeroizing::new(s.to_vec()))
            .ok_or(VaultError::NotFound)
    }

    fn delete(&self, record: &RecordId) -> Result<(), VaultError> {
        let mut records = self.records.lock().map_err(|_| VaultError::Unavailable)?;
        records.remove(record).map(drop).ok_or(VaultError::NotFound)
    }
}

impl Drop for SessionVault {
    fn drop(&mut self) {
        self.clear();
    }
}

/// The credential store used when the OS store cannot be opened: every
/// call reports `Unavailable`; there is no fallback to another store.
pub struct UnavailableVault;

impl Vault for UnavailableVault {
    fn put(&self, _: &RecordId, _: &[u8]) -> Result<(), VaultError> {
        Err(VaultError::Unavailable)
    }

    fn get(&self, _: &RecordId) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        Err(VaultError::Unavailable)
    }

    fn delete(&self, _: &RecordId) -> Result<(), VaultError> {
        Err(VaultError::Unavailable)
    }
}

impl crate::authority::CredentialStore for UnavailableVault {
    fn purge(&self) -> Result<(), VaultError> {
        Err(VaultError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_records_are_isolated_and_removable() {
        let vault = SessionVault::new();
        let a = RecordId::issue().expect("id");
        let b = RecordId::issue().expect("id");
        vault.put(&a, b"key-a").expect("put");
        assert_eq!(vault.get(&a).expect("get").as_slice(), b"key-a");
        assert_eq!(vault.get(&b).err(), Some(VaultError::NotFound));
        vault.delete(&a).expect("delete");
        assert_eq!(vault.delete(&a).err(), Some(VaultError::NotFound));
        vault.put(&b, b"key-b").expect("put");
        vault.clear();
        assert!(vault.is_empty());
    }

    #[test]
    fn record_ids_have_the_issued_shape_only() {
        let id = RecordId::issue().expect("id");
        assert_eq!(RecordId::parse(id.as_str()), Some(id));
        assert!(RecordId::parse("cr-zz23456789abcdef").is_none());
        assert!(serde_json::from_str::<RecordId>("\"../../etc\"").is_err());
    }
}
