//! OS adapters for the controlled live runner (C25).
//!
//! The runner drives the production core, but not the desktop host: there
//! is no window, Keychain write or saved document. Credentials are session
//! records (zeroized on removal) and the document lives in memory. The
//! host's interactive parts are narrowed to what a scoped run needs:
//!
//! - a page the core opens (device sign-in, company or MCP sign-in) is
//!   handed to the [`Operator`], who completes it in the browser; the runner
//!   never completes it and learns the result only from the core;
//! - a native confirmation is accepted only when the runner armed it for
//!   the one change it just reviewed (and checked against its scope);
//! - a one-time secret shown to the host is dropped (zeroized) unseen;
//! - the folder sheets answer with the run's own scratch folder, every
//!   other sheet as cancelled.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use desktop_core::ImportError;
use desktop_core::store::MemoryStore;
use desktop_core::vault::{RecordId, SessionVault, Vault, VaultError};
use desktop_core::{
    Adapters, ConfirmationRequest, CredentialStore, FileSaver, FolderPicker, ImagePicker,
    ImportFile, ImportPicker, OperatorConfirmation, PagePrinter, SecretPresenter, UrlOpener,
};
use zeroize::Zeroizing;

type Boxed<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// The person running a live check: completes browser steps and access
/// changes the runner asks for. The runner only ever observes the core.
pub trait Operator: Send + Sync {
    /// Asks the operator to do `instruction`; `url` is a page the core
    /// produced (the issuer's verification or authorization page).
    fn prompt(&self, instruction: &str, url: Option<&url::Url>);
}

/// The terminal: the instruction on stderr and the page in the default
/// browser (as the desktop host does).
pub struct TerminalOperator;

impl Operator for TerminalOperator {
    fn prompt(&self, instruction: &str, url: Option<&url::Url>) {
        eprintln!("OPERATOR STEP: {instruction}");
        if let Some(url) = url {
            eprintln!("  page: {url}");
            if url.scheme() == "https" || url.host_str() == Some("127.0.0.1") {
                let _ = std::process::Command::new("/usr/bin/open")
                    .arg(url.as_str())
                    .status();
            }
        }
    }
}

/// Native confirmations: accepted only once armed, once per arming.
#[derive(Default)]
pub struct Confirmations {
    armed: Mutex<u32>,
    shown: Mutex<Vec<(String, bool)>>,
}

impl Confirmations {
    /// Accept the next confirmation the core opens.
    pub fn arm(&self) {
        if let Ok(mut armed) = self.armed.lock() {
            *armed = 1;
        }
    }

    pub fn disarm(&self) {
        if let Ok(mut armed) = self.armed.lock() {
            *armed = 0;
        }
    }

