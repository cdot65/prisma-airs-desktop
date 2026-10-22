//! Rust-owned application storage for nonsecret desktop state.
//!
//! One versioned JSON document holds the appearance preference, tenant
//! profiles, the active tenant, the Keychain credential index (record IDs and
//! their bound destinations, never secrets) and the pending cleanup ledger.
//! Session-only credentials are never written. The file store uses an
//! owner-only directory and file, atomic replacement (temporary file, fsync,
//! rename, directory fsync) and an exclusive lock file held for the store's
//! lifetime, so a second instance cannot interleave writes. An unreadable
//! document is reported as corrupt and left untouched until the operator
//! explicitly resets local data.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::tenants::{TenantId, TenantProfile};
use crate::vault::RecordId;
use crate::wire::Theme;

/// Schema of the persisted state document.
pub const STATE_SCHEMA: &str = "prisma-airs-desktop-v2/state/1";
/// Ceiling for the persisted document.
pub const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;
const STATE_FILE: &str = "state.json";
const LOCK_FILE: &str = "state.lock";

/// Which capability a credential authorizes (D013: a management
/// registration never supplies scan authentication, nor the reverse).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum CredentialClass {
    /// The tenant's AIRS runtime scan key.
    #[default]
    Scan,
    /// The tenant's management OAuth client secret (C08).
    Management,
    /// A Gateway workspace inference key (C18): used only for that
    /// workspace's inference, sent only to its runtime endpoint.
    Inference,
}

/// A Keychain credential the core may use after restart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredCredential {
    pub record: RecordId,
    pub tenant: TenantId,
    /// The approved origin this credential may be sent to (the scan origin,
    /// or the management OAuth token origin).
    pub destination: String,
    /// The capability it authorizes (scan when absent, as before C08).
    #[serde(default)]
    pub class: CredentialClass,
    /// The management OAuth client ID (management credentials only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The workspace and runtime endpoint an inference key is for
    /// (inference credentials only; the destination is that endpoint).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inference: Option<crate::gateway_inference::InferenceTarget>,
}

/// The signed-in Academy learner account (nonsecret; the refresh token is in
/// the learner Keychain namespace under `record`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredLearner {
    pub account_id: String,
    pub display_name: String,
    pub record: RecordId,
    /// The issuer and public client the session was granted by.
    pub issuer: String,
    pub client_id: String,
}

/// The Gateway workspace a tenant last chose (C16; nonsecret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayPreference {
    pub tenant: TenantId,
    pub workspace: String,
}

/// A tenant's company sign-in for the Harness Gateway credential (C20;
/// nonsecret: the refresh token is in the Keychain under `grant.record`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredSignIn {
    pub tenant: TenantId,
    /// The Gateway workspace and runtime endpoint the token is used for.
    pub target: crate::gateway_inference::InferenceTarget,
    pub issuer: String,
    pub client_id: String,
    pub audience: String,
    /// The Harness uses this sign-in rather than the workspace key.
    #[serde(default)]
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<StoredGrant>,
}

/// A signed-in account: the stable grant a conversation is bound to.
/// Renewals keep it; signing out ends it and a new sign-in is a new grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredGrant {
    pub id: String,
    pub subject: String,
    pub label: String,
    pub record: RecordId,
}

/// A tenant's Harness project folder (C21; nonsecret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredProject {
    pub tenant: TenantId,
    /// Canonical absolute path.
    pub path: String,
    /// Changes in it are allowed, each with the operator's approval.
    pub write: bool,
}

/// A workspace MCP server a tenant uses with the Harness (C22; nonsecret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredMcpServer {
    pub tenant: TenantId,
    /// The Gateway workspace it is published in.
    pub workspace: String,
    pub server_id: String,
    /// On for new conversations (at most eight per workspace).
    pub enabled: bool,
    pub grant: Option<StoredMcpGrant>,
}

