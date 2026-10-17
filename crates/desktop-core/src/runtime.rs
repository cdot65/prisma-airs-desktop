//! Bounded remote operations: Queued → DispatchClaimed → Settled.
//!
//! The registry lives inside the authority state, so context retirement,
//! cancellation and dispatch claiming share one lock (DESIGN §1). Admission
//! refuses with `Busy` when the queue is full by count or bytes; an accepted
//! operation is never dropped. Work starts on the application's Tokio runtime
//! only after it is claimed for dispatch. Queued work that is cancelled,
//! retired or past its deadline settles NotSent. Once dispatch may have
//! begun, cancellation or timeout alone yields Unknown unless the work itself
//! reports stronger evidence. Settlement is single-use; late completions are
//! ignored. Nothing here retries or replays work.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::Serialize;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::authority::Refusal;

/// Explicit operation bounds (DESIGN: start at 4 active, 64 queued, 64MiB).
#[derive(Clone, Copy, Debug)]
pub struct OperationLimits {
    pub max_active: usize,
    pub max_queued: usize,
    pub max_queued_bytes: usize,
    /// Interactive deadline, measured from submission.
    pub interactive_budget: Duration,
    /// Settled results retained for observation.
    pub retained_settled: usize,
}

impl Default for OperationLimits {
    fn default() -> Self {
        Self {
            max_active: 4,
            max_queued: 64,
            max_queued_bytes: 64 * 1024 * 1024,
            interactive_budget: Duration::from_secs(60),
            retained_settled: 256,
        }
    }
}

/// Native-issued operation reference.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(transparent)]
pub struct OperationId(String);

impl OperationId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> serde::Deserialize<'de> for OperationId {
    /// Accepts only the native-issued shape (`op-` + decimal digits).
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        let digits = text.strip_prefix("op-").unwrap_or_default();
        let ok =
            !digits.is_empty() && digits.len() <= 20 && digits.bytes().all(|b| b.is_ascii_digit());
        ok.then_some(Self(text))
            .ok_or_else(|| serde::de::Error::custom("not an operation reference"))
    }
}

/// What is known about the remote effect of an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// Nothing reached the remote service.
    NotSent,
    /// The service reliably refused it without effect.
    Rejected,
    /// The service reported completion.
    Completed,
    /// It may or may not have taken effect.
    Unknown,
}

