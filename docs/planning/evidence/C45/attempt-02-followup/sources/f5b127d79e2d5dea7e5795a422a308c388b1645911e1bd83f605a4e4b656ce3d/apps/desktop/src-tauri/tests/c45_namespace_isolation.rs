//! C45 protected-service isolation using one disposable FILE keychain only.
//! No login Keychain, production data, search-list query or live endpoint.
#![cfg(target_os = "macos")]
use desktop_core::CredentialStore;
use desktop_core::vault::{RecordId, Vault, VaultError};
use desktop_host::keychain::{
    ACADEMY_SERVICE, FIXTURE_ACADEMY_SERVICE, FIXTURE_SCAN_SERVICE, Interaction, KeychainStore,
    SCAN_SERVICE,
};
use std::path::PathBuf;
struct FileKeychain(PathBuf);
impl Drop for FileKeychain {
    fn drop(&mut self) {
        let _ = std::process::Command::new("/usr/bin/security")
            .args(["delete-keychain", self.0.to_str().unwrap_or_default()])
            .output();
        let _ = std::fs::remove_file(&self.0);
    }
}
#[test]
fn new_scan_and_academy_services_do_not_read_or_purge_prior_or_other_new_services() {
    let mut entropy = [0u8; 8];
    getrandom::fill(&mut entropy).expect("synthetic identity");
    let file = FileKeychain(std::env::temp_dir().join(format!(
            "airs-desktop-c45-{}.keychain-db",
            entropy
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )));
    let services = [
        "io.cdot.prisma-airs-desktop",
        "io.cdot.prisma-airs-desktop.academy",
        "io.cdot.prisma-airs-desktop.v2.scan",
        "io.cdot.prisma-airs-desktop.v2.academy",
        "io.cdot.prisma-airs-desktop.v2.fixture.scan",
        "io.cdot.prisma-airs-desktop.v2.fixture.academy",
        SCAN_SERVICE,
        ACADEMY_SERVICE,
        FIXTURE_SCAN_SERVICE,
        FIXTURE_ACADEMY_SERVICE,
    ];
    assert_eq!(SCAN_SERVICE, "io.cdot.airs-desktop.scan");
    assert_eq!(ACADEMY_SERVICE, "io.cdot.airs-desktop.academy");
    assert_eq!(FIXTURE_SCAN_SERVICE, "io.cdot.airs-desktop.fixture.scan");
    assert_eq!(
        FIXTURE_ACADEMY_SERVICE,
        "io.cdot.airs-desktop.fixture.academy"
    );
    let stores: Vec<_> = services
        .iter()
        .map(|service| {
            KeychainStore::file(
                &file.0,
                "SYNTHETIC-C45-PRIVATE-FILE",
                service,
                Interaction::Disallowed,
            )
            .expect("private file store")
        })
        .collect();
    let id =
        RecordId::parse("cr-0123456789abcdef").expect("same synthetic account across services");
    for (index, store) in stores.iter().take(6).enumerate() {
        store
            .put(&id, format!("SYNTHETIC-OLD-{index}").as_bytes())
            .expect("seed private canary");
    }
    for store in stores.iter().skip(6) {
        assert_eq!(
            store.get(&id).err(),
            Some(VaultError::NotFound),
            "old-only account cannot be read through a new service"
        );
    }
    for (index, store) in stores.iter().enumerate().skip(6) {
        store
            .put(&id, format!("SYNTHETIC-NEW-{index}").as_bytes())
            .expect("new item");
    }
    for index in 6..stores.len() {
        stores[index].purge().expect("exact service purge");
        assert_eq!(stores[index].get(&id).err(), Some(VaultError::NotFound));
        for (old, store) in stores.iter().take(6).enumerate() {
            assert_eq!(
                store.get(&id).expect("old file canary intact").as_slice(),
                format!("SYNTHETIC-OLD-{old}").as_bytes()
            );
        }
        for (other, store) in stores.iter().enumerate().skip(index + 1) {
            assert_eq!(
                store.get(&id).expect("other new service intact").as_slice(),
                format!("SYNTHETIC-NEW-{other}").as_bytes()
            );
        }
    }
}
