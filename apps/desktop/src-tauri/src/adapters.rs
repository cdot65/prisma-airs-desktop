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

use desktop_core::avatar::MAX_SOURCE_BYTES;
use desktop_core::import::{ImportError, MAX_IMPORT_BYTES};
use desktop_core::store::{AppStore, FileStore, UnavailableStore};
use desktop_core::vault::UnavailableVault;
use desktop_core::{
    Adapters, CredentialStore, ImagePicker, ImportFile, ImportPicker, PagePrinter, UrlOpener,
};
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
        learner_keychain: learner_store(root.as_deref()),
        store,
        picker: Arc::new(NativePicker { app: app.clone() }),
        opener: opener(root.as_deref()),
        printer: printer(app, root.as_deref()),
        images: images(app, root.as_deref()),
        offline_root: root.as_ref().map(|r| r.join("academy-offline")),
    }
}

/// The native image picker for the learner's avatar.
#[cfg(not(feature = "test-fixtures"))]
fn images<R: Runtime>(app: &AppHandle<R>, _root: Option<&Path>) -> Arc<dyn ImagePicker> {
    Arc::new(NativeImagePicker { app: app.clone() })
}

/// The fixture build answers the avatar picker with the image the
/// acceptance driver staged in its data root (none staged: cancelled), so
/// automation never blocks on a modal sheet.
#[cfg(feature = "test-fixtures")]
fn images<R: Runtime>(_app: &AppHandle<R>, root: Option<&Path>) -> Arc<dyn ImagePicker> {
    struct StagedImage(Option<PathBuf>);
    impl ImagePicker for StagedImage {
        fn choose_image(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
            let path = self.0.clone();
            Box::pin(async move {
                let path = path.filter(|p| p.is_file())?;
                let read = tokio::task::spawn_blocking(move || {
                    let bytes = read_limited(&path, MAX_SOURCE_BYTES);
                    let _ = std::fs::remove_file(&path);
                    bytes
                })
                .await
                .ok()?;
                Some(read.map(|bytes| ImportFile {
                    name: FIXTURE_AVATAR.to_owned(),
                    bytes,
                }))
            })
        }
    }
    Arc::new(StagedImage(root.map(|r| r.join(FIXTURE_AVATAR))))
}

/// The image the next fixture avatar pick returns (consumed by the pick).
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_AVATAR: &str = "fixture-avatar.img";
/// While present, fixture print sheets fail to open.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_PRINT_UNAVAILABLE: &str = "fixture-print-unavailable";

/// Opens the main window's native print sheet (print or save as PDF).
#[cfg(not(feature = "test-fixtures"))]
fn printer<R: Runtime>(app: &AppHandle<R>, _root: Option<&Path>) -> Arc<dyn PagePrinter> {
    struct NativePrinter<R: Runtime>(AppHandle<R>);
    impl<R: Runtime> PagePrinter for NativePrinter<R> {
        fn print(&self) -> bool {
            self.0
                .get_webview_window(MAIN_WINDOW)
                .is_some_and(|window| window.print().is_ok())
        }
    }
    Arc::new(NativePrinter(app.clone()))
}

/// The fixture build records print requests in its data root instead of
/// opening a sheet that would block automation.
#[cfg(feature = "test-fixtures")]
fn printer<R: Runtime>(_app: &AppHandle<R>, root: Option<&Path>) -> Arc<dyn PagePrinter> {
    struct RecordingPrinter(Option<PathBuf>);
    impl PagePrinter for RecordingPrinter {
        fn print(&self) -> bool {
            use std::io::Write;
            let unavailable = self
                .0
                .as_ref()
                .and_then(|p| p.parent())
                .is_some_and(|root| root.join(FIXTURE_PRINT_UNAVAILABLE).exists());
            !unavailable
                && self.0.as_ref().is_some_and(|path| {
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .and_then(|mut file| writeln!(file, "print"))
                        .is_ok()
                })
        }
    }
    Arc::new(RecordingPrinter(root.map(|r| r.join(FIXTURE_PRINTS))))
}

/// Print sheets the fixture build was asked to open, one line each.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_PRINTS: &str = "fixture-prints.log";

/// Opens a verified HTTPS page with LaunchServices (`/usr/bin/open`). The
/// core passes only URLs the Academy issuer returned; the presentation
/// never supplies one.
#[cfg(not(feature = "test-fixtures"))]
fn opener(_root: Option<&Path>) -> Arc<dyn UrlOpener> {
    struct SystemOpener;
    impl UrlOpener for SystemOpener {
        fn open(&self, url: &url::Url) -> bool {
            url.scheme() == "https"
                && std::process::Command::new("/usr/bin/open")
                    .arg(url.as_str())
                    .env_clear()
                    .status()
                    .is_ok_and(|s| s.success())
        }
    }
    Arc::new(SystemOpener)
}

/// The fixture build records requested pages in its data root instead of
/// launching a browser, so acceptance can verify the exact URL.
#[cfg(feature = "test-fixtures")]
fn opener(root: Option<&Path>) -> Arc<dyn UrlOpener> {
    struct RecordingOpener(Option<PathBuf>);
    impl UrlOpener for RecordingOpener {
        fn open(&self, url: &url::Url) -> bool {
            use std::io::Write;
            self.0.as_ref().is_some_and(|path| {
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut file| writeln!(file, "{url}"))
                    .is_ok()
            })
        }
    }
    Arc::new(RecordingOpener(root.map(|r| r.join(FIXTURE_OPENED_URLS))))
}

