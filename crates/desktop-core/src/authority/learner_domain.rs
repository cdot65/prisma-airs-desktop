//! The Academy learner domain inside the authority: device sign-in, the
//! protected learner session, sign-out and learner procedure calls.
//!
//! The learner account is independent of the selected tenant. Its own
//! [`LearnerRef`] (operations-runtime domain `lx-`) is issued on sign-in,
//! restart, sign-out and transport revision; tenant switches never touch it.
//! The refresh token lives only in the learner Keychain namespace and in
//! session memory; access tokens only in memory. Nothing credential-bearing
//! reaches a snapshot or call result.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::time::Instant;

use serde::Serialize;
use tokio::sync::Notify;
use ts_rs::TS;
use url::Url;
use zeroize::Zeroizing;

use super::{Effect, Inner, State, lock};
use crate::academy::{
    AcademyClient, AcademyFailure, CallFailure, CallReply, DesktopConfig, Endpoints, Poll,
};
use crate::context::LearnerRef;
use crate::learner::{AcademyView, ProcedureKind};
use crate::runtime::{Claim, OperationId, OperationState, Outcome, Work};
use crate::store::StoredLearner;
use crate::vault::{RecordId, VaultError};

/// Refresh this long before the access token's stated expiry.
const EXPIRY_SKEW: Duration = Duration::from_secs(30);
/// Interactive learner call deadline (the runtime's budget also applies).
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Settled call results kept for one observation each.
const RETAINED_CALLS: usize = 256;

/// In-memory sign-in progress (the saved account is `Data::learner`).
#[derive(Clone, Debug)]
pub(super) enum LearnerPhase {
    SignedOut,
    Starting {
        flow: u64,
    },
    Awaiting {
        flow: u64,
        user_code: String,
        verification: Url,
        expires_at: Instant,
    },
    Completing {
        flow: u64,
    },
    SignedIn,
    Failed {
        failure: AcademyFailure,
        vault: Option<VaultError>,
    },
}

impl LearnerPhase {
    pub(super) fn flow(&self) -> Option<u64> {
        match self {
            Self::Starting { flow } | Self::Awaiting { flow, .. } | Self::Completing { flow } => {
                Some(*flow)
            }
            _ => None,
        }
    }
}

/// The secret saved in the learner Keychain namespace.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedSecret {
    refresh_token: String,
}

fn secret_bytes(refresh: &str) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(
        serde_json::to_vec(&SavedSecret {
            refresh_token: refresh.to_owned(),
        })
        .unwrap_or_default(),
    )
}

struct AuthState {
    discovered: Option<(DesktopConfig, Endpoints)>,
    access: Option<(Zeroizing<String>, Instant)>,
    refresh: Option<Zeroizing<String>>,
    record: RecordId,
}

/// One signed-in learner account generation.
pub(super) struct LearnerSession {
    pub(super) learner: LearnerRef,
    client: AcademyClient,
    stored: StoredLearner,
    inner: Weak<Inner>,
    auth: tokio::sync::Mutex<AuthState>,
}

impl LearnerSession {
    pub(super) fn client(&self) -> &AcademyClient {
        &self.client
    }

    /// A usable access token (renewed when `force`).
    pub(super) async fn access_token(
        self: &Arc<Self>,
        force: bool,
    ) -> Result<Zeroizing<String>, CallFailure> {
        self.access(force).await
    }

    pub(super) fn new(
        learner: LearnerRef,
        client: AcademyClient,
        stored: StoredLearner,
        inner: Weak<Inner>,
        tokens: Option<(Zeroizing<String>, Zeroizing<String>, Duration)>,
        discovered: Option<(DesktopConfig, Endpoints)>,
    ) -> Arc<Self> {
        let (access, refresh) = match tokens {
            Some((access, refresh, expires_in)) => {
                (Some((access, Instant::now() + expires_in)), Some(refresh))
            }
            None => (None, None),
        };
        Arc::new(Self {
            learner,
            client,
            auth: tokio::sync::Mutex::new(AuthState {
                discovered,
                access,
                refresh,
                record: stored.record.clone(),
            }),
            stored,
            inner,
        })
    }

    fn current(&self, state: &State) -> bool {
        state.learner_ref.as_ref() == Some(&self.learner)
    }

    /// Marks the saved account as needing sign-in again (if still current).
    fn expire(&self, failure: AcademyFailure, vault: Option<VaultError>) {
        let Some(inner) = self.inner.upgrade() else {
            return;
        };
        if let Ok(mut state) = lock(&inner)
            && self.current(&state)
        {
            state.learner = LearnerPhase::Failed { failure, vault };
            state.changed();
        }
    }

    /// An access token, renewing it when missing, expiring or `force`d.
    /// Only one renewal runs at a time per account generation, and it runs
    /// to completion even if the caller is cancelled: a token the server
    /// rotated is then either saved or revoked, never lost.
    async fn access(self: &Arc<Self>, force: bool) -> Result<Zeroizing<String>, CallFailure> {
        let session = Arc::clone(self);
        tokio::spawn(async move { session.renew(force).await })
            .await
            .unwrap_or(Err(CallFailure::Cancelled))
    }

    async fn renew(&self, force: bool) -> Result<Zeroizing<String>, CallFailure> {
        let mut auth = self.auth.lock().await;
        if !force
            && let Some((token, expires)) = &auth.access
            && Instant::now() + EXPIRY_SKEW < *expires
        {
            return Ok(token.clone());
        }
        if auth.discovered.is_none() {
            let config = self.client.config().await.map_err(network_or_expired)?;
            if config.issuer != self.stored.issuer || config.client_id != self.stored.client_id {
                self.expire(AcademyFailure::AccountChanged, None);
                return Err(CallFailure::SessionExpired);
            }
            let endpoints = self
                .client
                .endpoints(&config)
                .await
                .map_err(network_or_expired)?;
            auth.discovered = Some((config, endpoints));
        }
        if auth.refresh.is_none() {
            let Some(inner) = self.inner.upgrade() else {
                return Err(CallFailure::Cancelled);
            };
            let record = auth.record.clone();
            let read =
                tokio::task::spawn_blocking(move || inner.adapters.learner_keychain.get(&record))
                    .await
                    .unwrap_or(Err(VaultError::Unavailable));
            match read.map(|bytes| serde_json::from_slice::<SavedSecret>(&bytes)) {
                Ok(Ok(saved)) => auth.refresh = Some(Zeroizing::new(saved.refresh_token)),
                Ok(Err(_)) => {
                    self.expire(AcademyFailure::SessionExpired, Some(VaultError::Corrupt));
                    return Err(CallFailure::CredentialUnavailable);
                }
                Err(error) => {
                    self.expire(AcademyFailure::SessionExpired, Some(error));
                    return Err(CallFailure::CredentialUnavailable);
                }
            }
        }
        let (config, endpoints) = auth.discovered.clone().ok_or(CallFailure::Network)?;
        let previous = auth
            .refresh
            .clone()
            .ok_or(CallFailure::CredentialUnavailable)?;
        let tokens = match self.client.refresh(&config, &endpoints, &previous).await {
            Ok(tokens) => tokens,
            Err(AcademyFailure::Network) => return Err(CallFailure::Network),
            Err(failure) => {
                auth.access = None;
                self.expire(
                    match failure {
                        AcademyFailure::ScopeMissing => AcademyFailure::ScopeMissing,
                        _ => AcademyFailure::SessionExpired,
                    },
                    None,
                );
                return Err(CallFailure::SessionExpired);
            }
        };
        if tokens.refresh.as_str() != previous.as_str() {
            // A rotated refresh token replaces the saved one only while this
            // account generation is current; otherwise it is revoked.
            match self.rotate(&auth.record, &tokens.refresh).await {
                Some(record) => auth.record = record,
                None => {
                    let _ = self
                        .client
                        .revoke(&config, &endpoints, &tokens.refresh)
                        .await;
                    return Err(CallFailure::Cancelled);
                }
            }
        } else if !self.still_current() {
            return Err(CallFailure::Cancelled);
        }
        auth.refresh = Some(tokens.refresh);
        auth.access = Some((tokens.access.clone(), Instant::now() + tokens.expires_in));
        Ok(tokens.access)
    }

