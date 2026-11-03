//! Fixture-only self-capture of the main window's WebView rendering.
//!
//! Native acceptance needs pixels (for example the keyboard focus ring), but
//! screen capture requires a Screen Recording grant for the automation host.
//! The app can instead snapshot its own WKWebView with
//! `takeSnapshotWithConfiguration:completionHandler:`, which renders exactly
//! what the window shows. This module compiles only with `test-fixtures`
//! (distinct fixture identity); production builds contain neither the code
//! nor its menu item. Captures go to a fixed temporary directory.
#![allow(unsafe_code)]

use std::path::PathBuf;

use block2::RcBlock;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
use objc2_foundation::{NSDictionary, NSError};
use objc2_web_kit::WKWebView;
use tauri::{Runtime, WebviewWindow};

/// Directory receiving fixture captures.
pub fn capture_dir() -> PathBuf {
    std::env::temp_dir().join("prisma-airs-desktop-fixture-captures")
}

/// Snapshots the window's WebView to `<capture_dir>/capture-<millis>.png`.
pub fn capture<R: Runtime>(window: &WebviewWindow<R>) {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = capture_dir();
    let path = dir.join(format!("capture-{millis}.png"));
    let _ = window.with_webview(move |platform| {
        let pointer = platform.inner().cast::<WKWebView>();
        // SAFETY: on macOS Tauri's platform webview handle is the window's
        // live WKWebView; with_webview runs this closure on the main thread
        // while the webview exists.
        let Some(webview) = (unsafe { pointer.as_ref() }) else {
            return;
        };
        let handler = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
            // SAFETY: WebKit passes either null or a valid NSImage for the
            // duration of the completion handler.
            let Some(image) = (unsafe { image.as_ref() }) else {
                return;
            };
            if let Some(bytes) = png(image) {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(&path, bytes);
            }
        });
        // SAFETY: a nil configuration snapshots the visible bounds; the block
        // is retained by WebKit until it is called.
        unsafe { webview.takeSnapshotWithConfiguration_completionHandler(None, &handler) };
    });
}

fn png(image: &NSImage) -> Option<Vec<u8>> {
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
    // SAFETY: an empty property dictionary is a valid PNG encoding request.
    let data = unsafe {
        rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    Some(data.to_vec())
}
