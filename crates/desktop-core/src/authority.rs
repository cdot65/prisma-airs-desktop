//! Native authority: principal-bound handles, finite intents and observation.

use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use crate::delivery::{Admit, Delivery, DeliverySession, Sessions};
use crate::features::{self, FeatureEntry};
use crate::observe::{Subscribers, Subscription, Update};
use crate::wire::Theme;

/// The native-established identity on whose behalf work is requested.
///
/// It is chosen by the native host when it creates a handle, never by a
/// request payload. Restricted Harness principals arrive with the Harness
/// (C19) and receive no intents they cannot hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    Operator,
}

/// A typed request for one desktop action under the caller's authority.
///
/// The union is finite and grows only with shipped, explicitly typed
/// instructions; it never carries method names or arbitrary SDK payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Intent {
    /// Set the desktop-wide appearance preference (not an Academy setting).
    SetAppearance { theme: Theme },
}

/// Native acknowledgement that an Intent was admitted. It is not evidence of
/// operator confirmation or remote completion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Receipt {
    Admitted { request: String },
}

/// Native rejection of an Intent before admission. Sanitized: it names the
/// class of refusal only, never secret or remote content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum Refusal {
    #[error("the presented context is no longer current")]
    StaleContext,
    #[error("the caller lacks the required capability")]
    CapabilityMissing,
    #[error("the request is invalid")]
    InvalidInput,
    #[error("the review is no longer valid")]
    ReviewInvalid,
    #[error("the review was already consumed")]
    ReviewConsumed,
    #[error("the desktop is busy")]
    Busy,
    #[error("protected storage is locked")]
    LockedStore,
    #[error("protected storage access was denied")]
    DeniedStore,
    #[error("this delivery was already processed")]
    AlreadyProcessed,
    #[error("a delivery sequence is missing")]
    SequenceGap,
    #[error("the desktop is shutting down")]
    ShuttingDown,
}

/// The product context a caller asks to observe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewScope {
    Shell,
}

/// A scoped, redacted view of desktop state plus its ordered updates.
#[derive(Debug)]
pub enum Observation {
    Shell {
        snapshot: ShellSnapshot,
        updates: Subscription,
    },
}

/// Redacted shell state: appearance and the feature catalogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ShellSnapshot {
    /// Opaque, monotonically increasing revision (string-encoded u64).
    pub revision: String,
    pub appearance: Theme,
    pub features: Vec<FeatureEntry>,
    /// True only in the distinct test-fixtures identity.
    pub fixture_build: bool,
}

/// Bounds for the core's in-memory structures.
#[derive(Clone, Copy, Debug)]
pub struct CoreConfig {
    /// Settled admission results retained per delivery session.
    pub retained_results: usize,
    /// Notifications queued per observer before it must resynchronize.
    pub observer_queue: usize,
}

impl Default for CoreConfig {
    fn default() -> Self {
        Self {
            retained_results: 256,
            observer_queue: 256,
        }
    }
}

struct State {
    revision: u64,
    appearance: Theme,
    operator_sessions: Sessions,
    subscribers: Subscribers,
    admitted: u64,
}

impl State {
    fn snapshot(&self) -> ShellSnapshot {
        ShellSnapshot {
            revision: self.revision.to_string(),
            appearance: self.appearance,
            features: features::catalogue(),
            fixture_build: cfg!(feature = "test-fixtures"),
        }
    }
}

/// The application core. The native host creates exactly one.
#[derive(Clone)]
pub struct Core {
    state: Arc<Mutex<State>>,
}

impl Core {
    pub fn new(config: CoreConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                revision: 1,
                appearance: Theme::System,
                operator_sessions: Sessions::new(config.retained_results),
                subscribers: Subscribers::new(config.observer_queue),
                admitted: 0,
            })),
        }
    }

    /// The operator handle, created by the host for its packaged main window.
    pub fn operator(&self) -> AuthorityHandle {
        AuthorityHandle {
            principal: Principal::Operator,
            state: Arc::clone(&self.state),
        }
    }
}

/// Principal-scoped entry to native desktop operations and observations.
#[derive(Clone)]
pub struct AuthorityHandle {
    principal: Principal,
    state: Arc<Mutex<State>>,
}

impl AuthorityHandle {
    pub fn principal(&self) -> Principal {
        self.principal
    }