    fn still_current(&self) -> bool {
        self.inner
            .upgrade()
            .and_then(|inner| lock(&inner).ok().map(|state| self.current(&state)))
            .unwrap_or(false)
    }

    /// Saves a rotated refresh token under write-ahead ownership: the new
    /// record joins the durable learner ledger before it is written, the
    /// account then points at it and the old record joins the ledger in the
    /// same save. The ledger is drained only after a confirmed save, so an
    /// unconfirmed transfer never deletes the record a recovered document
    /// may still name. `None` if the account changed or saving failed (the
    /// caller revokes the new token).
    async fn rotate(&self, old: &RecordId, refresh: &str) -> Option<RecordId> {
        let inner = self.inner.upgrade()?;
        let record = RecordId::issue()?;
        let begun = {
            let mut state = lock(&inner).ok()?;
            self.current(&state) && state.begin_learner_write(&record).is_ok()
        };
        if !begun {
            drain_learner_cleanup(&inner);
            return None;
        }
        let secret = secret_bytes(refresh);
        let writer = Arc::clone(&inner);
        let written = record.clone();
        let put = tokio::task::spawn_blocking(move || {
            writer.adapters.learner_keychain.put(&written, &secret)
        })
        .await
        .unwrap_or(Err(VaultError::Unavailable));
        let committed = {
            let Ok(mut state) = lock(&inner) else {
                return None;
            };
            state.learner_writes.retain(|r| r != &record);
            put.is_ok() && self.current(&state) && {
                let before = state.data.clone();
                if let Some(learner) = state.data.learner.as_mut() {
                    learner.record = record.clone();
                }
                state.data.learner_cleanup.retain(|r| r != &record);
                state.data.learner_cleanup.push(old.clone());
                state.save_with(&before).is_ok()
            }
        };
        drain_learner_cleanup(&inner);
        if committed {
            Some(record)
        } else {
            if put.is_ok() && self.still_current() {
                // Could not persist the rotation: the old record no longer
                // matches the server; the operator signs in again.
                self.expire(AcademyFailure::SessionExpired, None);
            }
            None
        }
    }

    /// Best-effort revocation of this generation's refresh token. A session
    /// restored after relaunch has not loaded it yet: it is read from the
    /// Keychain (before the sign-out purge) and the issuer is rediscovered.
    pub(super) async fn revoke(&self, inner: &Arc<Inner>) {
        let mut auth = self.auth.lock().await;
        if auth.refresh.is_none() {
            let reader = Arc::clone(inner);
            let record = auth.record.clone();
            let read =
                tokio::task::spawn_blocking(move || reader.adapters.learner_keychain.get(&record))
                    .await
                    .unwrap_or(Err(VaultError::Unavailable));
            auth.refresh = read
                .ok()
                .and_then(|bytes| serde_json::from_slice::<SavedSecret>(&bytes).ok())
                .map(|saved| Zeroizing::new(saved.refresh_token));
        }
        if auth.discovered.is_none() {
            let Ok(config) = self.client.config().await else {
                return;
            };
            if config.issuer != self.stored.issuer || config.client_id != self.stored.client_id {
                return;
            }
            let Ok(endpoints) = self.client.endpoints(&config).await else {
                return;
            };
            auth.discovered = Some((config, endpoints));
        }
        let (Some((config, endpoints)), Some(refresh)) = (&auth.discovered, &auth.refresh) else {
            return;
        };
        let _ = self.client.revoke(config, endpoints, refresh).await;
    }
}

fn network_or_expired(failure: AcademyFailure) -> CallFailure {
    match failure {
        AcademyFailure::Network => CallFailure::Network,
        _ => CallFailure::SessionExpired,
    }
}

/// What a learner call produced for the presentation's tRPC link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AcademyCallResult {
    /// `result.data` in SuperJSON wire form (`{json, meta?}`) as text.
    Data { data: String },
    /// A tRPC error envelope as text, with its HTTP status.
    #[serde(rename_all = "camelCase")]
    /// `outcome`: Rejected for 4xx, Unknown for 5xx (the request may have
    /// had an effect).
    Error {
        error: String,
        http_status: u16,
        outcome: Outcome,
    },
    /// No tRPC reply; `outcome` says what is known of the remote effect.
    Failed {
        failure: CallFailure,
        outcome: Outcome,
    },
}

#[derive(Default)]
struct Cell {
    result: Mutex<Option<AcademyCallResult>>,
    ready: Notify,
}

/// A single-use wait for one learner call's result.
pub struct AcademyCallHandle(Arc<Cell>);

impl std::fmt::Debug for AcademyCallHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AcademyCallHandle")
    }
}

impl AcademyCallHandle {
    pub async fn result(self) -> AcademyCallResult {
        loop {
            let notified = self.0.ready.notified();
            if let Some(result) = self.0.result.lock().ok().and_then(|r| r.clone()) {
                return result;
            }
            notified.await;
        }
    }
}

type Slot = Arc<Mutex<Option<Result<CallReply, CallFailure>>>>;

pub(super) struct CallRecord {
    operation: OperationId,
    learner: LearnerRef,
    slot: Slot,
    cell: Arc<Cell>,
    settled: bool,
    observed: bool,
}

