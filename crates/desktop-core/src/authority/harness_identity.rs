//! Company sign-in for the Harness Gateway credential inside the authority
//! (C20, D047).
//!
//! A tenant may configure one company sign-in: the Gateway workspace and
//! runtime endpoint the token is for, and the identity provider's issuer,
//! public client and Gateway audience. Signing in creates a grant: a stable
//! ID a conversation's engine binding names. Renewals and a sign-in again
//! to the same account keep it; signing out, removing the configuration or
//! removing the tenant ends it, and a later sign-in is a new grant that
//! never revives an old conversation.
//!
//! The refresh token is kept in the Keychain under write-ahead ownership
//! (the record joins the cleanup ledger before it is written and leaves it
//! only in the save that names it). The access token stays in memory. One
//! renewal runs at a time per grant and runs to completion even if its
//! caller goes away; a token for another account, a refused renewal or an
//! unreadable Keychain item means signing in again, never a send.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zeroize::Zeroizing;

use super::{Effect, Inner, Refusal, State, lock};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway::ListedWorkspace;
use crate::gateway_inference::InferenceTarget;
use crate::harness::callback::{self, Callback};
use crate::harness::history::CredentialSource;
use crate::harness::identity::{
    Access, IdentityClient, Provider, SignInFailure, SsoConfig, Verifier,
};
use crate::store::{StoredGrant, StoredSignIn};
use crate::tenants::TenantId;
use crate::vault::{RecordId, VaultError};

/// Wall-clock seconds since the epoch (token times are judged on it).
pub(super) fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Where an interactive sign-in stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SignInStep {
    /// Reading the provider's configuration.
    Contacting,
    /// Waiting for the browser to come back.
    Browser,
    /// Exchanging and verifying the tokens.
    Completing,
}

/// The company sign-in, as the presentation sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSignInView {
    pub workspace: ListedWorkspace,
    pub endpoint: String,
    pub issuer: String,
    pub client_id: String,
    pub audience: String,
    /// The Harness uses this sign-in rather than the workspace key.
    pub selected: bool,
    /// The signed-in account's name, while signed in.
    pub account: Option<String>,
    /// Signing in is in progress.
    pub step: Option<SignInStep>,
    /// The last sign-in or renewal failed; while signed in, this means it
    /// must be renewed by signing in again before anything is sent.
    pub failure: Option<SignInFailure>,
}

/// An interactive sign-in.
pub(super) struct SignInFlow {
    flow: u64,
    tenant: TenantId,
    step: Option<SignInStep>,
    failure: Option<SignInFailure>,
    cancel: CancellationToken,
}

/// What renewing needs, read off the lock.
pub(super) struct GrantAuth {
    provider: Option<Provider>,
    verifier: Verifier,
    access: Option<Access>,
    refresh: Option<Zeroizing<String>>,
    record: RecordId,
}

/// A signed-in grant's live tokens (memory only).
pub(super) struct Grant {
    pub(super) tenant: TenantId,
    pub(super) id: String,
    config: SsoConfig,
    slug: String,
    subject: String,
    auth: tokio::sync::Mutex<GrantAuth>,
    /// When the cached access token should be renewed (0: none cached).
    renew_at: AtomicU64,
    /// Why it must be renewed by signing in again.
    expired: Mutex<Option<SignInFailure>>,
}

impl Grant {
    fn new(sign_in: &StoredSignIn, grant: &StoredGrant, config: SsoConfig) -> Arc<Self> {
        Arc::new(Self {
            tenant: sign_in.tenant.clone(),
            id: grant.id.clone(),
            config,
            slug: sign_in.target.workspace.slug.clone(),
            subject: grant.subject.clone(),
            auth: tokio::sync::Mutex::new(GrantAuth {
                provider: None,
                verifier: Verifier::new(),
                access: None,
                refresh: None,
                record: grant.record.clone(),
            }),
            renew_at: AtomicU64::new(0),
            expired: Mutex::new(None),
        })
    }

    pub(super) fn expired(&self) -> Option<SignInFailure> {
        self.expired.lock().ok().and_then(|e| *e)
    }

    /// Signed in again: usable until it next lapses.
    fn revive(&self) {
        if let Ok(mut e) = self.expired.lock() {
            *e = None;
        }
    }

    fn expire(&self, failure: SignInFailure) {
        if let Ok(mut e) = self.expired.lock() {
            *e = Some(failure);
        }
        self.renew_at.store(0, Ordering::SeqCst);
    }

