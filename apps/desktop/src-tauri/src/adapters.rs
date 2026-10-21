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
    Adapters, CredentialStore, FileSaver, FolderPicker, ImagePicker, ImportFile, ImportPicker,
    PagePrinter, UrlOpener,
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
        confirmer: confirmer(app),
        secrets: secrets(app),
        text_files: sheet_picker(
            root.as_deref(),
            text_files(app, root.as_deref()),
            Arc::new(NativeTextPicker { app: app.clone() }),
        ),
        offline_root: root.as_ref().map(|r| r.join("academy-offline")),
        saver: sheet_saver(
            root.as_deref(),
            saver(app, root.as_deref()),
            Arc::new(NativeSaver { app: app.clone() }),
        ),
        sessions_root: root.as_ref().map(|r| r.join("sessions")),
        case_files: sheet_picker(
            root.as_deref(),
            case_files(app, root.as_deref()),
            Arc::new(NativeCasePicker { app: app.clone() }),
        ),
        library_root: root.as_ref().map(|r| r.join("library")),
        folders: folders(app, root.as_deref()),
        workbench_files: sheet_picker(
            root.as_deref(),
            workbench_files(root.as_deref()),
            Arc::new(NativeExportPicker { app: app.clone() }),
        ),
        workbench_root: root.as_ref().map(|r| r.join("api-client")),
        gateway_files: sheet_picker(
            root.as_deref(),
            gateway_files(app, root.as_deref()),
            Arc::new(NativeUploadPicker { app: app.clone() }),
        ),
        harness: harness(app, root.as_deref()),
    }
}

/// The bundled Harness engine (`Contents/Resources/harness/airs-harness`),
/// its OS launcher and where its ephemeral runtime homes are made. A build
/// without the engine resource offers no Harness.
fn harness<R: Runtime>(
    app: &AppHandle<R>,
    root: Option<&Path>,
) -> desktop_core::harness::sidecar::HarnessAdapters {
    use desktop_core::harness::{engine::Engine, sidecar::HarnessAdapters};
    let engine = app
        .path()
        .resource_dir()
        .ok()
        .map(|r| {
            r.join("harness")
                .join(desktop_core::harness::engine::pin().executable)
        })
        .and_then(|p| p.canonicalize().ok())
        .filter(|p| p.is_file())
        .map(|executable| Engine { executable });
    #[cfg(target_os = "macos")]
    let launcher: Arc<dyn desktop_core::harness::sidecar::SidecarLauncher> =
        Arc::new(crate::sidecar::OsLauncher);
    #[cfg(not(target_os = "macos"))]
    let launcher: Arc<dyn desktop_core::harness::sidecar::SidecarLauncher> =
        Arc::new(desktop_core::harness::sidecar::UnavailableLauncher);
    HarnessAdapters {
        launcher,
        engine,
        runtime_root: root.map(|r| r.join("harness-runtime")),
        #[cfg(feature = "test-fixtures")]
        fixture: None,
    }
}

/// The v1 API Client profile offered for adoption (read only). Fixture
/// builds use a staged copy under their data root, never a real profile.
#[cfg(not(feature = "test-fixtures"))]
pub fn adoption_source<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .data_dir()
        .ok()
        .map(|d| d.join("Prisma AIRS Desktop").join("api-client"))
}

#[cfg(feature = "test-fixtures")]
pub fn adoption_source<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("fixture-v1-api-client"))
}

