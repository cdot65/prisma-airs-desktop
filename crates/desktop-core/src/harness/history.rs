//! Protected Harness history (C20, D047).
//!
//! A conversation is saved after each turn, as one sealed record: its
//! transcript, the binding it ran under, and the engine's own replay with
//! the engine context it was exported for (`desktop/history/read`). Records
//! are kept per scope (tenant and Gateway workspace) under
//! `<root>/<tenant digest>/<scope digest>/<conversation>.sealed`, sealed with
//! XChaCha20-Poly1305 under a random per-scope key held in the Keychain;
//! the associated data binds the schema, the scope and the conversation.
//! A tenant's scopes share one directory, removed with the tenant.
//! Writes replace a record atomically (0600 in a 0700 directory). A record
//! that does not open is named unreadable and left for the operator to
//! delete, never rewritten. Nothing is ever written in plaintext: without
//! the key (locked, denied, missing) history is unavailable.
//!
//! These are blocking functions, run off the authority lock.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::{HarnessEntry, HarnessUsage, MAX_ENTRIES};
use crate::vault::{RecordId, Vault, VaultError};

pub const SCHEMA: &str = "prisma-airs-desktop-v2/harness-history/1";
/// Saved conversations per scope.
pub const MAX_CONVERSATIONS: usize = 16;
/// The largest sealed record (the engine's replay is at most 4 MiB).
pub const MAX_RECORD_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_TITLE_CHARS: usize = 80;
const SUFFIX: &str = ".sealed";

/// Why history could not be read or written.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum HistoryFailure {
    #[error("the Keychain is locked")]
    Locked,
    #[error("Keychain access was denied")]
    Denied,
    #[error("history storage is unavailable")]
    Unavailable,
    #[error("history is full for this workspace")]
    Full,
    #[error("the conversation is too large to save")]
    TooLarge,
    #[error("the saved conversation cannot be read")]
    Unreadable,
    #[error("the saved conversation no longer exists")]
    Missing,
}

impl From<VaultError> for HistoryFailure {
    fn from(error: VaultError) -> Self {
        match error {
            VaultError::Locked => Self::Locked,
            VaultError::Denied => Self::Denied,
            _ => Self::Unavailable,
        }
    }
}

/// Which credential a conversation ran with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CredentialSource {
    WorkspaceKey,
    CompanySignIn,
}

/// The binding a conversation ran under (its engine identity).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedBinding {
    pub tenant: String,
    pub workspace: String,
    pub credential: String,
    pub credential_epoch: u64,
    pub endpoint: String,
    pub source: CredentialSource,
    /// The project and access it ran with (C21; none before).
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub write: bool,
    /// The workspace MCP scope it ran with (C22; none before).
    #[serde(default)]
    pub mcp: Vec<crate::harness::mcp::ScopeServer>,
}

/// One saved conversation.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Record {
    pub schema: String,
    /// The conversation's own ID (a UUID; also the record's file name).
    pub id: String,
    pub title: String,
    /// Wall-clock seconds since the epoch.
    pub created: u64,
    pub updated: u64,
    pub binding: SavedBinding,
    pub entries: Vec<HarnessEntry>,
    pub usage: Option<HarnessUsage>,
    /// The engine's context binding, exactly as exported.
    pub context: Value,
    /// The engine's replay, exactly as exported.
    pub replay: Value,
    /// Where it came from, when not from this app's own conversation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
}

/// A record's provenance (C23).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// Imported from v1: a transcript only, never continued.
    ImportedV1,
}

/// What the list shows of a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listed {
    Saved {
        id: String,
        title: String,
        updated: u64,
        entries: usize,
        binding: SavedBinding,
    },
    /// Present but not openable with this scope's key.
    Unreadable { id: String },
}

/// One scope's history: where it lives and the Keychain record of its key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub dir: PathBuf,
    pub key_record: RecordId,
    digest: String,
}

impl Scope {
    /// The scope of a tenant's Gateway workspace under `root`.
    pub fn new(root: &Path, tenant: &str, workspace: &str) -> Self {
        let digest = crate::offline::digest(&format!("harness-history:{tenant}:{workspace}"));
        let key = crate::offline::digest(&format!("harness-history-key:{tenant}:{workspace}"));
        Self {
            dir: tenant_dir(root, tenant).join(&digest[..32]),
            key_record: RecordId::parse(&format!("cr-{}", &key[..16])).expect("16 hex digits"),
            digest,
        }
    }