    /// A usable access token and when to renew it, renewing it when it is
    /// missing or inside its margin. Runs to completion even if the caller
    /// is dropped: a rotated refresh token is then saved or revoked.
    pub(super) async fn access(
        self: &Arc<Self>,
        inner: &Arc<Inner>,
    ) -> Result<(Zeroizing<String>, u64), SignInFailure> {
        let grant = Arc::clone(self);
        let inner = Arc::clone(inner);
        tokio::spawn(async move { grant.renew(&inner).await })
            .await
            .unwrap_or(Err(SignInFailure::Cancelled))
    }

    async fn renew(&self, inner: &Arc<Inner>) -> Result<(Zeroizing<String>, u64), SignInFailure> {
        let mut auth = self.auth.lock().await;
        let now = now();
        if let Some(access) = auth.access.as_ref()
            && now < access.renew_at()
        {
            return Ok((access.token.clone(), access.renew_at()));
        }
        if let Some(failure) = self.expired() {
            return Err(failure);
        }
        let client = identity_client(inner).ok_or(SignInFailure::Network)?;
        if auth.provider.is_none() {
            auth.provider = Some(client.discover(&self.config).await?);
        }
        let provider = auth.provider.clone().ok_or(SignInFailure::InvalidConfig)?;
        if auth.refresh.is_none() {
            let reader = Arc::clone(inner);
            let record = auth.record.clone();
            let read = tokio::task::spawn_blocking(move || reader.adapters.keychain.get(&record))
                .await
                .unwrap_or(Err(VaultError::Unavailable));
            match read.map(|bytes| saved_refresh(&bytes)) {
                Ok(Some(Some(refresh))) => auth.refresh = Some(refresh),
                // Signed in without a refresh token, or the item is gone or
                // unreadable: once the access token lapses, only signing in
                // again renews it.
                Ok(_) | Err(VaultError::NotFound) => {
                    self.expire(SignInFailure::SessionExpired);
                    return Err(SignInFailure::SessionExpired);
                }
                Err(_) => return Err(SignInFailure::StoreFailed),
            }
        }
        let previous = auth.refresh.clone().ok_or(SignInFailure::SessionExpired)?;
        let tokens = match client.refresh(&provider, &self.config, &previous).await {
            Ok(tokens) => tokens,
            // A transient failure is not an expiry: the next send retries.
            Err(SignInFailure::Network) => return Err(SignInFailure::Network),
            Err(failure) => {
                self.expire(SignInFailure::SessionExpired);
                return Err(if failure == SignInFailure::Denied {
                    SignInFailure::SessionExpired
                } else {
                    failure
                });
            }
        };
        let verified = auth
            .verifier
            .access_token(
                &client,
                &provider,
                &self.config,
                tokens.access,
                Some(&self.subject),
                &self.slug,
                now,
            )
            .await;
        let access = match verified {
            Ok(access) => access,
            Err(failure) => {
                // Another account or an unverifiable token: never kept.
                if let Some(refresh) = &tokens.refresh
                    && refresh.as_str() != previous.as_str()
                {
                    let _ = client.revoke(&provider, &self.config, refresh).await;
                }
                if failure != SignInFailure::Network {
                    self.expire(failure);
                }
                return Err(failure);
            }
        };
        if let Some(rotated) = tokens.refresh.filter(|r| r.as_str() != previous.as_str()) {
            match rotate(inner, self, &auth.record, &rotated).await {
                Some(record) => {
                    auth.record = record;
                    auth.refresh = Some(rotated);
                }
                None => {
                    let _ = client.revoke(&provider, &self.config, &rotated).await;
                    return Err(SignInFailure::StoreFailed);
                }
            }
        }
        let renew_at = access.renew_at();
        let token = access.token.clone();
        auth.access = Some(access);
        self.renew_at.store(renew_at, Ordering::SeqCst);
        Ok((token, renew_at))
    }