/// A sign-in to a workspace MCP server: bound to the server's URL and
/// catalog revision when it was made. Its tokens are a Keychain item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StoredMcpGrant {
    /// A UUID made for this sign-in (a new sign-in is a new grant).
    pub id: String,
    pub url: String,
    pub revision: String,
    /// The client registered for this sign-in (public).
    pub client_id: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub record: RecordId,
}

/// The persisted document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersistedState {
    pub schema: String,
    pub appearance: Theme,
    pub tenants: Vec<TenantProfile>,
    pub active_tenant: Option<TenantId>,
    pub credentials: Vec<StoredCredential>,
    /// Keychain records to delete (replaced, signed out, orphaned or reset).
    pub pending_cleanup: Vec<RecordId>,
    /// The Academy learner account, if signed in (C04; absent before).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learner: Option<StoredLearner>,
    /// Learner-namespace records to delete (write-ahead entries, replaced,
    /// signed out or abandoned), drained only after a confirmed save.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub learner_cleanup: Vec<RecordId>,
    /// Each tenant's chosen Gateway workspace (C16; absent before).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gateway_workspaces: Vec<GatewayPreference>,
    /// Company sign-ins for the Harness (C20; absent before).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harness_sign_ins: Vec<StoredSignIn>,
    /// Workspaces whose protected Harness history may exist (C20), so a
    /// tenant's removal also removes their keys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harness_history: Vec<GatewayPreference>,
    /// Each tenant's Harness project (C21; absent before).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harness_projects: Vec<StoredProject>,
    /// Workspace MCP servers and their sign-ins (C22; absent before).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harness_mcp: Vec<StoredMcpServer>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            schema: STATE_SCHEMA.to_owned(),
            appearance: Theme::System,
            tenants: Vec::new(),
            active_tenant: None,
            credentials: Vec::new(),
            pending_cleanup: Vec::new(),
            learner: None,
            learner_cleanup: Vec::new(),
            gateway_workspaces: Vec::new(),
            harness_sign_ins: Vec::new(),
            harness_history: Vec::new(),
            harness_projects: Vec::new(),
            harness_mcp: Vec::new(),
        }
    }
}

