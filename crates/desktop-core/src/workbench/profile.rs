//! The API Client's private profile on this Mac (C14): recent collections,
//! the value-free index of stored secrets, the built-in collection's write
//! record and, per collection, its tabs, selected environment, private
//! environments, protected drafts, archived conflict versions and the save
//! journal. Everything is sealed (XChaCha20-Poly1305) under one key in the
//! credential Keychain namespace; reset removes the key and the folder.
//! Project files stay in their collections and are never copied here except
//! as archived conflict evidence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use super::document::{Document, Fields};
use crate::vault::{RecordId, Vault, VaultError};

const SCHEMA: &str = "prisma-airs-desktop-v2/api-client/1";
pub const PROFILE_BYTES: usize = 1024 * 1024;
pub const SCOPE_BYTES: usize = 64 * 1024 * 1024;
pub const RECENT: usize = 8;
/// Drafts kept per collection (closed tabs keep theirs).
pub const MAX_DRAFTS: usize = 64;
/// Archived conflict versions kept per collection (oldest dropped).
pub const MAX_ARCHIVES: usize = 16;
/// Archived file text kept per version.
pub const ARCHIVE_BYTES: usize = 1024 * 1024;

/// Why the private profile could not be opened or saved (sanitized).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PrivateFailure {
    Locked,
    Denied,
    Unavailable,
    /// A sealed file did not open under its key; it was kept aside.
    Unreadable,
}

/// The credential-namespace record holding the profile key.
pub fn key_record() -> RecordId {
    let digest = crate::offline::digest("api-client-profile-key");
    RecordId::parse(&format!("cr-{}", &digest[..16])).expect("16 hex digits")
}

/// The record holding one workbench secret value (never a tenant record:
/// a different derivation and prefix in the digest).
pub fn secret_record(collection: &str, reference: &str) -> RecordId {
    let digest = crate::offline::digest(&format!("api-client-secret:{collection}\0{reference}"));
    RecordId::parse(&format!("cr-{}", &digest[..16])).expect("16 hex digits")
}

/// A collection's private scope: its folder and identity (v1).
pub fn scope(root: &Path, collection: &str) -> String {
    let digest = Sha256::digest(format!("{}\0{collection}", root.display()).as_bytes());
    hex::encode(&digest[..16])
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recent {
    pub root: PathBuf,
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub recent: Vec<Recent>,
    /// (collection, reference) pairs with a value in the Keychain.
    pub secrets: Vec<(String, String)>,
    /// Built-in collection files and the hash this desktop last wrote.
    pub scm: BTreeMap<String, String>,
    /// Absent in existing profiles: first use opens SCM. Closed is an
    /// explicit preference, not another first-use marker.
    #[serde(default)]
    pub selection: Option<Selection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Selection {
    BuiltIn,
    Custom { root: PathBuf, id: String },
    Closed,
}

impl Profile {
    pub fn remember(&mut self, recent: Recent) {
        self.recent.retain(|r| r.root != recent.root);
        self.recent.insert(0, recent);
        self.recent.truncate(RECENT);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub document: Document,
    pub fields: Fields,
}

/// Both versions kept before a conflict choice replaced either.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Archive {
    pub relative: String,
    pub at: u64,
    pub local: Fields,
    pub disk_metadata: Option<String>,
    pub disk_body: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub open: Vec<String>,
    pub selected: Option<String>,
    pub environment: Option<String>,
    /// Private environments by name (their TOML text).
    pub private: BTreeMap<String, String>,
    pub drafts: BTreeMap<String, Draft>,
    pub archives: Vec<Archive>,
}

impl Scope {
    pub fn archive(&mut self, archive: Archive) {
        self.archives.push(archive);
        let excess = self.archives.len().saturating_sub(MAX_ARCHIVES);
        self.archives.drain(..excess);
    }
}

/// A save in progress: what each file held before and will hold after.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub request: String,
    pub relative: String,
    pub files: Vec<(String, Option<String>, String)>,
}

fn failure(error: VaultError) -> PrivateFailure {
    match error {
        VaultError::Locked => PrivateFailure::Locked,
        VaultError::Denied => PrivateFailure::Denied,
        _ => PrivateFailure::Unavailable,
    }
}

/// The profile key (created on first use when `create`).
pub(crate) fn key(keychain: &dyn Vault, create: bool) -> Result<Option<[u8; 32]>, PrivateFailure> {
    let record = key_record();
    let bytes = match keychain.get(&record) {
        Ok(bytes) => bytes,
        Err(VaultError::NotFound) if create => {
            let fresh = crate::offline::new_key().ok_or(PrivateFailure::Unavailable)?;
            keychain.put(&record, &fresh).map_err(failure)?;
            fresh
        }
        Err(VaultError::NotFound) => return Ok(None),
        Err(e) => return Err(failure(e)),
    };
    let key: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| PrivateFailure::Unreadable)?;
    Ok(Some(key))
}