    /// Best-effort revocation of this grant's refresh token (read from the
    /// Keychain first when this launch has not loaded it).
    async fn revoke(&self, inner: &Arc<Inner>) {
        let mut auth = self.auth.lock().await;
        auth.access = None;
        self.renew_at.store(0, Ordering::SeqCst);
        let Some(client) = identity_client(inner) else {
            return;
        };
        if auth.refresh.is_none() {
            let reader = Arc::clone(inner);
            let record = auth.record.clone();
            if let Ok(Ok(bytes)) =
                tokio::task::spawn_blocking(move || reader.adapters.keychain.get(&record)).await
            {
                auth.refresh = saved_refresh(&bytes).flatten();
            }
        }
        let Some(refresh) = auth.refresh.take() else {
            return;
        };
        let provider = match auth.provider.clone() {
            Some(provider) => provider,
            None => match client.discover(&self.config).await {
                Ok(provider) => provider,
                Err(_) => return,
            },
        };
        let _ = client.revoke(&provider, &self.config, &refresh).await;
    }
}

/// The Keychain item: the refresh token, if the provider issued one.
#[derive(serde::Serialize, serde::Deserialize)]
struct SavedSecret {
    refresh_token: Option<String>,
}

fn secret_bytes(refresh: Option<&str>) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(
        serde_json::to_vec(&SavedSecret {
            refresh_token: refresh.map(str::to_owned),
        })
        .unwrap_or_default(),
    )
}

fn saved_refresh(bytes: &[u8]) -> Option<Option<Zeroizing<String>>> {
    let saved: SavedSecret = serde_json::from_slice(bytes).ok()?;
    Some(saved.refresh_token.map(Zeroizing::new))
}

fn identity_client(inner: &Arc<Inner>) -> Option<IdentityClient> {
    let state = lock(inner).ok()?;
    let transport = state.transport.as_ref()?.for_admission().ok()?;
    Some(IdentityClient::new(transport.http().clone()))
}

/// Saves a rotated refresh token under write-ahead ownership; `None` if the
/// grant ended or its record changed meanwhile, or saving failed (the
/// caller revokes it). The record is swapped only from `old`, the one this
/// renewal started from, so a stale completion never overwrites newer
/// material (C20 review F3).
async fn rotate(
    inner: &Arc<Inner>,
    grant: &Grant,
    old: &RecordId,
    refresh: &str,
) -> Option<RecordId> {
    let record = RecordId::issue()?;
    {
        let mut state = lock(inner).ok()?;
        if !state.grant_holds(grant, old) || !begin_write(&mut state, &record) {
            return None;
        }
    }
    let written = write(inner, &record, secret_bytes(Some(refresh))).await;
    let committed = {
        let mut state = lock(inner).ok()?;
        settle_write(&mut state, &record);
        written && state.grant_holds(grant, old) && {
            let before = state.data.clone();
            if let Some(g) = state
                .tenant_mut(&grant.tenant)
                .and_then(|t| t.harness_sign_in.as_mut())
                .and_then(|s| s.grant.as_mut())
            {
                g.record = record.clone();
            }
            state.data.pending_cleanup.retain(|r| r != &record);
            state.data.pending_cleanup.push(old.clone());
            state.save_with(&before).is_ok()
        }
    };
    super::run_effects(inner, vec![Effect::Cleanup]);
    committed.then_some(record)
}

/// Puts `record` in the durable cleanup ledger before it is written and
/// marks the write in flight, so cleanup and reset wait for it. Only a
/// ledger entry confirmed durable authorizes the Keychain write (C20
/// review F2); otherwise nothing is written and the entry settles absent.
pub(super) fn begin_write(state: &mut State, record: &RecordId) -> bool {
    let before = state.data.clone();
    state.data.pending_cleanup.push(record.clone());
    if state.save_with(&before).is_err()
        || (state.durability_unconfirmed && !state.confirm_durable())
    {
        return false;
    }
    state.writes_in_flight.push(record.clone());
    true
}

/// Writes a record begun with [`begin_write`]. It stays in flight (so
/// cleanup leaves it alone) until the caller settles it with
/// [`settle_write`] under the same lock as the save that names it or leaves
/// it in the ledger (P06, C22 review F4).
pub(super) async fn write(
    inner: &Arc<Inner>,
    record: &RecordId,
    bytes: Zeroizing<Vec<u8>>,
) -> bool {
    let writer = Arc::clone(inner);
    let written = record.clone();
    let put = tokio::task::spawn_blocking(move || writer.adapters.keychain.put(&written, &bytes))
        .await
        .unwrap_or(Err(VaultError::Unavailable));
    let hold = lock(inner).map(|s| Arc::clone(&s.settle_hold));
    if let Ok(hold) = hold {
        let _settling = hold.lock().await;
    }
    put.is_ok()
}