/// Why the desktop stopped waiting, separately from the remote outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    Cancelled,
    ContextRetired,
    Deadline,
    ShuttingDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Settlement {
    pub outcome: Outcome,
    pub stop: Option<StopReason>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationState {
    Queued,
    DispatchClaimed,
    Settled(Settlement),
}

/// Handed to work when it is claimed for dispatch.
pub struct Claim {
    pub cancel: CancellationToken,
    pub deadline: Instant,
}

pub type WorkFuture = Pin<Box<dyn Future<Output = Outcome> + Send>>;
/// One remote operation: started once, with its claim.
pub type Work = Box<dyn FnOnce(Claim) -> WorkFuture + Send>;

struct Queued {
    id: OperationId,
    context: String,
    bytes: usize,
    deadline: Instant,
    cancelled: Option<StopReason>,
    work: Work,
}

struct Active {
    context: String,
    cancel: CancellationToken,
    stop: Option<StopReason>,
}

/// Work to start outside the lock (see [`spawn`]).
pub struct Start {
    pub id: OperationId,
    pub claim: Claim,
    pub work: Work,
}

/// The operation registry.
pub struct Operations {
    limits: OperationLimits,
    issued: u64,
    queue: VecDeque<Queued>,
    queued_bytes: usize,
    active: HashMap<OperationId, Active>,
    settled: VecDeque<(OperationId, Settlement)>,
}

impl Operations {
    pub fn new(limits: OperationLimits) -> Self {
        Self {
            limits,
            issued: 0,
            queue: VecDeque::new(),
            queued_bytes: 0,
            active: HashMap::new(),
            settled: VecDeque::new(),
        }
    }

    pub fn limits(&self) -> OperationLimits {
        self.limits
    }

    /// Admits work bound to `context`. The deadline is measured from now.
    /// Returns its reference and any work that may start immediately.
    pub fn admit(
        &mut self,
        context: &str,
        bytes: usize,
        work: Work,
        now: Instant,
    ) -> Result<(OperationId, Vec<Start>), Refusal> {
        let full = self.queue.len() >= self.limits.max_queued
            || self.queued_bytes.saturating_add(bytes) > self.limits.max_queued_bytes;
        if full || bytes > self.limits.max_queued_bytes {
            return Err(Refusal::Busy);
        }
        self.issued += 1;
        let id = OperationId(format!("op-{}", self.issued));
        self.queue.push_back(Queued {
            id: id.clone(),
            context: context.to_owned(),
            bytes,
            deadline: now + self.limits.interactive_budget,
            cancelled: None,
            work,
        });
        self.queued_bytes += bytes;
        Ok((id, self.claim(now)))
    }

    /// Claims queued work for dispatch while capacity allows.
    fn claim(&mut self, now: Instant) -> Vec<Start> {
        let mut starts = Vec::new();
        while self.active.len() < self.limits.max_active {
            let Some(next) = self.queue.pop_front() else {
                break;
            };
            self.queued_bytes -= next.bytes;
            let stop = next
                .cancelled
                .or_else(|| (now >= next.deadline).then_some(StopReason::Deadline));
            if let Some(stop) = stop {
                self.record(
                    next.id,
                    Settlement {
                        outcome: Outcome::NotSent,
                        stop: Some(stop),
                    },
                );
                continue;
            }
            let cancel = CancellationToken::new();
            self.active.insert(
                next.id.clone(),
                Active {
                    context: next.context,
                    cancel: cancel.clone(),
                    stop: None,
                },
            );
            starts.push(Start {
                id: next.id,
                claim: Claim {
                    cancel,
                    deadline: next.deadline,
                },
                work: next.work,
            });
        }
        starts
    }

    /// Settles claimed work once; later or unknown completions are ignored.
    pub fn complete(&mut self, id: &OperationId, outcome: Outcome, now: Instant) -> Vec<Start> {
        if let Some(active) = self.active.remove(id) {
            self.record(
                id.clone(),
                Settlement {
                    outcome,
                    stop: active.stop,
                },
            );
        }
        self.claim(now)
    }

    /// Stops queued work (NotSent) or signals claimed work. Returns whether
    /// the operation was still unsettled.
    pub fn cancel(
        &mut self,
        id: &OperationId,
        reason: StopReason,
        now: Instant,
    ) -> (bool, Vec<Start>) {
        if let Some(queued) = self.queue.iter_mut().find(|q| &q.id == id) {
            queued.cancelled = Some(reason);
            return (true, self.settle_cancelled_queue(now));
        }
        if let Some(active) = self.active.get_mut(id) {
            active.stop.get_or_insert(reason);
            active.cancel.cancel();
            return (true, Vec::new());
        }
        (false, Vec::new())
    }

    /// Retires every operation not bound to `keep`: queued work settles
    /// NotSent, claimed work is cancelled (its outcome then comes from the
    /// work, normally Unknown once dispatch began).
    pub fn retire_contexts(&mut self, keep: Option<&str>, now: Instant) -> Vec<Start> {
        for queued in self.queue.iter_mut() {
            if keep != Some(queued.context.as_str()) {
                queued.cancelled.get_or_insert(StopReason::ContextRetired);
            }
        }
        for active in self.active.values_mut() {
            if keep != Some(active.context.as_str()) {
                active.stop.get_or_insert(StopReason::ContextRetired);
                active.cancel.cancel();
            }
        }
        self.settle_cancelled_queue(now)
    }

    fn settle_cancelled_queue(&mut self, now: Instant) -> Vec<Start> {
        let mut kept = VecDeque::new();
        while let Some(queued) = self.queue.pop_front() {
            match queued.cancelled {
                Some(stop) => {
                    self.queued_bytes -= queued.bytes;
                    self.record(
                        queued.id,
                        Settlement {
                            outcome: Outcome::NotSent,
                            stop: Some(stop),
                        },
                    );
                }
                None => kept.push_back(queued),
            }
        }
        self.queue = kept;
        self.claim(now)
    }

    fn record(&mut self, id: OperationId, settlement: Settlement) {
        self.settled.push_back((id, settlement));
        while self.settled.len() > self.limits.retained_settled.max(1) {
            self.settled.pop_front();
        }
    }

    pub fn state(&self, id: &OperationId) -> Option<OperationState> {
        if self.queue.iter().any(|q| &q.id == id) {
            return Some(OperationState::Queued);
        }
        if self.active.contains_key(id) {
            return Some(OperationState::DispatchClaimed);
        }
        self.settled
            .iter()
            .rev()
            .find(|(s, _)| s == id)
            .map(|(_, settlement)| OperationState::Settled(*settlement))
    }

    pub fn queued(&self) -> (usize, usize) {
        (self.queue.len(), self.queued_bytes)
    }

    pub fn active(&self) -> usize {
        self.active.len()
    }
}

/// Runs claimed work on the current Tokio runtime and reports its outcome.
/// The deadline is enforced here too: work still running at its deadline is
/// cancelled and settles Unknown (dispatch may have begun).
pub fn spawn<F>(start: Start, settle: F)
where
    F: FnOnce(OperationId, Outcome, Option<StopReason>) + Send + 'static,
{
    let Start { id, claim, work } = start;
    let deadline = claim.deadline;
    let cancel = claim.cancel.clone();
    tokio::spawn(async move {
        let future = work(claim);
        tokio::select! {
            outcome = future => settle(id, outcome, None),
            () = tokio::time::sleep_until(deadline) => {
                cancel.cancel();
                settle(id, Outcome::Unknown, Some(StopReason::Deadline));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> Work {
        Box::new(|_claim| Box::pin(async { Outcome::Completed }))
    }

    fn limits(active: usize, queued: usize, bytes: usize) -> OperationLimits {
        OperationLimits {
            max_active: active,
            max_queued: queued,
            max_queued_bytes: bytes,
            ..OperationLimits::default()
        }
    }

    #[test]
    fn admission_is_bounded_by_count_and_bytes_without_dropping_work() {
        let now = Instant::now();
        let mut ops = Operations::new(limits(1, 2, 100));
        let (first, starts) = ops.admit("cx", 10, idle(), now).expect("first");
        assert_eq!(starts.len(), 1, "capacity starts it at once");
        let (second, _) = ops.admit("cx", 60, idle(), now).expect("queued");
        assert_eq!(
            ops.admit("cx", 50, idle(), now).err(),
            Some(Refusal::Busy),
            "bytes"
        );
        let (third, _) = ops.admit("cx", 40, idle(), now).expect("queued");
        assert_eq!(
            ops.admit("cx", 0, idle(), now).err(),
            Some(Refusal::Busy),
            "count"
        );
        assert_eq!(ops.state(&first), Some(OperationState::DispatchClaimed));
        assert_eq!(ops.state(&second), Some(OperationState::Queued));
        let next = ops.complete(&first, Outcome::Completed, now);
        assert_eq!(
            next.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
            vec![second.clone()]
        );
        assert_eq!(ops.state(&third), Some(OperationState::Queued));
    }

    #[test]
    fn retirement_settles_queued_not_sent_and_cancels_claimed() {
        let now = Instant::now();
        let mut ops = Operations::new(limits(1, 8, 1000));
        let (claimed, starts) = ops.admit("old", 1, idle(), now).expect("claimed");
        let token = starts[0].claim.cancel.clone();
        let (queued, _) = ops.admit("old", 1, idle(), now).expect("queued");
        let (other, _) = ops.admit("new", 1, idle(), now).expect("queued");
        let started = ops.retire_contexts(Some("new"), now);
        assert!(token.is_cancelled());
        assert_eq!(
            ops.state(&queued),
            Some(OperationState::Settled(Settlement {
                outcome: Outcome::NotSent,
                stop: Some(StopReason::ContextRetired)
            }))
        );
        assert!(started.is_empty(), "the claimed slot is still busy");
        let next = ops.complete(&claimed, Outcome::Unknown, now);
        assert_eq!(
            ops.state(&claimed),
            Some(OperationState::Settled(Settlement {
                outcome: Outcome::Unknown,
                stop: Some(StopReason::ContextRetired)
            }))
        );
        assert_eq!(next[0].id, other);
    }

    #[test]
    fn settlement_is_single_use_and_queued_deadlines_are_not_sent() {
        let now = Instant::now();
        let mut ops = Operations::new(limits(1, 8, 1000));
        let (claimed, _) = ops.admit("cx", 1, idle(), now).expect("claimed");
        let (late, _) = ops.admit("cx", 1, idle(), now).expect("queued");
        let later = now + OperationLimits::default().interactive_budget + Duration::from_secs(1);
        let next = ops.complete(&claimed, Outcome::Completed, later);
        assert!(next.is_empty(), "past its deadline before dispatch");
        assert_eq!(
            ops.state(&late),
            Some(OperationState::Settled(Settlement {
                outcome: Outcome::NotSent,
                stop: Some(StopReason::Deadline)
            }))
        );
        ops.complete(&claimed, Outcome::Rejected, later);
        assert_eq!(
            ops.state(&claimed),
            Some(OperationState::Settled(Settlement {
                outcome: Outcome::Completed,
                stop: None
            })),
            "a duplicate completion cannot change the settlement"
        );
    }

    #[test]
    fn cancelling_queued_work_is_not_sent_and_claimed_work_is_signalled() {
        let now = Instant::now();
        let mut ops = Operations::new(limits(1, 8, 1000));
        let (claimed, starts) = ops.admit("cx", 1, idle(), now).expect("claimed");
        let (queued, _) = ops.admit("cx", 1, idle(), now).expect("queued");
        assert!(ops.cancel(&queued, StopReason::Cancelled, now).0);
        assert_eq!(
            ops.state(&queued),
            Some(OperationState::Settled(Settlement {
                outcome: Outcome::NotSent,
                stop: Some(StopReason::Cancelled)
            }))
        );
        assert!(ops.cancel(&claimed, StopReason::Cancelled, now).0);
        assert!(starts[0].claim.cancel.is_cancelled());
        assert_eq!(
            ops.state(&claimed),
            Some(OperationState::DispatchClaimed),
            "awaits the work's own outcome"
        );
        assert!(
            !ops.cancel(&queued, StopReason::Cancelled, now).0,
            "already settled"
        );
    }
}
