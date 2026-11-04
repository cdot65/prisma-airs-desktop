//! Workspace MCP servers for the Harness inside the authority (C22, D049).
//!
//! The chosen Gateway workspace's MCP catalog is read through the tenant's
//! management client. The operator turns servers on (at most eight per
//! workspace) and signs in to each in the browser; the desktop does the
//! OAuth ([`crate::harness::mcp_auth`]), never the engine. A sign-in is a
//! grant: a UUID bound to the server's URL and catalog revision when it was
//! made, its tokens a Keychain item kept under write-ahead ownership (the
//! record joins the cleanup ledger, confirmed durable, and is reserved as
//! in flight before it is written; it leaves the ledger only in the save
//! that names it). A renewed token rotates the item the same way.
//!
//! A conversation's MCP scope is fixed when it starts: the servers on in
//! the loaded catalog, each with its grant if it was made for the server's
//! current URL and revision. Signing out, signing in again, turning a
//! server off or a revision change retires a conversation that holds the
//! server; an expired sign-in does not (the engine gets the signed-out
//! placeholder, the server's tools fail to start and inference goes on).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use url::Url;
use zeroize::Zeroizing;

use super::harness_identity::{SignInStep, begin_write, now, settle_write, write};
use super::{Effect, Inner, Refusal, State, lock};
use crate::context::{ContextRef, WorkspaceRef};
use crate::gateway::{GatewayRead, GatewayResult, McpCatalog};
use crate::harness::callback::{self, Callback};
use crate::harness::mcp::{self, CatalogServer, ScopeServer};
use crate::harness::mcp_auth::{McpAuthClient, McpProvider, McpSignInFailure, McpTokens};
use crate::management::{ManagementFailure, ManagementRequest, ManagementResult};
use crate::store::{StoredMcpGrant, StoredMcpServer};
use crate::tenants::TenantId;
use crate::vault::{RecordId, VaultError};

/// A scope's grants, in scope order (`None`: no current sign-in).
pub(super) type ScopeGrants = Vec<Option<Arc<McpGrant>>>;
/// A scope's tokens (`None`: the signed-out placeholder) and the earliest
/// time one must be renewed.
pub(super) type ScopeTokens = (Vec<Option<Zeroizing<String>>>, Option<u64>);

/// How a conversation's MCP scope stands (see `mcp_scope_check`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ScopeCheck {
    Current,
    Unchecked,
    Changed,
}

/// The chosen workspace's catalog, as last read.
pub(super) struct CatalogState {
    tenant: TenantId,
    workspace: String,
    lease: u64,
    loading: bool,
    catalog: Option<McpCatalog>,
    failure: Option<ManagementFailure>,
}

/// An interactive sign-in to one server.
pub(super) struct McpFlow {
    flow: u64,
    tenant: TenantId,
    workspace: String,
    server_id: String,
    step: Option<SignInStep>,
    failure: Option<McpSignInFailure>,
    cancel: CancellationToken,
}

/// Where the chosen workspace's catalog stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum McpListState {
    Loading,
    Ready,
    Failed,
}

/// One catalog server, as the presentation sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub id: String,
    pub name: String,
    pub namespace: String,
    pub url: String,
    pub status: String,
    /// The engine can take it.
    pub supported: bool,
    /// On for new conversations.
    pub enabled: bool,
    /// Signed in for the server's current URL and revision.
    pub signed_in: bool,
    /// Signed in for an earlier revision or URL: sign in again.
    pub stale: bool,
    /// Why the sign-in must be renewed by signing in again.
    pub expired: Option<McpSignInFailure>,
    /// Signing in is in progress.
    pub step: Option<SignInStep>,
    /// The last sign-in to it failed.
    pub failure: Option<McpSignInFailure>,
}

/// The chosen workspace's MCP servers (C22).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HarnessMcpView {
    pub workspace: String,
    pub state: McpListState,
    pub failure: Option<ManagementFailure>,
    /// Every catalog page was read.
    pub complete: bool,
    pub servers: Vec<McpServerView>,
}

/// What renewing a grant's token needs, read off the lock.
struct McpAuth {
    loaded: bool,
    access: Option<Zeroizing<String>>,
    refresh: Option<Zeroizing<String>>,
    issued: u64,
    expires: Option<u64>,
    record: RecordId,
}

/// A signed-in server's live tokens.
pub(super) struct McpGrant {
    pub(super) tenant: TenantId,
    pub(super) workspace: String,
    pub(super) server_id: String,
    pub(super) id: String,
    url: String,
    client_id: String,
    token_endpoint: Option<Url>,
    revocation: Option<Url>,
    auth: tokio::sync::Mutex<McpAuth>,
    /// When the cached access token should be renewed (0: none cached;
    /// `u64::MAX`: it does not expire).
    renew_at: AtomicU64,
    /// When the token in use expires (0: not loaded yet; `u64::MAX`: it
    /// does not expire). Checked against the clock before any call is
    /// approved (P06, C22 review F1).
    expires_at: AtomicU64,
    expired: Mutex<Option<McpSignInFailure>>,
}

