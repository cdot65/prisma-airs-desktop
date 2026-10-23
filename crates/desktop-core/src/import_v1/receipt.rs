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

/// Writes the receipt durably: a temporary file synced and renamed over the
/// old one, then the directory synced.
pub fn store(root: &Path, receipt: &Receipt) -> Result<(), ReceiptFailure> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let target = path(root, &receipt.id).ok_or(ReceiptFailure::Unavailable)?;
    let bytes = serde_json::to_vec_pretty(receipt).map_err(|_| ReceiptFailure::Unavailable)?;
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
        file.write_all(&bytes)?;
        file.sync_all()?;
        std::fs::rename(&temp, &target)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
        return Err(ReceiptFailure::Unavailable);
    }
    #[cfg(feature = "test-fixtures")]
    if sync_fault(root) {
        return Err(ReceiptFailure::Unconfirmed);
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

/// Every receipt, oldest first. A damaged one is reported, never removed.
pub fn load_all(root: &Path) -> (Vec<Receipt>, usize) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return (Vec::new(), 0);
    };
    let mut receipts = Vec::new();
    let mut damaged = 0;
    for entry in entries.flatten().take(64) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".json").filter(|id| id_ok(id)) else {
            continue;
        };
        match load(root, id) {
            Ok(r) => receipts.push(r),
            Err(_) => damaged += 1,
        }
    }
    receipts.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
    (receipts, damaged)
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

/// Forgets a receipt (its imported work stays; it can no longer be rolled
/// back).
pub fn forget(root: &Path, id: &str) -> Result<(), ReceiptFailure> {
    let path = path(root, id).ok_or(ReceiptFailure::Corrupt)?;
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(ReceiptFailure::Unavailable),
    }
    std::fs::File::open(root)
        .and_then(|d| d.sync_all())
        .map_err(|_| ReceiptFailure::Unconfirmed)
}

/// Removes every receipt (local-data reset).
pub fn purge(root: &Path) -> bool {
    match std::fs::remove_dir_all(root) {
        Ok(()) => true,
        Err(e) => e.kind() == std::io::ErrorKind::NotFound,
    }
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
        let (all, damaged) = load_all(&root);
        assert_eq!((all.len(), damaged), (1, 1));
        assert!(load(&root, "../x").is_err());
        forget(&root, &receipt.id).expect("forgot");
        assert!(purge(&root));
    }
}
