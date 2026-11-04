//! Best-effort clipboard cleanup of a copied one-time secret: only this
//! desktop's own, unchanged copy is ever cleared. Runs against a scripted
//! pasteboard and a real, uniquely named AppKit pasteboard (never the
//! general clipboard).
#![allow(unsafe_code)]

use std::cell::RefCell;

use desktop_host::clipboard_secret::{ClipboardGuard, Pasteboard};

#[derive(Default)]
struct Board {
    count: RefCell<isize>,
    text: RefCell<Option<String>>,
    owner: RefCell<Option<String>>,
}

impl Board {
    fn foreign_write(&self, text: &str) {
        *self.count.borrow_mut() += 1;
        *self.text.borrow_mut() = Some(text.to_owned());
        *self.owner.borrow_mut() = None;
    }
}

impl Pasteboard for &Board {
    fn write(&self, secret: &str, owner: &str) -> Option<isize> {
        *self.count.borrow_mut() += 1;
        *self.text.borrow_mut() = Some(secret.to_owned());
        *self.owner.borrow_mut() = Some(owner.to_owned());
        Some(*self.count.borrow())
    }
    fn change_count(&self) -> isize {
        *self.count.borrow()
    }
    fn owner(&self) -> Option<String> {
        self.owner.borrow().clone()
    }
    fn clear(&self) {
        *self.count.borrow_mut() += 1;
        *self.text.borrow_mut() = None;
        *self.owner.borrow_mut() = None;
    }
}

#[test]
fn only_an_unchanged_own_copy_is_cleared() {
    let board = Board::default();
    let guard = ClipboardGuard::new(&board);
    let copied = guard.copy("SYNTHETIC-ONE-TIME").expect("copied");
    assert_eq!(board.text.borrow().as_deref(), Some("SYNTHETIC-ONE-TIME"));
    assert!(guard.cleanup(&copied));
    assert_eq!(*board.text.borrow(), None);
    // Something else wrote since: left alone.
    let copied = guard.copy("SYNTHETIC-ONE-TIME").expect("copied");
    board.foreign_write("operator's own text");
    assert!(!guard.cleanup(&copied));
    assert_eq!(board.text.borrow().as_deref(), Some("operator's own text"));
    // A later copy of ours supersedes an earlier one's cleanup.
    let first = guard.copy("A").expect("copied");
    let second = guard.copy("B").expect("copied");
    assert_ne!(first.owner, second.owner, "owner markers are random");
    assert!(!guard.cleanup(&first));
    assert!(guard.cleanup(&second));
}

#[cfg(target_os = "macos")]
#[test]
fn a_real_pasteboard_keeps_foreign_contents_and_marks_the_secret_concealed() {
    use desktop_host::clipboard_secret::{
        AppKitPasteboard, CONCEALED_TYPE, OWNER_TYPE, TRANSIENT_TYPE,
    };
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;

    let board = NSPasteboard::pasteboardWithUniqueName();
    let guard = ClipboardGuard::new(AppKitPasteboard(board.clone()));
    let copied = guard.copy("SYNTHETIC-ONE-TIME").expect("copied");
    // SAFETY: an AppKit constant string.
    let string_type = unsafe { NSPasteboardTypeString };
    assert_eq!(
        board
            .stringForType(string_type)
            .map(|s| s.to_string())
            .as_deref(),
        Some("SYNTHETIC-ONE-TIME")
    );
    assert_eq!(
        board
            .stringForType(&NSString::from_str(OWNER_TYPE))
            .map(|s| s.to_string()),
        Some(copied.owner.clone())
    );
    for hint in [CONCEALED_TYPE, TRANSIENT_TYPE] {
        assert!(
            board.dataForType(&NSString::from_str(hint)).is_some(),
            "{hint}"
        );
    }
    // A foreign write after our copy: cleanup leaves it.
    board.clearContents();
    board.setString_forType(&NSString::from_str("operator text"), string_type);
    assert!(!guard.cleanup(&copied));
    assert_eq!(
        board
            .stringForType(string_type)
            .map(|s| s.to_string())
            .as_deref(),
        Some("operator text")
    );
    // Our own unchanged copy: cleared.
    let copied = guard.copy("SYNTHETIC-ONE-TIME").expect("copied");
    assert!(guard.cleanup(&copied));
    assert_eq!(board.stringForType(string_type), None);
}