/// `exp - min(120 s, lifetime / 2)`, as for the Gateway credential.
fn renew_at(issued: u64, expires: Option<u64>) -> u64 {
    match expires {
        Some(exp) => exp.saturating_sub((exp.saturating_sub(issued) / 2).min(120)),
        None => u64::MAX,
    }
}

impl McpGrant {
    fn new(tenant: &TenantId, server: &StoredMcpServer, grant: &StoredMcpGrant) -> Arc<Self> {
        Arc::new(Self {
            tenant: tenant.clone(),
            workspace: server.workspace.clone(),
            server_id: server.server_id.clone(),
            id: grant.id.clone(),
            url: grant.url.clone(),
            client_id: grant.client_id.clone(),
            token_endpoint: Url::parse(&grant.token_endpoint).ok(),
            revocation: grant
                .revocation_endpoint
                .as_deref()
                .and_then(|r| Url::parse(r).ok()),
            auth: tokio::sync::Mutex::new(McpAuth {
                loaded: false,
                access: None,
                refresh: None,
                issued: 0,
                expires: None,
                record: grant.record.clone(),
            }),
            renew_at: AtomicU64::new(0),
            expires_at: AtomicU64::new(0),
            expired: Mutex::new(None),
        })
    }

    pub(super) fn expired(&self) -> Option<McpSignInFailure> {
        self.expired.lock().ok().and_then(|e| *e)
    }

    /// Whether its token may be used now: not expired by a failure, and
    /// not past its own expiry by the clock (a call held across expiry,
    /// sleep included, is never approved with a lapsed token).
    pub(super) fn usable_now(&self) -> bool {
        self.expired().is_none() && now() < self.expires_at.load(Ordering::SeqCst)
    }

    fn set_expiry(&self, expires: Option<u64>) {
        self.expires_at
            .store(expires.unwrap_or(u64::MAX), Ordering::SeqCst);
    }

    fn expire(&self, failure: McpSignInFailure) {
        if let Ok(mut e) = self.expired.lock() {
            *e = Some(failure);
        }
        self.renew_at.store(0, Ordering::SeqCst);
    }

    #[cfg(feature = "test-fixtures")]
    pub(super) fn expire_for_tests(&self) {
        self.expire(McpSignInFailure::SessionExpired);
    }

    /// When the token in use should be renewed (0: unknown).
    pub(super) fn renew_at(&self) -> u64 {
        self.renew_at.load(Ordering::SeqCst)
    }

    /// A usable access token, renewed first when it is missing or inside
    /// its margin. Runs to completion even if the caller is dropped: a
    /// rotated token is then saved or revoked.
    pub(super) async fn token(
        self: &Arc<Self>,
        inner: &Arc<Inner>,
    ) -> Result<Zeroizing<String>, McpSignInFailure> {
        let grant = Arc::clone(self);
        let inner = Arc::clone(inner);
        tokio::spawn(async move { grant.renew(&inner).await })
            .await
            .unwrap_or(Err(McpSignInFailure::Cancelled))
    }

    async fn renew(&self, inner: &Arc<Inner>) -> Result<Zeroizing<String>, McpSignInFailure> {
        let mut auth = self.auth.lock().await;
        if let Some(failure) = self.expired() {
            return Err(failure);
        }
        if !auth.loaded {
            let reader = Arc::clone(inner);
            let record = auth.record.clone();
            let read = tokio::task::spawn_blocking(move || reader.adapters.keychain.get(&record))
                .await
                .unwrap_or(Err(VaultError::Unavailable));
            match read.map(|bytes| SavedTokens::read(&bytes)) {
                Ok(Some(saved)) => {
                    auth.access = Some(Zeroizing::new(saved.access_token));
                    auth.refresh = saved.refresh_token.map(Zeroizing::new);
                    auth.issued = saved.issued;
                    auth.expires = saved.expires;
                    auth.loaded = true;
                    self.set_expiry(saved.expires);
                }
                Ok(None) | Err(VaultError::NotFound) => {
                    self.expire(McpSignInFailure::SessionExpired);
                    return Err(McpSignInFailure::SessionExpired);
                }
                Err(_) => return Err(McpSignInFailure::StoreFailed),
            }
        }
        let due = renew_at(auth.issued, auth.expires);
        if let Some(access) = auth.access.as_ref()
            && now() < due
        {
            self.renew_at.store(due, Ordering::SeqCst);
            return Ok(access.clone());
        }
        let (Some(previous), Some(endpoint)) = (auth.refresh.clone(), self.token_endpoint.clone())
        else {
            self.expire(McpSignInFailure::SessionExpired);
            return Err(McpSignInFailure::SessionExpired);
        };
        let client = auth_client(inner).ok_or(McpSignInFailure::Network)?;
        let tokens = match client
            .refresh(&endpoint, &self.client_id, &self.url, &previous)
            .await
        {
            Ok(tokens) => tokens,
            // A transient failure is not an expiry: the next send retries.
            Err(McpSignInFailure::Network) => return Err(McpSignInFailure::Network),
            Err(failure) => {
                let failure = if failure == McpSignInFailure::Denied {
                    McpSignInFailure::SessionExpired
                } else {
                    failure
                };
                self.expire(failure);
                return Err(failure);
            }
        };
        let issued = now();
        let refresh = tokens.refresh.clone().or(Some(previous.clone()));
        let saved = SavedTokens::bytes(
            &tokens.access,
            refresh.as_deref().map(|r| r.as_str()),
            issued,
            tokens.expires_in,
        );
        match rotate(inner, self, &auth.record, saved).await {
            Some(record) => {
                auth.record = record;
                auth.refresh = refresh;
                auth.access = Some(tokens.access.clone());
                auth.issued = issued;
                auth.expires = tokens.expires_in.map(|s| issued + s);
                self.set_expiry(auth.expires);
                let due = renew_at(issued, auth.expires);
                self.renew_at.store(due, Ordering::SeqCst);
                Ok(tokens.access)
            }
            None => {
                if let (Some(r), Some(url)) = (&tokens.refresh, &self.revocation)
                    && r.as_str() != previous.as_str()
                {
                    let _ = client
                        .revoke(url, &self.client_id, r, "refresh_token")
                        .await;
                }
                Err(McpSignInFailure::StoreFailed)
            }
        }
    }

