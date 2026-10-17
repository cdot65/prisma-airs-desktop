//! Classic macOS Keychain credential adapter (DESIGN "Native Harness,
//! identity and protected storage").
//!
//! Generic-password items with service = this build's v2 namespace and
//! account = the immutable credential record ID; local, non-synchronizing,
//! default application ACL. Production uses the login Keychain and allows the
//! OS authorization prompts; fixture builds use a dedicated test keychain and
//! the fixture namespace. Every call is serialized (the user-interaction
//! setting is process-global), and OS status codes map to sanitized
//! [`VaultError`]s: a keychain that is locked reports `Locked`, an item the
//! process may not read reports `Denied`. Purge deletes only items in this
//! namespace; v1 and foreign items are never addressed.
//!
//! `SecKeychainItem::delete` in security-framework discards the OS status, and
//! the keychain lock status is not bound, so this module calls those two
//! Security functions directly.
#![allow(unsafe_code)]

use std::path::Path;
use std::sync::Mutex;

use core_foundation::base::TCFType;
use desktop_core::CredentialStore;
use desktop_core::vault::{RecordId, Vault, VaultError};
use security_framework::item::{ItemClass, ItemSearchOptions, Limit, Reference, SearchResult};
use security_framework::os::macos::keychain::{CreateOptions, SecKeychain};
use security_framework::os::macos::keychain_item::SecKeychainItem;
use security_framework_sys::base::SecKeychainRef;

/// Security status code (`OSStatus` in `<MacTypes.h>`).
type OSStatus = i32;
use zeroize::Zeroizing;

/// The production v2 scan-credential namespace (independent of v1's
/// `io.cdot.prisma-airs-desktop`).
pub const SCAN_SERVICE: &str = "io.cdot.prisma-airs-desktop.v2.scan";
/// The fixture build's namespace, used only in its dedicated test keychain.
pub const FIXTURE_SCAN_SERVICE: &str = "io.cdot.prisma-airs-desktop.v2.fixture.scan";

const ERR_SEC_ITEM_NOT_FOUND: OSStatus = -25300;
const ERR_SEC_INTERACTION_NOT_ALLOWED: OSStatus = -25308;
const ERR_SEC_AUTH_FAILED: OSStatus = -25293;
const ERR_SEC_USER_CANCELED: OSStatus = -128;
const ERR_SEC_NO_SUCH_KEYCHAIN: OSStatus = -25294;
const ERR_SEC_INVALID_KEYCHAIN: OSStatus = -25295;
const K_SEC_UNLOCK_STATE_STATUS: u32 = 1;

unsafe extern "C" {
    fn SecKeychainGetStatus(keychain: SecKeychainRef, status: *mut u32) -> OSStatus;
    #[cfg(feature = "test-fixtures")]
    fn SecKeychainLock(keychain: SecKeychainRef) -> OSStatus;
    #[cfg(feature = "test-fixtures")]
    fn SecKeychainUnlock(
        keychain: SecKeychainRef,
        length: u32,
        password: *const std::ffi::c_void,
        use_password: u8,
    ) -> OSStatus;
}

/// Whether the OS may show its own authorization prompts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interaction {
    Allowed,
    Disallowed,
}

static KEYCHAIN_CALLS: Mutex<()> = Mutex::new(());

/// A credential store over one keychain and namespace.
pub struct KeychainStore {
    keychain: SecKeychain,
    service: &'static str,
    interaction: Interaction,
}

// SAFETY (Send/Sync): SecKeychainRef is a Core Foundation object whose
// Security APIs are thread-safe; every call here is additionally serialized.
unsafe impl Send for KeychainStore {}
unsafe impl Sync for KeychainStore {}

impl KeychainStore {
    /// The login Keychain with the production namespace.
    pub fn login() -> Result<Self, VaultError> {
        Ok(Self {
            keychain: SecKeychain::default().map_err(|e| map(e.code(), None))?,
            service: SCAN_SERVICE,
            interaction: Interaction::Allowed,
        })
    }

    /// A dedicated file keychain (fixture builds and tests). Creates it with
    /// `password` when absent and unlocks it.
    pub fn file(
        path: &Path,
        password: &str,
        service: &'static str,
        interaction: Interaction,
    ) -> Result<Self, VaultError> {
        let mut keychain = if path.exists() {
            SecKeychain::open(path).map_err(|e| map(e.code(), None))?
        } else {
            CreateOptions::new()
                .password(password)
                .prompt_user(false)
                .create(path)
                .map_err(|e| map(e.code(), None))?
        };
        keychain
            .unlock(Some(password))
            .map_err(|e| map(e.code(), None))?;
        Ok(Self {
            keychain,
            service,
            interaction,
        })
    }

    pub fn service(&self) -> &'static str {
        self.service
    }

    /// Whether the keychain currently reports itself locked.
    pub fn is_locked(&self) -> bool {
        let mut status = 0u32;
        // SAFETY: the keychain reference is valid for `self`'s lifetime and
        // `status` is a valid out-pointer.
        let code =
            unsafe { SecKeychainGetStatus(self.keychain.as_concrete_TypeRef(), &mut status) };
        code == 0 && status & K_SEC_UNLOCK_STATE_STATUS == 0
    }

    /// Runs `f` serialized, with OS prompts disabled when configured. The
    /// lock state is read after the prompt guard is released: with user
    /// interaction disabled the status call itself fails.
    fn call<T>(&self, f: impl FnOnce() -> Result<T, OSStatus>) -> Result<T, VaultError> {
        let _serial = KEYCHAIN_CALLS.lock().map_err(|_| VaultError::Unavailable)?;
        let result = {
            let _guard = match self.interaction {
                Interaction::Allowed => None,
                Interaction::Disallowed => Some(
                    SecKeychain::disable_user_interaction().map_err(|_| VaultError::Unavailable)?,
                ),
            };
            f()
        };
        result.map_err(|code| map(code, Some(self.is_locked())))
    }

    fn find(&self, record: &RecordId) -> Result<(Zeroizing<Vec<u8>>, SecKeychainItem), OSStatus> {
        self.keychain
            .find_generic_password(self.service, record.as_str())
            .map(|(password, item)| (Zeroizing::new(password.to_vec()), item))
            .map_err(|e| e.code())
    }
}