/// Ends a write's flight; called under the lock that settles who owns it.
pub(super) fn settle_write(state: &mut State, record: &RecordId) {
    state.writes_in_flight.retain(|r| r != record);
}

impl State {
    /// The tenant's sign-in, as configured.
    fn sign_in_of(&self, tenant: &TenantId) -> Option<&StoredSignIn> {
        self.tenant(tenant)?.harness_sign_in.as_ref()
    }

    /// Whether `grant` is still the tenant's signed-in grant.
    fn grant_current(&self, grant: &Grant) -> bool {
        self.sign_in_of(&grant.tenant)
            .and_then(|s| s.grant.as_ref())
            .is_some_and(|g| g.id == grant.id && g.subject == grant.subject)
    }

    /// Whether `grant` is current and its stored refresh token is `record`.
    fn grant_holds(&self, grant: &Grant, record: &RecordId) -> bool {
        self.grant_current(grant)
            && self
                .sign_in_of(&grant.tenant)
                .and_then(|s| s.grant.as_ref())
                .is_some_and(|g| &g.record == record)
    }

    /// The live grant of the tenant's signed-in account, made on first use.
    pub(super) fn grant(&mut self, tenant: &TenantId) -> Option<Arc<Grant>> {
        let sign_in = self.sign_in_of(tenant)?.clone();
        let stored = sign_in.grant.clone()?;
        if let Some(live) = self
            .harness_grants
            .iter()
            .find(|g| &g.tenant == tenant && g.id == stored.id)
        {
            return Some(Arc::clone(live));
        }
        let config = SsoConfig::parse(&sign_in.issuer, &sign_in.client_id, &sign_in.audience)?;
        let live = Grant::new(&sign_in, &stored, config);
        self.harness_grants.retain(|g| &g.tenant != tenant);
        self.harness_grants.push(Arc::clone(&live));
        Some(live)
    }

    /// The source and grant a new conversation in `workspace` would use:
    /// the signed-in grant when the sign-in is chosen for that workspace.
    pub(super) fn harness_source(
        &mut self,
        tenant: &TenantId,
        workspace: &ListedWorkspace,
    ) -> Option<(StoredSignIn, Arc<Grant>)> {
        let sign_in = self.sign_in_of(tenant)?.clone();
        if !sign_in.selected || sign_in.target.workspace.id != workspace.id {
            return None;
        }
        let grant = self.grant(tenant)?;
        Some((sign_in, grant))
    }