/// A separate native sheet for the close guard (its own registry, so its
/// nonce never meets a change review's).
#[cfg(target_os = "macos")]
pub fn close_guard<R: Runtime>(app: &AppHandle<R>) -> Arc<dyn desktop_core::OperatorConfirmation> {
    Arc::new(crate::native_confirmation::NativeConfirmation::new(
        app.clone(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn close_guard<R: Runtime>(_app: &AppHandle<R>) -> Arc<dyn desktop_core::OperatorConfirmation> {
    Arc::new(crate::native_confirmation::Unsupported)
}

/// Insomnia exports are bounded like v1 (1 MiB); one byte more is read so
/// the core can report "too large".
pub const MAX_EXPORT_BYTES: usize = desktop_core::workbench::insomnia::EXPORT_BYTES + 1;

/// The native folder sheets for API Client collections (C14).
fn folders<R: Runtime>(app: &AppHandle<R>, root: Option<&Path>) -> Arc<dyn FolderPicker> {
    let native = Arc::new(NativeFolders { app: app.clone() });
    staged_folders(root, native)
}

#[cfg(not(feature = "test-fixtures"))]
fn staged_folders(
    _root: Option<&Path>,
    native: Arc<NativeFolders<impl Runtime>>,
) -> Arc<dyn FolderPicker> {
    native
}

/// Where the fixture driver names the folder the open sheet answers.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_OPEN_FOLDER: &str = "fixture-open-folder";
/// Where the fixture driver names the folder the new-collection sheet
/// answers.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_NEW_FOLDER: &str = "fixture-new-folder";
/// Where the fixture driver names the file the save-response sheet answers.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_SAVE_RESPONSE: &str = "fixture-save-response";

/// The fixture build answers the folder sheets with the paths the driver
/// staged (none: cancelled), unless the native-sheet flag asks for the real
/// NSOpenPanel / NSSavePanel.
#[cfg(feature = "test-fixtures")]
fn staged_folders<R: Runtime>(
    root: Option<&Path>,
    native: Arc<NativeFolders<R>>,
) -> Arc<dyn FolderPicker> {
    struct Staged<R: Runtime>(Option<PathBuf>, Arc<NativeFolders<R>>);
    impl<R: Runtime> Staged<R> {
        fn staged(&self, name: &str) -> BoxFuture<'static, Option<PathBuf>> {
            let file = self.0.as_ref().map(|r| r.join(name));
            Box::pin(async move {
                let text = std::fs::read_to_string(file?).ok()?;
                Some(PathBuf::from(text.trim()))
            })
        }
        fn native(&self) -> bool {
            self.0
                .as_ref()
                .is_some_and(|r| r.join(FIXTURE_NATIVE_SHEETS).exists())
        }
    }
    impl<R: Runtime> FolderPicker for Staged<R> {
        fn choose_folder(&self) -> BoxFuture<'static, Option<PathBuf>> {
            if self.native() {
                self.1.choose_folder()
            } else {
                self.staged(FIXTURE_OPEN_FOLDER)
            }
        }
        fn choose_new(&self) -> BoxFuture<'static, Option<PathBuf>> {
            if self.native() {
                self.1.choose_new()
            } else {
                self.staged(FIXTURE_NEW_FOLDER)
            }
        }
        fn choose_save(&self, suggested: String) -> BoxFuture<'static, Option<PathBuf>> {
            if self.native() {
                self.1.choose_save(suggested)
            } else {
                self.staged(FIXTURE_SAVE_RESPONSE)
            }
        }
    }
    Arc::new(Staged(root.map(Path::to_path_buf), native))
}

pub struct NativeFolders<R: Runtime> {
    app: AppHandle<R>,
}

/// Which native sheet to show.
enum Sheet {
    Open,
    New,
    /// Save a response body under a suggested name (the panel confirms
    /// replacing an existing file).
    Save(String),
}

impl<R: Runtime> NativeFolders<R> {
    fn sheet(&self, sheet: Sheet) -> BoxFuture<'static, Option<PathBuf>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW)?;
            app.run_on_main_thread(move || {
                let dialog = rfd::FileDialog::new().set_parent(&window);
                let chosen = match sheet {
                    Sheet::New => dialog
                        .set_title("New collection")
                        .set_file_name("New collection")
                        .save_file(),
                    Sheet::Open => dialog.set_title("Open collection").pick_folder(),
                    Sheet::Save(name) => dialog
                        .set_title("Save response")
                        .set_file_name(name)
                        .save_file(),
                };
                let _ = tx.send(chosen);
            })
            .ok()?;
            rx.await.ok()?
        })
    }
}

impl<R: Runtime> FolderPicker for NativeFolders<R> {
    fn choose_folder(&self) -> BoxFuture<'static, Option<PathBuf>> {
        self.sheet(Sheet::Open)
    }
    fn choose_new(&self) -> BoxFuture<'static, Option<PathBuf>> {
        self.sheet(Sheet::New)
    }
    fn choose_save(&self, suggested: String) -> BoxFuture<'static, Option<PathBuf>> {
        self.sheet(Sheet::Save(suggested))
    }
}

/// Where the fixture driver stages Insomnia exports.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_WORKBENCH_FILES: &str = "fixture-workbench-files";