impl PersistedState {
    /// Structural checks beyond the schema: unique IDs and references to
    /// existing tenants with valid profiles.
    pub fn is_consistent(&self) -> bool {
        let mut ids = BTreeSet::new();
        let tenants_ok = self.tenants.iter().all(|t| {
            ids.insert(t.id.clone())
                && crate::tenants::ProfileDraft {
                    label: t.label.clone(),
                    tsg_id: t.tsg_id.clone(),
                    destination: t.destination.clone(),
                }
                .validate()
                .is_some_and(|d| {
                    d.label == t.label && d.tsg_id == t.tsg_id && d.destination == t.destination
                })
        });
        let mut records = BTreeSet::new();
        let credentials_ok = self.credentials.iter().all(|c| {
            records.insert(c.record.clone())
                && match c.class {
                    CredentialClass::Scan => {
                        c.client_id.is_none()
                            && c.inference.is_none()
                            && self.tenants.iter().any(|t| {
                                t.id == c.tenant && t.destination.origin() == c.destination
                            })
                    }
                    // A management credential needs the tenant's TSG, its
                    // client ID and an exact HTTPS token origin.
                    CredentialClass::Management => {
                        c.inference.is_none()
                            && c.client_id
                                .as_deref()
                                .is_some_and(crate::management::client_id_is_valid)
                            && crate::management::is_origin(&c.destination)
                            && self
                                .tenants
                                .iter()
                                .any(|t| t.id == c.tenant && t.tsg_id.is_some())
                    }
                    // An inference key names its workspace and the HTTPS
                    // runtime endpoint it is sent to.
                    CredentialClass::Inference => {
                        c.client_id.is_none()
                            && c.inference
                                .as_ref()
                                .is_some_and(|t| t.is_valid() && t.endpoint == c.destination)
                            && self.tenants.iter().any(|t| t.id == c.tenant)
                    }
                }
        });
        let one_per_tenant = {
            let mut seen = BTreeSet::new();
            self.credentials
                .iter()
                .all(|c| seen.insert((c.tenant.clone(), c.class)))
        };
        let learner_ok = self.learner.as_ref().is_none_or(|l| {
            l.account_id.starts_with("user_")
                && !l.display_name.trim().is_empty()
                && crate::academy::origin_of(&l.issuer).as_deref() == Some(l.issuer.as_str())
                && !l.client_id.is_empty()
        });
        // One remembered Gateway workspace per existing tenant.
        let mut chose = BTreeSet::new();
        let gateway_ok = self.gateway_workspaces.iter().all(|p| {
            ids.contains(&p.tenant)
                && chose.insert(p.tenant.clone())
                && crate::gateway::is_id(&p.workspace)
        });
        // One company sign-in per existing tenant, with a valid target and
        // configuration and a UUID grant.
        let mut signed = BTreeSet::new();
        let sign_ins_ok = self.harness_sign_ins.iter().all(|s| {
            ids.contains(&s.tenant)
                && signed.insert(s.tenant.clone())
                && s.target.is_valid()
                && crate::harness::identity::SsoConfig::parse(&s.issuer, &s.client_id, &s.audience)
                    .is_some_and(|c| c.issuer == s.issuer)
                && s.grant.as_ref().is_none_or(|g| {
                    crate::harness::protocol::is_uuid(&g.id)
                        && !g.subject.is_empty()
                        && g.subject.len() <= 255
                        && g.label.len() <= 256
                })
        });
        let history_ok = self
            .harness_history
            .iter()
            .all(|p| ids.contains(&p.tenant) && crate::gateway::is_id(&p.workspace));
        let mut projected = BTreeSet::new();
        let projects_ok = self.harness_projects.iter().all(|p| {
            ids.contains(&p.tenant)
                && projected.insert(p.tenant.clone())
                && p.path.starts_with('/')
                && p.path.len() <= 4096
                && !p.path.chars().any(char::is_control)
        });
        // One entry per tenant, workspace and server; at most eight on per
        // workspace; a grant names its URL, endpoints and record exactly.
        let mut servers = BTreeSet::new();
        let mut grant_records = BTreeSet::new();
        let mcp_ok = self.harness_mcp.iter().all(|m| {
            ids.contains(&m.tenant)
                && crate::gateway::is_id(&m.workspace)
                && crate::harness::mcp::supported_id(&m.server_id)
                && servers.insert((m.tenant.clone(), m.workspace.clone(), m.server_id.clone()))
                && m.grant.as_ref().is_none_or(|g| {
                    let origin = |u: &str| {
                        url::Url::parse(u)
                            .ok()
                            .map(|u| u.origin().ascii_serialization())
                    };
                    crate::harness::protocol::is_uuid(&g.id)
                        && crate::harness::mcp::canonical_url(&g.url).as_deref()
                            == Some(g.url.as_str())
                        && g.revision.len() == 64
                        && g.revision.bytes().all(|b| b.is_ascii_hexdigit())
                        && !g.client_id.is_empty()
                        && g.client_id.len() <= 256
                        && g.client_id.bytes().all(|b| b.is_ascii_graphic())
                        && origin(&g.token_endpoint).is_some()
                        && origin(&g.token_endpoint) == origin(&g.url)
                        && g.revocation_endpoint
                            .as_deref()
                            .is_none_or(|r| origin(r) == origin(&g.url))
                        && grant_records.insert(g.record.clone())
                })
        }) && {
            let mut per_workspace = std::collections::BTreeMap::new();
            self.harness_mcp.iter().filter(|m| m.enabled).for_each(|m| {
                *per_workspace
                    .entry((&m.tenant, &m.workspace))
                    .or_insert(0usize) += 1
            });
            per_workspace
                .values()
                .all(|&n| n <= crate::harness::mcp::MAX_SERVERS)
        };
        self.schema == STATE_SCHEMA
            && mcp_ok
            && projects_ok
            && sign_ins_ok
            && history_ok
            && learner_ok
            && gateway_ok
            && tenants_ok
            && credentials_ok
            && one_per_tenant
            && self.active_tenant.as_ref().is_none_or(|a| ids.contains(a))
    }
}