    /// The titles shown so far and whether each was accepted.
    pub fn shown(&self) -> Vec<(String, bool)> {
        self.shown.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

/// What a desk's host offers.
#[derive(Clone)]
pub struct Host {
    pub operator: Arc<dyn Operator>,
    pub confirmations: Arc<Confirmations>,
    /// Answered by the folder sheets (the run's scratch collection).
    pub folder: Option<PathBuf>,
    pub offline_root: Option<PathBuf>,
    pub workbench_root: Option<PathBuf>,
    /// Where the save sheet puts a response body (a run-owned folder).
    pub save_dir: Option<PathBuf>,
    /// What the image sheet answers with (a synthetic avatar).
    pub image: Option<Vec<u8>>,
}

/// Session-only credentials for one run.
#[derive(Default)]
struct RunVault(SessionVault);

impl Vault for RunVault {
    fn put(&self, record: &RecordId, secret: &[u8]) -> Result<(), VaultError> {
        self.0.put(record, secret)
    }

    fn get(&self, record: &RecordId) -> Result<Zeroizing<Vec<u8>>, VaultError> {
        self.0.get(record)
    }

    fn delete(&self, record: &RecordId) -> Result<(), VaultError> {
        self.0.delete(record)
    }
}

impl CredentialStore for RunVault {
    fn purge(&self) -> Result<(), VaultError> {
        self.0.clear();
        Ok(())
    }
}

struct Sheets {
    operator: Arc<dyn Operator>,
    confirmations: Arc<Confirmations>,
    folder: Option<PathBuf>,
    save_dir: Option<PathBuf>,
    image: Option<Vec<u8>>,
}

impl ImportPicker for Sheets {
    fn choose(&self) -> Boxed<Option<Result<ImportFile, ImportError>>> {
        Box::pin(async { None })
    }
}

impl UrlOpener for Sheets {
    fn open(&self, url: &url::Url) -> bool {
        self.operator.prompt(
            "complete this sign-in in the browser with the designated test identity",
            Some(url),
        );
        true
    }
}

impl PagePrinter for Sheets {
    fn print(&self) -> bool {
        false
    }
}

impl ImagePicker for Sheets {
    fn choose_image(&self) -> Boxed<Option<Result<ImportFile, ImportError>>> {
        let file = self.image.clone().map(|bytes| {
            Ok(ImportFile {
                name: "c25-avatar.png".to_owned(),
                bytes: Zeroizing::new(bytes),
            })
        });
        Box::pin(async move { file })
    }
}

impl OperatorConfirmation for Sheets {
    fn confirm(&self, _nonce: u64, request: ConfirmationRequest) -> Boxed<bool> {
        let accepted = self
            .confirmations
            .armed
            .lock()
            .map(|mut armed| {
                let ok = *armed > 0;
                *armed = 0;
                ok
            })
            .unwrap_or(false);
        if let Ok(mut shown) = self.confirmations.shown.lock() {
            shown.push((request.title, accepted));
        }
        Box::pin(async move { accepted })
    }

    fn dismiss(&self, _nonce: u64) {}
}

impl SecretPresenter for Sheets {
    fn reveal(&self, _label: String, secret: Zeroizing<String>) {
        drop(secret);
    }

    fn dismiss(&self) {}
}

impl FileSaver for Sheets {
    fn save(&self, _suggested: String, _bytes: Vec<u8>) -> Boxed<Result<Option<String>, ()>> {
        Box::pin(async { Ok(None) })
    }
}

impl FolderPicker for Sheets {
    fn choose_folder(&self) -> Boxed<Option<PathBuf>> {
        let folder = self.folder.clone();
        Box::pin(async move { folder })
    }

    fn choose_new(&self) -> Boxed<Option<PathBuf>> {
        Box::pin(async { None })
    }

    fn choose_project(&self) -> Boxed<Option<PathBuf>> {
        Box::pin(async { None })
    }

    fn choose_save(&self, suggested: String) -> Boxed<Option<PathBuf>> {
        let name: String = suggested
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || "._-".contains(*c))
            .take(64)
            .collect();
        let path = self.save_dir.as_ref().map(|d| {
            d.join(if name.is_empty() {
                "response.bin".to_owned()
            } else {
                name
            })
        });
        Box::pin(async move { path })
    }
}

/// The adapters one live core runs with.
pub fn adapters(host: &Host) -> Adapters {
    let sheets = Arc::new(Sheets {
        operator: Arc::clone(&host.operator),
        confirmations: Arc::clone(&host.confirmations),
        folder: host.folder.clone(),
        save_dir: host.save_dir.clone(),
        image: host.image.clone(),
    });
    Adapters {
        keychain: Arc::new(RunVault::default()),
        learner_keychain: Arc::new(RunVault::default()),
        store: Arc::new(MemoryStore::new()),
        opener: sheets.clone(),
        printer: sheets.clone(),
        images: sheets.clone(),
        confirmer: sheets.clone(),
        secrets: sheets.clone(),
        text_files: sheets.clone(),
        offline_root: host.offline_root.clone(),
        saver: sheets.clone(),
        sessions_root: None,
        case_files: sheets.clone(),
        library_root: None,
        folders: sheets.clone(),
        workbench_files: sheets.clone(),
        gateway_files: sheets,
        workbench_root: host.workbench_root.clone(),
        harness: desktop_core::harness::sidecar::HarnessAdapters::unavailable(),
        logs_root: None,
    }
}