impl State {
    pub(super) fn academy_view(&self) -> AcademyView {
        let saved = self.data.learner.as_ref();
        match &self.learner {
            LearnerPhase::SignedOut => AcademyView::SignedOut,
            LearnerPhase::Starting { .. } => AcademyView::Starting,
            LearnerPhase::Awaiting {
                user_code,
                verification,
                expires_at,
                ..
            } => AcademyView::AwaitingApproval {
                user_code: user_code.clone(),
                verification_uri: verification.to_string(),
                expires_in: expires_at
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .min(u64::from(u32::MAX)) as u32,
            },
            LearnerPhase::Completing { .. } => AcademyView::Completing,
            LearnerPhase::SignedIn => match (saved, &self.learner_ref) {
                (Some(saved), Some(learner)) => AcademyView::SignedIn {
                    display_name: saved.display_name.clone(),
                    account_id: saved.account_id.clone(),
                    learner: learner.clone(),
                },
                _ => AcademyView::SignedOut,
            },
            LearnerPhase::Failed { failure, vault } => AcademyView::Failed {
                failure: *failure,
                vault: *vault,
                display_name: saved.map(|s| s.display_name.clone()),
            },
        }
    }

    /// Write-ahead for a learner Keychain record: it joins the durable
    /// learner ledger (confirmed, with one confirming retry) and is marked
    /// in flight so cleanup and reset leave it alone until it settles.
    pub(super) fn begin_learner_write(&mut self, record: &RecordId) -> Result<(), VaultError> {
        let before = self.data.clone();
        self.data.learner_cleanup.push(record.clone());
        self.save_with(&before)
            .map_err(|_| VaultError::Unavailable)?;
        if self.durability_unconfirmed && !self.confirm_durable() {
            return Err(VaultError::Unavailable);
        }
        self.learner_writes.push(record.clone());
        Ok(())
    }

    /// Moves the saved learner's record into the ledger and saves; the
    /// record stays busy until its refresh token is revoked.
    pub(super) fn retire_saved_learner(&mut self) -> Result<Option<RecordId>, super::Refusal> {
        let Some(stored) = self.data.learner.clone() else {
            return Ok(None);
        };
        let before = self.data.clone();
        self.data.learner = None;
        self.data.learner_cleanup.push(stored.record.clone());
        self.save_with(&before)?;
        self.learner_busy.push(stored.record.clone());
        Ok(Some(stored.record))
    }

    /// The Academy client under the current transport policy.
    pub(super) fn academy_client(&self) -> Option<AcademyClient> {
        let transport = self.transport.as_ref()?.for_admission().ok()?;
        AcademyClient::new(transport.http().clone(), &self.academy_origin)
    }

    /// (Re)creates the saved learner's session under a new reference; its
    /// access token is renewed from the Keychain on first use.
    /// Begins the signed-in generation's offline work (opened by
    /// [`Effect::OpenOffline`], off the state lock).
    pub(super) fn start_offline(
        &mut self,
        inner: &Arc<Inner>,
    ) -> Option<Arc<super::offline_domain::OfflineGen>> {
        let account = self.data.learner.as_ref()?.account_id.clone();
        let generation = super::offline_domain::OfflineGen::new(
            account,
            inner.adapters.offline_root.clone(),
            Arc::clone(&inner.adapters.learner_keychain),
            Arc::downgrade(inner),
        );
        self.offline = Some(Arc::clone(&generation));
        Some(generation)
    }

    pub(super) fn restore_learner_session(
        &mut self,
        inner: &Arc<Inner>,
    ) -> Vec<crate::runtime::Start> {
        let Some(stored) = self.data.learner.clone() else {
            return Vec::new();
        };
        let client = self.academy_client();
        // The same account's offline work stays open across a transport
        // revision (only its learner reference changes).
        let previous = self
            .offline
            .clone()
            .filter(|o| o.account() == stored.account_id);
        let starts = self.reissue_learner();
        match (client, self.learner_ref.clone()) {
            (Some(client), Some(learner)) => {
                self.learner_session = Some(LearnerSession::new(
                    learner,
                    client,
                    stored,
                    Arc::downgrade(inner),
                    None,
                    None,
                ));
                match previous {
                    Some(offline) => self.offline = Some(offline),
                    None => {
                        self.start_offline(inner);
                    }
                }
            }
            _ => {
                self.learner_session = None;
                self.learner = LearnerPhase::Failed {
                    failure: AcademyFailure::Network,
                    vault: None,
                };
            }
        }
        starts
    }

    /// Issues a new learner reference (an account or the guest generation)
    /// and retires every learner operation bound to the previous one.
    pub(super) fn reissue_learner(&mut self) -> Vec<crate::runtime::Start> {
        self.learner_epoch += 1;
        // The previous generation's offline work is locked (its store is
        // dropped once no job holds it).
        self.offline = None;
        // Signed out, the reference names a guest generation (public catalog
        // reads only); either way a new value retires the previous one.
        self.learner_ref = Some(LearnerRef::issue(&self.salt, self.learner_epoch));
        let keep = self.learner_ref.as_ref().map(|l| l.as_str().to_owned());
        let starts =
            self.operations
                .retire_contexts(LearnerRef::PREFIX, keep.as_deref(), Instant::now());
        self.sync_academy_calls();
        // Settled results of an earlier account are withdrawn: nothing can
        // observe them once the account changed.
        let current = self.learner_ref.clone();
        self.academy_calls
            .retain(|r| !r.settled || current.as_ref() == Some(&r.learner));
        starts
    }

    /// Ends the learner session in memory: no account, no flow, no work.
    /// Returns what the effect must still clean up (vault and revocation).
    pub(super) fn sign_out_learner(
        &mut self,
    ) -> (Vec<crate::runtime::Start>, Option<Arc<LearnerSession>>) {
        self.learner_flow += 1;
        self.learner = LearnerPhase::SignedOut;
        let session = self.learner_session.take();
        self.data.learner = None;
        (self.reissue_learner(), session)
    }

    /// Delivers settled learner call results to their single observation.
    pub(super) fn sync_academy_calls(&mut self) {
        let current = self.learner_ref.clone();
        for record in self.academy_calls.iter_mut().filter(|r| !r.settled) {
            let settlement = match self.operations.state(&record.operation) {
                Some(OperationState::Settled(s)) => s,
                None => crate::runtime::Settlement {
                    outcome: Outcome::Unknown,
                    stop: None,
                },
                Some(_) => continue,
            };
            record.settled = true;
            let reply = record.slot.lock().ok().and_then(|mut s| s.take());
            let retired = current.as_ref() != Some(&record.learner);
            let result = match (reply, retired, settlement.stop) {
                // A reply for a retired account generation is never shown.
                (_, true, _) | (_, _, Some(_)) | (None, _, _) => AcademyCallResult::Failed {
                    failure: CallFailure::Cancelled,
                    outcome: settlement.outcome,
                },
                (Some(Ok(CallReply::Data(data))), false, None) => AcademyCallResult::Data { data },
                (Some(Ok(CallReply::Error { error, status })), false, None) => {
                    AcademyCallResult::Error {
                        error,
                        http_status: status,
                        outcome: settlement.outcome,
                    }
                }
                (Some(Err(failure)), false, None) => AcademyCallResult::Failed {
                    failure,
                    outcome: settlement.outcome,
                },
            };
            if let Ok(mut slot) = record.cell.result.lock() {
                *slot = Some(result);
            }
            record.cell.ready.notify_waiters();
        }
        // Keep unsettled calls and the newest settled, unobserved ones.
        let mut settled = 0;
        let mut keep = vec![true; self.academy_calls.len()];
        for (i, record) in self.academy_calls.iter().enumerate().rev() {
            if record.settled {
                settled += 1;
                keep[i] = !record.observed && settled <= RETAINED_CALLS;
            }
        }
        let mut index = 0;
        self.academy_calls.retain(|_| {
            index += 1;
            keep[index - 1]
        });
    }

