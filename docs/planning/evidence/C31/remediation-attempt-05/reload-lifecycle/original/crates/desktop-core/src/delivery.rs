//! Delivery sessions: transport deduplication of Intent deliveries.
//!
//! A delivery session is the current native-registered channel for one
//! caller's Intent deliveries (GLOSSARY). Its sequence distinguishes a
//! duplicate delivery from an explicit new action; it is not authorization and
//! not a ContextRef. Registering a new session for a principal retires the
//! previous one, so a reloaded renderer can never replay old deliveries.

use std::collections::VecDeque;

use crate::authority::{Receipt, Refusal};

/// Highest sequence a session accepts; afterwards the caller rotates sessions.
pub const SEQUENCE_LIMIT: u64 = u64::MAX;

/// Identifies one delivery: the session nonce and its sequence number.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub session: String,
    pub sequence: u64,
}

/// A freshly registered session handed to the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeliverySession {
    pub nonce: String,
    pub next_sequence: u64,
}

pub(crate) type AdmissionResult = Result<Receipt, Refusal>;

/// What the session table decided about a delivery before domain admission.
pub(crate) enum Admit {
    /// A new sequence: run admission, then [`Sessions::record`] the result.
    Fresh,
    /// A retained duplicate with identical input: return the same result.
    Duplicate(AdmissionResult),
    /// Refused without consuming or advancing the sequence.
    Refused(Refusal),
}

struct Retained {
    sequence: u64,
    digest: [u8; 32],
    result: AdmissionResult,
}

struct Session {
    nonce: String,
    high_water: u64,
    retained: VecDeque<Retained>,
}

/// Per-principal delivery sessions. Only the current session may deliver.
pub(crate) struct Sessions {
    current: Option<Session>,
    retain: usize,
    issued: u64,
}

impl Sessions {
    pub(crate) fn new(retain: usize) -> Self {
        Self {
            current: None,
            retain: retain.max(1),
            issued: 0,
        }
    }

    /// Registers a new session and retires the previous one.
    pub(crate) fn register(&mut self) -> Result<DeliverySession, Refusal> {
        let mut entropy = [0u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| Refusal::ShuttingDown)?;
        self.issued += 1;
        let nonce = session_nonce(self.issued, &entropy);
        self.current = Some(Session {
            nonce: nonce.clone(),
            high_water: 0,
            retained: VecDeque::new(),
        });
        Ok(DeliverySession {
            nonce,
            next_sequence: 1,
        })
    }

    pub(crate) fn check(&self, delivery: &Delivery, digest: &[u8; 32]) -> Admit {
        let Some(session) = self.current.as_ref() else {
            return Admit::Refused(Refusal::StaleContext);
        };
        if session.nonce != delivery.session {
            return Admit::Refused(Refusal::StaleContext);
        }
        if delivery.sequence == 0 {
            return Admit::Refused(Refusal::InvalidInput);
        }
        if delivery.sequence <= session.high_water {
            return match session
                .retained
                .iter()
                .find(|r| r.sequence == delivery.sequence)
            {
                Some(r) if &r.digest == digest => Admit::Duplicate(r.result.clone()),
                Some(_) => Admit::Refused(Refusal::InvalidInput),
                None => Admit::Refused(Refusal::AlreadyProcessed),
            };
        }
        match session.high_water.checked_add(1) {
            Some(next) if next == delivery.sequence => Admit::Fresh,
            _ => Admit::Refused(Refusal::SequenceGap),
        }
    }

    /// Records the admission result for a fresh sequence and advances the
    /// high-water mark. Refusals consume their sequence too.
    pub(crate) fn record(
        &mut self,
        delivery: &Delivery,
        digest: [u8; 32],
        result: AdmissionResult,
    ) {
        let retain = self.retain;
        let Some(session) = self.current.as_mut() else {
            return;
        };
        if session.nonce != delivery.session {
            return;
        }
        session.high_water = delivery.sequence;
        session.retained.push_back(Retained {
            sequence: delivery.sequence,
            digest,
            result,
        });
        while session.retained.len() > retain {
            session.retained.pop_front();
        }
    }
}

fn session_nonce(issued: u64, entropy: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"prisma-airs-desktop-v2/delivery-session");
    hasher.update(issued.to_be_bytes());
    hasher.update(entropy);
    format!("ds-{}", hex::encode(&hasher.finalize()[..16]))
}
