//! Copying a one-time secret to the clipboard with best-effort cleanup
//! (DESIGN "Fresh editor input and Configuration review"; v1
//! `src/ui/mod.rs` copied with a marker and cleared after 60 s only if the
//! marker was still there).
//!
//! A copy writes the secret, a random owner marker under a private type and
//! the `org.nspasteboard.ConcealedType`/`TransientType` hints, then records
//! the pasteboard's change count. Cleanup clears the pasteboard only when
//! both the change count and the owner marker are still this copy's: if
//! anything else wrote to the clipboard since, it is left alone. macOS has
//! no atomic compare-and-swap for the pasteboard, so this is best effort;
//! the hints are honored only by cooperating clipboard managers.

use std::time::Duration;

/// How long a copied secret may stay on the clipboard (v1: 60 s).
pub const CLEAR_AFTER: Duration = Duration::from_secs(60);
/// The private pasteboard type carrying this desktop's owner marker.
pub const OWNER_TYPE: &str = "io.cdot.prisma-airs-desktop.v2.secret-owner";
/// Clipboard-manager hints (nspasteboard.org).
pub const CONCEALED_TYPE: &str = "org.nspasteboard.ConcealedType";
pub const TRANSIENT_TYPE: &str = "org.nspasteboard.TransientType";

/// The pasteboard operations cleanup depends on.
pub trait Pasteboard {
    /// Replaces the contents with `secret`, the `owner` marker and the
    /// concealment hints; the change count after writing, if it succeeded.
    fn write(&self, secret: &str, owner: &str) -> Option<isize>;
    fn change_count(&self) -> isize;
    /// The owner marker currently on the pasteboard, if any.
    fn owner(&self) -> Option<String>;
    fn clear(&self);
}

/// One copy this desktop made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    pub owner: String,
    pub change_count: isize,
}

/// Copies secrets and cleans up only its own copies.
pub struct ClipboardGuard<P: Pasteboard> {
    board: P,
}

impl<P: Pasteboard> ClipboardGuard<P> {
    pub fn new(board: P) -> Self {
        Self { board }
    }

    pub fn copy(&self, secret: &str) -> Option<Copied> {
        let mut entropy = [0u8; 16];
        getrandom::fill(&mut entropy).ok()?;
        let owner = entropy
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let change_count = self.board.write(secret, &owner)?;
        Some(Copied {
            owner,
            change_count,
        })
    }

    /// Clears the pasteboard if it still holds `copied` (and nothing was
    /// written since); true when it cleared.
    pub fn cleanup(&self, copied: &Copied) -> bool {
        let ours = self.board.change_count() == copied.change_count
            && self.board.owner().as_deref() == Some(copied.owner.as_str());
        if ours {
            self.board.clear();
        }
        ours
    }
}

#[cfg(target_os = "macos")]
mod appkit {
    #![allow(unsafe_code)]

    use objc2::rc::Retained;
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSData, NSString};

    use super::{CONCEALED_TYPE, OWNER_TYPE, Pasteboard, TRANSIENT_TYPE};

    /// An AppKit pasteboard (the general one, or a unique one in tests).
    pub struct AppKitPasteboard(pub Retained<NSPasteboard>);

    impl AppKitPasteboard {
        pub fn general() -> Self {
            Self(NSPasteboard::generalPasteboard())
        }
    }

    fn string_type() -> &'static NSString {
        // SAFETY: an AppKit constant string, valid for the process lifetime.
        unsafe { NSPasteboardTypeString }
    }

    impl Pasteboard for AppKitPasteboard {
        fn write(&self, secret: &str, owner: &str) -> Option<isize> {
            let board = &self.0;
            board.clearContents();
            let written = board.setString_forType(&NSString::from_str(secret), string_type())
                && board
                    .setString_forType(&NSString::from_str(owner), &NSString::from_str(OWNER_TYPE))
                && board.setData_forType(Some(&NSData::new()), &NSString::from_str(CONCEALED_TYPE))
                && board.setData_forType(Some(&NSData::new()), &NSString::from_str(TRANSIENT_TYPE));
            written.then(|| board.changeCount())
        }

        fn change_count(&self) -> isize {
            self.0.changeCount()
        }

        fn owner(&self) -> Option<String> {
            self.0
                .stringForType(&NSString::from_str(OWNER_TYPE))
                .map(|s| s.to_string())
        }

        fn clear(&self) {
            self.0.clearContents();
        }
    }
}

#[cfg(target_os = "macos")]
pub use appkit::AppKitPasteboard;