    /// The single observation of a learner call's result.
    pub(super) fn observe_academy_call(
        &mut self,
        operation: &OperationId,
    ) -> Result<AcademyCallHandle, super::Refusal> {
        let record = self
            .academy_calls
            .iter_mut()
            .find(|r| &r.operation == operation && !r.observed)
            .ok_or(super::Refusal::InvalidInput)?;
        record.observed = true;
        let handle = AcademyCallHandle(Arc::clone(&record.cell));
        if record.settled {
            self.sync_academy_calls();
        }
        Ok(handle)
    }

    /// Admits one learner procedure call for the current account.
    pub(super) fn admit_academy_call(
        &mut self,
        learner: LearnerRef,
        procedure: &str,
        kind: ProcedureKind,
        input: Option<String>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), super::Refusal> {
        use super::Refusal;
        if self.storage == super::StorageHealth::Resetting
            || self.learner_ref.as_ref() != Some(&learner)
        {
            return Err(Refusal::StaleContext);
        }
        // With the account credential: the pinned desktop-token allowlist,
        // signed in only. Without any credential: the public catalog set
        // (guests, and public-only procedures for signed-in learners).
        let signed_in = matches!(self.learner, LearnerPhase::SignedIn);
        let authenticated = signed_in && crate::learner::procedure_kind(procedure) == Some(kind);
        let public = crate::learner::is_public(procedure) && kind == ProcedureKind::Query;
        if !authenticated && !public {
            return Err(Refusal::CapabilityMissing);
        }
        if let Some(input) = &input
            && (input.len() > crate::academy::CALL_INPUT_LIMIT
                || serde_json::from_str::<serde_json::Value>(input).is_err())
        {
            return Err(Refusal::InvalidInput);
        }
        let session = if authenticated {
            Some(
                self.learner_session
                    .clone()
                    .ok_or(Refusal::CapabilityMissing)?,
            )
        } else {
            None
        };
        let client = match &session {
            Some(session) => session.client.clone(),
            None => self.academy_client().ok_or(Refusal::CapabilityMissing)?,
        };
        let slot: Slot = Arc::default();
        let bytes = input.as_ref().map_or(0, String::len);
        let job = CallJob {
            session,
            client,
            procedure: procedure.to_owned(),
            kind,
            input,
            slot: Arc::clone(&slot),
            offline: if signed_in {
                self.offline.clone()
            } else {
                None
            },
        };
        let (operation, starts) =
            self.operations
                .admit(learner.as_str(), bytes, job.into_work(), Instant::now())?;
        self.academy_calls.push(CallRecord {
            operation: operation.clone(),
            learner,
            slot,
            cell: Arc::default(),
            settled: false,
            observed: false,
        });
        Ok((operation, starts))
    }

    /// Admits a native learner job for the current generation, observed like
    /// a learner call (and, like one, never delivered to a retired
    /// generation). `account` jobs need the signed-in account session.
    fn admit_learner_job(
        &mut self,
        learner: LearnerRef,
        work: Work,
        budget: Duration,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), super::Refusal> {
        let (operation, starts) =
            self.operations
                .admit_for(learner.as_str(), 0, work, Instant::now(), budget)?;
        Ok((operation, starts))
    }

    fn current_learner(&self, learner: &LearnerRef) -> Result<(), super::Refusal> {
        if self.storage == super::StorageHealth::Resetting
            || self.learner_ref.as_ref() != Some(learner)
        {
            return Err(super::Refusal::StaleContext);
        }
        Ok(())
    }

    fn record_job(&mut self, operation: &OperationId, learner: LearnerRef, slot: Slot) {
        self.academy_calls.push(CallRecord {
            operation: operation.clone(),
            learner,
            slot,
            cell: Arc::default(),
            settled: false,
            observed: false,
        });
    }

    /// Opens the native print sheet for the current page (an Academy
    /// certificate): reports whether it opened; changes no learner state.
    pub(super) fn admit_print(
        &mut self,
        learner: LearnerRef,
        printer: Arc<dyn super::PagePrinter>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), super::Refusal> {
        self.current_learner(&learner)?;
        let slot: Slot = Arc::default();
        let job = PrintJob {
            printer,
            slot: Arc::clone(&slot),
        };
        let admitted = self.admit_learner_job(learner.clone(), job.into_work(), SHEET_BUDGET)?;
        self.record_job(&admitted.0, learner, slot);
        Ok(admitted)
    }

    /// Replaces the signed-in learner's avatar with an image chosen in the
    /// native picker: checked here, stored at Academy's presigned
    /// destination, then confirmed with Academy.
    pub(super) fn admit_avatar_upload(
        &mut self,
        learner: LearnerRef,
        picker: Arc<dyn super::ImagePicker>,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), super::Refusal> {
        self.current_learner(&learner)?;
        if !matches!(self.learner, LearnerPhase::SignedIn) {
            return Err(super::Refusal::CapabilityMissing);
        }
        let session = self
            .learner_session
            .clone()
            .ok_or(super::Refusal::CapabilityMissing)?;
        let slot: Slot = Arc::default();
        let job = AvatarJob {
            client: session.client.clone(),
            session,
            picker,
            slot: Arc::clone(&slot),
        };
        let admitted = self.admit_learner_job(learner.clone(), job.into_work(), SHEET_BUDGET)?;
        self.record_job(&admitted.0, learner, slot);
        Ok(admitted)
    }