/// Why the store could not be used. Sanitized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    #[error("local desktop data is unreadable")]
    Corrupt,
    /// Nothing was changed (the failure happened before the replacement).
    #[error("local desktop data is unavailable")]
    Unavailable,
    /// The document was replaced (or removed) but the directory could not be
    /// synced, so its durability across a crash is not confirmed. The change
    /// is visible and is not rolled back.
    #[error("local desktop data was written but not confirmed durable")]
    Unconfirmed,
}

/// Failure points that storage fault tests can inject (fixture builds only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreFault {
    /// Fail after writing the temporary file, before the rename.
    BeforeRename,
    /// Fail after the rename, at the directory sync.
    AfterRename,
}

/// Persistence for [`PersistedState`].
pub trait AppStore: Send + Sync + 'static {
    /// The saved document, or `None` before the first save.
    fn load(&self) -> Result<Option<PersistedState>, StoreError>;
    /// Atomically replaces the saved document.
    fn save(&self, state: &PersistedState) -> Result<(), StoreError>;
    /// Removes the saved document (explicit operator reset).
    fn reset(&self) -> Result<(), StoreError>;
}

/// File-backed store under the v2 data root.
pub struct FileStore {
    dir: PathBuf,
    // Held for the store's lifetime: the exclusive cross-process lease.
    _lock: File,
    #[cfg(any(test, feature = "test-fixtures"))]
    fault: std::sync::Mutex<Option<StoreFault>>,
}

impl FileStore {
    /// Opens (creating) the owner-only data directory and takes its lease.
    pub fn open(dir: &Path) -> Result<Self, StoreError> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .map_err(|_| StoreError::Unavailable)?;
        let meta = fs::symlink_metadata(dir).map_err(|_| StoreError::Unavailable)?;
        if !meta.is_dir() {
            return Err(StoreError::Unavailable);
        }
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|_| StoreError::Unavailable)?;
        let lock = open_owner_only(&dir.join(LOCK_FILE), false)?;
        lock.try_lock().map_err(|_| StoreError::Unavailable)?;
        Ok(Self {
            dir: dir.to_owned(),
            _lock: lock,
            #[cfg(any(test, feature = "test-fixtures"))]
            fault: std::sync::Mutex::new(None),
        })
    }

    fn path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn inject(&self, fault: Option<StoreFault>) {
        if let Ok(mut f) = self.fault.lock() {
            *f = fault;
        }
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    fn faulted(&self, point: StoreFault) -> bool {
        self.fault
            .lock()
            .map(|f| *f == Some(point))
            .unwrap_or(false)
    }

    #[cfg(not(any(test, feature = "test-fixtures")))]
    fn faulted(&self, _: StoreFault) -> bool {
        false
    }

    fn sync_dir(&self) -> Result<(), StoreError> {
        if self.faulted(StoreFault::AfterRename) {
            return Err(StoreError::Unconfirmed);
        }
        File::open(&self.dir)
            .and_then(|d| d.sync_all())
            .map_err(|_| StoreError::Unconfirmed)
    }
}

fn open_owner_only(path: &Path, truncate: bool) -> Result<File, StoreError> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(truncate)
        .mode(0o600)
        .custom_flags(libc_nofollow())
        .open(path)
        .map_err(|_| StoreError::Unavailable)
}