    fn aad(&self, id: &str) -> Vec<u8> {
        format!("{SCHEMA}:{}:{id}", self.digest).into_bytes()
    }

    fn path(&self, id: &str) -> Option<PathBuf> {
        super::protocol::is_uuid(id).then(|| self.dir.join(format!("{id}{SUFFIX}")))
    }

    /// The IDs of the records present, oldest name first.
    pub(crate) fn ids(&self) -> Result<Vec<String>, HistoryFailure> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(_) => return Err(HistoryFailure::Unavailable),
        };
        let mut ids: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| {
                e.file_name()
                    .to_str()?
                    .strip_suffix(SUFFIX)
                    .filter(|id| super::protocol::is_uuid(id))
                    .map(str::to_owned)
            })
            .collect();
        ids.sort();
        Ok(ids)
    }
}

/// Where all of a tenant's scopes live.
pub fn tenant_dir(root: &Path, tenant: &str) -> PathBuf {
    root.join(&crate::offline::digest(&format!("harness-history-tenant:{tenant}"))[..32])
}

/// The scope's key, if one was ever made.
fn key(vault: &dyn Vault, scope: &Scope) -> Result<Option<Zeroizing<[u8; 32]>>, HistoryFailure> {
    match vault.get(&scope.key_record) {
        Ok(bytes) => {
            let key: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| HistoryFailure::Unreadable)?;
            Ok(Some(Zeroizing::new(key)))
        }
        Err(VaultError::NotFound) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// The scope's key, made and stored on first use.
fn key_or_new(vault: &dyn Vault, scope: &Scope) -> Result<Zeroizing<[u8; 32]>, HistoryFailure> {
    if let Some(key) = key(vault, scope)? {
        return Ok(key);
    }
    let mut fresh = Zeroizing::new([0u8; 32]);
    getrandom::fill(&mut *fresh).map_err(|_| HistoryFailure::Unavailable)?;
    vault.put(&scope.key_record, &*fresh)?;
    Ok(fresh)
}

fn open_record(key: &[u8; 32], scope: &Scope, id: &str) -> Result<Record, HistoryFailure> {
    let path = scope.path(id).ok_or(HistoryFailure::Missing)?;
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(HistoryFailure::Missing),
        Err(_) => return Err(HistoryFailure::Unavailable),
    };
    if !meta.is_file() || meta.len() > (MAX_RECORD_BYTES + 64) as u64 {
        return Err(HistoryFailure::Unreadable);
    }
    let sealed = std::fs::read(&path).map_err(|_| HistoryFailure::Unavailable)?;
    let plain = crate::sessions::open(key, &scope.aad(id), &sealed)
        .map(Zeroizing::new)
        .ok_or(HistoryFailure::Unreadable)?;
    let record: Record = serde_json::from_slice(&plain).map_err(|_| HistoryFailure::Unreadable)?;
    if record.schema != SCHEMA || record.id != id || !valid(&record) {
        return Err(HistoryFailure::Unreadable);
    }
    Ok(record)
}

fn valid(record: &Record) -> bool {
    record.title.chars().count() <= MAX_TITLE_CHARS
        && record.entries.len() <= MAX_ENTRIES
        && record.updated >= record.created
}

/// Every record of the scope, newest first. Without a key there is
/// nothing saved (records made under a lost key are unreadable).
pub fn list(vault: &dyn Vault, scope: &Scope) -> Result<Vec<Listed>, HistoryFailure> {
    let ids = scope.ids()?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let key = key(vault, scope)?;
    let mut listed: Vec<(u64, Listed)> = ids
        .into_iter()
        .map(
            |id| match key.as_deref().map(|k| open_record(k, scope, &id)) {
                Some(Ok(record)) => (
                    record.updated,
                    Listed::Saved {
                        id,
                        title: record.title,
                        updated: record.updated,
                        entries: record.entries.len(),
                        binding: record.binding,
                    },
                ),
                _ => (0, Listed::Unreadable { id }),
            },
        )
        .collect();
    listed.sort_by_key(|l| std::cmp::Reverse(l.0));
    Ok(listed.into_iter().map(|(_, l)| l).collect())
}

/// One record, opened.
pub fn read(vault: &dyn Vault, scope: &Scope, id: &str) -> Result<Record, HistoryFailure> {
    let key = key(vault, scope)?.ok_or(HistoryFailure::Unreadable)?;
    open_record(&key, scope, id)
}