    /// Admits an offline-work operation for the signed-in current learner.
    pub(super) fn admit_offline(
        &mut self,
        learner: LearnerRef,
        op: OfflineOp,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), super::Refusal> {
        self.current_learner(&learner)?;
        if !matches!(self.learner, LearnerPhase::SignedIn) {
            return Err(super::Refusal::CapabilityMissing);
        }
        // Course and attempt ids name records and appear in the snapshot.
        let id_ok = super::offline_domain::plain_id;
        let valid = match &op {
            OfflineOp::Download(id)
            | OfflineOp::Remove(id)
            | OfflineOp::LoadDraft(id, _)
            | OfflineOp::ClearDraft(id) => id_ok(id),
            OfflineOp::SaveDraft {
                attempt_id, draft, ..
            } => id_ok(attempt_id) && draft.len() <= crate::academy::CALL_INPUT_LIMIT * 4,
        };
        if !valid {
            return Err(super::Refusal::InvalidInput);
        }
        let generation = self
            .offline
            .clone()
            .ok_or(super::Refusal::CapabilityMissing)?;
        let session = self
            .learner_session
            .clone()
            .ok_or(super::Refusal::CapabilityMissing)?;
        let slot: Slot = Arc::default();
        let job = OfflineJob {
            generation,
            session,
            op,
            slot: Arc::clone(&slot),
        };
        let budget = self.operations.limits().interactive_budget * 5;
        let admitted = self.admit_learner_job(learner.clone(), job.into_work(), budget)?;
        self.record_job(&admitted.0, learner, slot);
        Ok(admitted)
    }

    /// Reads an avatar Academy returned (public, on Academy's origin) for
    /// the presentation, which cannot load remote images.
    pub(super) fn admit_image_read(
        &mut self,
        learner: LearnerRef,
        url: &str,
    ) -> Result<(OperationId, Vec<crate::runtime::Start>), super::Refusal> {
        self.current_learner(&learner)?;
        let client = self
            .academy_client()
            .ok_or(super::Refusal::CapabilityMissing)?;
        let url = crate::avatar::avatar_address(url, client.origin())
            .ok_or(super::Refusal::InvalidInput)?;
        let slot: Slot = Arc::default();
        let job = ImageJob {
            client,
            url,
            slot: Arc::clone(&slot),
        };
        let budget = self.operations.limits().interactive_budget;
        let admitted = self.admit_learner_job(learner.clone(), job.into_work(), budget)?;
        self.record_job(&admitted.0, learner, slot);
        Ok(admitted)
    }
}

/// Operator-paced jobs wait on a native sheet before any request.
const SHEET_BUDGET: Duration = Duration::from_secs(15 * 60);

fn leave(slot: &Slot, value: Result<CallReply, CallFailure>) {
    if let Ok(mut slot) = slot.lock() {
        *slot = Some(value);
    }
}

/// `{"json": value}` as a learner call's data.
fn data(value: serde_json::Value) -> CallReply {
    CallReply::Data(serde_json::json!({ "json": value }).to_string())
}

struct PrintJob {
    printer: Arc<dyn super::PagePrinter>,
    slot: Slot,
}

impl PrintJob {
    fn into_work(self) -> Work {
        Box::new(move |_claim| {
            Box::pin(async move {
                let printer = self.printer;
                let opened = tokio::task::spawn_blocking(move || printer.print())
                    .await
                    .unwrap_or(false);
                if opened {
                    leave(&self.slot, Ok(data(serde_json::json!({ "opened": true }))));
                    Outcome::Completed
                } else {
                    leave(&self.slot, Err(CallFailure::HostUnavailable));
                    Outcome::NotSent
                }
            })
        })
    }
}

struct ImageJob {
    client: AcademyClient,
    url: Url,
    slot: Slot,
}

impl ImageJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| {
            Box::pin(async move {
                let remaining = claim
                    .deadline
                    .saturating_duration_since(Instant::now())
                    .min(CALL_TIMEOUT);
                let read = tokio::select! {
                    read = self.client.get_image(self.url, crate::avatar::MAX_AVATAR_BYTES, remaining) => read,
                    () = claim.cancel.cancelled() => Err(CallFailure::Cancelled),
                };
                let reply = read.and_then(|(content_type, bytes)| {
                    crate::avatar::data_url(&content_type, &bytes)
                        .map(|src| data(serde_json::json!({ "src": src })))
                        .ok_or(CallFailure::InvalidResponse)
                });
                let outcome = if reply.is_ok() {
                    Outcome::Completed
                } else {
                    Outcome::Rejected
                };
                leave(&self.slot, reply);
                outcome
            })
        })
    }
}

struct AvatarJob {
    session: Arc<LearnerSession>,
    client: AcademyClient,
    picker: Arc<dyn super::ImagePicker>,
    slot: Slot,
}

impl AvatarJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    /// One account request; a refused token is renewed and the request
    /// asked again only when `repeatable` (it changes nothing on Academy).
    async fn request(
        &self,
        procedure: &str,
        input: &str,
        repeatable: bool,
        claim: &Claim,
    ) -> Result<CallReply, CallFailure> {
        let mut force = false;
        loop {
            let token = tokio::select! {
                token = self.session.access(force) => token?,
                () = claim.cancel.cancelled() => return Err(CallFailure::Cancelled),
            };
            let remaining = claim
                .deadline
                .saturating_duration_since(Instant::now())
                .min(CALL_TIMEOUT);
            let reply = tokio::select! {
                reply = self.client.call(procedure, ProcedureKind::Mutation, Some(input), Some(token.as_str()), remaining) => reply,
                () = claim.cancel.cancelled() => Err(CallFailure::Cancelled),
            };
            if !force && matches!(reply, Ok(CallReply::Error { status: 401, .. })) {
                force = true;
                if repeatable {
                    continue;
                }
                let _ = self.session.access(true).await;
            }
            return reply;
        }
    }

    async fn run(self, claim: Claim) -> Outcome {
        // Nothing is sent until an acceptable image was chosen.
        let chosen = tokio::select! {
            chosen = self.picker.choose_image() => chosen,
            () = claim.cancel.cancelled() => None,
        };
        let file = match chosen {
            None => {
                leave(&self.slot, Err(CallFailure::Cancelled));
                return Outcome::NotSent;
            }
            Some(Err(_)) => {
                leave(&self.slot, Err(CallFailure::UnsupportedImage));
                return Outcome::NotSent;
            }
            Some(Ok(file)) => file,
        };
        // The photo is decoded within bounds and prepared as Academy's
        // 512-pixel JPEG; anything that does not decode is never uploaded.
        let source = file.bytes.to_vec();
        let prepared = tokio::select! {
            prepared = tokio::task::spawn_blocking(move || crate::avatar::prepare(&source)) => prepared.ok().flatten(),
            () = claim.cancel.cancelled() => {
                leave(&self.slot, Err(CallFailure::Cancelled));
                return Outcome::NotSent;
            }
        };
        let Some(jpeg) = prepared else {
            leave(&self.slot, Err(CallFailure::UnsupportedImage));
            return Outcome::NotSent;
        };
        // Presigning changes nothing on Academy, so it may be asked again.
        let presign = serde_json::json!({ "json": {
            "contentType": crate::avatar::PREPARED_CONTENT_TYPE, "size": jpeg.len() } })
        .to_string();
        let upload_url = match self
            .request("settings.avatarUploadUrl", &presign, true, &claim)
            .await
        {
            Ok(CallReply::Data(data)) => serde_json::from_str::<serde_json::Value>(&data)
                .ok()
                .and_then(|v| v["json"]["uploadUrl"].as_str().map(str::to_owned)),
            Ok(error @ CallReply::Error { .. }) => {
                leave(&self.slot, Ok(error));
                return Outcome::Rejected;
            }
            Err(failure) => {
                leave(&self.slot, Err(failure));
                return Outcome::NotSent;
            }
        };
        let Some(destination) = upload_url
            .as_deref()
            .and_then(|u| crate::avatar::upload_destination(u, self.client.origin()))
        else {
            leave(&self.slot, Err(CallFailure::DestinationRefused));
            return Outcome::NotSent;
        };
        // Academy's key is stable (`avatars/<user>.jpg`): a stored PUT can
        // replace the photo Academy already serves even if it is never
        // confirmed, so from here on a failure is not "unchanged".
        let remaining = claim
            .deadline
            .saturating_duration_since(Instant::now())
            .min(CALL_TIMEOUT);
        let stored = tokio::select! {
            stored = self.client.put_upload(destination, crate::avatar::PREPARED_CONTENT_TYPE, jpeg, remaining) => stored,
            () = claim.cancel.cancelled() => crate::academy::StoreOutcome::Uncertain,
        };
        match stored {
            crate::academy::StoreOutcome::Stored => {}
            crate::academy::StoreOutcome::Refused | crate::academy::StoreOutcome::NotSent => {
                leave(&self.slot, Err(CallFailure::UploadFailed));
                return if stored == crate::academy::StoreOutcome::Refused {
                    Outcome::Rejected
                } else {
                    Outcome::NotSent
                };
            }
            crate::academy::StoreOutcome::Uncertain => {
                leave(&self.slot, Err(CallFailure::UploadFailed));
                return Outcome::Unknown;
            }
        }
        let confirm =
            serde_json::json!({ "json": { "ext": crate::avatar::PREPARED_EXT } }).to_string();
        let reply = self
            .request("settings.confirmAvatar", &confirm, false, &claim)
            .await;
        // Only a confirmed photo is a known outcome: after a stored PUT, a
        // refused or failed confirmation may still leave the new photo
        // served under the existing address.
        let outcome = match &reply {
            Ok(CallReply::Data(_)) => Outcome::Completed,
            _ => Outcome::Unknown,
        };
        leave(&self.slot, reply);
        outcome
    }
}

