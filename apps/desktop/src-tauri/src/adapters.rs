//! Production OS adapters for the core: the Rust-owned data store, the
//! Keychain credential store and the native profile-import sheet.
//!
//! The data root is the OS application-data directory for this build's
//! bundle identifier (`io.cdot.prisma-airs-desktop.v2`, or `.fixture` in the
//! fixture build), never a v1 directory. Fixture builds keep credentials in a
//! dedicated test keychain inside that root under the fixture namespace, with
//! OS prompts disabled so failures are deterministic; production uses the
//! login Keychain. An adapter that cannot be opened is replaced by one that
//! reports `Unavailable`; there is no fallback to plaintext or another store.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use desktop_core::import::{ImportError, MAX_IMPORT_BYTES};
use desktop_core::store::{AppStore, FileStore, UnavailableStore};
use desktop_core::vault::UnavailableVault;
use desktop_core::{Adapters, CredentialStore, ImportFile, ImportPicker};
use futures_util::future::BoxFuture;
use tauri::{AppHandle, Manager, Runtime};
use zeroize::Zeroizing;

use crate::MAIN_WINDOW;

/// The fixture test keychain's file name inside the fixture data root.
pub const FIXTURE_KEYCHAIN: &str = "fixture-credentials.keychain-db";
/// The fixture test keychain's password. It protects only synthetic fixture
/// credentials in a keychain that exists solely for acceptance runs.
pub const FIXTURE_KEYCHAIN_PASSWORD: &str = "prisma-airs-desktop-v2-fixture-keychain";

/// Builds the production adapters for the app's data root.
pub fn production<R: Runtime>(app: &AppHandle<R>) -> Adapters {
    let root = app.path().app_data_dir().ok();
    let store: Arc<dyn AppStore> = root
        .as_deref()
        .and_then(|dir| FileStore::open(dir).ok())
        .map(|s| Arc::new(s) as Arc<dyn AppStore>)
        .unwrap_or_else(|| Arc::new(UnavailableStore));
    Adapters {
        keychain: credential_store(root.as_deref()),
        store,
        picker: Arc::new(NativePicker { app: app.clone() }),
    }
}

#[cfg(all(target_os = "macos", not(feature = "test-fixtures")))]
fn credential_store(_root: Option<&Path>) -> Arc<dyn CredentialStore> {
    crate::keychain::KeychainStore::login()
        .map(|k| Arc::new(k) as Arc<dyn CredentialStore>)
        .unwrap_or_else(|_| Arc::new(UnavailableVault))
}

/// The fixture keychain, for the fixture menu's lock/unlock items.
#[cfg(all(target_os = "macos", feature = "test-fixtures"))]
pub static FIXTURE_STORE: std::sync::OnceLock<Arc<crate::keychain::KeychainStore>> =
    std::sync::OnceLock::new();

#[cfg(all(target_os = "macos", feature = "test-fixtures"))]
fn credential_store(root: Option<&Path>) -> Arc<dyn CredentialStore> {
    use crate::keychain::{FIXTURE_SCAN_SERVICE, Interaction, KeychainStore};
    let store = root.and_then(|dir| {
        KeychainStore::file(
            &dir.join(FIXTURE_KEYCHAIN),
            FIXTURE_KEYCHAIN_PASSWORD,
            FIXTURE_SCAN_SERVICE,
            Interaction::Disallowed,
        )
        .ok()
    });
    match store {
        Some(store) => {
            let store = Arc::new(store);
            let _ = FIXTURE_STORE.set(Arc::clone(&store));
            store
        }
        None => Arc::new(UnavailableVault),
    }
}

#[cfg(not(target_os = "macos"))]
fn credential_store(_root: Option<&Path>) -> Arc<dyn CredentialStore> {
    Arc::new(UnavailableVault)
}

/// Reads at most [`MAX_IMPORT_BYTES`] from a regular file.
pub fn read_bounded(path: &Path) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    let meta = std::fs::metadata(path).map_err(|_| ImportError::Unsupported)?;
    if !meta.is_file() {
        return Err(ImportError::Unsupported);
    }
    if meta.len() > MAX_IMPORT_BYTES as u64 {
        return Err(ImportError::TooLarge);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_IMPORT_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|_| ImportError::Unsupported)?;
    if bytes.len() > MAX_IMPORT_BYTES {
        return Err(ImportError::TooLarge);
    }
    Ok(bytes)
}

/// The native open sheet attached to the main window.
pub struct NativePicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ImportPicker for NativePicker<R> {
    fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW)?;
            app.run_on_main_thread(move || {
                let chosen = rfd::FileDialog::new()
                    .set_parent(&window)
                    .set_title("Import tenant profiles")
                    .add_filter("Tenant profiles", &["json"])
                    .pick_file();
                let _ = tx.send(chosen);
            })
            .ok()?;
            let path = rx.await.ok()??;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let read = tokio::task::spawn_blocking(move || read_bounded(&path))
                .await
                .ok()?;
            Some(read.map(|bytes| ImportFile { name, bytes }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_are_bounded_and_limited_to_regular_files() {
        let dir = std::env::temp_dir().join(format!("airs-v2-import-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let small = dir.join("small.json");
        std::fs::write(&small, b"{}").expect("write");
        assert_eq!(read_bounded(&small).expect("read").as_slice(), b"{}");
        let big = dir.join("big.json");
        std::fs::write(&big, vec![b' '; MAX_IMPORT_BYTES + 1]).expect("write");
        assert_eq!(read_bounded(&big).err(), Some(ImportError::TooLarge));
        assert_eq!(read_bounded(&dir).err(), Some(ImportError::Unsupported));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