/// Saves `record`, replacing its earlier version. A new conversation is
/// refused once the scope holds [`MAX_CONVERSATIONS`]: saved work is never
/// evicted to make room.
pub fn save(vault: &dyn Vault, scope: &Scope, record: &Record) -> Result<(), HistoryFailure> {
    let path = scope.path(&record.id).ok_or(HistoryFailure::Unavailable)?;
    if !valid(record) || record.schema != SCHEMA {
        return Err(HistoryFailure::Unavailable);
    }
    let ids = scope.ids()?;
    if !ids.contains(&record.id) && ids.len() >= MAX_CONVERSATIONS {
        return Err(HistoryFailure::Full);
    }
    let plain =
        Zeroizing::new(serde_json::to_vec(record).map_err(|_| HistoryFailure::Unavailable)?);
    if plain.len() > MAX_RECORD_BYTES {
        return Err(HistoryFailure::TooLarge);
    }
    let key = key_or_new(vault, scope)?;
    let sealed = crate::sessions::seal(&key, &scope.aad(&record.id), &plain)
        .ok_or(HistoryFailure::Unavailable)?;
    crate::sessions::write_private(&path, &sealed).map_err(|_| HistoryFailure::Unavailable)
}

/// Deletes one record.
pub fn delete(scope: &Scope, id: &str) -> Result<(), HistoryFailure> {
    let path = scope.path(id).ok_or(HistoryFailure::Missing)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(HistoryFailure::Missing),
        Err(_) => Err(HistoryFailure::Unavailable),
    }
}