struct CallJob {
    /// None for a public call (no credential is ever attached).
    session: Option<Arc<LearnerSession>>,
    client: AcademyClient,
    procedure: String,
    kind: ProcedureKind,
    input: Option<String>,
    slot: Slot,
    /// The signed-in account's offline work: reachability, cached attempt
    /// reads and offline answers.
    offline: Option<Arc<super::offline_domain::OfflineGen>>,
}

impl CallJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    fn leave(&self, value: Result<CallReply, CallFailure>) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(value);
        }
    }

    async fn send(
        &self,
        force: bool,
        claim: &Claim,
    ) -> Result<Result<CallReply, CallFailure>, CallFailure> {
        let token = match &self.session {
            Some(session) => Some(tokio::select! {
                token = session.access(force) => token?,
                () = claim.cancel.cancelled() => return Err(CallFailure::Cancelled),
            }),
            None => None,
        };
        let remaining = claim
            .deadline
            .saturating_duration_since(Instant::now())
            .min(CALL_TIMEOUT);
        Ok(tokio::select! {
            reply = self.client.call(&self.procedure, self.kind, self.input.as_deref(), token.as_deref().map(|t| t.as_str()), remaining) => reply,
            () = claim.cancel.cancelled() => Err(CallFailure::Cancelled),
        })
    }

    async fn attempt(&self, claim: &Claim) -> (Result<CallReply, CallFailure>, Outcome) {
        // No usable token means nothing was sent.
        let mut reply = match self.send(false, claim).await {
            Ok(reply) => reply,
            Err(failure) => return (Err(failure), Outcome::NotSent),
        };
        // A refused token is renewed once. A query is then asked again; a
        // mutation is never resent (the operator retries explicitly) and its
        // refusal is returned as is.
        if self.session.is_some() && matches!(reply, Ok(CallReply::Error { status: 401, .. })) {
            match self.kind {
                ProcedureKind::Query => match self.send(true, claim).await {
                    Ok(again) => reply = again,
                    Err(failure) => return (Err(failure), Outcome::Rejected),
                },
                ProcedureKind::Mutation => {
                    if let Some(session) = &self.session {
                        let _ = session.access(true).await;
                    }
                }
            }
        }
        let outcome = match &reply {
            Ok(CallReply::Data(_)) => Outcome::Completed,
            Ok(CallReply::Error { status, .. }) if (400..500).contains(status) => Outcome::Rejected,
            // Server errors, transport failures, timeouts, unreadable replies
            // and cancellation after sending: the effect is unknown.
            _ => Outcome::Unknown,
        };
        (reply, outcome)
    }

    async fn run(self, claim: Claim) -> Outcome {
        // When this read of Academy started: what it brings back is kept
        // only over what was observed before it.
        // Reads whose result is kept wait for opening, so the stamp follows
        // every persisted one.
        let observed = match &self.offline {
            Some(offline)
                if self.kind == ProcedureKind::Query
                    && matches!(
                        self.procedure.as_str(),
                        "desktop.attempt.get" | "desktop.attempt.list"
                    ) =>
            {
                let offline = Arc::clone(offline);
                tokio::task::spawn_blocking(move || offline.observe_seeded())
                    .await
                    .unwrap_or(0)
            }
            Some(offline) => offline.observe(),
            None => 0,
        };
        let (mut reply, mut outcome) = self.attempt(&claim).await;
        if let Some(offline) = &self.offline {
            match &reply {
                // Any reply means Academy is reachable: when it just became
                // reachable again, downloads are re-checked (nothing replayed).
                Ok(answer) => {
                    if offline.mark_reachable(true)
                        && let Some(session) = &self.session
                    {
                        tokio::spawn(super::offline_domain::revalidate(
                            Arc::clone(offline),
                            Arc::clone(session),
                        ));
                    }
                    if let CallReply::Data(data) = answer
                        && self.session.is_some()
                    {
                        let (cache, procedure, input, data, kind) = (
                            Arc::clone(offline),
                            self.procedure.clone(),
                            self.input.clone(),
                            data.clone(),
                            self.kind,
                        );
                        let _ = tokio::task::spawn_blocking(move || match kind {
                            ProcedureKind::Query => {
                                cache.cache(&procedure, input.as_deref(), &data, observed);
                            }
                            // A draft Academy confirmed stays with the kept
                            // attempt for offline use.
                            _ if matches!(
                                procedure.as_str(),
                                "desktop.draft.save" | "desktop.draft.resolve"
                            ) =>
                            {
                                cache.confirm_draft(&procedure, input.as_deref(), &data);
                            }
                            _ => {}
                        })
                        .await;
                    }
                }
                // Unreachable: a read a download or cached attempt can serve
                // is answered from this account's offline work.
                Err(CallFailure::Network | CallFailure::Timeout) => {
                    offline.mark_reachable(false);
                    if self.kind == ProcedureKind::Query
                        && super::offline_domain::OFFLINE_READS.contains(&self.procedure.as_str())
                    {
                        let (store, procedure, input) = (
                            Arc::clone(offline),
                            self.procedure.clone(),
                            self.input.clone(),
                        );
                        let answer = tokio::task::spawn_blocking(move || {
                            store.answer(&procedure, input.as_deref())
                        })
                        .await
                        .ok()
                        .flatten();
                        if let Some(data) = answer {
                            reply = Ok(CallReply::Data(data));
                            outcome = Outcome::Completed;
                        }
                    }
                }
                Err(_) => {}
            }
        }
        self.leave(reply);
        outcome
    }
}