/// Pages the fixture build was asked to open, one per line.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_OPENED_URLS: &str = "fixture-opened-urls.log";
/// The fixture build's Academy origin for the synthetic endpoint.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_ACADEMY_ORIGIN: &str = "fixture-academy-origin";

/// The fixture Academy origin, when the acceptance driver installed one.
#[cfg(feature = "test-fixtures")]
pub fn fixture_academy_origin<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let path = app.path().app_data_dir().ok()?.join(FIXTURE_ACADEMY_ORIGIN);
    let text = std::fs::read_to_string(path).ok()?;
    desktop_core::academy::origin_of(text.trim())
}

#[cfg(feature = "test-fixtures")]
pub const FIXTURE_MANAGEMENT_ORIGIN: &str = "fixture-management-origin";

/// The fixture management origin (token, API and DLP endpoints under it),
/// when the acceptance driver installed one.
#[cfg(feature = "test-fixtures")]
pub fn fixture_management_origin<R: Runtime>(app: &AppHandle<R>) -> Option<String> {
    let path = app
        .path()
        .app_data_dir()
        .ok()?
        .join(FIXTURE_MANAGEMENT_ORIGIN);
    let text = std::fs::read_to_string(path).ok()?;
    let origin = text.trim();
    desktop_core::management::is_origin(origin).then(|| origin.to_owned())
}

#[cfg(all(target_os = "macos", not(feature = "test-fixtures")))]
fn learner_store(_root: Option<&Path>) -> Arc<dyn CredentialStore> {
    crate::keychain::KeychainStore::login_service(crate::keychain::ACADEMY_SERVICE)
        .map(|k| Arc::new(k) as Arc<dyn CredentialStore>)
        .unwrap_or_else(|_| Arc::new(UnavailableVault))
}

#[cfg(all(target_os = "macos", feature = "test-fixtures"))]
fn learner_store(root: Option<&Path>) -> Arc<dyn CredentialStore> {
    use crate::keychain::{FIXTURE_ACADEMY_SERVICE, Interaction, KeychainStore};
    root.and_then(|dir| {
        KeychainStore::file(
            &dir.join(FIXTURE_KEYCHAIN),
            FIXTURE_KEYCHAIN_PASSWORD,
            FIXTURE_ACADEMY_SERVICE,
            Interaction::Disallowed,
        )
        .ok()
    })
    .map(|k| Arc::new(k) as Arc<dyn CredentialStore>)
    .unwrap_or_else(|| Arc::new(UnavailableVault))
}

#[cfg(not(target_os = "macos"))]
fn learner_store(_root: Option<&Path>) -> Arc<dyn CredentialStore> {
    Arc::new(UnavailableVault)
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

/// The fixture build's additional trust roots for the synthetic acceptance
/// endpoint, read from `fixture-transport-roots.pem` in the fixture data root.
/// Production builds have no such file or code path: they trust the OS store.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_TRANSPORT_ROOTS: &str = "fixture-transport-roots.pem";

/// The fixture transport policy, when the acceptance driver installed roots.
#[cfg(feature = "test-fixtures")]
pub fn fixture_transport<R: Runtime>(
    app: &AppHandle<R>,
) -> Option<desktop_core::transport::TransportPolicy> {
    let path = app
        .path()
        .app_data_dir()
        .ok()?
        .join(FIXTURE_TRANSPORT_ROOTS);
    let pem = std::fs::read(path).ok()?;
    Some(desktop_core::transport::TransportPolicy {
        revision: 2,
        extra_roots: vec![pem],
        ..Default::default()
    })
}

/// Reads at most [`MAX_IMPORT_BYTES`] from a regular file.
pub fn read_bounded(path: &Path) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    read_limited(path, MAX_IMPORT_BYTES)
}

/// Reads a regular file of at most `limit` bytes.
pub fn read_limited(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, ImportError> {
    let meta = std::fs::metadata(path).map_err(|_| ImportError::Unsupported)?;
    if !meta.is_file() {
        return Err(ImportError::Unsupported);
    }
    if meta.len() > limit as u64 {
        return Err(ImportError::TooLarge);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .and_then(|f| f.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|_| ImportError::Unsupported)?;
    if bytes.len() > limit {
        return Err(ImportError::TooLarge);
    }
    Ok(bytes)
}

/// The native image sheet for the learner's avatar, attached to the main
/// window; reads are bounded to the 16 MiB source-photo cap.
#[cfg_attr(feature = "test-fixtures", allow(dead_code))]
pub struct NativeImagePicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ImagePicker for NativeImagePicker<R> {
    fn choose_image(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW)?;
            app.run_on_main_thread(move || {
                let chosen = rfd::FileDialog::new()
                    .set_parent(&window)
                    .set_title("Choose a photo")
                    .add_filter("Photos", &["jpg", "jpeg", "png", "webp"])
                    .pick_file();
                let _ = tx.send(chosen);
            })
            .ok()?;
            let path = rx.await.ok()??;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let read = tokio::task::spawn_blocking(move || read_limited(&path, MAX_SOURCE_BYTES))
                .await
                .ok()?;
            Some(read.map(|bytes| ImportFile { name, bytes }))
        })
    }
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