    /// Registers a new delivery session for this principal, retiring the
    /// previous one (for example after a renderer reload).
    pub fn register_delivery_session(&self) -> Result<DeliverySession, Refusal> {
        self.lock()?.operator_sessions.register()
    }

    /// Admits or refuses one delivered Intent. Admission never awaits remote
    /// IO or an operator answer while holding the authoritative state.
    pub async fn dispatch(&self, delivery: Delivery, intent: Intent) -> Result<Receipt, Refusal> {
        let digest = intent_digest(&intent);
        let mut state = self.lock()?;
        match state.operator_sessions.check(&delivery, &digest) {
            Admit::Refused(refusal) => Err(refusal),
            Admit::Duplicate(result) => result,
            Admit::Fresh => {
                let result = admit(&mut state, intent);
                state
                    .operator_sessions
                    .record(&delivery, digest, result.clone());
                result
            }
        }
    }

    /// Returns the current redacted snapshot and subscribes to its updates
    /// atomically, so no update between them is lost.
    pub fn observe(&self, scope: ViewScope) -> Result<Observation, Refusal> {
        let mut state = self.lock()?;
        match scope {
            ViewScope::Shell => {
                let snapshot = state.snapshot();
                let updates = state.subscribers.subscribe().ok_or(Refusal::Busy)?;
                Ok(Observation::Shell { snapshot, updates })
            }
        }
    }

    /// Live observers registered with the core (retired ones are pruned).
    pub fn observer_count(&self) -> usize {
        self.lock().map(|mut s| s.subscribers.len()).unwrap_or(0)
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>, Refusal> {
        // A poisoned lock means an authoritative transition panicked; refuse
        // further work instead of continuing from a possibly torn state.
        self.state.lock().map_err(|_| Refusal::ShuttingDown)
    }
}

fn admit(state: &mut State, intent: Intent) -> Result<Receipt, Refusal> {
    match intent {
        Intent::SetAppearance { theme } => {
            state.admitted += 1;
            if state.appearance != theme {
                state.appearance = theme;
                state.revision += 1;
                let update = Update::Shell(state.snapshot());
                state.subscribers.publish(&update);
            }
            Ok(Receipt::Admitted {
                request: format!("rq-{}", state.admitted),
            })
        }
    }
}

fn intent_digest(intent: &Intent) -> [u8; 32] {
    // Serialization of this closed enum cannot fail; an empty digest input
    // would still only ever match itself.
    let bytes = serde_json::to_vec(intent).unwrap_or_default();
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delivery(session: &str, sequence: u64) -> Delivery {
        Delivery {
            session: session.to_owned(),
            sequence,
        }
    }

    #[test]
    fn observers_beyond_the_cap_are_refused_not_evicted() {
        let core = Core::new(CoreConfig::default());
        let handle = core.operator();
        let held: Vec<Observation> = (0..crate::observe::MAX_SUBSCRIBERS)
            .map(|_| handle.observe(ViewScope::Shell).expect("within cap"))
            .collect();
        assert_eq!(handle.observe(ViewScope::Shell).err(), Some(Refusal::Busy));
        assert_eq!(handle.observer_count(), crate::observe::MAX_SUBSCRIBERS);
        drop(held);
        assert_eq!(handle.observer_count(), 0, "closed observers are pruned");
        assert!(handle.observe(ViewScope::Shell).is_ok());
    }

    #[tokio::test]
    async fn observer_queue_overflow_ends_with_resync() {
        let core = Core::new(CoreConfig {
            retained_results: 4,
            observer_queue: 3,
        });
        let handle = core.operator();
        let Observation::Shell { mut updates, .. } =
            handle.observe(ViewScope::Shell).expect("observe");
        let session = handle.register_delivery_session().expect("session");
        let themes = [Theme::Dark, Theme::Light, Theme::Dark, Theme::Light];
        for (i, theme) in themes.into_iter().enumerate() {
            handle
                .dispatch(
                    delivery(&session.nonce, i as u64 + 1),
                    Intent::SetAppearance { theme },
                )
                .await
                .expect("admitted");
        }
        assert!(matches!(updates.try_next(), Some(Update::Shell(_))));
        assert!(matches!(updates.try_next(), Some(Update::Shell(_))));
        assert_eq!(updates.try_next(), Some(Update::Resync));
        assert_eq!(updates.next().await, None);
        assert_eq!(handle.state.lock().expect("lock").subscribers.len(), 0);
    }
}