/// A native offline-work operation, observed like a learner call.
pub(super) enum OfflineOp {
    Download(String),
    Remove(String),
    SaveDraft {
        attempt_id: String,
        base_version: u32,
        draft: String,
    },
    /// An attempt's kept draft; `true`: one an import has not committed yet
    /// is answered as none (D050).
    LoadDraft(String, bool),
    ClearDraft(String),
}

struct OfflineJob {
    generation: Arc<super::offline_domain::OfflineGen>,
    session: Arc<LearnerSession>,
    op: OfflineOp,
    slot: Slot,
}

impl OfflineJob {
    fn into_work(self) -> Work {
        Box::new(move |claim| {
            Box::pin(async move {
                use super::offline_domain as off;
                let generation = Arc::clone(&self.generation);
                let result = match self.op {
                    OfflineOp::Download(course) => tokio::select! {
                        r = off::download(&generation, &self.session, &course) => r,
                        () = claim.cancel.cancelled() => Err(CallFailure::Cancelled),
                    },
                    op => tokio::task::spawn_blocking(move || match op {
                        OfflineOp::Remove(course) => off::remove(&generation, &course),
                        OfflineOp::SaveDraft {
                            attempt_id,
                            base_version,
                            draft,
                        } => off::save_draft(&generation, &attempt_id, base_version, &draft),
                        OfflineOp::LoadDraft(attempt, hidden) => {
                            off::load_draft(&generation, &attempt, hidden)
                        }
                        OfflineOp::ClearDraft(attempt) => off::clear_draft(&generation, &attempt),
                        OfflineOp::Download(_) => Err(CallFailure::Cancelled),
                    })
                    .await
                    .unwrap_or(Err(CallFailure::OfflineUnavailable)),
                };
                let outcome = match &result {
                    Ok(CallReply::Data(_)) => Outcome::Completed,
                    Ok(CallReply::Error { status, .. }) if (400..500).contains(status) => {
                        Outcome::Rejected
                    }
                    Ok(CallReply::Error { .. }) => Outcome::Unknown,
                    Err(_) => Outcome::NotSent,
                };
                leave(&self.slot, result);
                outcome
            })
        })
    }
}

/// Runs a device authorization flow to completion, failure or replacement.
pub(super) async fn sign_in(inner: Arc<Inner>, flow: u64, client: AcademyClient) {
    let fail = |failure: AcademyFailure, vault: Option<VaultError>| {
        if let Ok(mut state) = lock(&inner)
            && state.learner.flow() == Some(flow)
        {
            state.learner = LearnerPhase::Failed { failure, vault };
            state.changed();
        }
    };
    let current = || lock(&inner).is_ok_and(|s| s.learner.flow() == Some(flow));
    let started = async {
        let config = client.config().await?;
        let endpoints = client.endpoints(&config).await?;
        let start = client.start(&config, &endpoints).await?;
        Ok::<_, AcademyFailure>((config, endpoints, start))
    }
    .await;
    let (config, endpoints, start) = match started {
        Ok(started) => started,
        Err(failure) => return fail(failure, None),
    };
    let expires_at = Instant::now() + start.expires_in;
    {
        let Ok(mut state) = lock(&inner) else { return };
        if state.learner.flow() != Some(flow) {
            return;
        }
        state.learner = LearnerPhase::Awaiting {
            flow,
            user_code: start.user_code.clone(),
            verification: start
                .verification_uri_complete
                .clone()
                .unwrap_or(start.verification_uri.clone()),
            expires_at,
        };
        state.changed();
    }
    let mut interval = start.interval;
    let tokens = loop {
        tokio::time::sleep(interval).await;
        if !current() {
            return;
        }
        if Instant::now() >= expires_at {
            return fail(AcademyFailure::Expired, None);
        }
        match client.poll(&config, &endpoints, &start.device_code).await {
            Ok(Poll::Pending) => {}
            // RFC 8628 §3.5: increase the interval by 5 seconds.
            Ok(Poll::SlowDown) => interval += Duration::from_secs(5),
            Ok(Poll::Granted(tokens)) => break tokens,
            Err(AcademyFailure::Network) => {}
            Err(failure) => return fail(failure, None),
        }
    };
    // A grant that lands after cancellation is revoked, never kept.
    let claimed = lock(&inner).is_ok_and(|mut s| {
        let ok = s.learner.flow() == Some(flow);
        if ok {
            s.learner = LearnerPhase::Completing { flow };
            s.changed();
        }
        ok
    });
    if !claimed {
        let _ = client.revoke(&config, &endpoints, &tokens.refresh).await;
        return;
    }
    let me = client
        .call(
            "desktop.me",
            ProcedureKind::Query,
            None,
            Some(&tokens.access),
            CALL_TIMEOUT,
        )
        .await;
    let identity = match me {
        Ok(CallReply::Data(data)) => identity(&data),
        _ => None,
    };
    let Some((account_id, display_name)) = identity else {
        let _ = client.revoke(&config, &endpoints, &tokens.refresh).await;
        return fail(AcademyFailure::InvalidResponse, None);
    };
    let Some(record) = RecordId::issue() else {
        let _ = client.revoke(&config, &endpoints, &tokens.refresh).await;
        return fail(AcademyFailure::AuthFailed, Some(VaultError::Unavailable));
    };
    // Write-ahead: the record joins the durable learner ledger before the
    // secret is written, and only while this flow is current. Nothing here
    // purges the namespace or touches another flow's records.
    let begun = lock(&inner).map(|mut state| {
        (state.learner.flow() == Some(flow)).then(|| state.begin_learner_write(&record))
    });
    match begun {
        Ok(Some(Ok(()))) => {}
        Ok(Some(Err(error))) => {
            let _ = client.revoke(&config, &endpoints, &tokens.refresh).await;
            return fail(AcademyFailure::AuthFailed, Some(error));
        }
        _ => {
            let _ = client.revoke(&config, &endpoints, &tokens.refresh).await;
            return;
        }
    }
    let writer = Arc::clone(&inner);
    let secret = secret_bytes(&tokens.refresh);
    let written = record.clone();
    let put = tokio::task::spawn_blocking(move || {
        writer.adapters.learner_keychain.put(&written, &secret)
    })
    .await
    .unwrap_or(Err(VaultError::Unavailable));
    let stored = StoredLearner {
        account_id,
        display_name,
        record: record.clone(),
        issuer: config.issuer.clone(),
        client_id: config.client_id.clone(),
    };
    enum Abandon {
        Failed(VaultError),
        Superseded,
    }
    let committed: Result<
        (
            Vec<crate::runtime::Start>,
            Option<Arc<super::offline_domain::OfflineGen>>,
        ),
        Abandon,
    > = (|| {
        let Ok(mut state) = lock(&inner) else {
            return Err(Abandon::Superseded);
        };
        state.learner_writes.retain(|r| r != &record);
        put.map_err(Abandon::Failed)?;
        if state.learner.flow() != Some(flow) {
            return Err(Abandon::Superseded);
        }
        let before = state.data.clone();
        state.data.learner = Some(stored.clone());
        state.data.learner_cleanup.retain(|r| r != &record);
        state
            .save_with(&before)
            .map_err(|_| Abandon::Failed(VaultError::Unavailable))?;
        let starts = state.reissue_learner();
        let learner = state.learner_ref.clone().expect("issued");
        state.learner_session = Some(LearnerSession::new(
            learner,
            client.clone(),
            stored,
            Arc::downgrade(&inner),
            Some((
                tokens.access.clone(),
                tokens.refresh.clone(),
                tokens.expires_in,
            )),
            Some((config.clone(), endpoints.clone())),
        ));
        state.learner = LearnerPhase::SignedIn;
        let offline = state.start_offline(&inner);
        state.changed();
        Ok((starts, offline))
    })();
    match committed {
        Ok((starts, offline)) => {
            let mut effects = vec![Effect::Start(starts)];
            effects.extend(offline.map(Effect::OpenOffline));
            super::run_effects(&inner, effects);
        }
        Err(abandon) => {
            // The record stays in the ledger and is deleted by cleanup; the
            // grant it held is revoked.
            let _ = client.revoke(&config, &endpoints, &tokens.refresh).await;
            if let Abandon::Failed(error) = abandon {
                fail(AcademyFailure::AuthFailed, Some(error));
            }
        }
    }
    drain_learner_cleanup(&inner);
}