#[cfg(not(feature = "test-fixtures"))]
fn workbench_files(_root: Option<&Path>) -> Arc<dyn ImportPicker> {
    Arc::new(Cancelled)
}

/// Production builds use the native sheet (`sheet_picker` picks it).
#[cfg(not(feature = "test-fixtures"))]
struct Cancelled;

#[cfg(not(feature = "test-fixtures"))]
impl ImportPicker for Cancelled {
    fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        Box::pin(async { None })
    }
}

/// The fixture build answers the export sheet with the first file staged
/// in `fixture-workbench-files/` (none: cancelled). The file is only read.
#[cfg(feature = "test-fixtures")]
fn workbench_files(root: Option<&Path>) -> Arc<dyn ImportPicker> {
    struct Staged(Option<PathBuf>);
    impl ImportPicker for Staged {
        fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
            let dir = self.0.clone();
            Box::pin(async move {
                let dir = dir?;
                tokio::task::spawn_blocking(move || {
                    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
                        .ok()?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .collect();
                    files.sort();
                    let path = files.into_iter().next()?;
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some(
                        read_limited(&path, MAX_EXPORT_BYTES)
                            .map(|bytes| ImportFile { name, bytes }),
                    )
                })
                .await
                .ok()?
            })
        }
    }
    Arc::new(Staged(root.map(|r| r.join(FIXTURE_WORKBENCH_FILES))))
}

/// The native file sheet for Insomnia exports (C14).
pub struct NativeExportPicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ImportPicker for NativeExportPicker<R> {
    fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW)?;
            app.run_on_main_thread(move || {
                let chosen = rfd::FileDialog::new()
                    .set_parent(&window)
                    .set_title("Import an Insomnia export")
                    .add_filter("Insomnia export", &["yaml", "yml", "json"])
                    .pick_file();
                let _ = tx.send(chosen);
            })
            .ok()?;
            let path = rx.await.ok()??;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let read = tokio::task::spawn_blocking(move || read_limited(&path, MAX_EXPORT_BYTES))
                .await
                .ok()?;
            Some(read.map(|bytes| ImportFile { name, bytes }))
        })
    }
}

/// When this file exists in the fixture data root, the fixture build uses
/// the production rfd sheets (NSOpenPanel / NSSavePanel) instead of staged
/// files, so native acceptance can drive the real panels.
#[cfg(feature = "test-fixtures")]
pub const FIXTURE_NATIVE_SHEETS: &str = "fixture-native-sheets";

/// Production builds always use the native sheet.
#[cfg(not(feature = "test-fixtures"))]
fn sheet_picker(
    _root: Option<&Path>,
    configured: Arc<dyn ImportPicker>,
    _native: Arc<dyn ImportPicker>,
) -> Arc<dyn ImportPicker> {
    configured
}

#[cfg(feature = "test-fixtures")]
fn sheet_picker(
    root: Option<&Path>,
    staged: Arc<dyn ImportPicker>,
    native: Arc<dyn ImportPicker>,
) -> Arc<dyn ImportPicker> {
    struct Switch(
        Option<PathBuf>,
        Arc<dyn ImportPicker>,
        Arc<dyn ImportPicker>,
    );
    impl ImportPicker for Switch {
        fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
            if self.0.as_ref().is_some_and(|flag| flag.exists()) {
                self.2.choose()
            } else {
                self.1.choose()
            }
        }
    }
    Arc::new(Switch(
        root.map(|r| r.join(FIXTURE_NATIVE_SHEETS)),
        staged,
        native,
    ))
}

#[cfg(not(feature = "test-fixtures"))]
fn sheet_saver(
    _root: Option<&Path>,
    configured: Arc<dyn FileSaver>,
    _native: Arc<dyn FileSaver>,
) -> Arc<dyn FileSaver> {
    configured
}

#[cfg(feature = "test-fixtures")]
fn sheet_saver(
    root: Option<&Path>,
    staged: Arc<dyn FileSaver>,
    native: Arc<dyn FileSaver>,
) -> Arc<dyn FileSaver> {
    struct Switch(Option<PathBuf>, Arc<dyn FileSaver>, Arc<dyn FileSaver>);
    impl FileSaver for Switch {
        fn save(
            &self,
            suggested: String,
            bytes: Vec<u8>,
        ) -> BoxFuture<'static, Result<Option<String>, ()>> {
            if self.0.as_ref().is_some_and(|flag| flag.exists()) {
                self.2.save(suggested, bytes)
            } else {
                self.1.save(suggested, bytes)
            }
        }
    }
    Arc::new(Switch(
        root.map(|r| r.join(FIXTURE_NATIVE_SHEETS)),
        staged,
        native,
    ))
}

