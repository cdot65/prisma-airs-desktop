//! Sleep (C19, DESIGN §3): when the Mac is about to sleep, a running
//! Harness conversation is ended, so no authorized engine is left running
//! unattended across sleep. The observer is registered once, for the app's
//! life, on NSWorkspace's notification center.

// AppKit notification calls are FFI.
#![allow(unsafe_code)]

use std::ptr::NonNull;

use block2::RcBlock;
use desktop_core::Core;
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
use objc2_foundation::NSNotification;

/// Ends the Harness conversation whenever the Mac is about to sleep.
pub fn observe_sleep(core: Core) {
    let block = RcBlock::new(move |_: NonNull<NSNotification>| core.system_will_sleep());
    // SAFETY: AppKit globals read on the main thread during setup; the
    // block captures only the core, which is Send + Sync.
    unsafe {
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        let token = center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceWillSleepNotification),
            None,
            None,
            &block,
        );
        // Observed for the app's whole life.
        std::mem::forget(token);
    }
}

/// Posts the system's will-sleep notification in this process, so native
/// acceptance exercises the production observer (fixture builds only).
#[cfg(feature = "test-fixtures")]
pub fn post_will_sleep() {
    // SAFETY: posting a notification name AppKit defines, with no object.
    unsafe {
        NSWorkspace::sharedWorkspace()
            .notificationCenter()
            .postNotificationName_object(NSWorkspaceWillSleepNotification, None);
    }
}