    /// Configures company sign-in for the chosen workspace. Refused while
    /// signed in: a changed configuration is a new identity (sign out
    /// first).
    pub(super) fn configure_harness_sign_in(
        &mut self,
        context: &ContextRef,
        reference: &WorkspaceRef,
        endpoint: String,
        issuer: &str,
        client_id: &str,
        audience: &str,
    ) -> Result<(), Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let workspace = self.inference_workspace(context, reference)?;
        let target = InferenceTarget {
            workspace,
            endpoint,
        };
        let config = SsoConfig::parse(issuer, client_id, audience).ok_or(Refusal::InvalidInput)?;
        if !target.is_valid() {
            return Err(Refusal::InvalidInput);
        }
        let current = self.tenant(&tenant).ok_or(Refusal::StaleContext)?;
        if current
            .harness_sign_in
            .as_ref()
            .is_some_and(|s| s.grant.is_some())
            || self
                .harness_sign_in
                .as_ref()
                .is_some_and(|f| f.tenant == tenant)
        {
            return Err(Refusal::Busy);
        }
        let before = self.data.clone();
        let selected = current.harness_sign_in.as_ref().is_some_and(|s| s.selected);
        if let Some(t) = self.tenant_mut(&tenant) {
            t.harness_sign_in = Some(StoredSignIn {
                tenant: tenant.clone(),
                target,
                issuer: config.issuer,
                client_id: config.client_id,
                audience: config.audience,
                selected,
                grant: None,
            });
        }
        self.save_with(&before)
    }

    /// Removes the configuration (signed out first).
    pub(super) fn remove_harness_sign_in(
        &mut self,
        context: &ContextRef,
    ) -> Result<Option<Arc<Grant>>, Refusal> {
        let ended = self.sign_out_harness(context)?;
        let tenant = self.check_context(context)?;
        let before = self.data.clone();
        if let Some(t) = self.tenant_mut(&tenant) {
            t.harness_sign_in = None;
        }
        self.save_with(&before)?;
        Ok(ended)
    }

    /// Chooses which credential new conversations use.
    pub(super) fn choose_harness_credential(
        &mut self,
        context: &ContextRef,
        source: CredentialSource,
    ) -> Result<(), Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let before = self.data.clone();
        let sign_in = self
            .tenant_mut(&tenant)
            .and_then(|t| t.harness_sign_in.as_mut())
            .ok_or(Refusal::CapabilityMissing)?;
        sign_in.selected = source == CredentialSource::CompanySignIn;
        self.save_with(&before)
    }

    /// Starts an interactive sign-in for the tenant's configuration.
    pub(super) fn begin_harness_sign_in(
        &mut self,
        context: &ContextRef,
    ) -> Result<Effect, Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let sign_in = self
            .sign_in_of(&tenant)
            .cloned()
            .ok_or(Refusal::CapabilityMissing)?;
        if self
            .harness_sign_in
            .as_ref()
            .is_some_and(|f| f.step.is_some())
        {
            return Err(Refusal::Busy);
        }
        let config = SsoConfig::parse(&sign_in.issuer, &sign_in.client_id, &sign_in.audience)
            .ok_or(Refusal::InvalidInput)?;
        let flow = self.lease();
        let cancel = CancellationToken::new();
        self.harness_sign_in = Some(SignInFlow {
            flow,
            tenant: tenant.clone(),
            step: Some(SignInStep::Contacting),
            failure: None,
            cancel: cancel.clone(),
        });
        Ok(Effect::HarnessSignIn(Box::new(SignInJob {
            flow,
            tenant,
            config,
            target: sign_in.target,
            existing: sign_in.grant,
            cancel,
        })))
    }

    pub(super) fn cancel_harness_sign_in(&mut self, context: &ContextRef) -> Result<(), Refusal> {
        let tenant = self.check_context(context)?;
        match self.harness_sign_in.take() {
            Some(flow) if flow.tenant == tenant => {
                flow.cancel.cancel();
                Ok(())
            }
            other => {
                self.harness_sign_in = other;
                Err(Refusal::InvalidInput)
            }
        }
    }

    /// Signs out: the grant ends (its conversation is retired by the next
    /// sync), its refresh token record joins the cleanup ledger, and the
    /// live grant is returned for revocation.
    pub(super) fn sign_out_harness(
        &mut self,
        context: &ContextRef,
    ) -> Result<Option<Arc<Grant>>, Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        if let Some(flow) = self.harness_sign_in.take() {
            if flow.tenant == tenant {
                flow.cancel.cancel();
            } else {
                self.harness_sign_in = Some(flow);
            }
        }
        let live = self.grant(&tenant);
        let before = self.data.clone();
        let Some(grant) = self
            .tenant_mut(&tenant)
            .and_then(|t| t.harness_sign_in.as_mut())
            .and_then(|s| s.grant.take())
        else {
            return Ok(None);
        };
        self.data.pending_cleanup.push(grant.record);
        self.save_with(&before)?;
        self.harness_grants.retain(|g| g.tenant != tenant);
        Ok(live)
    }

    pub(super) fn harness_sign_in_view(&self) -> Option<HarnessSignInView> {
        let tenant = self.data.active.as_ref()?;
        let sign_in = self.sign_in_of(tenant)?;
        let flow = self
            .harness_sign_in
            .as_ref()
            .filter(|f| &f.tenant == tenant);
        let live = sign_in.grant.as_ref().and_then(|g| {
            self.harness_grants
                .iter()
                .find(|l| &l.tenant == tenant && l.id == g.id)
        });
        Some(HarnessSignInView {
            workspace: sign_in.target.workspace.clone(),
            endpoint: sign_in.target.endpoint.clone(),
            issuer: sign_in.issuer.clone(),
            client_id: sign_in.client_id.clone(),
            audience: sign_in.audience.clone(),
            selected: sign_in.selected,
            account: sign_in.grant.as_ref().map(|g| g.label.clone()),
            step: flow.and_then(|f| f.step),
            // While signing in, only that attempt's own failure.
            failure: match flow {
                Some(f) => f.failure,
                None => live.and_then(|l| l.expired()),
            },
        })
    }

    fn sign_in_failed(&mut self, flow: u64, failure: SignInFailure) {
        if let Some(f) = self.harness_sign_in.as_mut().filter(|f| f.flow == flow) {
            f.step = None;
            f.failure = Some(failure);
            self.changed();
        }
    }

    fn sign_in_step(&mut self, flow: u64, step: SignInStep) -> bool {
        match self.harness_sign_in.as_mut().filter(|f| f.flow == flow) {
            Some(f) => {
                f.step = Some(step);
                self.changed();
                true
            }
            None => false,
        }
    }
}