/// Case exports are bounded like v1 (256 KiB).
pub const MAX_CASE_FILE_BYTES: usize = desktop_core::library::LIBRARY_BYTES;

/// The native file sheet for case imports (C12).
#[cfg(not(feature = "test-fixtures"))]
fn case_files<R: Runtime>(app: &AppHandle<R>, _root: Option<&Path>) -> Arc<dyn ImportPicker> {
    Arc::new(NativeCasePicker { app: app.clone() })
}

/// The fixture build answers the case-file sheet with the first file the
/// acceptance driver staged in `fixture-case-files/` (none: cancelled).
#[cfg(feature = "test-fixtures")]
fn case_files<R: Runtime>(_app: &AppHandle<R>, root: Option<&Path>) -> Arc<dyn ImportPicker> {
    struct StagedCases(Option<PathBuf>);
    impl ImportPicker for StagedCases {
        fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
            let dir = self.0.clone();
            Box::pin(async move {
                let dir = dir?;
                tokio::task::spawn_blocking(move || {
                    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
                        .ok()?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .collect();
                    files.sort();
                    let path = files.into_iter().next()?;
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some(
                        read_limited(&path, MAX_CASE_FILE_BYTES)
                            .map(|bytes| ImportFile { name, bytes }),
                    )
                })
                .await
                .ok()?
            })
        }
    }
    Arc::new(StagedCases(root.map(|r| r.join(FIXTURE_CASE_FILES))))
}

/// Where the fixture driver stages case import files.
pub const FIXTURE_CASE_FILES: &str = "fixture-case-files";

/// The native file sheet for Gateway file uploads (C18).
#[cfg(not(feature = "test-fixtures"))]
fn gateway_files<R: Runtime>(app: &AppHandle<R>, _root: Option<&Path>) -> Arc<dyn ImportPicker> {
    Arc::new(NativeUploadPicker { app: app.clone() })
}

/// The fixture build answers the upload sheet with the first file the
/// acceptance driver staged in `fixture-gateway-files/` (none: cancelled).
/// The file is only read.
#[cfg(feature = "test-fixtures")]
fn gateway_files<R: Runtime>(_app: &AppHandle<R>, root: Option<&Path>) -> Arc<dyn ImportPicker> {
    struct StagedUploads(Option<PathBuf>);
    impl ImportPicker for StagedUploads {
        fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
            let dir = self.0.clone();
            Box::pin(async move {
                let dir = dir?;
                tokio::task::spawn_blocking(move || {
                    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
                        .ok()?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .collect();
                    files.sort();
                    let path = files.into_iter().next()?;
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some(
                        read_limited(&path, MAX_UPLOAD_FILE_BYTES)
                            .map(|bytes| ImportFile { name, bytes }),
                    )
                })
                .await
                .ok()?
            })
        }
    }
    Arc::new(StagedUploads(root.map(|r| r.join(FIXTURE_GATEWAY_FILES))))
}

/// Where the fixture driver stages Gateway upload files.
pub const FIXTURE_GATEWAY_FILES: &str = "fixture-gateway-files";
/// Uploads are read up to the core's bound (one byte more is "too large").
const MAX_UPLOAD_FILE_BYTES: usize = desktop_core::gateway_files::MAX_UPLOAD_BYTES;

/// The native sheet for choosing a file to upload to the Gateway; reads
/// are bounded to the upload limit.
#[cfg_attr(feature = "test-fixtures", allow(dead_code))]
pub struct NativeUploadPicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ImportPicker for NativeUploadPicker<R> {
    fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW)?;
            app.run_on_main_thread(move || {
                let chosen = rfd::FileDialog::new()
                    .set_parent(&window)
                    .set_title("Choose a file to upload to the Gateway")
                    .pick_file();
                let _ = tx.send(chosen);
            })
            .ok()?;
            let path = rx.await.ok()??;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let read =
                tokio::task::spawn_blocking(move || read_limited(&path, MAX_UPLOAD_FILE_BYTES))
                    .await
                    .ok()?;
            Some(read.map(|bytes| ImportFile { name, bytes }))
        })
    }
}