/// Removes a scope's records and its key (its tenant was removed).
pub fn purge(vault: &dyn Vault, scope: &Scope) -> Result<(), HistoryFailure> {
    match std::fs::remove_dir_all(&scope.dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(HistoryFailure::Unavailable),
    }
    match vault.delete(&scope.key_record) {
        Ok(()) | Err(VaultError::NotFound) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// A title from the first message: one line, at most [`MAX_TITLE_CHARS`].
pub fn title(first: &str) -> String {
    let line: String = first
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if line.chars().count() <= MAX_TITLE_CHARS {
        return line;
    }
    let mut short: String = line.chars().take(MAX_TITLE_CHARS - 1).collect();
    short.push('…');
    short
}

/// The transcript as Markdown, plain text or JSON (no replay, no binding
/// secrets: the replay stays in the sealed store).
pub mod export {
    use super::Record;
    use crate::harness::HarnessRole;

    fn speaker(role: HarnessRole) -> &'static str {
        match role {
            HarnessRole::Operator => "You",
            HarnessRole::Assistant => "Harness",
            HarnessRole::Command => "Command",
            HarnessRole::FileChange => "Files changed",
            HarnessRole::Tool => "Desktop tool",
            HarnessRole::Mcp => "MCP tool",
        }
    }

    pub fn markdown(record: &Record) -> String {
        let mut out = format!("# {}\n\n", record.title.replace('\n', " "));
        for entry in &record.entries {
            out.push_str(&format!("## {}\n\n{}\n\n", speaker(entry.role), entry.text));
        }
        out
    }

    pub fn text(record: &Record) -> String {
        let mut out = format!("{}\n\n", record.title);
        for entry in &record.entries {
            out.push_str(&format!("{}:\n{}\n\n", speaker(entry.role), entry.text));
        }
        out
    }

    pub fn json(record: &Record) -> String {
        serde_json::to_string_pretty(&serde_json::json!({
            "title": record.title,
            "created": record.created,
            "updated": record.updated,
            "workspace": record.binding.workspace,
            "endpoint": record.binding.endpoint,
            "entries": record.entries,
        }))
        .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::{EntryState, HarnessRole};
    use crate::vault::SessionVault;

    fn root() -> PathBuf {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("fixture");
        std::env::temp_dir()
            .canonicalize()
            .expect("fixture")
            .join(format!("airs-history-{}", hex::encode(bytes)))
    }

    fn record(id: &str, text: &str) -> Record {
        Record {
            schema: SCHEMA.to_owned(),
            id: id.to_owned(),
            title: title(text),
            created: 1,
            updated: 2,
            binding: SavedBinding {
                tenant: "t".into(),
                workspace: "w".into(),
                credential: "c".into(),
                credential_epoch: 1,
                endpoint: "https://gateway.example".into(),
                source: CredentialSource::WorkspaceKey,
                project: None,
                write: false,
                mcp: Vec::new(),
            },
            origin: None,
            entries: vec![HarnessEntry {
                id: "u-1".into(),
                role: HarnessRole::Operator,
                text: text.to_owned(),
                state: EntryState::Complete,
            }],
            usage: None,
            context: serde_json::json!({"c": 1}),
            replay: serde_json::json!({"r": text}),
        }
    }

    fn id(n: u8) -> String {
        format!("8f0c3a52-5d1e-8b2a-9c4d-1e2f3a4b5c{n:02x}")
    }

    #[test]
    fn a_record_is_sealed_and_opens_only_in_its_own_scope_and_name() {
        let root = root();
        let vault = SessionVault::default();
        let a = Scope::new(&root, "tn-a", "ws-1");
        let b = Scope::new(&root, "tn-a", "ws-2");
        assert_ne!(a.dir, b.dir);
        assert_ne!(a.key_record, b.key_record);
        assert!(a.dir.starts_with(tenant_dir(&root, "tn-a")));
        save(&vault, &a, &record(&id(1), "periwinkle canary")).expect("saved");
        let path = a.path(&id(1)).expect("path");
        let sealed = std::fs::read(&path).expect("sealed");
        assert!(!sealed.windows(10).any(|w| w == b"periwinkle"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            read(&vault, &a, &id(1)).expect("read").entries[0].text,
            "periwinkle canary"
        );
        // The same bytes under another name or in another scope (even with
        // that scope's key equal) do not open.
        std::fs::copy(&path, a.path(&id(2)).expect("path")).expect("copy");
        assert_eq!(
            read(&vault, &a, &id(2)).err(),
            Some(HistoryFailure::Unreadable)
        );
        let key = vault.get(&a.key_record).expect("key");
        vault.put(&b.key_record, &key).expect("key");
        std::fs::create_dir_all(&b.dir).expect("dir");
        std::fs::copy(&path, b.path(&id(1)).expect("path")).expect("copy");
        assert_eq!(
            read(&vault, &b, &id(1)).err(),
            Some(HistoryFailure::Unreadable)
        );
        // A tampered byte does not open; the list names it unreadable.
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        std::fs::write(&path, &tampered).expect("tamper");
        assert!(
            list(&vault, &a)
                .expect("list")
                .contains(&Listed::Unreadable { id: id(1) })
        );
        purge(&vault, &a).expect("purge");
        assert!(!a.dir.exists());
        assert_eq!(vault.get(&a.key_record).err(), Some(VaultError::NotFound));
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn a_full_scope_refuses_new_records_and_ids_are_uuids() {
        let root = root();
        let vault = SessionVault::default();
        let scope = Scope::new(&root, "tn-a", "ws-1");
        for n in 0..MAX_CONVERSATIONS as u8 {
            save(&vault, &scope, &record(&id(n), "x")).expect("saved");
        }
        assert_eq!(
            save(&vault, &scope, &record(&id(99), "x")),
            Err(HistoryFailure::Full)
        );
        // An existing record is still replaced.
        save(&vault, &scope, &record(&id(0), "y")).expect("replaced");
        assert_eq!(list(&vault, &scope).expect("list").len(), MAX_CONVERSATIONS);
        assert!(save(&vault, &scope, &record("../escape", "x")).is_err());
        assert_eq!(
            read(&vault, &scope, "../escape").err(),
            Some(HistoryFailure::Missing)
        );
        let big = Record {
            replay: serde_json::json!({"r": "z".repeat(MAX_RECORD_BYTES)}),
            ..record(&id(0), "x")
        };
        assert_eq!(save(&vault, &scope, &big), Err(HistoryFailure::TooLarge));
        assert_eq!(title(&"w ".repeat(100)).chars().count(), MAX_TITLE_CHARS);
        assert_eq!(title("one\nline\ttitle"), "one line title");
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    #[test]
    fn exports_carry_the_transcript_only() {
        let r = record(&id(1), "periwinkle");
        let json: serde_json::Value = serde_json::from_str(&export::json(&r)).expect("json");
        assert!(json.get("replay").is_none() && json.get("context").is_none());
        assert_eq!(json["entries"][0]["text"], "periwinkle");
        assert!(export::markdown(&r).contains("## You\n\nperiwinkle"));
        assert!(!export::text(&r).contains("\"r\""));
    }
}
