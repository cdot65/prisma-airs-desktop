//! Bounded, ordered observation updates with resynchronization on overflow.
//!
//! Observers never apply backpressure to authority. When a subscriber's queue
//! has one slot left, the core enqueues [`Update::Resync`] in that slot and
//! retires the subscriber: it drains the earlier updates, sees `Resync`, then
//! the end of the subscription, and observes again for a fresh snapshot.
//! Closing a subscription never cancels work.

use tokio::sync::mpsc;

use crate::authority::ShellSnapshot;

/// An ordered update delivered to one subscriber.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Update {
    Shell(ShellSnapshot),
    /// Updates were dropped; observe again for a fresh snapshot.
    Resync,
}

/// The receiving end of one observation.
#[derive(Debug)]
pub struct Subscription {
    rx: mpsc::Receiver<Update>,
}

impl Subscription {
    /// Waits for the next update; `None` once the core retires the subscriber.
    pub async fn next(&mut self) -> Option<Update> {
        self.rx.recv().await
    }

    /// Returns a queued update without waiting.
    pub fn try_next(&mut self) -> Option<Update> {
        self.rx.try_recv().ok()
    }
}

/// Upper bound on concurrently registered observers across the core.
pub const MAX_SUBSCRIBERS: usize = 32;

pub(crate) struct Subscribers {
    capacity: usize,
    list: Vec<mpsc::Sender<Update>>,
}

impl Subscribers {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(2),
            list: Vec::new(),
        }
    }

    /// Registers a subscriber after pruning closed ones. At the cap the new
    /// subscriber is refused; existing observers are never silently evicted.
    pub(crate) fn subscribe(&mut self) -> Option<Subscription> {
        self.prune();
        if self.list.len() >= MAX_SUBSCRIBERS {
            return None;
        }
        let (tx, rx) = mpsc::channel(self.capacity);
        self.list.push(tx);
        Some(Subscription { rx })
    }

    pub(crate) fn prune(&mut self) {
        self.list.retain(|tx| !tx.is_closed());
    }

    pub(crate) fn len(&mut self) -> usize {
        self.prune();
        self.list.len()
    }

    pub(crate) fn publish(&mut self, update: &Update) {
        self.list.retain(|tx| {
            if tx.is_closed() {
                return false;
            }
            if tx.capacity() <= 1 {
                // Last slot: owe a resync and retire this subscriber.
                let _ = tx.try_send(Update::Resync);
                return false;
            }
            tx.try_send(update.clone()).is_ok()
        });
    }
}