#[cfg_attr(feature = "test-fixtures", allow(dead_code))]
pub struct NativeCasePicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ImportPicker for NativeCasePicker<R> {
    fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW)?;
            app.run_on_main_thread(move || {
                let chosen = rfd::FileDialog::new()
                    .set_parent(&window)
                    .set_title("Import test cases")
                    .add_filter("JSON", &["json"])
                    .pick_file();
                let _ = tx.send(chosen);
            })
            .ok()?;
            let path = rx.await.ok()??;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let read =
                tokio::task::spawn_blocking(move || read_limited(&path, MAX_CASE_FILE_BYTES))
                    .await
                    .ok()?;
            Some(read.map(|bytes| ImportFile { name, bytes }))
        })
    }
}

/// The native save sheet for report and investigation exports (C11).
#[cfg(not(feature = "test-fixtures"))]
fn saver<R: Runtime>(app: &AppHandle<R>, _root: Option<&Path>) -> Arc<dyn FileSaver> {
    Arc::new(NativeSaver { app: app.clone() })
}

/// The fixture build saves exports into `fixture-exports/` under its data
/// root without a sheet (never replacing a file), so acceptance can read
/// exactly what an operator would have saved.
#[cfg(feature = "test-fixtures")]
fn saver<R: Runtime>(_app: &AppHandle<R>, root: Option<&Path>) -> Arc<dyn FileSaver> {
    struct FixtureSaver(Option<PathBuf>);
    impl FileSaver for FixtureSaver {
        fn save(
            &self,
            suggested: String,
            bytes: Vec<u8>,
        ) -> BoxFuture<'static, Result<Option<String>, ()>> {
            let dir = self.0.clone();
            Box::pin(async move {
                let dir = dir.ok_or(())?;
                tokio::task::spawn_blocking(move || {
                    std::fs::create_dir_all(&dir).map_err(|_| ())?;
                    write_owner_only(&dir.join(&suggested), &bytes, false)?;
                    Ok(Some(suggested))
                })
                .await
                .map_err(|_| ())?
            })
        }
    }
    Arc::new(FixtureSaver(root.map(|r| r.join("fixture-exports"))))
}

/// Writes `bytes` owner-only: atomically through a temporary file beside the
/// target, replacing an existing file only when `replace` (the save sheet
/// already confirmed it).
fn write_owner_only(path: &Path, bytes: &[u8], replace: bool) -> Result<(), ()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let dir = path.parent().ok_or(())?;
    let mut suffix = [0u8; 8];
    getrandom::fill(&mut suffix).map_err(|_| ())?;
    let temp = dir.join(format!(".airs-export-{}.tmp", hex_of(&suffix)));
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    let placed = written.and_then(|()| {
        if replace {
            std::fs::rename(&temp, path)
        } else {
            // Never replace: a hard link fails if the target exists.
            std::fs::hard_link(&temp, path).and_then(|()| std::fs::remove_file(&temp))
        }
    });
    if placed.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    placed.map_err(|_| ())
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg_attr(feature = "test-fixtures", allow(dead_code))]
pub struct NativeSaver<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> FileSaver for NativeSaver<R> {
    fn save(
        &self,
        suggested: String,
        bytes: Vec<u8>,
    ) -> BoxFuture<'static, Result<Option<String>, ()>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW).ok_or(())?;
            let extension = if suggested.ends_with(".csv") {
                "csv"
            } else {
                "json"
            };
            app.run_on_main_thread(move || {
                let chosen = rfd::FileDialog::new()
                    .set_parent(&window)
                    .set_title("Export")
                    .set_file_name(&suggested)
                    .add_filter(extension.to_uppercase(), &[extension])
                    .save_file();
                let _ = tx.send(chosen);
            })
            .map_err(|_| ())?;
            let Some(path) = rx.await.map_err(|_| ())? else {
                return Ok(None);
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            tokio::task::spawn_blocking(move || write_owner_only(&path, &bytes, true))
                .await
                .map_err(|_| ())??;
            Ok(Some(name))
        })
    }
}

/// The native operator confirmation sheet (C09; fixture builds present the
/// real sheet too, so native acceptance drives it through Accessibility).
#[cfg(target_os = "macos")]
fn confirmer<R: Runtime>(app: &AppHandle<R>) -> Arc<dyn desktop_core::OperatorConfirmation> {
    Arc::new(crate::native_confirmation::NativeConfirmation::new(
        app.clone(),
    ))
}