/// Fixture-only lock control for native acceptance: a running app does not
/// observe a lock made by another process through `security lock-keychain`
/// (observed on macOS 27), so the fixture menu locks and unlocks this
/// process's own keychain reference instead.
#[cfg(feature = "test-fixtures")]
impl KeychainStore {
    pub fn lock_for_fixture(&self) -> bool {
        let _serial = KEYCHAIN_CALLS.lock();
        // SAFETY: the keychain reference is valid for `self`'s lifetime.
        unsafe { SecKeychainLock(self.keychain.as_concrete_TypeRef()) == 0 }
    }

    pub fn unlock_for_fixture(&self, password: &str) -> bool {
        let _serial = KEYCHAIN_CALLS.lock();
        // SAFETY: the reference is valid and the password pointer/length
        // describe a live UTF-8 buffer for the duration of the call.
        unsafe {
            SecKeychainUnlock(
                self.keychain.as_concrete_TypeRef(),
                password.len() as u32,
                password.as_ptr().cast(),
                1,
            ) == 0
        }
    }
}

fn delete_item(item: &SecKeychainItem) -> Result<(), OSStatus> {
    // SAFETY: the item reference is valid while `item` is alive.
    let code = unsafe {
        security_framework_sys::keychain_item::SecKeychainItemDelete(item.as_concrete_TypeRef())
    };
    if code == 0 { Ok(()) } else { Err(code) }
}

/// Maps an OS status to a sanitized vault error. `locked` is the keychain's
/// lock state when known: a locked keychain answers -25308 (prompting not
/// allowed), -25293 (authentication failed, observed on macOS 27 for an
/// already-open keychain locked externally) or -128 (unlock prompt
/// cancelled); the same codes on an unlocked keychain mean the item's ACL
/// denied this process.
fn map(code: OSStatus, locked: Option<bool>) -> VaultError {
    match code {
        ERR_SEC_ITEM_NOT_FOUND => VaultError::NotFound,
        ERR_SEC_INTERACTION_NOT_ALLOWED | ERR_SEC_AUTH_FAILED | ERR_SEC_USER_CANCELED
            if locked == Some(true) =>
        {
            VaultError::Locked
        }
        ERR_SEC_INTERACTION_NOT_ALLOWED | ERR_SEC_AUTH_FAILED | ERR_SEC_USER_CANCELED => {
            VaultError::Denied
        }
        ERR_SEC_NO_SUCH_KEYCHAIN | ERR_SEC_INVALID_KEYCHAIN => VaultError::Unavailable,
        _ => VaultError::Unavailable,
    }
}

impl Vault for KeychainStore {
    fn put(&self, record: &RecordId, secret: &[u8]) -> Result<(), VaultError> {
        self.call(|| {
            self.keychain
                .add_generic_password(self.service, record.as_str(), secret)
                .map_err(|e| e.code())
        })
    }

    fn get(&self, record: &RecordId) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        self.call(|| self.find(record).map(|(secret, _)| secret))
    }

    fn delete(&self, record: &RecordId) -> Result<(), VaultError> {
        self.call(|| {
            let (_, item) = self.find(record)?;
            delete_item(&item)
        })
    }
}

impl CredentialStore for KeychainStore {
    fn purge(&self) -> Result<(), VaultError> {
        self.call(|| {
            let found = ItemSearchOptions::new()
                .class(ItemClass::generic_password())
                .keychains(std::slice::from_ref(&self.keychain))
                .service(self.service)
                .limit(Limit::All)
                .load_refs(true)
                .search();
            let results = match found {
                Ok(results) => results,
                Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => return Ok(()),
                Err(e) => return Err(e.code()),
            };
            for result in results {
                if let SearchResult::Ref(Reference::KeychainItem(item)) = result {
                    match delete_item(&item) {
                        Ok(()) | Err(ERR_SEC_ITEM_NOT_FOUND) => {}
                        Err(code) => return Err(code),
                    }
                }
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_map_to_sanitized_failures() {
        assert_eq!(map(ERR_SEC_ITEM_NOT_FOUND, None), VaultError::NotFound);
        assert_eq!(
            map(ERR_SEC_INTERACTION_NOT_ALLOWED, Some(true)),
            VaultError::Locked
        );
        assert_eq!(
            map(ERR_SEC_INTERACTION_NOT_ALLOWED, Some(false)),
            VaultError::Denied
        );
        assert_eq!(map(ERR_SEC_USER_CANCELED, Some(false)), VaultError::Denied);
        assert_eq!(map(ERR_SEC_AUTH_FAILED, Some(false)), VaultError::Denied);
        assert_eq!(map(ERR_SEC_AUTH_FAILED, Some(true)), VaultError::Locked);
        assert_eq!(map(ERR_SEC_USER_CANCELED, Some(true)), VaultError::Locked);
        assert_eq!(map(ERR_SEC_NO_SUCH_KEYCHAIN, None), VaultError::Unavailable);
        assert_eq!(map(-1, None), VaultError::Unavailable);
    }
}