/// An interactive sign-in, run off the lock.
pub(super) struct SignInJob {
    flow: u64,
    tenant: TenantId,
    config: SsoConfig,
    target: InferenceTarget,
    /// Signing in again to a live grant keeps it only for the same account.
    existing: Option<StoredGrant>,
    cancel: CancellationToken,
}

pub(super) async fn sign_in(inner: Arc<Inner>, job: SignInJob) {
    let SignInJob {
        flow,
        tenant,
        config,
        target,
        existing,
        cancel,
    } = job;
    let fail = |failure: SignInFailure| {
        if let Ok(mut state) = lock(&inner) {
            state.sign_in_failed(flow, failure);
        }
    };
    let Some(client) = identity_client(&inner) else {
        return fail(SignInFailure::Network);
    };
    let authorized = tokio::select! {
        _ = cancel.cancelled() => return,
        result = authorize(&inner, flow, &client, &config) => result,
    };
    let (provider, attempt, code) = match authorized {
        Ok(authorized) => authorized,
        Err(failure) => return fail(failure),
    };
    if !lock(&inner).is_ok_and(|mut s| s.sign_in_step(flow, SignInStep::Completing)) {
        return;
    }
    let tokens = match client.exchange(&provider, &config, &attempt, &code.0).await {
        Ok(tokens) => tokens,
        Err(failure) => return fail(failure),
    };
    let revoke = |refresh: Option<Zeroizing<String>>| {
        let client = client.clone();
        let provider = provider.clone();
        let config = config.clone();
        async move {
            if let Some(refresh) = refresh {
                let _ = client.revoke(&provider, &config, &refresh).await;
            }
        }
    };
    let mut verifier = Verifier::new();
    let now = now();
    let id_token = tokens.id_token.clone().unwrap_or_default();
    let account = match verifier
        .id_token(&client, &provider, &config, &id_token, &attempt.nonce, now)
        .await
    {
        Ok(account) => account,
        Err(failure) => {
            revoke(tokens.refresh).await;
            return fail(failure);
        }
    };
    let expected = existing.as_ref().map(|g| g.subject.as_str());
    let access = match verifier
        .access_token(
            &client,
            &provider,
            &config,
            tokens.access,
            expected,
            &target.workspace.slug,
            now,
        )
        .await
    {
        Ok(access) => access,
        Err(failure) => {
            revoke(tokens.refresh).await;
            return fail(failure);
        }
    };
    let Some(record) = RecordId::issue() else {
        revoke(tokens.refresh).await;
        return fail(SignInFailure::StoreFailed);
    };
    let grant_id = match &existing {
        Some(g) => g.id.clone(),
        None => match new_grant_id() {
            Some(id) => id,
            None => {
                revoke(tokens.refresh).await;
                return fail(SignInFailure::StoreFailed);
            }
        },
    };
    // A sign-in again to the same account renews the live grant in place:
    // it waits for a renewal under way and holds the grant's tokens until
    // its own are in, so neither overwrites the other (C20 review F3).
    let live = existing.as_ref().and_then(|g| {
        lock(&inner)
            .ok()
            .and_then(|mut state| state.grant(&tenant))
            .filter(|l| l.id == g.id)
    });
    let held = match &live {
        Some(live) => Some(live.auth.lock().await),
        None => None,
    };
    // Write-ahead: the record joins the ledger before it is written, and
    // only while this flow is current.
    let begun = lock(&inner).is_ok_and(|mut state| {
        state
            .harness_sign_in
            .as_ref()
            .is_some_and(|f| f.flow == flow)
            && begin_write(&mut state, &record)
    });
    if !begun {
        drop(held);
        revoke(tokens.refresh).await;
        return fail(SignInFailure::StoreFailed);
    }
    let secret = secret_bytes(tokens.refresh.as_deref().map(|r| r.as_str()));
    let written = write(&inner, &record, secret).await;
    let committed = (|| {
        let mut state = lock(&inner).ok()?;
        settle_write(&mut state, &record);
        if !written
            || !state
                .harness_sign_in
                .as_ref()
                .is_some_and(|f| f.flow == flow)
        {
            return None;
        }
        let current = state.sign_in_of(&tenant)?.clone();
        // The configuration must still be the one signed in with.
        if current.issuer != config.issuer
            || current.client_id != config.client_id
            || current.audience != config.audience
            || current.target != target
            || current.grant.as_ref().map(|g| &g.id) != existing.as_ref().map(|g| &g.id)
        {
            return None;
        }
        let before = state.data.clone();
        let old = current.grant.as_ref().map(|g| g.record.clone());
        let stored = StoredGrant {
            id: grant_id.clone(),
            subject: access.subject.clone(),
            label: account.label.clone(),
            record: record.clone(),
        };
        if let Some(s) = state
            .tenant_mut(&tenant)
            .and_then(|t| t.harness_sign_in.as_mut())
        {
            s.grant = Some(stored.clone());
            s.selected = true;
        }
        state.data.pending_cleanup.retain(|r| r != &record);
        state.data.pending_cleanup.extend(old);
        state.save_with(&before).ok()?;
        state.harness_sign_in = None;
        let renewed = live
            .as_ref()
            .filter(|l| state.harness_grants.iter().any(|g| Arc::ptr_eq(g, l)))
            .cloned();
        let target = match renewed {
            Some(live) => live,
            None => {
                state.harness_grants.retain(|g| g.tenant != tenant);
                let sign_in = state.sign_in_of(&tenant)?.clone();
                let made = Grant::new(&sign_in, &stored, config.clone());
                state.harness_grants.push(Arc::clone(&made));
                made
            }
        };
        state.changed();
        Some(target)
    })();
    match committed {
        Some(target) => {
            let reused = live.as_ref().is_some_and(|l| Arc::ptr_eq(l, &target));
            let mut auth = match held {
                Some(auth) if reused => auth,
                other => {
                    drop(other);
                    target.auth.lock().await
                }
            };
            let renew_at = access.renew_at();
            auth.provider = Some(provider);
            auth.verifier = verifier;
            auth.refresh = tokens.refresh;
            auth.access = Some(access);
            auth.record = record;
            target.revive();
            target.renew_at.store(renew_at, Ordering::SeqCst);
            drop(auth);
            if let Ok(mut state) = lock(&inner) {
                state.changed();
            }
        }
        None => {
            drop(held);
            revoke(tokens.refresh).await;
            fail(SignInFailure::StoreFailed);
        }
    }
    super::run_effects(&inner, vec![Effect::Cleanup]);
}