#[cfg(target_os = "macos")]
fn secrets<R: Runtime>(app: &AppHandle<R>) -> Arc<dyn desktop_core::SecretPresenter> {
    Arc::new(crate::native_confirmation::NativeSecrets { app: app.clone() })
}

#[cfg(not(target_os = "macos"))]
fn confirmer<R: Runtime>(_app: &AppHandle<R>) -> Arc<dyn desktop_core::OperatorConfirmation> {
    Arc::new(crate::native_confirmation::Unsupported)
}

#[cfg(not(target_os = "macos"))]
fn secrets<R: Runtime>(_app: &AppHandle<R>) -> Arc<dyn desktop_core::SecretPresenter> {
    Arc::new(crate::native_confirmation::Unsupported)
}

/// Keyword files read for a dictionary are bounded well above v1's 64 Ki
/// UTF-16 units so the core can report "too large" precisely.
pub const MAX_TEXT_FILE_BYTES: usize = 256 * 1024;

/// The native file sheet for dictionary keyword files (C10).
#[cfg(not(feature = "test-fixtures"))]
fn text_files<R: Runtime>(app: &AppHandle<R>, _root: Option<&Path>) -> Arc<dyn ImportPicker> {
    Arc::new(NativeTextPicker { app: app.clone() })
}

/// The fixture build answers the keyword-file sheet with the first file the
/// acceptance driver staged in `fixture-dictionary-files/` (none: cancelled),
/// so automation never blocks on a modal sheet. The file is only read.
#[cfg(feature = "test-fixtures")]
fn text_files<R: Runtime>(_app: &AppHandle<R>, root: Option<&Path>) -> Arc<dyn ImportPicker> {
    struct StagedText(Option<PathBuf>);
    impl ImportPicker for StagedText {
        fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
            let dir = self.0.clone();
            Box::pin(async move {
                let dir = dir?;
                tokio::task::spawn_blocking(move || {
                    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
                        .ok()?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .collect();
                    files.sort();
                    let path = files.into_iter().next()?;
                    let name = path.file_name()?.to_string_lossy().into_owned();
                    Some(
                        read_limited(&path, MAX_TEXT_FILE_BYTES)
                            .map(|bytes| ImportFile { name, bytes }),
                    )
                })
                .await
                .ok()?
            })
        }
    }
    Arc::new(StagedText(root.map(|r| r.join(FIXTURE_DICTIONARY_FILES))))
}

/// Where the fixture driver stages dictionary keyword files.
pub const FIXTURE_DICTIONARY_FILES: &str = "fixture-dictionary-files";

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
pub const FIXTURE_CASE_CATALOG: &str = "fixture-case-catalog";

/// The fixture public-catalog URL (a local snapshot server), when the
/// acceptance driver installed one; it must still pass the catalog URL rule.
#[cfg(feature = "test-fixtures")]
pub fn fixture_case_catalog<R: Runtime>(app: &AppHandle<R>) -> Option<url::Url> {
    let path = app.path().app_data_dir().ok()?.join(FIXTURE_CASE_CATALOG);
    let text = std::fs::read_to_string(path).ok()?;
    let url = url::Url::parse(text.trim()).ok()?;
    desktop_core::library::catalog_url_ok(&url, &url).then_some(url)
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

/// The native file sheet for a dictionary's keyword file, attached to the
/// main window; the file is only read, bounded to [`MAX_TEXT_FILE_BYTES`].
#[cfg_attr(feature = "test-fixtures", allow(dead_code))]
pub struct NativeTextPicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ImportPicker for NativeTextPicker<R> {
    fn choose(&self) -> BoxFuture<'static, Option<Result<ImportFile, ImportError>>> {
        let app = self.app.clone();
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel::<Option<PathBuf>>();
            let window = app.get_webview_window(MAIN_WINDOW)?;
            app.run_on_main_thread(move || {
                let chosen = rfd::FileDialog::new()
                    .set_parent(&window)
                    .set_title("Choose a dictionary keyword file")
                    .add_filter("Text", &["txt", "csv"])
                    .pick_file();
                let _ = tx.send(chosen);
            })
            .ok()?;
            let path = rx.await.ok()??;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let read =
                tokio::task::spawn_blocking(move || read_limited(&path, MAX_TEXT_FILE_BYTES))
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