/// `desktop.me` → (account id, display name), validated.
fn identity(data: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    let json = value.get("json")?;
    let account = json.get("accountId")?.as_str()?;
    let name = json.get("displayName")?.as_str()?;
    let ok = account.starts_with("user_")
        && account.len() <= 128
        && account
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.trim().is_empty()
        && name.chars().count() <= 200
        && !name.chars().any(char::is_control);
    ok.then(|| (account.to_owned(), name.trim().to_owned()))
}

/// Cleans up after sign-out: the learner namespace and a best-effort
/// revocation of the refresh token.
pub(super) async fn signed_out(
    inner: Arc<Inner>,
    session: Option<Arc<LearnerSession>>,
    record: Option<RecordId>,
) {
    // Revoke first: a restored session reads its token from the Keychain.
    // The record is in the ledger and marked busy until then.
    if let Some(session) = session {
        session.revoke(&inner).await;
    }
    if let (Some(record), Ok(mut state)) = (record, lock(&inner)) {
        state.learner_busy.retain(|r| r != &record);
    }
    drain_learner_cleanup(&inner);
}

/// Deletes ledgered learner records that no write or revocation is using,
/// once the document naming their replacements is durably confirmed.
pub(super) fn drain_learner_cleanup(inner: &Arc<Inner>) {
    let records = {
        let Ok(mut state) = lock(inner) else {
            return;
        };
        if state.learner_cleaning
            || state.storage != super::StorageHealth::Ready
            || state.durability_unconfirmed
        {
            return;
        }
        let records: Vec<RecordId> = state
            .data
            .learner_cleanup
            .iter()
            .filter(|r| !state.learner_busy.contains(r) && !state.learner_writes.contains(r))
            .cloned()
            .collect();
        if records.is_empty() {
            return;
        }
        state.learner_cleaning = true;
        records
    };
    let inner = Arc::clone(inner);
    tokio::spawn(async move {
        let worker = Arc::clone(&inner);
        let done = tokio::task::spawn_blocking(move || {
            records
                .into_iter()
                .filter(|r| {
                    matches!(
                        worker.adapters.learner_keychain.delete(r),
                        Ok(()) | Err(VaultError::NotFound)
                    )
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        let Ok(mut state) = lock(&inner) else {
            return;
        };
        state.learner_cleaning = false;
        let before = state.data.clone();
        state.data.learner_cleanup.retain(|r| !done.contains(r));
        if state.data.learner_cleanup != before.learner_cleanup
            && state.storage == super::StorageHealth::Ready
        {
            // A failed save keeps the entries; the next drain settles them.
            let _ = state.save_with(&before);
        }
        state.changed();
    });
}

/// Opens a signed-in generation's offline work if its key exists, then
/// re-checks its downloads with Academy (off the state lock).
pub(super) async fn open_offline(
    inner: Arc<Inner>,
    generation: Arc<super::offline_domain::OfflineGen>,
) {
    generation.set_notifier(Arc::downgrade(&inner));
    let opener = Arc::clone(&generation);
    let state = tokio::task::spawn_blocking(move || opener.open_existing())
        .await
        .unwrap_or(super::offline_domain::OfflineState::Unavailable);
    let session = lock(&inner).ok().and_then(|s| {
        let current = s
            .offline
            .as_ref()
            .is_some_and(|o| Arc::ptr_eq(o, &generation));
        if current {
            s.learner_session.clone()
        } else {
            None
        }
    });
    if let Ok(mut s) = lock(&inner) {
        s.changed();
    }
    if state == super::offline_domain::OfflineState::Ready
        && let Some(session) = session
    {
        super::offline_domain::revalidate(generation, session).await;
    }
}

#[cfg(test)]
mod tests {
    use super::identity;

    #[test]
    fn identity_requires_an_academy_account_and_a_display_name() {
        assert_eq!(
            identity(r#"{"json":{"accountId":"user_2abc","displayName":" Ada L "}}"#),
            Some(("user_2abc".into(), "Ada L".into()))
        );
        for bad in [
            r#"{"json":{"accountId":"org_1","displayName":"A"}}"#,
            r#"{"json":{"accountId":"user_1","displayName":"  "}}"#,
            r#"{"json":{"accountId":"user_1/x","displayName":"A"}}"#,
            r#"{"json":{"displayName":"A"}}"#,
            r#"{"accountId":"user_1","displayName":"A"}"#,
        ] {
            assert_eq!(identity(bad), None, "{bad}");
        }
    }
}