    /// Best-effort revocation at the server after the grant is gone here.
    async fn revoke(&self, inner: &Arc<Inner>) {
        let mut auth = self.auth.lock().await;
        self.renew_at.store(0, Ordering::SeqCst);
        let (Some(url), Some(client)) = (self.revocation.clone(), auth_client(inner)) else {
            return;
        };
        if !auth.loaded {
            let reader = Arc::clone(inner);
            let record = auth.record.clone();
            if let Ok(Ok(bytes)) =
                tokio::task::spawn_blocking(move || reader.adapters.keychain.get(&record)).await
                && let Some(saved) = SavedTokens::read(&bytes)
            {
                auth.access = Some(Zeroizing::new(saved.access_token));
                auth.refresh = saved.refresh_token.map(Zeroizing::new);
            }
        }
        if let Some(refresh) = auth.refresh.take() {
            let _ = client
                .revoke(&url, &self.client_id, &refresh, "refresh_token")
                .await;
        }
        if let Some(access) = auth.access.take() {
            let _ = client
                .revoke(&url, &self.client_id, &access, "access_token")
                .await;
        }
    }
}

/// The Keychain item: the grant's tokens and their times.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedTokens {
    access_token: String,
    refresh_token: Option<String>,
    issued: u64,
    expires: Option<u64>,
}

impl SavedTokens {
    fn bytes(
        access: &str,
        refresh: Option<&str>,
        issued: u64,
        expires_in: Option<u64>,
    ) -> Zeroizing<Vec<u8>> {
        Zeroizing::new(
            serde_json::to_vec(&SavedTokens {
                access_token: access.to_owned(),
                refresh_token: refresh.map(str::to_owned),
                issued,
                expires: expires_in.map(|s| issued + s),
            })
            .unwrap_or_default(),
        )
    }

    fn read(bytes: &[u8]) -> Option<Self> {
        let saved: Self = serde_json::from_slice(bytes).ok()?;
        mcp::valid_token(&saved.access_token).then_some(saved)
    }
}

fn auth_client(inner: &Arc<Inner>) -> Option<McpAuthClient> {
    let state = lock(inner).ok()?;
    let transport = state.transport.as_ref()?.for_admission().ok()?;
    Some(McpAuthClient::new(transport.http().clone()))
}

