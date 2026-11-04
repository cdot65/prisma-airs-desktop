//! Native-only revocable ownership carried from admission to presentation.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// A native callback may present or copy only while its original owner lives.
/// This token is never a wire value, persisted record or mutation authority.
#[derive(Clone, Debug)]
pub struct PresentationLease(Arc<AtomicBool>);
impl Default for PresentationLease {
    fn default() -> Self {
        Self(Arc::new(AtomicBool::new(true)))
    }
}
impl PresentationLease {
    pub fn is_current(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub(crate) fn revoke(&self) {
        self.0.store(false, Ordering::Release);
    }
}
#[derive(Debug, Default)]
pub(crate) struct PresentationOwner(PresentationLease);
impl PresentationOwner {
    pub(crate) fn lease(&self) -> PresentationLease {
        self.0.clone()
    }
    pub(crate) fn revoke(&self) {
        self.0.revoke();
    }
}
impl Drop for PresentationOwner {
    fn drop(&mut self) {
        self.revoke();
    }
}

/// Fixture-only pause after real admission releases its lock, before a native
/// presentation effect runs. It adds no IPC route and never pauses ordinary work.
#[cfg(any(test, feature = "test-fixtures"))]
#[derive(Default)]
pub struct PresentationGate {
    entered: AtomicBool,
    released: std::sync::Mutex<bool>,
    wake: std::sync::Condvar,
}
#[cfg(any(test, feature = "test-fixtures"))]
impl PresentationGate {
    pub fn entered(&self) -> bool {
        self.entered.load(Ordering::Acquire)
    }
    pub fn release(&self) {
        if let Ok(mut go) = self.released.lock() {
            *go = true;
            self.wake.notify_all();
        }
    }
    pub(crate) fn pause(&self) {
        self.entered.store(true, Ordering::Release);
        if let Ok(go) = self.released.lock() {
            // A broken fixture cannot strand the process after an assertion.
            let _ = self
                .wake
                .wait_timeout_while(go, std::time::Duration::from_secs(15), |go| !*go);
        }
    }
}
