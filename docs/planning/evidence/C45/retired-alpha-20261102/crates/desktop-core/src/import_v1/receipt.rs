//! Import receipts: an import's commit record, kept beside v2's state
//! (`<data root>/imports/<id>.json`, 0600, replaced atomically with the file
//! and its directory synced).
//!
//! Every item is noted `writing` (with what it is about to add) before its
//! owner write and `applied` (with what it added) after; the receipt noted
//! `applied` commits the import. One the app ended during is rolled back at
//! the next launch, and can then be imported again (D050).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::Category;

pub const RECEIPT_SCHEMA: &str = "prisma-airs-desktop-v2/import-receipt/1";
/// Receipts kept; a new import waits until one is forgotten.
pub const MAX_RECEIPTS: usize = 8;
const RECEIPT_LIMIT: u64 = 4 * 1024 * 1024;

/// `imp-` and 16 hex digits.
pub fn new_id() -> Option<String> {
    let mut entropy = [0u8; 8];
    getrandom::fill(&mut entropy).ok()?;
    Some(format!("imp-{}", hex::encode(entropy)))
}

pub fn id_ok(id: &str) -> bool {
    id.strip_prefix("imp-")
        .is_some_and(|h| h.len() == 16 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReceiptState {
    Applying,
    Applied,
    RollingBack,
    RolledBack,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    Planned,
    /// Its owner write may have happened (a crash here is reconciled).
    Writing,
    Applied,
    /// Its write failed; the import was rolled back.
    Failed,
    Reverting,
    Reverted,
    /// Changed in v2 after the import: kept.
    KeptChanged,
    /// Removed in v2 since the import.
    Gone,
}

/// What one item wrote, with the content hash it wrote: a rollback removes
/// it only while v2 still holds exactly that.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Written {
    Tenant {
        id: String,
        hash: String,
    },
    Appearance {
        before: crate::wire::Theme,
        after: crate::wire::Theme,
    },
    Case {
        tenant: String,
        id: String,
        hash: String,
    },
    Report {
        tenant: String,
        scan_id: String,
        sub: u32,
        hash: String,
    },
    Draft {
        account: String,
        attempt: String,
        hash: String,
    },
    Recent {
        root: PathBuf,
        id: String,
    },
    Conversation {
        tenant: String,
        workspace: String,
        id: String,
        hash: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReceiptItem {
    pub index: u32,
    pub category: Category,
    pub label: String,
    /// The v2 tenant it writes into (pre-issued for a new tenant).
    pub target: Option<String>,
    /// The v1 tenant it was read for (a resume reads it again).
    pub source_tenant: Option<String>,
    pub source_hash: String,
    pub state: ItemState,
    /// What it wrote (confirmed by its owner).
    pub written: Vec<Written>,
    /// What it was about to write when noted `writing`: an interruption
    /// there leaves these unconfirmed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub intended: Vec<Written>,
    /// Entries a rollback kept because v2 changed them (or they are gone).
    #[serde(default)]
    pub kept: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub id: String,
    /// Seconds since 1970.
    pub created: u64,
    pub state: ReceiptState,
    /// The categories the operator selected (a resume reads the same).
    #[serde(default)]
    pub selected: Vec<Category>,
    /// It never committed (it failed, or the app ended during it) and was
    /// rolled back (D050); importing it again reads v1 again.
    #[serde(default)]
    pub recovered: bool,
    pub items: Vec<ReceiptItem>,
}

impl Receipt {
    pub fn terminal(&self) -> bool {
        matches!(self.state, ReceiptState::Applied | ReceiptState::RolledBack)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReceiptFailure {
    /// Writing failed; the previous receipt is unchanged.
    Unavailable,
    /// Written, but the directory sync failed: durability unconfirmed.
    Unconfirmed,
    Corrupt,
}

fn path(root: &Path, id: &str) -> Option<PathBuf> {
    id_ok(id).then(|| root.join(format!("{id}.json")))
}

/// The folder of every receipt's second copy, beside the first (D050): a
/// receipt whose file is damaged, or whose folder cannot be read, is read
/// from it.
pub fn copies(root: &Path) -> PathBuf {
    let mut name = root.as_os_str().to_owned();
    name.push(".copy");
    PathBuf::from(name)
}

/// Writes the receipt durably, in both of its copies, the first before the
/// second: each a temporary file synced and renamed over the old one, then
/// its directory synced. A receipt is recorded only once both are.
pub fn store(root: &Path, receipt: &Receipt) -> Result<(), ReceiptFailure> {
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|_| ReceiptFailure::Unavailable)?;
    write_copy(root, &receipt.id, &bytes)?;
    #[cfg(feature = "test-fixtures")]
    if sync_fault(root) {
        return Err(ReceiptFailure::Unconfirmed);
    }
    write_copy(&copies(root), &receipt.id, &bytes)
}

fn write_copy(root: &Path, id: &str, bytes: &[u8]) -> Result<(), ReceiptFailure> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let target = path(root, id).ok_or(ReceiptFailure::Unavailable)?;
    std::fs::DirBuilder::new()
        .recursive(true)
        .create(root)
        .map_err(|_| ReceiptFailure::Unavailable)?;
    std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
        .map_err(|_| ReceiptFailure::Unavailable)?;
    let mut entropy = [0u8; 8];
    getrandom::fill(&mut entropy).map_err(|_| ReceiptFailure::Unavailable)?;
    let temp = root.join(format!(".{}.tmp", hex::encode(entropy)));
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err(ReceiptFailure::Unavailable);
    }
    std::fs::File::open(root)
        .and_then(|d| d.sync_all())
        .map_err(|_| ReceiptFailure::Unconfirmed)
}

/// Fixture builds: receipt folders whose directory sync fails, after how
/// many more writes (each failing write counts once).
#[cfg(feature = "test-fixtures")]
static SYNC_FAULTS: std::sync::Mutex<Vec<(PathBuf, u32)>> = std::sync::Mutex::new(Vec::new());

/// Makes the directory sync of the receipt write after `after` more writes
/// under `root` fail, and every one after it, until cleared with `None`
/// (fixture builds only).
#[cfg(feature = "test-fixtures")]
pub fn fail_sync(root: &Path, after: Option<u32>) {
    if let Ok(mut faults) = SYNC_FAULTS.lock() {
        faults.retain(|(r, _)| r != root);
        if let Some(n) = after {
            faults.push((root.to_path_buf(), n));
        }
    }
}

#[cfg(feature = "test-fixtures")]
fn sync_fault(root: &Path) -> bool {
    SYNC_FAULTS.lock().is_ok_and(|mut faults| {
        faults
            .iter_mut()
            .find(|(r, _)| r == root)
            .is_some_and(|(_, n)| {
                if *n == 0 {
                    true
                } else {
                    *n -= 1;
                    false
                }
            })
    })
}

/// What the receipt folders hold.
#[derive(Debug, Default)]
pub struct Inventory {
    /// Every receipt read from either copy, oldest first.
    pub receipts: Vec<Receipt>,
    /// Receipts readable in neither copy: reported, never removed.
    pub damaged: Vec<String>,
    /// Neither folder can be listed: what they hold is unknown.
    pub unreadable: bool,
}

impl Inventory {
    /// Something an import wrote may be unaccounted for (D050).
    pub fn uncertain(&self) -> bool {
        self.unreadable || !self.damaged.is_empty()
    }
}

/// What listing a receipt folder gave.
enum Listing {
    /// It does not exist: it holds no receipts.
    Missing,
    Listed(Vec<String>),
    /// It exists but it, or an entry in it, cannot be read.
    Unreadable,
}

fn names(root: &Path) -> Listing {
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Listing::Missing,
        Err(_) => return Listing::Unreadable,
    };
    let mut names = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return Listing::Unreadable;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.strip_suffix(".json").filter(|id| id_ok(id)) {
            names.push(id.to_owned());
        }
    }
    Listing::Listed(names)
}