fn new_grant_id() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(crate::harness::protocol::derived_uuid(
        "sign-in-grant",
        &hex::encode(bytes),
    ))
}

/// Discovery, the loopback listener, the browser and its callback.
async fn authorize(
    inner: &Arc<Inner>,
    flow: u64,
    client: &IdentityClient,
    config: &SsoConfig,
) -> Result<(Provider, crate::harness::identity::Attempt, callback::Code), SignInFailure> {
    let provider = client.discover(config).await?;
    let listener = Callback::bind().await?;
    let attempt = crate::harness::identity::attempt(&provider, config, listener.port())
        .ok_or(SignInFailure::CallbackUnavailable)?;
    let opener = {
        let mut state = lock(inner).map_err(|_| SignInFailure::Cancelled)?;
        if !state.sign_in_step(flow, SignInStep::Browser) {
            return Err(SignInFailure::Cancelled);
        }
        Arc::clone(&inner.adapters.opener)
    };
    let url = attempt.url.clone();
    let opened = tokio::task::spawn_blocking(move || opener.open(&url))
        .await
        .unwrap_or(false);
    if !opened {
        return Err(SignInFailure::BrowserUnavailable);
    }
    let code = listener
        .wait(
            &attempt.state,
            &config.issuer,
            provider.iss_in_callback,
            Instant::now() + callback::ATTEMPT,
        )
        .await?;
    Ok((provider, attempt, code))
}

/// Ends a grant at the provider (best effort) after signing out.
pub(super) async fn signed_out(inner: Arc<Inner>, grant: Arc<Grant>) {
    grant.revoke(&inner).await;
    super::run_effects(&inner, vec![Effect::Cleanup]);
}
