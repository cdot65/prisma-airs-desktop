//! The native operator confirmation and the one-time secret sheet (ADR 0001,
//! DESIGN "Fresh editor input and Configuration review").
//!
//! Both are AppKit `NSAlert` sheets attached to the main window, built and
//! presented on the main thread; they are never a WebView or JS dialog, and
//! a sheet makes the main window modal, so the editor and navigation are
//! held while it is open. A confirmation's completion handler resolves a
//! private, nonce-bound future the core awaits: nothing about it is
//! serialized to the renderer, an unknown nonce is ignored and a nonce
//! resolves once. Retiring a review ends its sheet actively.
//!
//! The secret sheet shows the key in a selectable, non-editable field with
//! "Copy and Close" and "Close". It ends itself after [`REVEAL_FOR`] (first
//! reveal expiry), clearing the field and dropping (zeroizing) the host's
//! copy; a copy is cleaned from the clipboard after
//! [`crate::clipboard_secret::CLEAR_AFTER`] only if it is still ours.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::oneshot;

/// How long a revealed secret stays on screen.
pub const REVEAL_FOR: Duration = Duration::from_secs(60);

/// Open confirmations by nonce: each resolves exactly once.
#[derive(Default)]
pub struct Registry {
    pending: Mutex<HashMap<u64, oneshot::Sender<bool>>>,
}

impl Registry {
    /// Opens confirmation `nonce`; `None` if that nonce is already open.
    pub fn open(&self, nonce: u64) -> Option<oneshot::Receiver<bool>> {
        let mut pending = self.pending.lock().ok()?;
        if pending.contains_key(&nonce) {
            return None;
        }
        let (tx, rx) = oneshot::channel();
        pending.insert(nonce, tx);
        Some(rx)
    }

    /// Resolves `nonce` once; false for an unknown or already resolved one.
    pub fn complete(&self, nonce: u64, accepted: bool) -> bool {
        let sender = self.pending.lock().ok().and_then(|mut p| p.remove(&nonce));
        sender.is_some_and(|tx| tx.send(accepted).is_ok())
    }

    pub fn open_count(&self) -> usize {
        self.pending.lock().map(|p| p.len()).unwrap_or(0)
    }
}

/// The receiver side of a confirmation: false if it was dropped.
pub async fn outcome(receiver: Option<oneshot::Receiver<bool>>) -> bool {
    match receiver {
        Some(rx) => rx.await.unwrap_or(false),
        None => false,
    }
}

#[cfg(target_os = "macos")]
pub use appkit::{NativeConfirmation, NativeSecrets};

#[cfg(target_os = "macos")]
mod appkit {
    #![allow(unsafe_code)]

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::Arc;