fn path(root: &Path, name: &str) -> PathBuf {
    let digest = crate::offline::digest(&format!("api-client:{name}"));
    root.join(format!("{}.sealed", &digest[..32]))
}

pub(crate) fn aad(name: &str) -> Vec<u8> {
    format!("{SCHEMA}:{name}").into_bytes()
}

fn load<T: DeserializeOwned + Default>(
    root: &Path,
    keychain: &dyn Vault,
    name: &str,
    limit: usize,
) -> Result<T, PrivateFailure> {
    use std::io::Read;
    let file = path(root, name);
    let mut sealed = Vec::new();
    match std::fs::File::open(&file) {
        Ok(f) => {
            f.take(limit as u64 + 1024)
                .read_to_end(&mut sealed)
                .map_err(|_| PrivateFailure::Unavailable)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(_) => return Err(PrivateFailure::Unavailable),
    }
    let opened = key(keychain, false)?
        .and_then(|key| crate::sessions::open(&key, &aad(name), &sealed))
        .and_then(|plain| serde_json::from_slice::<T>(&plain).ok());
    match opened {
        Some(value) => Ok(value),
        None => {
            let _ = std::fs::rename(&file, file.with_extension("unreadable"));
            Err(PrivateFailure::Unreadable)
        }
    }
}

fn store<T: Serialize>(
    root: &Path,
    keychain: &dyn Vault,
    name: &str,
    value: &T,
    limit: usize,
) -> Result<(), PrivateFailure> {
    let plain = serde_json::to_vec(value).map_err(|_| PrivateFailure::Unavailable)?;
    if plain.len() > limit {
        return Err(PrivateFailure::Unavailable);
    }
    let key = key(keychain, true)?.ok_or(PrivateFailure::Unavailable)?;
    let sealed =
        crate::sessions::seal(&key, &aad(name), &plain).ok_or(PrivateFailure::Unavailable)?;
    crate::sessions::write_private(&path(root, name), &sealed)
        .map_err(|_| PrivateFailure::Unavailable)
}

pub fn load_profile(root: &Path, keychain: &dyn Vault) -> Result<Profile, PrivateFailure> {
    load(root, keychain, "profile", PROFILE_BYTES)
}

pub fn store_profile(
    root: &Path,
    keychain: &dyn Vault,
    profile: &Profile,
) -> Result<(), PrivateFailure> {
    store(root, keychain, "profile", profile, PROFILE_BYTES)
}

pub fn load_scope(root: &Path, keychain: &dyn Vault, scope: &str) -> Result<Scope, PrivateFailure> {
    load(root, keychain, &format!("scope:{scope}"), SCOPE_BYTES)
}

pub fn store_scope(
    root: &Path,
    keychain: &dyn Vault,
    scope: &str,
    data: &Scope,
) -> Result<(), PrivateFailure> {
    store(root, keychain, &format!("scope:{scope}"), data, SCOPE_BYTES)
}

pub fn load_history(
    root: &Path,
    keychain: &dyn Vault,
    scope: &str,
) -> Result<Vec<super::history::RunRecord>, PrivateFailure> {
    load(root, keychain, &format!("history:{scope}"), SCOPE_BYTES)
}

pub fn store_history(
    root: &Path,
    keychain: &dyn Vault,
    scope: &str,
    runs: &[super::history::RunRecord],
) -> Result<(), PrivateFailure> {
    store(
        root,
        keychain,
        &format!("history:{scope}"),
        &runs,
        SCOPE_BYTES,
    )
}

pub fn load_cookies(
    root: &Path,
    keychain: &dyn Vault,
    scope: &str,
) -> Result<Vec<super::cookies::Stored>, PrivateFailure> {
    load(
        root,
        keychain,
        &format!("cookies:{scope}"),
        super::cookies::JAR_BYTES,
    )
}

pub fn store_cookies(
    root: &Path,
    keychain: &dyn Vault,
    scope: &str,
    cookies: &[super::cookies::Stored],
) -> Result<(), PrivateFailure> {
    store(
        root,
        keychain,
        &format!("cookies:{scope}"),
        &cookies,
        super::cookies::JAR_BYTES,
    )
}

pub fn load_journal(
    root: &Path,
    keychain: &dyn Vault,
    scope: &str,
) -> Result<Vec<Journal>, PrivateFailure> {
    load(root, keychain, &format!("journal:{scope}"), PROFILE_BYTES)
}

pub fn store_journal(
    root: &Path,
    keychain: &dyn Vault,
    scope: &str,
    journal: &[Journal],
) -> Result<(), PrivateFailure> {
    store(
        root,
        keychain,
        &format!("journal:{scope}"),
        &journal,
        PROFILE_BYTES,
    )
}

/// How an interrupted save left its files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalState {
    /// Nothing was written.
    NotStarted,
    /// Every file was written.
    Completed,
    /// Some files hold the new content and some the old (or neither).
    Interrupted,
}