/// `O_NOFOLLOW` (macOS `<sys/fcntl.h>` value): never open through a planted
/// symlink. The desktop targets macOS only.
#[cfg(target_os = "macos")]
fn libc_nofollow() -> i32 {
    0x0100
}

impl AppStore for FileStore {
    fn load(&self) -> Result<Option<PersistedState>, StoreError> {
        let path = self.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(StoreError::Unavailable),
        };
        if !meta.is_file() || meta.len() > MAX_STATE_BYTES {
            return Err(StoreError::Corrupt);
        }
        let mut bytes = Vec::new();
        File::open(&path)
            .and_then(|f| f.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|_| StoreError::Unavailable)?;
        let state: PersistedState =
            serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt)?;
        if !state.is_consistent() {
            return Err(StoreError::Corrupt);
        }
        Ok(Some(state))
    }

    fn save(&self, state: &PersistedState) -> Result<(), StoreError> {
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| StoreError::Unavailable)?;
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).map_err(|_| StoreError::Unavailable)?;
        let temp = self
            .dir
            .join(format!(".{STATE_FILE}.{}", hex::encode(entropy)));
        // Before the rename nothing has changed (Unavailable); after it the
        // new document is in place and only its durability is unconfirmed.
        let replaced = (|| {
            let mut file = open_owner_only(&temp, true)?;
            file.write_all(&bytes)
                .map_err(|_| StoreError::Unavailable)?;
            file.sync_all().map_err(|_| StoreError::Unavailable)?;
            if self.faulted(StoreFault::BeforeRename) {
                return Err(StoreError::Unavailable);
            }
            fs::rename(&temp, self.path()).map_err(|_| StoreError::Unavailable)
        })();
        if replaced.is_err() {
            let _ = fs::remove_file(&temp);
            return replaced;
        }
        self.sync_dir()
    }

    fn reset(&self) -> Result<(), StoreError> {
        match fs::remove_file(self.path()) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(StoreError::Unavailable),
        }
        self.sync_dir()
    }
}

/// The store used when the data directory cannot be opened (for example
/// another instance holds its lease): every call reports `Unavailable`, so
/// the snapshot says so and nothing is written elsewhere.
pub struct UnavailableStore;

impl AppStore for UnavailableStore {
    fn load(&self) -> Result<Option<PersistedState>, StoreError> {
        Err(StoreError::Unavailable)
    }

    fn save(&self, _: &PersistedState) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }

    fn reset(&self) -> Result<(), StoreError> {
        Err(StoreError::Unavailable)
    }
}