/// Every receipt, from its first copy or else its second. One readable in
/// neither is reported, never removed; when neither folder can be listed
/// the inventory is unknown.
pub fn load_all(root: &Path) -> Inventory {
    let second = copies(root);
    let (first, other) = (names(root), names(&second));
    let mut ids: Vec<String> = [&first, &other]
        .into_iter()
        .filter_map(|l| match l {
            Listing::Listed(names) => Some(names.iter().cloned()),
            Listing::Missing | Listing::Unreadable => None,
        })
        .flatten()
        .collect();
    ids.sort();
    ids.dedup();
    let mut inventory = Inventory {
        // Each receipt is in both folders, so a folder that cannot be read
        // is stood in for only by the other one listed (a missing one, as
        // with a receipt written before the second copy existed, cannot).
        unreadable: matches!(
            (&first, &other),
            (Listing::Unreadable, Listing::Missing | Listing::Unreadable)
                | (Listing::Missing, Listing::Unreadable)
        ),
        ..Inventory::default()
    };
    for id in ids.into_iter().take(64) {
        match load(root, &id).or_else(|_| load(&second, &id)) {
            Ok(r) => inventory.receipts.push(r),
            Err(_) => inventory.damaged.push(id),
        }
    }
    inventory
        .receipts
        .sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
    inventory
}

