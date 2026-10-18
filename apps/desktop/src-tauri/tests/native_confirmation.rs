//! The native operator confirmation's private completion: nonce-bound,
//! single-use, unreachable from the renderer. The AppKit sheet itself is
//! exercised by native acceptance (C09-N*); these cover the registry the
//! sheet's completion handler resolves and the wire contract.

use desktop_host::native_confirmation::{REVEAL_FOR, Registry, outcome};

#[tokio::test]
async fn a_confirmation_resolves_once_and_only_for_its_nonce() {
    let registry = Registry::default();
    let pending = registry.open(41);
    assert!(pending.is_some());
    // A competing open of the same nonce is refused.
    assert!(registry.open(41).is_none());
    // A forged nonce resolves nothing.
    assert!(!registry.complete(7, true));
    assert_eq!(registry.open_count(), 1);
    assert!(registry.complete(41, true));
    // Repetition (a duplicate or late callback) is ignored.
    assert!(!registry.complete(41, false));
    assert!(outcome(pending).await);
}

#[tokio::test]
async fn a_dismissed_or_abandoned_confirmation_is_a_denial() {
    let registry = Registry::default();
    let dismissed = registry.open(1);
    // Retirement resolves it false before the sheet's own callback.
    assert!(registry.complete(1, false));
    assert!(
        !registry.complete(1, true),
        "the late sheet callback cannot accept"
    );
    assert!(!outcome(dismissed).await);
    // A sender dropped without an answer is not an approval.
    let abandoned = Registry::default();
    let receiver = abandoned.open(2);
    drop(abandoned);
    assert!(!outcome(receiver).await);
    assert!(!outcome(None).await);
}

#[test]
fn no_wire_intent_can_carry_an_approval() {
    for forged in [
        serde_json::json!({"kind": "confirmApply", "review": "rv-1", "accepted": true}),
        serde_json::json!({"kind": "requestApply", "review": "rv-1", "draft": {"kind": "topics",
            "action": "update", "target": "t", "body": "{}"}, "confirmation": "x", "approved": true}),
        serde_json::json!({"kind": "settleConfirmation", "nonce": 1, "accepted": true}),
    ] {
        assert!(
            serde_json::from_value::<desktop_core::Intent>(forged.clone()).is_err(),
            "{forged}"
        );
    }
}

#[test]
fn first_reveal_and_clipboard_timers_are_separate() {
    assert_eq!(REVEAL_FOR.as_secs(), 60);
    assert_eq!(desktop_host::clipboard_secret::CLEAR_AFTER.as_secs(), 60);
}