    use block2::RcBlock;
    use desktop_core::{ConfirmationRequest, OperatorConfirmation, SecretPresenter};
    use futures_util::future::BoxFuture;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSAlert, NSAlertFirstButtonReturn, NSAlertStyle, NSModalResponse, NSTextField, NSWindow,
    };
    use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
    use tauri::{AppHandle, Manager, Runtime};
    use zeroize::Zeroizing;

    use super::{REVEAL_FOR, Registry, outcome};
    use crate::MAIN_WINDOW;
    use crate::clipboard_secret::{AppKitPasteboard, CLEAR_AFTER, ClipboardGuard};

    /// A shown sheet and the window it is attached to.
    type Shown = (Retained<NSWindow>, Retained<NSWindow>);

    thread_local! {
        /// Shown sheets by key. Only touched on the main thread.
        static SHEETS: RefCell<HashMap<u64, Shown>> = RefCell::new(HashMap::new());
    }

    /// The secret sheet's key in [`SHEETS`] (confirmation nonces are random
    /// u64s; a collision with this key is not a concern in practice and
    /// would only end the wrong sheet).
    const SECRET_SHEET: u64 = u64::MAX;

    fn main_window<R: Runtime>(app: &AppHandle<R>) -> Option<Retained<NSWindow>> {
        let pointer = app.get_webview_window(MAIN_WINDOW)?.ns_window().ok()?;
        // SAFETY: Tauri returns the main window's live NSWindow; retaining
        // it keeps it valid while the sheet uses it.
        unsafe { Retained::retain(pointer.cast::<NSWindow>()) }
    }

    fn end_sheet(key: u64) {
        let shown = SHEETS.with(|s| s.borrow_mut().remove(&key));
        if let Some((parent, sheet)) = shown {
            parent.endSheet(&sheet);
        }
    }

    /// The native operator confirmation for the main window.
    pub struct NativeConfirmation<R: Runtime> {
        pub app: AppHandle<R>,
        pub registry: Arc<Registry>,
    }

    impl<R: Runtime> OperatorConfirmation for NativeConfirmation<R> {
        fn confirm(&self, nonce: u64, request: ConfirmationRequest) -> BoxFuture<'static, bool> {
            let receiver = self.registry.open(nonce);
            if receiver.is_some() {
                let registry = Arc::clone(&self.registry);
                let app = self.app.clone();
                let presented = self.app.run_on_main_thread(move || {
                    if !present(&app, nonce, &request, &registry) {
                        registry.complete(nonce, false);
                    }
                });
                if presented.is_err() {
                    self.registry.complete(nonce, false);
                }
            }
            Box::pin(outcome(receiver))
        }

        fn dismiss(&self, nonce: u64) {
            self.registry.complete(nonce, false);
            let _ = self.app.run_on_main_thread(move || end_sheet(nonce));
        }
    }

    fn present<R: Runtime>(
        app: &AppHandle<R>,
        nonce: u64,
        request: &ConfirmationRequest,
        registry: &Arc<Registry>,
    ) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(window) = main_window(app) else {
            return false;
        };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&request.title));
        alert.setInformativeText(&NSString::from_str(&request.message));
        let confirm = alert.addButtonWithTitle(&NSString::from_str(&request.confirm_label));
        let cancel = alert.addButtonWithTitle(&NSString::from_str("Cancel"));
        if request.destructive {
            alert.setAlertStyle(NSAlertStyle::Critical);
            confirm.setHasDestructiveAction(true);
            // Return cancels a destructive change; it must be chosen.
            confirm.setKeyEquivalent(&NSString::from_str(""));
            cancel.setKeyEquivalent(&NSString::from_str("\r"));
        }
        let registry = Arc::clone(registry);
        let handler = RcBlock::new(move |response: NSModalResponse| {
            registry.complete(nonce, response == NSAlertFirstButtonReturn);
            SHEETS.with(|s| s.borrow_mut().remove(&nonce));
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
        SHEETS.with(|s| s.borrow_mut().insert(nonce, (window, alert.window())));
        true
    }

    /// The transient one-time secret sheet for the main window.
    pub struct NativeSecrets<R: Runtime> {
        pub app: AppHandle<R>,
    }

    impl<R: Runtime> SecretPresenter for NativeSecrets<R> {
        fn reveal(&self, label: String, secret: Zeroizing<String>) {
            let app = self.app.clone();
            let _ = self
                .app
                .run_on_main_thread(move || reveal(&app, &label, secret));
        }

        fn dismiss(&self) {
            let _ = self.app.run_on_main_thread(|| end_sheet(SECRET_SHEET));
        }
    }

    fn reveal<R: Runtime>(app: &AppHandle<R>, label: &str, secret: Zeroizing<String>) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let Some(window) = main_window(app) else {
            return;
        };
        end_sheet(SECRET_SHEET);
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(label));
        alert.setInformativeText(&NSString::from_str(
            "Shown once. Copy it now: it is not stored and cannot be shown again after you close this. \
             A copy is cleared from the clipboard after 60 seconds if it is still there. \
             The configured scan key is unchanged.",
        ));
        let field = NSTextField::labelWithString(&NSString::from_str(&secret), mtm);
        field.setSelectable(true);
        field.setFrame(NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(360.0, 24.0),
        ));
        alert.setAccessoryView(Some(&field));
        alert.addButtonWithTitle(&NSString::from_str("Copy and Close"));
        alert.addButtonWithTitle(&NSString::from_str("Close"));
        let held = std::cell::RefCell::new(Some(secret));
        let shown_field = field.clone();
        let handler = RcBlock::new(move |response: NSModalResponse| {
            // The host's copy is dropped (zeroized) whatever the choice.
            let secret = held.borrow_mut().take();
            shown_field.setStringValue(&NSString::from_str(""));
            SHEETS.with(|s| s.borrow_mut().remove(&SECRET_SHEET));
            if response != NSAlertFirstButtonReturn {
                return;
            }
            let Some(secret) = secret else { return };
            let guard = ClipboardGuard::new(AppKitPasteboard::general());
            if let Some(copied) = guard.copy(&secret) {
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(CLEAR_AFTER).await;
                    let _ = tokio::task::spawn_blocking(move || {
                        ClipboardGuard::new(AppKitPasteboard::general()).cleanup(&copied)
                    })
                    .await;
                });
            }
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
        SHEETS.with(|s| {
            s.borrow_mut()
                .insert(SECRET_SHEET, (window, alert.window()))
        });
        // First reveal expiry: the sheet ends itself.
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(REVEAL_FOR).await;
            let _ = app.run_on_main_thread(|| end_sheet(SECRET_SHEET));
        });
    }

    impl<R: Runtime> NativeConfirmation<R> {
        pub fn new(app: AppHandle<R>) -> Self {
            Self {
                app,
                registry: Arc::default(),
            }
        }
    }
}

/// Non-macOS builds have no native sheet: confirmations are declined and
/// secrets are never shown.
#[cfg(not(target_os = "macos"))]
pub struct Unsupported;

#[cfg(not(target_os = "macos"))]
impl desktop_core::OperatorConfirmation for Unsupported {
    fn confirm(
        &self,
        _nonce: u64,
        _request: desktop_core::ConfirmationRequest,
    ) -> futures_util::future::BoxFuture<'static, bool> {
        Box::pin(async { false })
    }
    fn dismiss(&self, _nonce: u64) {}
}

#[cfg(not(target_os = "macos"))]
impl desktop_core::SecretPresenter for Unsupported {
    fn reveal(&self, _label: String, _secret: zeroize::Zeroizing<String>) {}
    fn dismiss(&self) {}
}
