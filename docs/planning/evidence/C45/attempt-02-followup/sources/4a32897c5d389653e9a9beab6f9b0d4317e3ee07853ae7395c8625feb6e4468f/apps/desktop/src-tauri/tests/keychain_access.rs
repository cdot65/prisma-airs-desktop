//! Native Keychain adapter qualification on this Mac (classic SecKeychain
//! APIs on the running macOS). Each test uses its own disposable file
//! keychain with synthetic secrets and OS prompts disabled; the login
//! Keychain, the user's keychain search list and v1 items are never touched.
#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::Command;

use desktop_core::CredentialStore;
use desktop_core::vault::{RecordId, Vault, VaultError};
use desktop_host::keychain::{FIXTURE_SCAN_SERVICE, Interaction, KeychainStore, SCAN_SERVICE};

const PASSWORD: &str = "synthetic-test-keychain-password";
const V1_SERVICE: &str = "io.cdot.prisma-airs-desktop";

struct TestKeychain {
    path: PathBuf,
}

impl TestKeychain {
    fn new(name: &str) -> Self {
        let mut entropy = [0u8; 6];
        getrandom::fill(&mut entropy).expect("entropy");
        let hex: String = entropy.iter().map(|b| format!("{b:02x}")).collect();
        Self {
            path: std::env::temp_dir().join(format!("airs-v2-{name}-{hex}.keychain-db")),
        }
    }

    fn store(&self) -> KeychainStore {
        KeychainStore::file(&self.path, PASSWORD, SCAN_SERVICE, Interaction::Disallowed)
            .expect("keychain")
    }

    fn security(&self, args: &[&str]) -> std::process::Output {
        let mut all: Vec<&str> = args.to_vec();
        all.push(self.path.to_str().expect("path"));
        Command::new("/usr/bin/security")
            .args(&all)
            .output()
            .expect("security")
    }

    /// Item attributes only (no secret), as the `security` tool sees them.
    fn exists(&self, service: &str, account: &str) -> bool {
        self.security(&["find-generic-password", "-s", service, "-a", account])
            .status
            .success()
    }
}

impl Drop for TestKeychain {
    fn drop(&mut self) {
        let _ = Command::new("/usr/bin/security")
            .args(["delete-keychain", self.path.to_str().unwrap_or_default()])
            .output();
        let _ = std::fs::remove_file(&self.path);
    }
}