/// Whether the saved document's copy of a receipt can stand for its files
/// (D050): the current schema, a valid ID, and not settled, as only an
/// unsettled receipt is kept there. The document fails to load otherwise.
pub fn binding_ok(r: &Receipt) -> bool {
    r.schema == RECEIPT_SCHEMA && id_ok(&r.id) && !r.terminal()
}

pub fn load(root: &Path, id: &str) -> Result<Receipt, ReceiptFailure> {
    let path = path(root, id).ok_or(ReceiptFailure::Corrupt)?;
    let meta = std::fs::symlink_metadata(&path).map_err(|_| ReceiptFailure::Corrupt)?;
    if !meta.is_file() || meta.len() > RECEIPT_LIMIT {
        return Err(ReceiptFailure::Corrupt);
    }
    let bytes = std::fs::read(&path).map_err(|_| ReceiptFailure::Corrupt)?;
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| ReceiptFailure::Corrupt)?;
    if receipt.schema != RECEIPT_SCHEMA || receipt.id != id {
        return Err(ReceiptFailure::Corrupt);
    }
    Ok(receipt)
}

/// Forgets a receipt, both copies (its imported work stays; it can no
/// longer be rolled back).
pub fn forget(root: &Path, id: &str) -> Result<(), ReceiptFailure> {
    for dir in [root.to_path_buf(), copies(root)] {
        let path = path(&dir, id).ok_or(ReceiptFailure::Corrupt)?;
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return Err(ReceiptFailure::Unavailable),
        }
        std::fs::File::open(&dir)
            .and_then(|d| d.sync_all())
            .map_err(|_| ReceiptFailure::Unconfirmed)?;
    }
    Ok(())
}

/// Removes every receipt, both copies (local-data reset).
pub fn purge(root: &Path) -> bool {
    [root.to_path_buf(), copies(root)]
        .iter()
        .all(|dir| match std::fs::remove_dir_all(dir) {
            Ok(()) => true,
            Err(e) => e.kind() == std::io::ErrorKind::NotFound,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipts_round_trip_and_damage_is_reported() {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).expect("entropy");
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("tmp")
            .join(format!("airs-receipts-{}", hex::encode(entropy)));
        let receipt = Receipt {
            schema: RECEIPT_SCHEMA.to_owned(),
            id: new_id().expect("id"),
            created: 1,
            state: ReceiptState::Applying,
            selected: vec![Category::Cases],
            recovered: false,
            items: vec![ReceiptItem {
                index: 0,
                category: Category::Cases,
                label: "Prod".into(),
                target: Some("tn-0123456789abcdef".into()),
                source_tenant: None,
                source_hash: "ab".into(),
                state: ItemState::Writing,
                written: vec![],
                intended: vec![],
                kept: 0,
            }],
        };
        store(&root, &receipt).expect("stored");
        assert_eq!(load(&root, &receipt.id).expect("loaded"), receipt);
        std::fs::write(root.join("imp-00000000000000ff.json"), b"{").expect("write");
        let inventory = load_all(&root);
        assert_eq!((inventory.receipts.len(), inventory.damaged.len()), (1, 1));
        // A damaged first copy is read from the second.
        std::fs::write(root.join(format!("{}.json", receipt.id)), b"{").expect("write");
        assert_eq!(load_all(&root).receipts, vec![receipt.clone()]);
        assert!(load(&root, "../x").is_err());
        forget(&root, &receipt.id).expect("forgot");
        assert!(purge(&root));
    }
}