/// Saves a renewed token under write-ahead ownership and swaps the grant's
/// record; `None` if the grant ended meanwhile or saving failed.
async fn rotate(
    inner: &Arc<Inner>,
    grant: &McpGrant,
    old: &RecordId,
    bytes: Zeroizing<Vec<u8>>,
) -> Option<RecordId> {
    let record = RecordId::issue()?;
    {
        let mut state = lock(inner).ok()?;
        if !state.mcp_grant_current(grant) || !begin_write(&mut state, &record) {
            return None;
        }
    }
    let written = write(inner, &record, bytes).await;
    let committed = {
        let mut state = lock(inner).ok()?;
        settle_write(&mut state, &record);
        written && state.mcp_grant_current(grant) && {
            let before = state.data.clone();
            if let Some(g) = state
                .stored_mcp_mut(&grant.tenant, &grant.workspace, &grant.server_id)
                .and_then(|m| m.grant.as_mut())
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

impl State {
    pub(super) fn stored_mcp(
        &self,
        tenant: &TenantId,
        workspace: &str,
        server: &str,
    ) -> Option<&StoredMcpServer> {
        self.tenant(tenant)?
            .harness_mcp
            .iter()
            .find(|m| m.workspace == workspace && m.server_id == server)
    }

    fn stored_mcp_mut(
        &mut self,
        tenant: &TenantId,
        workspace: &str,
        server: &str,
    ) -> Option<&mut StoredMcpServer> {
        self.tenant_mut(tenant)?
            .harness_mcp
            .iter_mut()
            .find(|m| m.workspace == workspace && m.server_id == server)
    }

    /// Whether `grant` is still the server's sign-in.
    fn mcp_grant_current(&self, grant: &McpGrant) -> bool {
        self.stored_mcp(&grant.tenant, &grant.workspace, &grant.server_id)
            .and_then(|m| m.grant.as_ref())
            .is_some_and(|g| g.id == grant.id)
    }

    /// The live grant of a stored sign-in, made on first use.
    fn mcp_grant(
        &mut self,
        tenant: &TenantId,
        workspace: &str,
        server: &str,
    ) -> Option<Arc<McpGrant>> {
        let stored = self.stored_mcp(tenant, workspace, server)?.clone();
        let grant = stored.grant.clone()?;
        if let Some(live) = self.mcp_live.iter().find(|g| {
            &g.tenant == tenant
                && g.workspace == workspace
                && g.server_id == server
                && g.id == grant.id
        }) {
            return Some(Arc::clone(live));
        }
        let live = McpGrant::new(tenant, &stored, &grant);
        self.mcp_live.retain(|g| {
            !(&g.tenant == tenant && g.workspace == workspace && g.server_id == server)
        });
        self.mcp_live.push(Arc::clone(&live));
        Some(live)
    }

    /// Every live grant of a tenant (for revocation when it is removed).
    pub(super) fn mcp_grants_of(&mut self, tenant: &TenantId) -> Vec<Arc<McpGrant>> {
        let keys: Vec<(String, String)> = self
            .tenant(tenant)
            .map(|t| {
                t.harness_mcp
                    .iter()
                    .filter(|m| m.grant.is_some())
                    .map(|m| (m.workspace.clone(), m.server_id.clone()))
                    .collect()
            })
            .unwrap_or_default();
        keys.into_iter()
            .filter_map(|(w, s)| self.mcp_grant(tenant, &w, &s))
            .collect()
    }

    /// The loaded catalog of `tenant`'s `workspace`, as last read (shown,
    /// and enough to see that a server changed).
    fn mcp_catalog_of(&self, tenant: &TenantId, workspace: &str) -> Option<&McpCatalog> {
        self.mcp_catalog
            .as_ref()
            .filter(|c| &c.tenant == tenant && c.workspace == workspace)
            .and_then(|c| c.catalog.as_ref())
    }

    /// The catalog only when its latest read succeeded and none is under
    /// way: the only one that authorizes remote use (P06, C22 review F2).
    fn mcp_catalog_checked(&self, tenant: &TenantId, workspace: &str) -> Option<&McpCatalog> {
        self.mcp_catalog
            .as_ref()
            .filter(|c| {
                &c.tenant == tenant && c.workspace == workspace && !c.loading && c.failure.is_none()
            })
            .and_then(|c| c.catalog.as_ref())
    }

    /// Reads the chosen workspace's catalog.
    pub(super) fn list_mcp_servers(
        &mut self,
        context: &ContextRef,
        reference: &WorkspaceRef,
    ) -> Result<Effect, Refusal> {
        let tenant = self.check_context(context)?;
        let workspace = self.inference_workspace(context, reference)?;
        let lease = self.lease();
        let kept = self
            .mcp_catalog
            .take()
            .filter(|c| c.tenant == tenant && c.workspace == workspace.id)
            .and_then(|c| c.catalog);
        self.mcp_catalog = Some(CatalogState {
            tenant,
            workspace: workspace.id,
            lease,
            loading: true,
            catalog: kept,
            failure: None,
        });
        Ok(Effect::McpCatalog {
            context: context.clone(),
            reference: reference.clone(),
            lease,
        })
    }

    fn mcp_catalog_settled(&mut self, lease: u64, result: Result<McpCatalog, ManagementFailure>) {
        let Some(c) = self.mcp_catalog.as_mut().filter(|c| c.lease == lease) else {
            return;
        };
        c.loading = false;
        match result {
            Ok(catalog) if catalog.workspace == c.workspace => {
                c.catalog = Some(catalog);
                c.failure = None;
            }
            Ok(_) => c.failure = Some(ManagementFailure::InvalidResponse),
            Err(f) => c.failure = Some(f),
        }
    }

    /// Turns a catalog server on or off for new conversations. Turning it
    /// off retires a conversation that holds it.
    pub(super) fn enable_mcp_server(
        &mut self,
        context: &ContextRef,
        reference: &WorkspaceRef,
        server: &str,
        enabled: bool,
    ) -> Result<(), Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let workspace = self.inference_workspace(context, reference)?;
        let known = self
            .mcp_catalog_of(&tenant, &workspace.id)
            .and_then(|c| c.servers.iter().find(|s| s.id == server))
            .is_some_and(|s| s.supported);
        if enabled && !known {
            return Err(Refusal::InvalidInput);
        }
        let on = self
            .tenant(&tenant)
            .map(|t| {
                t.harness_mcp
                    .iter()
                    .filter(|m| m.workspace == workspace.id && m.enabled && m.server_id != server)
                    .count()
            })
            .unwrap_or(0);
        if enabled && on >= mcp::MAX_SERVERS {
            return Err(Refusal::Busy);
        }
        let before = self.data.clone();
        let t = self.tenant_mut(&tenant).ok_or(Refusal::StaleContext)?;
        match t
            .harness_mcp
            .iter_mut()
            .find(|m| m.workspace == workspace.id && m.server_id == server)
        {
            Some(m) => m.enabled = enabled,
            None if enabled => t.harness_mcp.push(StoredMcpServer {
                tenant: tenant.clone(),
                workspace: workspace.id.clone(),
                server_id: server.to_owned(),
                enabled,
                grant: None,
            }),
            None => return Ok(()),
        }
        t.harness_mcp.retain(|m| m.enabled || m.grant.is_some());
        self.save_with(&before)
    }

    /// Starts a browser sign-in to a catalog server.
    pub(super) fn begin_mcp_sign_in(
        &mut self,
        context: &ContextRef,
        reference: &WorkspaceRef,
        server: &str,
    ) -> Result<Effect, Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let workspace = self.inference_workspace(context, reference)?;
        let target = self
            .mcp_catalog_of(&tenant, &workspace.id)
            .and_then(|c| c.servers.iter().find(|s| s.id == server))
            .filter(|s| s.supported)
            .cloned()
            .ok_or(Refusal::InvalidInput)?;
        if self.mcp_flow.as_ref().is_some_and(|f| f.step.is_some()) {
            return Err(Refusal::Busy);
        }
        let flow = self.lease();
        let cancel = CancellationToken::new();
        self.mcp_flow = Some(McpFlow {
            flow,
            tenant: tenant.clone(),
            workspace: workspace.id.clone(),
            server_id: target.id.clone(),
            step: Some(SignInStep::Contacting),
            failure: None,
            cancel: cancel.clone(),
        });
        Ok(Effect::McpSignIn(Box::new(McpSignInJob {
            flow,
            tenant,
            workspace: workspace.id,
            server: target,
            cancel,
        })))
    }

    pub(super) fn cancel_mcp_sign_in(&mut self, context: &ContextRef) -> Result<(), Refusal> {
        let tenant = self.check_context(context)?;
        match self.mcp_flow.take() {
            Some(flow) if flow.tenant == tenant => {
                flow.cancel.cancel();
                Ok(())
            }
            other => {
                self.mcp_flow = other;
                Err(Refusal::InvalidInput)
            }
        }
    }

    /// Signs out of a server: its grant ends (a conversation holding it is
    /// retired by the next sync), its token record joins the cleanup
    /// ledger, and the live grant is returned for revocation.
    pub(super) fn sign_out_mcp_server(
        &mut self,
        context: &ContextRef,
        reference: &WorkspaceRef,
        server: &str,
    ) -> Result<Option<Arc<McpGrant>>, Refusal> {
        self.require_storage()?;
        let tenant = self.check_context(context)?;
        let workspace = self.inference_workspace(context, reference)?;
        // A sign-in to this server under way ends too: a late callback or
        // token reply can never sign it back in (P06, C22 review F3).
        if let Some(flow) = self.mcp_flow.take() {
            if flow.tenant == tenant && flow.workspace == workspace.id && flow.server_id == server {
                flow.cancel.cancel();
                self.changed();
            } else {
                self.mcp_flow = Some(flow);
            }
        }
        let live = self.mcp_grant(&tenant, &workspace.id, server);
        let before = self.data.clone();
        let Some(grant) = self
            .stored_mcp_mut(&tenant, &workspace.id, server)
            .and_then(|m| m.grant.take())
        else {
            return Ok(None);
        };
        if let Some(t) = self.tenant_mut(&tenant) {
            t.harness_mcp.retain(|m| m.enabled || m.grant.is_some());
        }
        self.data.pending_cleanup.push(grant.record);
        self.save_with(&before)?;
        self.mcp_live.retain(|g| {
            !(g.tenant == tenant && g.workspace == workspace.id && g.server_id == server)
        });
        Ok(live)
    }

    pub(super) fn harness_mcp_view(&self) -> Option<HarnessMcpView> {
        let tenant = self.data.active.as_ref()?;
        let c = self.mcp_catalog.as_ref().filter(|c| &c.tenant == tenant)?;
        let flow = self
            .mcp_flow
            .as_ref()
            .filter(|f| &f.tenant == tenant && f.workspace == c.workspace);
        let servers = c
            .catalog
            .as_ref()
            .map(|catalog| {
                catalog
                    .servers
                    .iter()
                    .map(|s| {
                        let stored = self.stored_mcp(tenant, &c.workspace, &s.id);
                        let grant = stored.and_then(|m| m.grant.as_ref());
                        let current =
                            grant.is_some_and(|g| g.url == s.url && g.revision == s.revision);
                        let live = grant.and_then(|g| {
                            self.mcp_live.iter().find(|l| {
                                &l.tenant == tenant
                                    && l.workspace == c.workspace
                                    && l.server_id == s.id
                                    && l.id == g.id
                            })
                        });
                        let mine = flow.filter(|f| f.server_id == s.id);
                        McpServerView {
                            id: s.id.clone(),
                            name: s.name.clone(),
                            namespace: s.namespace.clone(),
                            url: s.url.clone(),
                            status: s.status.clone(),
                            supported: s.supported,
                            enabled: stored.is_some_and(|m| m.enabled),
                            signed_in: current,
                            stale: grant.is_some() && !current,
                            expired: live.and_then(|l| l.expired()),
                            step: mine.and_then(|f| f.step),
                            failure: mine.and_then(|f| f.failure),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        Some(HarnessMcpView {
            workspace: c.workspace.clone(),
            state: if c.loading {
                McpListState::Loading
            } else if c.catalog.is_some() {
                McpListState::Ready
            } else {
                McpListState::Failed
            },
            failure: c.failure,
            complete: c.catalog.as_ref().is_some_and(|c| c.complete),
            servers,
        })
    }

    /// A new conversation's MCP scope in `workspace`: the servers on, in
    /// catalog order, each with its grant when it was made for the
    /// server's current URL and revision. With none on, nothing is needed;
    /// otherwise the catalog must be loaded.
    pub(super) fn mcp_scope(
        &mut self,
        tenant: &TenantId,
        workspace: &str,
    ) -> Result<(Vec<ScopeServer>, ScopeGrants), Refusal> {
        let scope = self.mcp_scope_now(tenant, workspace).ok_or(Refusal::Busy)?;
        let grants = scope
            .iter()
            .map(|s| {
                s.grant
                    .as_ref()
                    .and_then(|_| self.mcp_grant(tenant, workspace, &s.server_id))
            })
            .collect();
        Ok((scope, grants))
    }

    /// The scope a new conversation would have now; `None` while servers
    /// are on but the catalog is not loaded.
    pub(super) fn mcp_scope_now(
        &self,
        tenant: &TenantId,
        workspace: &str,
    ) -> Option<Vec<ScopeServer>> {
        let enabled: Vec<StoredMcpServer> = self
            .tenant(tenant)
            .map(|t| {
                t.harness_mcp
                    .iter()
                    .filter(|m| m.workspace == workspace && m.enabled)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if enabled.is_empty() {
            return Some(Vec::new());
        }
        let catalog = self.mcp_catalog_checked(tenant, workspace)?;
        let mut scope = Vec::new();
        for server in catalog.servers.iter().filter(|s| s.supported) {
            let Some(stored) = enabled.iter().find(|m| m.server_id == server.id) else {
                continue;
            };
            let grant = stored
                .grant
                .as_ref()
                .filter(|g| g.url == server.url && g.revision == server.revision);
            scope.push(ScopeServer {
                name: server.namespace.clone(),
                server_id: server.id.clone(),
                url: server.url.clone(),
                revision: server.revision.clone(),
                grant: grant.map(|g| g.id.clone()),
            });
        }
        Some(scope)
    }

    /// How a conversation's scope stands. `Changed`: a server is off, has
    /// another sign-in, or the catalog last read names another revision
    /// or URL (the conversation is retired). `Unchecked`: unchanged, but
    /// the catalog is loading or its last read failed, so nothing remote
    /// may run until it is read again. `Current`: checked against a fresh
    /// read.
    pub(super) fn mcp_scope_check(
        &self,
        tenant: &TenantId,
        workspace: &str,
        scope: &[ScopeServer],
    ) -> ScopeCheck {
        if scope.is_empty() {
            return ScopeCheck::Current;
        }
        let listed = |c: &McpCatalog, s: &ScopeServer| {
            c.servers
                .iter()
                .any(|c| c.id == s.server_id && c.revision == s.revision && c.url == s.url)
        };
        let last = self.mcp_catalog_of(tenant, workspace);
        let changed = scope.iter().any(|s| {
            let stored = self.stored_mcp(tenant, workspace, &s.server_id);
            !stored
                .is_some_and(|m| m.enabled && m.grant.as_ref().map(|g| &g.id) == s.grant.as_ref())
                || last.is_some_and(|c| !listed(c, s))
        });
        if changed {
            return ScopeCheck::Changed;
        }
        match self.mcp_catalog_checked(tenant, workspace) {
            Some(c) if scope.iter().all(|s| listed(c, s)) => ScopeCheck::Current,
            _ => ScopeCheck::Unchecked,
        }
    }

    /// Whether the server `name` of a conversation's scope may run a tool
    /// now: checked against a fresh catalog, with a current sign-in whose
    /// token has not expired.
    pub(super) fn mcp_server_usable(
        &self,
        tenant: &TenantId,
        workspace: &str,
        scope: &[ScopeServer],
        name: &str,
    ) -> bool {
        let Some(s) = scope.iter().find(|s| s.name == name) else {
            return false;
        };
        let Some(grant) = s.grant.as_ref() else {
            return false;
        };
        self.mcp_scope_check(tenant, workspace, std::slice::from_ref(s)) == ScopeCheck::Current
            && self
                .mcp_live
                .iter()
                .find(|l| {
                    &l.tenant == tenant
                        && l.workspace == workspace
                        && l.server_id == s.server_id
                        && &l.id == grant
                })
                .is_some_and(|l| l.usable_now())
    }

    fn mcp_failed(&mut self, flow: u64, failure: McpSignInFailure) {
        if let Some(f) = self.mcp_flow.as_mut().filter(|f| f.flow == flow) {
            f.step = None;
            f.failure = Some(failure);
            self.changed();
        }
    }

    fn mcp_step(&mut self, flow: u64, step: SignInStep) -> bool {
        match self.mcp_flow.as_mut().filter(|f| f.flow == flow) {
            Some(f) => {
                f.step = Some(step);
                self.changed();
                true
            }
            None => false,
        }
    }
}

/// Reads the catalog through the tenant's management client.
pub(super) async fn read_catalog(
    inner: Arc<Inner>,
    context: ContextRef,
    reference: WorkspaceRef,
    lease: u64,
) {
    let admitted = (|| {
        let mut state = lock(&inner).ok()?;
        let request = ManagementRequest::Gateway {
            workspace: Some(reference.clone()),
            read: GatewayRead::McpServers,
        };
        let (operation, mut starts) = state
            .admit_management(
                context.clone(),
                request,
                Arc::clone(&inner.adapters.keychain),
                Arc::clone(&inner.session),
            )
            .ok()?;
        starts.extend(state.take_gateway_starts());
        let handle = state.observe_management(&operation).ok()?;
        state.changed();
        Some((handle, starts))
    })();
    let Some((handle, starts)) = admitted else {
        if let Ok(mut state) = lock(&inner) {
            state.mcp_catalog_settled(lease, Err(ManagementFailure::CredentialMissing));
            state.changed();
        }
        return;
    };
    super::run_effects(&inner, vec![Effect::Start(starts)]);
    let result = match handle.result().await {
        ManagementResult::Gateway {
            gateway: GatewayResult::McpServers { catalog },
        } => Ok(catalog),
        ManagementResult::Failed { failure, .. } => Err(failure),
        _ => Err(ManagementFailure::InvalidResponse),
    };
    if let Ok(mut state) = lock(&inner) {
        state.mcp_catalog_settled(lease, result);
        state.changed();
    }
}

/// An interactive sign-in to one server, run off the lock.
pub(super) struct McpSignInJob {
    flow: u64,
    tenant: TenantId,
    workspace: String,
    server: CatalogServer,
    cancel: CancellationToken,
}

pub(super) async fn sign_in(inner: Arc<Inner>, job: McpSignInJob) {
    let McpSignInJob {
        flow,
        tenant,
        workspace,
        server,
        cancel,
    } = job;
    let fail = |failure: McpSignInFailure| {
        if let Ok(mut state) = lock(&inner) {
            state.mcp_failed(flow, failure);
        }
    };
    let Some(client) = auth_client(&inner) else {
        return fail(McpSignInFailure::Network);
    };
    let authorized = tokio::select! {
        _ = cancel.cancelled() => return,
        result = authorize(&inner, flow, &client, &server.url) => result,
    };
    let (provider, attempt, code) = match authorized {
        Ok(authorized) => authorized,
        Err(failure) => return fail(failure),
    };
    if !lock(&inner).is_ok_and(|mut s| s.mcp_step(flow, SignInStep::Completing)) {
        return;
    }
    let tokens: McpTokens = match client.exchange(&provider, &attempt, &code.0).await {
        Ok(tokens) => tokens,
        Err(failure) => return fail(failure),
    };
    let revoke = |tokens: &McpTokens| {
        let client = client.clone();
        let url = provider.revocation.clone();
        let client_id = attempt.client_id.clone();
        let refresh = tokens.refresh.clone();
        let access = tokens.access.clone();
        async move {
            if let Some(url) = url {
                if let Some(r) = refresh {
                    let _ = client.revoke(&url, &client_id, &r, "refresh_token").await;
                }
                let _ = client
                    .revoke(&url, &client_id, &access, "access_token")
                    .await;
            }
        }
    };
    let (Some(record), Some(grant_id)) = (RecordId::issue(), new_grant_id()) else {
        revoke(&tokens).await;
        return fail(McpSignInFailure::StoreFailed);
    };
    // Write-ahead, while this flow is current.
    let begun = lock(&inner).map(|mut state| {
        state.mcp_flow.as_ref().is_some_and(|f| f.flow == flow) && begin_write(&mut state, &record)
    });
    if !matches!(begun, Ok(true)) {
        revoke(&tokens).await;
        return fail(McpSignInFailure::StoreFailed);
    }
    let issued = now();
    let saved = SavedTokens::bytes(
        &tokens.access,
        tokens.refresh.as_deref().map(|r| r.as_str()),
        issued,
        tokens.expires_in,
    );
    let written = write(&inner, &record, saved).await;
    let committed = (|| {
        let mut state = lock(&inner).ok()?;
        settle_write(&mut state, &record);
        if !written || !state.mcp_flow.as_ref().is_some_and(|f| f.flow == flow) {
            return None;
        }
        // The server must still be in the catalog as signed in to.
        let current = state
            .mcp_catalog_of(&tenant, &workspace)
            .and_then(|c| c.servers.iter().find(|s| s.id == server.id))
            .is_some_and(|s| s.url == server.url && s.revision == server.revision);
        if !current {
            return None;
        }
        let before = state.data.clone();
        let grant = StoredMcpGrant {
            id: grant_id.clone(),
            url: server.url.clone(),
            revision: server.revision.clone(),
            client_id: attempt.client_id.clone(),
            token_endpoint: provider.token.to_string(),
            revocation_endpoint: provider.revocation.as_ref().map(Url::to_string),
            record: record.clone(),
        };
        let t = state.tenant_mut(&tenant)?;
        let old = match t
            .harness_mcp
            .iter_mut()
            .find(|m| m.workspace == workspace && m.server_id == server.id)
        {
            Some(m) => m.grant.replace(grant).map(|g| g.record),
            None => {
                t.harness_mcp.push(StoredMcpServer {
                    tenant: tenant.clone(),
                    workspace: workspace.clone(),
                    server_id: server.id.clone(),
                    enabled: false,
                    grant: Some(grant),
                });
                None
            }
        };
        state.data.pending_cleanup.retain(|r| r != &record);
        state.data.pending_cleanup.extend(old);
        state.save_with(&before).ok()?;
        state.mcp_flow = None;
        state.mcp_live.retain(|g| {
            !(g.tenant == tenant && g.workspace == workspace && g.server_id == server.id)
        });
        let live = state.mcp_grant(&tenant, &workspace, &server.id)?;
        state.changed();
        Some(live)
    })();
    match committed {
        Some(live) => {
            let mut auth = live.auth.lock().await;
            auth.loaded = true;
            auth.access = Some(tokens.access.clone());
            auth.refresh = tokens.refresh.clone();
            auth.issued = issued;
            auth.expires = tokens.expires_in.map(|s| issued + s);
            live.set_expiry(auth.expires);
            live.renew_at
                .store(renew_at(issued, auth.expires), Ordering::SeqCst);
        }
        None => {
            revoke(&tokens).await;
            fail(McpSignInFailure::StoreFailed);
        }
    }
    super::run_effects(&inner, vec![Effect::Cleanup]);
}

fn new_grant_id() -> Option<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(crate::harness::protocol::derived_uuid(
        "mcp-grant",
        &hex::encode(bytes),
    ))
}

/// Discovery, registration, the loopback listener, the browser and its
/// callback.
async fn authorize(
    inner: &Arc<Inner>,
    flow: u64,
    client: &McpAuthClient,
    url: &str,
) -> Result<
    (
        McpProvider,
        crate::harness::mcp_auth::McpAttempt,
        callback::Code,
    ),
    McpSignInFailure,
> {
    let provider = client.discover(url).await?;
    let listener = Callback::bind()
        .await
        .map_err(|_| McpSignInFailure::CallbackUnavailable)?;
    let redirect = format!("http://127.0.0.1:{}/callback", listener.port());
    let client_id = client.register(&provider, &redirect).await?;
    let attempt = crate::harness::mcp_auth::attempt(&provider, &client_id, listener.port())
        .ok_or(McpSignInFailure::CallbackUnavailable)?;
    let opener = {
        let mut state = lock(inner).map_err(|_| McpSignInFailure::Cancelled)?;
        if !state.mcp_step(flow, SignInStep::Browser) {
            return Err(McpSignInFailure::Cancelled);
        }
        Arc::clone(&inner.adapters.opener)
    };
    let browse = attempt.url.clone();
    let opened = tokio::task::spawn_blocking(move || opener.open(&browse))
        .await
        .unwrap_or(false);
    if !opened {
        return Err(McpSignInFailure::BrowserUnavailable);
    }
    let code = listener
        .wait(
            &attempt.state,
            &provider.issuer,
            provider.iss_in_callback,
            Instant::now() + callback::ATTEMPT,
        )
        .await
        .map_err(|f| match f {
            crate::harness::identity::SignInFailure::TimedOut => McpSignInFailure::TimedOut,
            crate::harness::identity::SignInFailure::Denied => McpSignInFailure::Denied,
            _ => McpSignInFailure::CallbackUnavailable,
        })?;
    Ok((provider, attempt, code))
}

/// Ends a grant at its server (best effort) after signing out.
pub(super) async fn signed_out(inner: Arc<Inner>, grant: Arc<McpGrant>) {
    grant.revoke(&inner).await;
    super::run_effects(&inner, vec![Effect::Cleanup]);
}

/// Each granted server's token for a conversation's scope (`None`: no
/// current sign-in, so the signed-out placeholder), and the earliest time
/// one must be renewed.
pub(super) async fn scope_tokens(
    inner: &Arc<Inner>,
    grants: &[Option<Arc<McpGrant>>],
) -> ScopeTokens {
    let mut tokens = Vec::with_capacity(grants.len());
    let mut renew: Option<u64> = None;
    for grant in grants {
        match grant {
            Some(g) => match g.token(inner).await {
                Ok(token) => {
                    let at = g.renew_at();
                    if at != u64::MAX && at != 0 {
                        renew = Some(renew.map_or(at, |r| r.min(at)));
                    }
                    tokens.push(Some(token));
                }
                // A server that could not be reached is tried again before
                // the next message (the engine is restarted if it then has
                // a token); an expired or refused sign-in is not.
                Err(McpSignInFailure::Network) => {
                    renew = Some(0);
                    tokens.push(None);
                }
                Err(_) => tokens.push(None),
            },
            None => tokens.push(None),
        }
    }
    (tokens, renew)
}