pub fn journal_state(root: &Path, journal: &Journal) -> JournalState {
    let mut before = true;
    let mut after = true;
    for (path, old, new) in &journal.files {
        let now = super::document::hash_file(root, path).ok().flatten();
        before &= &now == old;
        after &= now.as_deref() == Some(new.as_str());
    }
    match (before, after) {
        (_, true) => JournalState::Completed,
        (true, false) => JournalState::NotStarted,
        _ => JournalState::Interrupted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::SessionVault;

    #[test]
    fn an_existing_profile_has_an_uninitialized_collection_choice() {
        let old: Profile =
            serde_json::from_value(serde_json::json!({"recent": [], "secrets": [], "scm": {}}))
                .expect("existing profile");
        assert_eq!(old.selection, None);
        for choice in [
            Selection::BuiltIn,
            Selection::Closed,
            Selection::Custom {
                root: "/owned/api".into(),
                id: "c14".into(),
            },
        ] {
            let profile = Profile {
                selection: Some(choice),
                ..old.clone()
            };
            let restored: Profile =
                serde_json::from_slice(&serde_json::to_vec(&profile).expect("encode"))
                    .expect("decode");
            assert_eq!(restored, profile);
        }
    }

    #[test]
    fn sealed_files_open_only_under_their_key_and_name() {
        let dir = crate::workbench::testdir::Dir::new();
        let vault = SessionVault::new();
        let mut profile = Profile::default();
        profile.remember(Recent {
            root: "/a".into(),
            id: "a".into(),
            name: "A".into(),
        });
        store_profile(dir.path(), &vault, &profile).expect("fixture");
        assert_eq!(load_profile(dir.path(), &vault).expect("fixture"), profile);
        let raw = std::fs::read_dir(dir.path())
            .expect("fixture")
            .next()
            .expect("fixture")
            .expect("fixture")
            .path();
        assert!(!String::from_utf8_lossy(&std::fs::read(&raw).expect("fixture")).contains("/a"));
        // Another key cannot open it; the file is set aside, not overwritten.
        let other = SessionVault::new();
        other.put(&key_record(), &[7u8; 32]).expect("fixture");
        assert_eq!(
            load_profile(dir.path(), &other),
            Err(PrivateFailure::Unreadable)
        );
        assert!(raw.with_extension("unreadable").exists());
        assert_eq!(
            load_scope(dir.path(), &vault, "s").expect("fixture"),
            Scope::default()
        );
    }

    #[test]
    fn recent_collections_are_bounded_and_newest_first() {
        let mut p = Profile::default();
        for i in 0..10 {
            p.remember(Recent {
                root: format!("/{i}").into(),
                id: i.to_string(),
                name: String::new(),
            });
        }
        p.remember(Recent {
            root: "/5".into(),
            id: "5".into(),
            name: String::new(),
        });
        assert_eq!(p.recent.len(), RECENT);
        assert_eq!(p.recent[0].root, PathBuf::from("/5"));
        assert_eq!(
            p.recent
                .iter()
                .filter(|r| r.root.as_path() == Path::new("/5"))
                .count(),
            1
        );
    }

    #[test]
    fn workbench_secrets_never_share_a_tenant_record() {
        assert_ne!(secret_record("c", "r"), secret_record("c", "s"));
        assert_ne!(secret_record("c", "r"), secret_record("d", "r"));
        assert_ne!(secret_record("c", "r"), key_record());
    }
}