fn search_list() -> String {
    let out = Command::new("/usr/bin/security")
        .args(["list-keychains", "-d", "user"])
        .output()
        .expect("list");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn record() -> RecordId {
    let mut entropy = [0u8; 8];
    getrandom::fill(&mut entropy).expect("entropy");
    RecordId::parse(&format!(
        "cr-{}",
        entropy
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
    .expect("id")
}

#[test]
fn records_round_trip_in_the_v2_namespace_without_touching_the_search_list() {
    let before = search_list();
    let keychain = TestKeychain::new("roundtrip");
    let store = keychain.store();
    let id = record();
    store.put(&id, b"synthetic-scan-key").expect("put");
    assert_eq!(
        store.get(&id).expect("get").as_slice(),
        b"synthetic-scan-key"
    );
    assert!(
        keychain.exists(SCAN_SERVICE, id.as_str()),
        "service = v2 namespace, account = record ID"
    );
    assert!(!keychain.exists(FIXTURE_SCAN_SERVICE, id.as_str()));
    store.delete(&id).expect("delete");
    assert_eq!(store.get(&id).err(), Some(VaultError::NotFound));
    assert_eq!(store.delete(&id).err(), Some(VaultError::NotFound));
    assert!(!keychain.exists(SCAN_SERVICE, id.as_str()));
    drop(store);
    drop(keychain);
    assert_eq!(
        search_list(),
        before,
        "the user's keychain search list is unchanged"
    );
}

#[test]
fn a_locked_keychain_reports_locked_and_recovers_after_unlock() {
    let keychain = TestKeychain::new("locked");
    let store = keychain.store();
    let id = record();
    store.put(&id, b"synthetic-scan-key").expect("put");
    assert!(keychain.security(&["lock-keychain"]).status.success());
    assert_eq!(store.get(&id).err(), Some(VaultError::Locked));
    assert_eq!(store.put(&record(), b"x").err(), Some(VaultError::Locked));
    let reopened = keychain.store();
    assert_eq!(
        reopened.get(&id).expect("unlocked").as_slice(),
        b"synthetic-scan-key"
    );
}

#[test]
fn an_item_this_process_is_not_trusted_for_is_denied_not_locked() {
    let keychain = TestKeychain::new("denied");
    let store = keychain.store();
    let id = record();
    // Created by the `security` tool with its own default ACL: this test
    // binary is not a trusted application for it.
    let added = keychain.security(&[
        "add-generic-password",
        "-s",
        SCAN_SERVICE,
        "-a",
        id.as_str(),
        "-w",
        "synthetic-foreign",
    ]);
    assert!(
        added.status.success(),
        "{}",
        String::from_utf8_lossy(&added.stderr)
    );
    assert_eq!(store.get(&id).err(), Some(VaultError::Denied));
    assert!(
        keychain.exists(SCAN_SERVICE, id.as_str()),
        "the item is untouched"
    );
}

#[test]
fn purge_removes_only_the_v2_namespace() {
    let keychain = TestKeychain::new("purge");
    let store = keychain.store();
    let ours = [record(), record()];
    for id in &ours {
        store.put(id, b"synthetic").expect("put");
    }
    let v1 = keychain.security(&[
        "add-generic-password",
        "-s",
        V1_SERVICE,
        "-a",
        "credentials-v1",
        "-w",
        "synthetic-v1",
        "-A",
    ]);
    assert!(v1.status.success());
    let fixture = KeychainStore::file(
        &keychain.path,
        PASSWORD,
        FIXTURE_SCAN_SERVICE,
        Interaction::Disallowed,
    )
    .expect("fixture");
    let other = record();
    fixture.put(&other, b"synthetic-other").expect("put");
    let v1_before = keychain.security(&[
        "find-generic-password",
        "-s",
        V1_SERVICE,
        "-a",
        "credentials-v1",
        "-w",
    ]);
    store.purge().expect("purge");
    for id in &ours {
        assert!(!keychain.exists(SCAN_SERVICE, id.as_str()));
    }
    assert!(
        keychain.exists(FIXTURE_SCAN_SERVICE, other.as_str()),
        "another namespace is untouched"
    );
    let v1_after = keychain.security(&[
        "find-generic-password",
        "-s",
        V1_SERVICE,
        "-a",
        "credentials-v1",
        "-w",
    ]);
    assert!(v1_after.status.success());
    assert_eq!(
        v1_after.stdout, v1_before.stdout,
        "the v1 item is byte-identical"
    );
    store.purge().expect("an empty namespace purges cleanly");
}

#[test]
fn an_unusable_keychain_location_is_unavailable() {
    // A regular file where a directory is needed: creation cannot succeed
    // (and nothing is created elsewhere).
    let blocker = TestKeychain::new("blocker").path.with_extension("file");
    std::fs::write(&blocker, b"not a directory").expect("write");
    let inside = blocker.join("x.keychain-db");
    assert_eq!(
        KeychainStore::file(&inside, PASSWORD, SCAN_SERVICE, Interaction::Disallowed).err(),
        Some(VaultError::Unavailable)
    );
    std::fs::remove_file(&blocker).expect("cleanup");
}

#[test]
fn the_classic_login_keychain_api_is_available_read_only() {
    // macOS 27 still serves the deprecated classic SecKeychain API for the
    // login keychain; nothing is written to it here.
    let login = KeychainStore::login().expect("the login keychain opens");
    let _ = login.is_locked();
    let absent = record();
    assert_eq!(
        login.get(&absent).err(),
        Some(VaultError::NotFound),
        "a search in the v2 namespace answers"
    );
}