/// In-memory store for builds and tests that must not touch the disk.
#[derive(Default)]
pub struct MemoryStore {
    state: std::sync::Mutex<Option<PersistedState>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl AppStore for MemoryStore {
    fn load(&self) -> Result<Option<PersistedState>, StoreError> {
        self.state
            .lock()
            .map(|s| s.clone())
            .map_err(|_| StoreError::Unavailable)
    }

    fn save(&self, state: &PersistedState) -> Result<(), StoreError> {
        *self.state.lock().map_err(|_| StoreError::Unavailable)? = Some(state.clone());
        Ok(())
    }

    fn reset(&self) -> Result<(), StoreError> {
        *self.state.lock().map_err(|_| StoreError::Unavailable)? = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tenants::{Destination, Region};

    fn scratch(name: &str) -> PathBuf {
        let mut entropy = [0u8; 6];
        getrandom::fill(&mut entropy).expect("entropy");
        std::env::temp_dir().join(format!("airs-v2-store-{name}-{}", hex::encode(entropy)))
    }

    fn sample() -> PersistedState {
        let id = TenantId::parse("tn-0123456789abcdef").expect("id");
        PersistedState {
            appearance: Theme::Dark,
            tenants: vec![TenantProfile {
                id: id.clone(),
                label: "Prod".into(),
                tsg_id: Some("1234".into()),
                destination: Destination::Region {
                    region: Region::Europe,
                },
            }],
            active_tenant: Some(id.clone()),
            credentials: vec![StoredCredential {
                record: RecordId::parse("cr-0123456789abcdef").expect("record"),
                tenant: id,
                destination: Region::Europe.origin().into(),
                class: CredentialClass::Scan,
                client_id: None,
                inference: None,
            }],
            ..PersistedState::default()
        }
    }

    #[test]
    fn saves_atomically_with_owner_only_modes_and_reloads() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("roundtrip");
        let store = FileStore::open(&dir).expect("open");
        assert_eq!(store.load(), Ok(None));
        store.save(&sample()).expect("save");
        assert_eq!(store.load(), Ok(Some(sample())));
        let mode = |p: &Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join(STATE_FILE)), 0o600);
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "no temporary files remain");
        store.reset().expect("reset");
        assert_eq!(store.load(), Ok(None));
        drop(store);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_second_instance_cannot_take_the_lease() {
        let dir = scratch("lease");
        let first = FileStore::open(&dir).expect("open");
        assert_eq!(FileStore::open(&dir).err(), Some(StoreError::Unavailable));
        drop(first);
        assert!(FileStore::open(&dir).is_ok(), "released with the store");
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn unreadable_or_inconsistent_documents_are_corrupt_and_untouched() {
        let dir = scratch("corrupt");
        let store = FileStore::open(&dir).expect("open");
        for bytes in [
            b"{not json".to_vec(),
            serde_json::to_vec(&PersistedState {
                schema: "other".into(),
                ..sample()
            })
            .expect("json"),
            serde_json::to_vec(&PersistedState {
                active_tenant: TenantId::parse("tn-ffffffffffffffff"),
                ..sample()
            })
            .expect("json"),
            br#"{"schema":"prisma-airs-desktop-v2/state/1","appearance":"dark","tenants":[],"activeTenant":null,"credentials":[],"pendingCleanup":[],"secret":"x"}"#.to_vec(),
        ] {
            fs::write(dir.join(STATE_FILE), &bytes).expect("write");
            assert_eq!(store.load(), Err(StoreError::Corrupt));
            assert_eq!(fs::read(dir.join(STATE_FILE)).expect("read"), bytes, "left untouched");
        }
        drop(store);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn failures_before_the_rename_change_nothing_and_after_it_are_unconfirmed() {
        let dir = scratch("faults");
        let store = FileStore::open(&dir).expect("open");
        store.save(&PersistedState::default()).expect("baseline");
        store.inject(Some(StoreFault::BeforeRename));
        assert_eq!(store.save(&sample()), Err(StoreError::Unavailable));
        assert_eq!(
            store.load(),
            Ok(Some(PersistedState::default())),
            "old document kept"
        );
        store.inject(Some(StoreFault::AfterRename));
        assert_eq!(store.save(&sample()), Err(StoreError::Unconfirmed));
        assert_eq!(
            store.load(),
            Ok(Some(sample())),
            "the new document is in place"
        );
        store.inject(None);
        let leftovers = fs::read_dir(&dir)
            .expect("dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .count();
        assert_eq!(leftovers, 0, "no temporary files remain");
        drop(store);
        fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_planted_symlink_is_not_written_through() {
        let dir = scratch("symlink");
        let store = FileStore::open(&dir).expect("open");
        let outside = scratch("outside");
        fs::write(&outside, b"keep").expect("write");
        std::os::unix::fs::symlink(&outside, dir.join(STATE_FILE)).expect("symlink");
        assert_eq!(store.load(), Err(StoreError::Corrupt), "not a regular file");
        store
            .save(&sample())
            .expect("rename replaces the link itself");
        assert_eq!(fs::read(&outside).expect("read"), b"keep");
        drop(store);
        fs::remove_dir_all(&dir).expect("cleanup");
        fs::remove_file(&outside).expect("cleanup");
    }
}
