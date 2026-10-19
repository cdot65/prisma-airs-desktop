//! Management OAuth binding and read operations (C08).
//!
//! A tenant's management capability is its own confidential OAuth client
//! (client ID, client secret, the tenant's TSG), separate from the scan key
//! (D013): it is bound to the management token, API and DLP origins, the
//! grant goes only to the admitted token origin, and nothing here ever uses
//! or reveals the scan key. Reads go through the pinned SDK's management and
//! DLP clients (SDK Phase 4) with the app-owned transport, an exact-origin
//! destination policy, explicit limits and no retries. Each page is one
//! bounded request; completeness is claimed only when the service says no
//! further page exists. Items travel as the SDK serialized them (unknown
//! enum values and fields preserved) with secret-bearing fields removed.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use scm_sdk::airs::agentguard::AgentGuardClient;
use scm_sdk::airs::management::{
    DeploymentProfileListOptions, ManagementClient, PaginationOptions, TopicListOptions,
};
use scm_sdk::airs::model_security::ModelSecurityClient;
use scm_sdk::airs::red_team::RedTeamClient;
use scm_sdk::auth::AuthContext;
use scm_sdk::dlp::{
    DataFilteringProfileListParams, DictionaryListParams, DlpClient, SpringListParams,
};
use scm_sdk::transport::Transport;
use scm_sdk::{
    Client, Endpoint, ErrorKind, Limits, OriginPolicy, RemoteOutcome, RequestOptions, Secret,
    Session,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::OnceCell;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::authority::CredentialStore;
use crate::runtime::{Claim, Outcome, Work};
use crate::vault::{RecordId, SessionVault, StorageMode, Vault, VaultError};

/// The production management OAuth token endpoint.
pub const TOKEN_URL: &str = "https://auth.apps.paloaltonetworks.com/oauth2/access_token";
/// The production AIRS management API base.
pub const API_URL: &str = "https://api.apps.paloaltonetworks.com/aisec";
/// The production DLP API base.
pub const DLP_URL: &str = "https://api.dlp.paloaltonetworks.com";
/// The production AI Red Teaming planes (the SDK's defaults).
pub const RED_TEAM_DATA_URL: &str =
    "https://api.apps.paloaltonetworks.com/ai-red-teaming/data-plane";
pub const RED_TEAM_MGMT_URL: &str =
    "https://api.apps.paloaltonetworks.com/ai-red-teaming/mgmt-plane";
pub const RED_TEAM_BROKER_URL: &str =
    "https://api.apps.paloaltonetworks.com/ai-red-teaming/data-plane/network-broker";
/// The production Model Security planes (the SDK's defaults).
pub const MODEL_DATA_URL: &str = "https://api.apps.paloaltonetworks.com/aims/data";
pub const MODEL_MGMT_URL: &str = "https://api.apps.paloaltonetworks.com/aims/mgmt";
/// The production AgentGuard (skill scanning) planes (the SDK's defaults).
pub const AGENTGUARD_DATA_URL: &str = "https://api.apps.paloaltonetworks.com/aiag/data";
pub const AGENTGUARD_MGMT_URL: &str = "https://api.apps.paloaltonetworks.com/aiag/mgmt";
/// Largest page a read may ask for.
pub const MAX_PAGE: u32 = 100;
/// The bounded walk a detail read may make (v1: 40 pages of 25).
pub const DETAIL_PAGES: u32 = 40;
pub const DETAIL_PAGE_SIZE: u32 = 25;
/// Interactive deadline for one management read, from submission.
pub const READ_DEADLINE: Duration = Duration::from_secs(60);
/// Largest serialized page handed to the presentation.
pub const MAX_PAGE_BYTES: usize = 8 * 1024 * 1024;

/// The management origins a tenant's management capability is bound to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagementEndpoints {
    /// The full OAuth token URL (the grant goes only to its origin).
    pub token: String,
    /// The AIRS management API base URL.
    pub api: String,
    /// The DLP API base URL.
    pub dlp: String,
    /// AI Red Teaming data, management and network-broker planes (C13).
    pub red_team_data: String,
    pub red_team_mgmt: String,
    pub red_team_broker: String,
    /// Model Security data and management planes (C13).
    pub model_data: String,
    pub model_mgmt: String,
    /// AgentGuard skill-scanning data and management planes (C13).
    pub agentguard_data: String,
    pub agentguard_mgmt: String,
}

impl Default for ManagementEndpoints {
    fn default() -> Self {
        Self {
            token: TOKEN_URL.to_owned(),
            api: API_URL.to_owned(),
            dlp: DLP_URL.to_owned(),
            red_team_data: RED_TEAM_DATA_URL.to_owned(),
            red_team_mgmt: RED_TEAM_MGMT_URL.to_owned(),
            red_team_broker: RED_TEAM_BROKER_URL.to_owned(),
            model_data: MODEL_DATA_URL.to_owned(),
            model_mgmt: MODEL_MGMT_URL.to_owned(),
            agentguard_data: AGENTGUARD_DATA_URL.to_owned(),
            agentguard_mgmt: AGENTGUARD_MGMT_URL.to_owned(),
        }
    }
}

impl ManagementEndpoints {
    /// Every endpoint at one origin, under the production paths (fixture
    /// builds).
    pub fn at_origin(origin: &str) -> Self {
        Self {
            token: format!("{origin}/oauth2/access_token"),
            api: format!("{origin}/aisec"),
            dlp: format!("{origin}/dlp"),
            red_team_data: format!("{origin}/ai-red-teaming/data-plane"),
            red_team_mgmt: format!("{origin}/ai-red-teaming/mgmt-plane"),
            red_team_broker: format!("{origin}/ai-red-teaming/data-plane/network-broker"),
            model_data: format!("{origin}/aims/data"),
            model_mgmt: format!("{origin}/aims/mgmt"),
            agentguard_data: format!("{origin}/aiag/data"),
            agentguard_mgmt: format!("{origin}/aiag/mgmt"),
        }
    }

    /// The origin the client secret may be sent to.
    pub fn token_origin(&self) -> Option<String> {
        Endpoint::parse(&self.token)
            .ok()
            .map(|e| e.origin().to_owned())
    }
}

/// Whether `value` is exactly an HTTPS origin.
pub fn is_origin(value: &str) -> bool {
    Endpoint::parse(value).is_ok_and(|e| e.origin() == value && value.starts_with("https://"))
}

/// The SDK's client ID rules (non-empty, bounded, no control characters).
pub fn client_id_is_valid(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 1024
        && value == value.trim()
        && !value.chars().any(char::is_control)
}

/// The SDK's TSG rules (1-32 digits).
pub fn tsg_is_valid(value: &str) -> bool {
    !value.is_empty() && value.len() <= 32 && value.bytes().all(|b| b.is_ascii_digit())
}

/// A management inventory or catalog the desktop reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ManagementResource {
    Profiles,
    Topics,
    Applications,
    ApiKeys,
    DeploymentProfiles,
    DlpCatalog,
    DataProfiles,
    DataPatterns,
    FilteringProfiles,
    Dictionaries,
    /// The 24-hour Runtime sessions inventory (C11).
    Sessions,
}

impl ManagementResource {
    /// The item field that identifies an item for a detail read.
    pub fn id_field(self) -> &'static str {
        match self {
            Self::Profiles => "profile_id",
            Self::Topics => "topic_id",
            Self::Applications => "customer_appId",
            Self::ApiKeys => "api_key_id",
            Self::DeploymentProfiles => "dp_name",
            Self::DlpCatalog | Self::DataProfiles | Self::DataPatterns => "id",
            Self::FilteringProfiles | Self::Dictionaries => "id",
            Self::Sessions => "session_id",
        }
    }

    fn is_dlp(self) -> bool {
        matches!(
            self,
            Self::DataProfiles | Self::DataPatterns | Self::FilteringProfiles | Self::Dictionaries
        )
    }
}

/// One management read the presentation asks for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ManagementRequest {
    /// The credential check: a token and a one-item profile read.
    Verify,
    /// One page of an inventory.
    Page {
        resource: ManagementResource,
        offset: u32,
        limit: u32,
    },
    /// One item, found by a bounded walk of its inventory.
    Detail {
        resource: ManagementResource,
        id: String,
    },
    /// One session's actions (one page of 25, over 30 days; v1).
    #[serde(rename_all = "camelCase")]
    Session {
        session_id: String,
        app_id: String,
        app_name: String,
    },
    /// One transaction's metadata (no content).
    #[serde(rename_all = "camelCase")]
    Transaction {
        session_id: String,
        app_id: String,
        app_name: String,
        scan_id: String,
        sub_request: u32,
    },
    /// A scan's prompt and response: only when the operator reveals it.
    #[serde(rename_all = "camelCase")]
    Content { scan_id: String, sub_request: u32 },
    /// A bounded search by session ID, scan ID or detector (v1).
    Search { query: String },
    /// The Runtime Home/Inbox sources: the last day's sessions chart and
    /// the API key inventory's first page (C12).
    Overview,
    /// AI Red Teaming scan statistics over the last `window` days (7, 15 or
    /// 30) and the first 100 scans (C13).
    RedTeam { window: u8 },
    /// Model Security scans, AgentGuard 30-day skill statistics and skill
    /// scans (C13).
    SupplyChain,
    /// One page (0-9, 100 each) of a Red Teaming scan's attack results.
    #[serde(rename_all = "camelCase")]
    Attacks { scan_id: String, page: u8 },
}

impl ManagementRequest {
    /// Admission-time validation (bounded pages, plain IDs).
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Verify => true,
            Self::Page {
                resource,
                offset,
                limit,
            } => {
                (1..=MAX_PAGE).contains(limit)
                    && *offset <= 1_000_000
                    && (!resource.is_dlp() || offset % limit == 0)
            }
            Self::Detail { id, .. } => plain(id),
            Self::Session {
                session_id,
                app_id,
                app_name,
            } => plain(session_id) && plain(app_id) && plain(app_name),
            Self::Transaction {
                session_id,
                app_id,
                app_name,
                scan_id,
                ..
            } => plain(session_id) && plain(app_id) && plain(app_name) && plain(scan_id),
            Self::Content { scan_id, .. } => plain(scan_id),
            Self::Search { query } => crate::sessions::classify(query).is_some(),
            Self::Overview | Self::SupplyChain => true,
            Self::RedTeam { window } => crate::red_team::WINDOWS.contains(window),
            Self::Attacks { scan_id, page } => {
                crate::red_team::is_uuid(scan_id) && *page < crate::red_team::ATTACK_PAGES
            }
        }
    }
}

/// A plain identifier: 1-256 bytes, no control characters.
fn plain(id: &str) -> bool {
    (1..=256).contains(&id.len()) && !id.chars().any(char::is_control)
}

/// One session's actions as the service returned them (secret fields
/// removed). The service reports no total, so `more` says only that a full
/// page came back ("at least 25").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionRead {
    /// The session as JSON (its `session_actions` included).
    pub session: String,
    pub actions: u32,
    pub more: bool,
}

/// A scan's content, only ever in the single result of a Content read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScanContent {
    pub prompt: Option<String>,
    pub response: Option<String>,
}

/// What a bounded search found and how far it looked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearch {
    pub kind: crate::sessions::SearchKind,
    pub query: String,
    /// Matching sessions as a JSON array.
    pub sessions: String,
    /// Sessions read by the walk (0 when answered from the cache or a scan
    /// lookup).
    pub scanned: u32,
    /// The walk reached the end of its window (absence is proven).
    pub complete: bool,
    /// The detector search stopped at its match bound.
    pub match_limit: bool,
    /// A scan found by ID: its metadata (never content), as JSON.
    pub scan: Option<String>,
    /// Answered from this credential's investigation cache.
    pub cached: bool,
}

/// One page as the service returned it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementPage {
    pub resource: ManagementResource,
    pub offset: u32,
    pub limit: u32,
    /// The page's items as a JSON array (SDK-serialized, secret fields
    /// removed, unknown values preserved).
    pub items: String,
    pub count: u32,
    /// Where the next page starts; `None` with `complete` when the service
    /// said nothing follows.
    pub next_offset: Option<u32>,
    /// True only when the service said this is the last page.
    pub complete: bool,
    /// The service's total, when it reports one.
    pub total: Option<u32>,
}

/// What a detail read found.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DetailFound {
    Found,
    /// The whole inventory was read and the item is not in it.
    Absent,
    /// The bounded walk ended before the inventory did: nothing is claimed.
    Incomplete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementDetail {
    pub resource: ManagementResource,
    pub id: String,
    pub found: DetailFound,
    /// The item as JSON when found.
    pub item: Option<String>,
}

/// Why a management read did not produce data (sanitized; no server text).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ManagementFailure {
    /// The token grant was refused (bad client ID or secret, wrong TSG).
    Unauthorized,
    /// The tenant's licensing or permissions do not allow this read.
    Denied,
    NotFound,
    RateLimited,
    /// The service failed (5xx).
    Unavailable,
    /// Another 4xx.
    Rejected,
    Timeout,
    Cancelled,
    Network,
    /// A redirect or an origin outside the management destinations.
    Destination,
    /// The reply did not match the contract (or a page did not advance).
    InvalidResponse,
    CredentialLocked,
    CredentialDenied,
    CredentialMissing,
    /// The client could not be configured for this tenant.
    Configuration,
}

impl ManagementFailure {
    pub fn from_sdk(error: &scm_sdk::Error) -> Self {
        match (error.kind(), error.status()) {
            (ErrorKind::Authentication, _) => Self::Unauthorized,
            (ErrorKind::Http, Some(401)) => Self::Unauthorized,
            (ErrorKind::Http, Some(403)) => Self::Denied,
            (ErrorKind::Http, Some(404)) => Self::NotFound,
            (ErrorKind::Http, Some(429)) => Self::RateLimited,
            (ErrorKind::Http, Some(s)) if s >= 500 => Self::Unavailable,
            (ErrorKind::Http, _) => Self::Rejected,
            (ErrorKind::Deadline, _) => Self::Timeout,
            (ErrorKind::Cancelled, _) => Self::Cancelled,
            (ErrorKind::Transport | ErrorKind::Io, _) => Self::Network,
            (ErrorKind::Destination, _) => Self::Destination,
            (ErrorKind::Configuration, _) => Self::Configuration,
            _ => Self::InvalidResponse,
        }
    }
}

pub fn outcome_of(error: &scm_sdk::Error) -> Outcome {
    match error.remote_outcome() {
        RemoteOutcome::NotSent => Outcome::NotSent,
        RemoteOutcome::Rejected => Outcome::Rejected,
        _ => Outcome::Unknown,
    }
}

/// What a management operation produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ManagementResult {
    /// The credential check passed: a token was granted and one profile
    /// read succeeded. It proves neither the scan key nor write access.
    Verified,
    Page {
        page: ManagementPage,
    },
    Detail {
        detail: ManagementDetail,
    },
    Session {
        session: SessionRead,
    },
    Transaction {
        /// The transaction's metadata as JSON (no prompt or response).
        transaction: String,
    },
    Content {
        content: ScanContent,
    },
    Search {
        search: SessionSearch,
    },
    Overview {
        overview: crate::observation::RuntimeOverview,
    },
    RedTeam {
        observation: crate::red_team::RedTeamObservation,
    },
    SupplyChain {
        observation: crate::red_team::SupplyChainObservation,
    },
    Attacks {
        attacks: crate::red_team::AttackPage,
    },
    Failed {
        failure: ManagementFailure,
        outcome: Outcome,
    },
}

/// Field names whose values are key material or licensing secrets. They are
/// removed at any depth before an item leaves the core (an API key's value
/// is a C09 one-time reveal, never inventory).
const SECRET_FIELDS: [&str; 10] = [
    "api_key",
    "auth_code",
    "client_secret",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "password",
    "private_key",
    "api_key_value",
];

/// Removes secret-bearing fields at any depth.
pub fn redact(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.retain(|key, _| !SECRET_FIELDS.contains(&key.as_str()));
            for item in map.values_mut() {
                redact(item);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact(item);
            }
        }
        _ => {}
    }
}

/// The explicit management client limits.
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_response_bytes = 16 * 1024 * 1024;
    limits.operation_timeout = READ_DEADLINE;
    limits.max_retries = 0;
    limits
}

/// Management, DLP, Red Team, Model Security and AgentGuard clients bound
/// to one credential generation.
pub struct ManagementBinding {
    management: ManagementClient,
    dlp: DlpClient,
    red_team: RedTeamClient,
    model_security: ModelSecurityClient,
    agentguard: AgentGuardClient,
}

impl ManagementBinding {
    /// Builds the binding: the client secret goes only to the token origin,
    /// reads only to the API, DLP and product (Red Team, Model Security,
    /// AgentGuard) origins.
    pub fn new(
        endpoints: &ManagementEndpoints,
        client_id: &str,
        tsg: &str,
        secret: Zeroizing<String>,
        transport: Arc<dyn Transport>,
        session: Session,
    ) -> Result<Self, ManagementFailure> {
        let parse = |url: &str| Endpoint::parse(url).map_err(|_| ManagementFailure::Configuration);
        let token = parse(&endpoints.token)?;
        let api = parse(&endpoints.api)?;
        let dlp = parse(&endpoints.dlp)?;
        let products = [
            &endpoints.red_team_data,
            &endpoints.red_team_mgmt,
            &endpoints.red_team_broker,
            &endpoints.model_data,
            &endpoints.model_mgmt,
            &endpoints.agentguard_data,
            &endpoints.agentguard_mgmt,
        ]
        .map(|url| parse(url));
        let [
            rt_data,
            rt_mgmt,
            rt_broker,
            ms_data,
            ms_mgmt,
            ag_data,
            ag_mgmt,
        ] = products;
        let (rt_data, rt_mgmt, rt_broker) = (rt_data?, rt_mgmt?, rt_broker?);
        let (ms_data, ms_mgmt, ag_data, ag_mgmt) = (ms_data?, ms_mgmt?, ag_data?, ag_mgmt?);
        let all = [
            &token, &api, &dlp, &rt_data, &rt_mgmt, &rt_broker, &ms_data, &ms_mgmt, &ag_data,
            &ag_mgmt,
        ];
        if !all.iter().all(|e| e.origin().starts_with("https://")) {
            return Err(ManagementFailure::Destination);
        }
        let context = AuthContext::new(token.clone(), client_id, 1, tsg)
            .map_err(|_| ManagementFailure::Configuration)?;
        let secret =
            Secret::new(secret.as_str()).map_err(|_| ManagementFailure::CredentialMissing)?;
        let client = Client::builder(api.clone())
            .oauth_credentials(context, secret, false)
            .transport(transport)
            .destination_policy(Arc::new(OriginPolicy::new(all.map(Endpoint::clone))))
            .limits(limits())
            .session(session)
            .build()
            .map_err(|_| ManagementFailure::Configuration)?;
        let management =
            ManagementClient::new(client).map_err(|_| ManagementFailure::Configuration)?;
        let configured = |_| ManagementFailure::Configuration;
        let red_team = RedTeamClient::new(
            management
                .client()
                .with_endpoint(rt_data)
                .map_err(configured)?,
            rt_mgmt,
            rt_broker,
        )
        .map_err(configured)?;
        let model_security =
            ModelSecurityClient::new(management.client().clone(), ms_data, ms_mgmt)
                .map_err(configured)?;
        let agentguard = AgentGuardClient::new(management.client().clone(), ag_data, ag_mgmt)
            .map_err(configured)?;
        let dlp = management.dlp(dlp).map_err(configured)?;
        Ok(Self {
            management,
            dlp,
            red_team,
            model_security,
            agentguard,
        })
    }

    fn options(deadline: Instant) -> Option<RequestOptions> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        RequestOptions::with_timeout(remaining.max(Duration::from_millis(1)))
            .and_then(|o| o.with_retries(0))
            .ok()
    }

    /// The credential check (v1 `checktenant`): one profile, latest only.
    pub async fn verify(&self, deadline: Instant) -> Result<(), scm_sdk::Error> {
        let mut page = PaginationOptions::default();
        page.limit = 1;
        page.latest = Some(true);
        self.management
            .profiles()
            .list(page, Self::options(deadline).as_ref())
            .await
            .map(|_| ())
    }

    /// One page of `resource`, normalized.
    pub async fn page(
        &self,
        resource: ManagementResource,
        offset: u32,
        limit: u32,
        deadline: Instant,
    ) -> Result<ManagementPage, PageError> {
        let options = Self::options(deadline);
        let options = options.as_ref();
        let offset_page = || {
            let mut page = PaginationOptions::default();
            page.offset = u64::from(offset);
            page.limit = limit;
            page
        };
        let json = |value: &dyn erased::Json| value.to_json().map_err(|_| PageError::Invalid);
        let (value, paging) = match resource {
            ManagementResource::Profiles => {
                let mut page = offset_page();
                page.latest = Some(true);
                let reply = self.management.profiles().list(page, options).await?;
                (
                    json(&reply)?,
                    Paging::Offset {
                        key: "ai_profiles",
                        required: true,
                    },
                )
            }
            ManagementResource::Topics => {
                let mut page = TopicListOptions::default();
                page.offset = u64::from(offset);
                page.limit = limit;
                let reply = self.management.topics().list(page, options).await?;
                (
                    json(&reply)?,
                    Paging::Offset {
                        key: "custom_topics",
                        required: true,
                    },
                )
            }
            ManagementResource::Applications => {
                let reply = self
                    .management
                    .customer_apps()
                    .list(offset_page(), options)
                    .await?;
                (
                    json(&reply)?,
                    Paging::Offset {
                        key: "customer_apps",
                        required: false,
                    },
                )
            }
            ManagementResource::ApiKeys => {
                let reply = self
                    .management
                    .api_keys()
                    .list(offset_page(), options)
                    .await?;
                (
                    json(&reply)?,
                    Paging::Offset {
                        key: "api_keys",
                        required: false,
                    },
                )
            }
            ManagementResource::DeploymentProfiles => {
                let reply = self
                    .management
                    .deployment_profiles()
                    .list(DeploymentProfileListOptions::default(), options)
                    .await?;
                (
                    json(&reply)?,
                    Paging::Local {
                        key: "deployment_profiles",
                        required: true,
                    },
                )
            }
            ManagementResource::DlpCatalog => {
                let reply = self.management.dlp_profiles().list(options).await?;
                (
                    json(&reply)?,
                    Paging::Local {
                        key: "dlp_profiles",
                        required: false,
                    },
                )
            }
            ManagementResource::DataProfiles => {
                let params = SpringListParams::new()
                    .page(u64::from(offset / limit))
                    .size(limit);
                let reply = self.dlp.data_profiles().list(&params, options).await?;
                (json(&reply)?, Paging::Spring)
            }
            ManagementResource::DataPatterns => {
                let params = SpringListParams::new()
                    .page(u64::from(offset / limit))
                    .size(limit);
                let reply = self.dlp.data_patterns().list(&params, options).await?;
                (json(&reply)?, Paging::Spring)
            }
            ManagementResource::FilteringProfiles => {
                let mut params = DataFilteringProfileListParams::default();
                params.page = Some(u64::from(offset / limit));
                params.size = Some(limit);
                let reply = self
                    .dlp
                    .data_filtering_profiles()
                    .list(&params, options)
                    .await?;
                (json(&reply)?, Paging::Spring)
            }
            ManagementResource::Sessions => {
                let value = self.overview(1, "day", offset, limit, options).await?;
                (
                    value,
                    Paging::Offset {
                        key: "items",
                        required: true,
                    },
                )
            }
            ManagementResource::Dictionaries => {
                let mut params = DictionaryListParams::default();
                params.page = Some(u64::from(offset / limit));
                params.size = Some(limit);
                // v1 lists dictionaries without their keyword lists.
                params.keywords = Some(false);
                let reply = self.dlp.dictionaries().list(&params, options).await?;
                (json(&reply)?, Paging::Spring)
            }
        };
        normalize(resource, offset, limit, value, paging)
    }

    /// One item: a DLP resource by its own read (a 404 means absent), any
    /// other by a bounded walk of its inventory (40 pages of 25), as v1.
    pub async fn detail(
        &self,
        resource: ManagementResource,
        id: &str,
        deadline: Instant,
    ) -> Result<ManagementDetail, PageError> {
        if let Some(found) = self.dlp_get(resource, id, deadline).await {
            let found = match found {
                Ok(mut item) => {
                    redact(&mut item);
                    Some(item.to_string())
                }
                Err(PageError::Sdk(error))
                    if ManagementFailure::from_sdk(&error) == ManagementFailure::NotFound =>
                {
                    None
                }
                Err(error) => return Err(error),
            };
            return Ok(ManagementDetail {
                resource,
                id: id.to_owned(),
                found: if found.is_some() {
                    DetailFound::Found
                } else {
                    DetailFound::Absent
                },
                item: found,
            });
        }
        let field = resource.id_field();
        let walked = self
            .walk(resource, deadline, |item| {
                identity(item.get(field)).as_deref() == Some(id)
            })
            .await?;
        let (found, item) = match walked {
            Walk::Found(item) => (DetailFound::Found, Some(item.to_string())),
            Walk::Ended => (DetailFound::Absent, None),
            Walk::Incomplete => (DetailFound::Incomplete, None),
        };
        Ok(ManagementDetail {
            resource,
            id: id.to_owned(),
            found,
            item,
        })
    }

    /// A bounded walk (40 pages of 25, v1) of `resource` for the first item
    /// `wanted` accepts. Items are redacted. `Ended` only when the service
    /// said the inventory ended.
    pub async fn walk(
        &self,
        resource: ManagementResource,
        deadline: Instant,
        mut wanted: impl FnMut(&Value) -> bool,
    ) -> Result<Walk, PageError> {
        let mut offset = 0u32;
        for _ in 0..DETAIL_PAGES {
            let page = self
                .page(resource, offset, DETAIL_PAGE_SIZE, deadline)
                .await?;
            let items: Vec<Value> =
                serde_json::from_str(&page.items).map_err(|_| PageError::Invalid)?;
            if let Some(item) = items.into_iter().find(|item| wanted(item)) {
                return Ok(Walk::Found(item));
            }
            match (page.complete, page.next_offset) {
                (true, _) => return Ok(Walk::Ended),
                (false, Some(next)) => offset = next,
                (false, None) => break,
            }
        }
        Ok(Walk::Incomplete)
    }

    /// The deployment profiles as the service returned them, auth codes
    /// included (they never leave Rust; v1 injects them into key creates).
    pub async fn deployment_profiles(&self, deadline: Instant) -> Result<Vec<Value>, PageError> {
        let options = Self::options(deadline);
        let reply = self
            .management
            .deployment_profiles()
            .list(DeploymentProfileListOptions::default(), options.as_ref())
            .await?;
        let mut value = erased::Json::to_json(&reply).map_err(|_| PageError::Invalid)?;
        array_at(&mut value, "deployment_profiles", true)
    }

    /// Performs one reviewed change with the SDK call v1 makes for it
    /// (retries off). The reply may hold a one-time secret: callers pass it
    /// to [`crate::changes::take_secret`] before anything else.
    pub async fn write(
        &self,
        mutation: &crate::changes::Mutation,
        before: Option<&Value>,
        deadline: Instant,
    ) -> Result<Value, PageError> {
        use crate::changes::{ChangeAction as A, ChangeKind as K, item_name};
        let options = Self::options(deadline);
        let o = options.as_ref();
        fn model<T: serde::de::DeserializeOwned>(body: &Value) -> Result<T, PageError> {
            serde_json::from_value(body.clone()).map_err(|_| PageError::Invalid)
        }
        let json = |value: &dyn erased::Json| value.to_json().map_err(|_| PageError::Invalid);
        let name = || {
            before
                .and_then(|b| item_name(mutation.kind, b))
                .ok_or(PageError::Invalid)
        };
        let author = mutation
            .body
            .get("updated_by")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let target = mutation.target.as_str();
        let m = &self.management;
        match (mutation.kind, mutation.action) {
            (K::Profiles, A::Create) => {
                json(&m.profiles().create(&model(&mutation.body)?, o).await?)
            }
            (K::Profiles, A::Update) => json(
                &m.profiles()
                    .update(target, &model(&mutation.body)?, o)
                    .await?,
            ),
            (K::Profiles, A::Delete) => json(&m.profiles().delete(target, o).await?),
            (K::Topics, A::Create) => json(&m.topics().create(&model(&mutation.body)?, o).await?),
            (K::Topics, A::Update) => json(
                &m.topics()
                    .update(target, &model(&mutation.body)?, o)
                    .await?,
            ),
            (K::Topics, A::Delete) => json(&m.topics().delete(target, o).await?),
            (K::Applications, A::Update) => json(
                &m.customer_apps()
                    .update(target, &model(&mutation.body)?, o)
                    .await?,
            ),
            (K::Applications, A::Delete) => {
                json(&m.customer_apps().delete(&name()?, author, o).await?)
            }
            (K::ApiKeys, A::Create | A::Version) => {
                json(&m.api_keys().create(&model(&mutation.body)?, o).await?)
            }
            (K::ApiKeys, A::Rotate) => json(
                &m.api_keys()
                    .regenerate(target, &model(&mutation.body)?, o)
                    .await?,
            ),
            (K::ApiKeys, A::Delete) => json(&m.api_keys().delete(&name()?, author, o).await?),
            (K::DataPatterns, A::Create) => json(
                &self
                    .dlp
                    .data_patterns()
                    .create(&model(&mutation.body)?, o)
                    .await?,
            ),
            (K::DataPatterns, A::Update) => json(
                &self
                    .dlp
                    .data_patterns()
                    .replace(target, &model(&mutation.body)?, o)
                    .await?,
            ),
            (K::DataPatterns, A::Delete) => {
                self.dlp.data_patterns().delete(target, o).await?;
                Ok(Value::Object(serde_json::Map::new()))
            }
            (K::DataProfiles, A::Create) => json(
                &self
                    .dlp
                    .data_profiles()
                    .create(&model(&mutation.body)?, o)
                    .await?,
            ),
            (K::DataProfiles, A::Update) => json(
                &self
                    .dlp
                    .data_profiles()
                    .replace(target, &model(&mutation.body)?, o)
                    .await?,
            ),
            (K::FilteringProfiles, A::Update) => json(
                &self
                    .dlp
                    .data_filtering_profiles()
                    .replace(target, &model(&mutation.body)?, o)
                    .await?,
            ),
            (K::Dictionaries, A::Create | A::Update) => {
                let metadata = model(&mutation.body["metadata"])?;
                let file = mutation.body["file"].as_str().ok_or(PageError::Invalid)?;
                let upload = scm_sdk::dlp::DictionaryUpload::text(metadata, file)?;
                if mutation.action == A::Create {
                    json(&self.dlp.dictionaries().create(&upload, o).await?)
                } else {
                    // A 204 replies with no body.
                    match self.dlp.dictionaries().replace(target, &upload, o).await? {
                        Some(reply) => json(&reply),
                        None => Ok(Value::Object(serde_json::Map::new())),
                    }
                }
            }
            (K::Dictionaries, A::Delete) => {
                self.dlp.dictionaries().delete(target, o).await?;
                Ok(Value::Object(serde_json::Map::new()))
            }
            _ => Err(PageError::Invalid),
        }
    }
}

/// Sessions investigation reads (C11): the dashboard's raw replies, as v1
/// read them leniently, with secret fields removed.
impl ManagementBinding {
    fn dashboard(&self) -> Result<scm_sdk::airs::management::DashboardClient, PageError> {
        Ok(self.management.dashboard())
    }

    /// One sessions-overview reply over the last `interval` `unit`s.
    async fn overview(
        &self,
        interval: u64,
        unit: &str,
        offset: u32,
        limit: u32,
        options: Option<&RequestOptions>,
    ) -> Result<Value, PageError> {
        let mut query = scm_sdk::airs::management::DashboardSessionsOverviewQuery::default();
        query.time_interval = Some(interval);
        query.time_unit = Some(unit.to_owned());
        query.offset = Some(u64::from(offset));
        query.limit = Some(u64::from(limit));
        self.dashboard()?
            .sessions_overview_raw(&query, options)
            .await?
            .map(Value::from)
            .ok_or(PageError::Invalid)
    }

    /// One session's first page of actions (v1: 25 over 30 days).
    pub async fn session(
        &self,
        session_id: &str,
        app_id: &str,
        app_name: &str,
        deadline: Instant,
    ) -> Result<SessionRead, PageError> {
        let options = Self::options(deadline);
        let mut query =
            scm_sdk::airs::management::DashboardSessionQuery::new(session_id, app_id, app_name);
        query.time_interval = Some(30);
        query.time_unit = Some("days".into());
        query.offset = Some(0);
        query.limit = Some(u64::from(crate::sessions::SESSION_ACTIONS));
        let mut value = self
            .dashboard()?
            .session_raw(&query, options.as_ref())
            .await?
            .map(Value::from)
            .ok_or(PageError::Invalid)?;
        if !value.is_object() {
            return Err(PageError::Invalid);
        }
        redact(&mut value);
        crate::sessions::strip_content(&mut value);
        let actions = value
            .get("session_actions")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        let actions = u32::try_from(actions).map_err(|_| PageError::Invalid)?;
        Ok(SessionRead {
            session: value.to_string(),
            actions,
            more: actions >= crate::sessions::SESSION_ACTIONS,
        })
    }

    /// One transaction's metadata, content removed.
    pub async fn transaction(
        &self,
        session_id: &str,
        app_id: &str,
        app_name: &str,
        scan_id: &str,
        sub_request: u32,
        deadline: Instant,
    ) -> Result<Value, PageError> {
        let options = Self::options(deadline);
        let query = scm_sdk::airs::management::DashboardSessionTransactionQuery::new(
            session_id,
            app_id,
            app_name,
            scan_id,
            u64::from(sub_request),
        );
        let mut value = self
            .dashboard()?
            .session_transaction_raw(&query, options.as_ref())
            .await?
            .map(Value::from)
            .ok_or(PageError::Invalid)?;
        if !value.is_object() {
            return Err(PageError::Invalid);
        }
        redact(&mut value);
        crate::sessions::strip_content(&mut value);
        Ok(value)
    }

    /// A scan's report with its content (`scan_contents` or flat fields).
    pub async fn scan_report(
        &self,
        scan_id: &str,
        sub_request: u32,
        deadline: Instant,
    ) -> Result<Value, PageError> {
        let options = Self::options(deadline);
        let query = scm_sdk::airs::management::DashboardScanContentQuery::new(
            scan_id,
            u64::from(sub_request),
        );
        let mut value = self
            .dashboard()?
            .scan_content_raw(&query, options.as_ref())
            .await?
            .map(Value::from)
            .ok_or(PageError::Invalid)?;
        if !value.is_object() {
            return Err(PageError::Invalid);
        }
        redact(&mut value);
        Ok(value)
    }

    /// The last day's sessions chart (raw; `None` for an empty reply).
    pub async fn sessions_chart(&self, deadline: Instant) -> Result<Option<Value>, PageError> {
        let options = Self::options(deadline);
        let mut query = scm_sdk::airs::management::DashboardTimeRangeQuery::default();
        query.time_interval = Some(1);
        query.time_unit = Some("day".to_owned());
        let mut value = self
            .dashboard()?
            .sessions_chart_raw(&query, options.as_ref())
            .await?
            .map(Value::from);
        if let Some(value) = value.as_mut() {
            redact(value);
        }
        Ok(value)
    }

    /// A bounded walk of the sessions overview, newest first, in pages of
    /// [`crate::sessions::SEARCH_PAGE`] over `days`: at most `max_sessions /
    /// SEARCH_PAGE` requests and `max_sessions` sessions read (v1
    /// `runtime.ts`). It collects rows `wanted` accepts and stops after the
    /// page on which `stop_at` matched. The service's own pagination decides
    /// what follows: a short or empty page that reports more (a total or a
    /// cursor) is continued from where the service says, never taken as the
    /// end. Returns the matches (at most `stop_at`), the sessions read, and
    /// whether the window ended with no more than `stop_at` matches (absence
    /// proven).
    pub async fn walk_sessions(
        &self,
        days: u64,
        max_sessions: u32,
        stop_at: usize,
        deadline: Instant,
        mut wanted: impl FnMut(&Value) -> bool,
    ) -> Result<(Vec<Value>, u32, bool), PageError> {
        let options = Self::options(deadline);
        let (interval, unit) = if days == 1 {
            (1, "day")
        } else {
            (days, "days")
        };
        let limit = crate::sessions::SEARCH_PAGE;
        let mut offset = 0u32;
        let mut matches = Vec::new();
        let mut scanned = 0u32;
        for _ in 0..max_sessions.div_ceil(limit) {
            let value = self
                .overview(interval, unit, offset, limit, options.as_ref())
                .await?;
            let page = normalize(
                ManagementResource::Sessions,
                offset,
                limit,
                value,
                Paging::Offset {
                    key: "items",
                    required: true,
                },
            )?;
            let items: Vec<Value> =
                serde_json::from_str(&page.items).map_err(|_| PageError::Invalid)?;
            scanned = scanned.saturating_add(page.count);
            matches.extend(items.into_iter().filter(|row| wanted(row)));
            if page.complete || matches.len() >= stop_at {
                let complete = page.complete && matches.len() <= stop_at;
                matches.truncate(stop_at);
                return Ok((matches, scanned, complete));
            }
            match page.next_offset {
                Some(next) if next > offset => offset = next,
                // An incomplete page with no way forward proves nothing.
                _ => break,
            }
            if scanned >= max_sessions {
                break;
            }
        }
        matches.truncate(stop_at);
        Ok((matches, scanned, false))
    }
}

/// Where a bounded walk ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Walk {
    Found(Value),
    /// The service said the inventory ended without a match.
    Ended,
    /// The walk's bound was reached first.
    Incomplete,
}

impl ManagementBinding {
    /// A DLP item by its own read, secret fields removed; `None` when the
    /// service answers 404.
    pub async fn dlp_item(
        &self,
        resource: ManagementResource,
        id: &str,
        deadline: Instant,
    ) -> Result<Option<Value>, PageError> {
        match self.dlp_get(resource, id, deadline).await {
            Some(Ok(mut item)) => {
                redact(&mut item);
                Ok(Some(item))
            }
            Some(Err(PageError::Sdk(error)))
                if ManagementFailure::from_sdk(&error) == ManagementFailure::NotFound =>
            {
                Ok(None)
            }
            Some(Err(error)) => Err(error),
            None => Err(PageError::Invalid),
        }
    }

    /// The DLP resources' own reads; `None` for resources read by a walk.
    async fn dlp_get(
        &self,
        resource: ManagementResource,
        id: &str,
        deadline: Instant,
    ) -> Option<Result<Value, PageError>> {
        let options = Self::options(deadline);
        let options = options.as_ref();
        let json = |value: &dyn erased::Json| value.to_json().map_err(|_| PageError::Invalid);
        let value = match resource {
            ManagementResource::DataProfiles => self
                .dlp
                .data_profiles()
                .get(id, options)
                .await
                .map(|r| json(&r)),
            ManagementResource::DataPatterns => self
                .dlp
                .data_patterns()
                .get(id, options)
                .await
                .map(|r| json(&r)),
            ManagementResource::FilteringProfiles => self
                .dlp
                .data_filtering_profiles()
                .get(id, options)
                .await
                .map(|r| json(&r)),
            ManagementResource::Dictionaries => self
                .dlp
                .dictionaries()
                .get(id, None, options)
                .await
                .map(|r| json(&r)),
            _ => return None,
        };
        Some(match value {
            Ok(Ok(Value::Object(item))) => Ok(Value::Object(item)),
            Ok(_) => Err(PageError::Invalid),
            Err(error) => Err(PageError::Sdk(error)),
        })
    }
}

/// An item's identity as text: a string, or a number's decimal form (DLP
/// identifiers may be numeric).
pub fn identity(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// How a resource pages.
#[derive(Clone, Copy, Debug)]
enum Paging {
    /// AIRS offset/limit with `pagination.total_items` or `next_offset`
    /// (items under `key`). v1 fails closed when a required inventory's
    /// array is absent and reads an absent optional one as empty.
    Offset { key: &'static str, required: bool },
    /// Unpaginated: the whole list, sliced here (items under `key`).
    Local { key: &'static str, required: bool },
    /// DLP Spring pages (`content`, `last`, `totalPages`, `totalElements`).
    Spring,
}

/// The items array under `key`: absent or null is empty unless required.
fn array_at(value: &mut Value, key: &str, required: bool) -> Result<Vec<Value>, PageError> {
    match value.get_mut(key).map(Value::take) {
        Some(Value::Array(items)) => Ok(items),
        Some(Value::Null) | None if !required => Ok(Vec::new()),
        _ => Err(PageError::Invalid),
    }
}

/// Why a page could not be produced.
#[derive(Debug)]
pub enum PageError {
    Sdk(scm_sdk::Error),
    /// The reply did not match the paging contract.
    Invalid,
}

impl From<scm_sdk::Error> for PageError {
    fn from(error: scm_sdk::Error) -> Self {
        Self::Sdk(error)
    }
}

mod erased {
    /// Serializes an SDK model to JSON (they all implement `Serialize`).
    pub trait Json {
        fn to_json(&self) -> Result<serde_json::Value, serde_json::Error>;
    }
    impl<T: serde::Serialize> Json for T {
        fn to_json(&self) -> Result<serde_json::Value, serde_json::Error> {
            serde_json::to_value(self)
        }
    }
}

fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(|v| v.as_u64().or_else(|| v.as_f64().map(|f| f as u64)))
}

/// Normalizes one reply into a page. Completeness is claimed only from the
/// service's own signal; a page that does not advance, or holds more items
/// than asked for, is an invalid reply.
fn normalize(
    resource: ManagementResource,
    offset: u32,
    limit: u32,
    mut value: Value,
    paging: Paging,
) -> Result<ManagementPage, PageError> {
    let (mut items, next_offset, complete, total) = match paging {
        Paging::Offset { key, required } => {
            let items = array_at(&mut value, key, required)?;
            if items.len() > limit as usize {
                return Err(PageError::Invalid);
            }
            let end = u64::from(offset) + u64::from(limit);
            let total_items = value
                .get("pagination")
                .and_then(|p| p.get("total_items"))
                .and_then(Value::as_u64);
            if let Some(total) = total_items {
                // The service's own total decides what follows (v1).
                let total = u32::try_from(total).map_err(|_| PageError::Invalid)?;
                let next = (end < u64::from(total)).then(|| offset + limit);
                (items, next, next.is_none(), Some(total))
            } else {
                // v1 reads a zero cursor as none.
                let reported = number(value.get("next_offset")).filter(|n| *n != 0);
                let next = match reported {
                    Some(next) if next <= u64::from(offset) => return Err(PageError::Invalid),
                    Some(next) => Some(u32::try_from(next).map_err(|_| PageError::Invalid)?),
                    // No cursor and a short page: the service says nothing
                    // follows. A full page without a cursor proves nothing.
                    None if items.len() < limit as usize => None,
                    None => Some(offset + limit),
                };
                let complete = reported.is_none() && items.len() < limit as usize;
                (items, next, complete, None)
            }
        }
        Paging::Local { key, required } => {
            let all = array_at(&mut value, key, required)?;
            let total = all.len();
            let start = (offset as usize).min(total);
            let end = (start + limit as usize).min(total);
            let items: Vec<Value> = all.into_iter().skip(start).take(end - start).collect();
            let complete = end >= total;
            let next = (!complete).then(|| offset + limit);
            (
                items,
                next,
                complete,
                Some(u32::try_from(total).map_err(|_| PageError::Invalid)?),
            )
        }
        Paging::Spring => {
            let items = match value.get_mut("content").map(Value::take) {
                Some(Value::Array(items)) => items,
                _ => return Err(PageError::Invalid),
            };
            if items.len() > limit as usize {
                return Err(PageError::Invalid);
            }
            let last = value.get("last").and_then(Value::as_bool);
            let pages = value.get("totalPages").and_then(Value::as_u64);
            let total = number(value.get("totalElements"));
            let complete = match (last, pages, total) {
                (Some(last), ..) => last,
                (None, Some(pages), _) => u64::from(offset / limit) + 1 >= pages,
                (None, None, Some(total)) => u64::from(offset) + items.len() as u64 >= total,
                (None, None, None) => items.len() < limit as usize,
            };
            let next = (!complete).then(|| offset + limit);
            let total = total
                .map(u32::try_from)
                .transpose()
                .map_err(|_| PageError::Invalid)?;
            (items, next, complete, total)
        }
    };
    for item in &mut items {
        redact(item);
    }
    let count = u32::try_from(items.len()).map_err(|_| PageError::Invalid)?;
    let items = Value::Array(items).to_string();
    if items.len() > MAX_PAGE_BYTES {
        return Err(PageError::Invalid);
    }
    Ok(ManagementPage {
        resource,
        offset,
        limit,
        items,
        count,
        next_offset,
        complete,
        total,
    })
}

/// The bound clients of one credential generation, built once by the first
/// read that needs them and reused (with the SDK's token cache) until the
/// generation retires.
/// A failed build (a locked Keychain, say) is not cached: the next explicit
/// read tries again.
pub type BindingCell = Arc<OnceCell<ManagementBinding>>;

pub type ResultSlot = Arc<std::sync::Mutex<Option<ManagementResult>>>;

/// Everything needed to build (or reuse) a credential generation's
/// binding: shared by management reads and reviewed changes.
#[derive(Clone)]
pub(crate) struct BindingSource {
    pub keychain: Arc<dyn CredentialStore>,
    pub session_vault: Arc<SessionVault>,
    pub record: RecordId,
    pub mode: StorageMode,
    pub client_id: String,
    pub tsg: String,
    pub endpoints: ManagementEndpoints,
    pub transport: Arc<dyn Transport>,
    pub session: Session,
    pub binding: BindingCell,
    /// The generation's investigation cache (C11).
    pub investigation: Arc<crate::sessions::Investigation>,
}

/// Everything one admitted management read needs at dispatch.
pub(crate) struct ManagementJob {
    pub source: BindingSource,
    pub request: ManagementRequest,
    pub slot: ResultSlot,
}

impl BindingSource {
    /// The generation's binding, built by the first caller that needs it.
    pub async fn bind(&self) -> Result<&ManagementBinding, ManagementFailure> {
        self.binding
            .get_or_try_init(|| async {
                let keychain = Arc::clone(&self.keychain);
                let session_vault = Arc::clone(&self.session_vault);
                let record = self.record.clone();
                let mode = self.mode;
                let read = tokio::task::spawn_blocking(move || match mode {
                    StorageMode::Keychain => keychain.get(&record),
                    StorageMode::Session => session_vault.get(&record),
                })
                .await
                .unwrap_or(Err(VaultError::Unavailable));
                let secret = match read {
                    Ok(secret) => secret,
                    Err(VaultError::Locked) => return Err(ManagementFailure::CredentialLocked),
                    Err(VaultError::Denied) => return Err(ManagementFailure::CredentialDenied),
                    Err(_) => return Err(ManagementFailure::CredentialMissing),
                };
                let secret = match std::str::from_utf8(&secret) {
                    Ok(text) => Zeroizing::new(text.to_owned()),
                    Err(_) => return Err(ManagementFailure::CredentialMissing),
                };
                ManagementBinding::new(
                    &self.endpoints,
                    &self.client_id,
                    &self.tsg,
                    secret,
                    Arc::clone(&self.transport),
                    self.session.clone(),
                )
            })
            .await
    }
}

fn failure_of(error: PageError) -> ManagementFailure {
    match error {
        PageError::Invalid => ManagementFailure::InvalidResponse,
        PageError::Sdk(e) => ManagementFailure::from_sdk(&e),
    }
}

/// One Runtime overview read: both sources are read; each keeps its own
/// data or sanitized failure, and what they could not establish is a gap.
async fn runtime_overview(
    binding: &ManagementBinding,
    deadline: Instant,
) -> crate::observation::RuntimeOverview {
    use crate::observation as obs;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let chart = binding
        .sessions_chart(deadline)
        .await
        .and_then(|v| obs::parse_chart(v).ok_or(PageError::Invalid));
    let keys = binding
        .page(ManagementResource::ApiKeys, 0, obs::KEY_PAGE, deadline)
        .await;
    let mut gaps = Vec::new();
    let mut items = Vec::new();
    let (chart, chart_failure, summary) = match chart {
        Ok(chart) => {
            let (summary, chart_gaps) = obs::summarize(&chart);
            gaps.extend(chart_gaps);
            items.extend(obs::activity_item(&summary));
            (Some(chart), None, summary)
        }
        Err(error) => {
            gaps.push("Runtime activity unavailable".to_owned());
            (
                None,
                Some(failure_of(error)),
                obs::RuntimeSummary::default(),
            )
        }
    };
    let (keys_complete, keys_failure) = match keys {
        Ok(page) => {
            let rows: Vec<Value> = serde_json::from_str(&page.items).unwrap_or_default();
            let complete = page.complete;
            let (key_items, key_gaps) =
                obs::key_items(&rows, complete, i64::try_from(now).unwrap_or(i64::MAX));
            items.extend(key_items);
            gaps.extend(key_gaps);
            (complete, None)
        }
        Err(error) => {
            gaps.push("Runtime API keys unavailable".to_owned());
            (false, Some(failure_of(error)))
        }
    };
    obs::sort_items(&mut items);
    gaps.sort();
    gaps.dedup();
    obs::RuntimeOverview {
        read_at: u32::try_from(now).unwrap_or(u32::MAX),
        chart,
        chart_failure,
        summary,
        keys_complete,
        keys_failure,
        items,
        gaps,
    }
}

/// The product data plane a C13 read goes to.
#[derive(Clone, Copy)]
enum Product {
    RedTeam,
    ModelSecurity,
    AgentGuard,
}

impl ManagementBinding {
    /// One read-only GET to a product's data plane through its SDK client
    /// (OAuth, origin policy, limits and the tenant header as that family
    /// requires), returning the reply as JSON for v1's tolerant rules.
    async fn product_read(
        &self,
        product: Product,
        path: &str,
        query: Vec<(String, String)>,
        deadline: Instant,
    ) -> Result<Value, PageError> {
        let client = match product {
            Product::RedTeam => self.red_team.client(),
            Product::ModelSecurity => self.model_security.data_client(),
            Product::AgentGuard => self.agentguard.data_client(),
        };
        let reply: scm_sdk::data::JsonValue = client
            .send_json(
                scm_sdk::RawRequest::read(path).query(query),
                Self::options(deadline).as_ref(),
            )
            .await?;
        Ok(Value::from(reply))
    }
}

fn now_secs() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

fn list_query(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut query = vec![
        ("skip".to_owned(), "0".to_owned()),
        ("limit".to_owned(), crate::red_team::LIST_LIMIT.to_string()),
    ];
    query.extend(
        extra
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
    );
    query
}

/// A source's data or its sanitized failure.
fn sourced<T>(read: Result<T, PageError>) -> (Option<T>, Option<ManagementFailure>) {
    match read {
        Ok(data) => (Some(data), None),
        Err(error) => (None, Some(failure_of(error))),
    }
}

/// One Red Teaming read: statistics for the window (absent statistics are
/// "no scans" only when a one-row scan list proves it) and the first 100
/// scans. A read in which no source answered fails.
async fn red_team_read(
    binding: &ManagementBinding,
    window: u8,
    deadline: Instant,
) -> Result<crate::red_team::RedTeamObservation, PageError> {
    use crate::red_team as rt;
    let range = format!("LAST_{window}_DAYS");
    let statistics = match binding
        .product_read(
            Product::RedTeam,
            "/v1/dashboard/scan-statistics",
            vec![("date_range".to_owned(), range)],
            deadline,
        )
        .await
    {
        Ok(reply) => rt::statistics(&reply).ok_or(PageError::Invalid),
        Err(PageError::Sdk(error))
            if ManagementFailure::from_sdk(&error) == ManagementFailure::NotFound =>
        {
            let mut query = list_query(&[]);
            query[1].1 = "1".to_owned();
            match binding
                .product_read(Product::RedTeam, "/v1/scan", query, deadline)
                .await
            {
                Ok(reply) if rt::proves_no_scans(&reply) => Ok(rt::no_scans()),
                _ => Err(PageError::Sdk(error)),
            }
        }
        Err(error) => Err(error),
    };
    let scans = binding
        .product_read(Product::RedTeam, "/v1/scan", list_query(&[]), deadline)
        .await
        .and_then(|reply| rt::red_team_scans(&reply).ok_or(PageError::Invalid));
    let (statistics, scans) = match (statistics, scans) {
        (Err(error), Err(_)) => return Err(error),
        both => both,
    };
    let (statistics, statistics_failure) = sourced(statistics);
    let (scans, scans_failure) = sourced(scans);
    Ok(rt::RedTeamObservation {
        read_at: now_secs(),
        window,
        statistics,
        statistics_failure,
        scans,
        scans_failure,
    })
}

/// One Model and skill read: Model Security scans, AgentGuard's 30-day
/// skill statistics and skill scans, each independent. A read in which no
/// source answered fails.
async fn supply_chain_read(
    binding: &ManagementBinding,
    deadline: Instant,
) -> Result<crate::red_team::SupplyChainObservation, PageError> {
    use crate::red_team as rt;
    let background = [("isBackgroundRefresh", "false")];
    let models = binding
        .product_read(
            Product::ModelSecurity,
            "/v1/scans",
            list_query(&background),
            deadline,
        )
        .await
        .and_then(|reply| rt::model_scans(&reply).ok_or(PageError::Invalid));
    let statistics = binding
        .product_read(
            Product::AgentGuard,
            "/v1/stats/scans",
            vec![("time_period".to_owned(), "30_DAYS".to_owned())],
            deadline,
        )
        .await
        .and_then(|reply| rt::skill_statistics(&reply).ok_or(PageError::Invalid));
    let salt = rt::salt().ok_or(PageError::Invalid)?;
    let skills = binding
        .product_read(
            Product::AgentGuard,
            "/v1/scans",
            list_query(&background),
            deadline,
        )
        .await
        .and_then(|reply| rt::skill_scans(&reply, &salt).ok_or(PageError::Invalid));
    let (models, statistics, skills) = match (models, statistics, skills) {
        (Err(error), Err(_), Err(_)) => return Err(error),
        all => all,
    };
    let (models, models_failure) = sourced(models);
    let (skill_statistics, skill_statistics_failure) = sourced(statistics);
    let (skills, skills_failure) = sourced(skills);
    Ok(rt::SupplyChainObservation {
        read_at: now_secs(),
        models,
        models_failure,
        skill_statistics,
        skill_statistics_failure,
        skills,
        skills_failure,
    })
}

/// Cache identities: a session by (session, app, name); a transaction and
/// its content by (scan, sub-request).
fn session_key(session_id: &str, app_id: &str, app_name: &str) -> String {
    format!("{session_id}\u{1f}{app_id}\u{1f}{app_name}")
}

fn scan_key(scan_id: &str, sub_request: u32) -> String {
    format!("{scan_id}\u{1f}{sub_request}")
}

/// A scan report's prompt and response: flat fields or `scan_contents` (an
/// object or the first of an array), as v1 read them.
fn scan_content(report: &Value) -> ScanContent {
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    let nested = match report.get("scan_contents") {
        Some(Value::Array(items)) => items.first(),
        Some(value @ Value::Object(_)) => Some(value),
        _ => None,
    };
    ScanContent {
        prompt: text(report, "prompt").or_else(|| nested.and_then(|n| text(n, "prompt"))),
        response: text(report, "response").or_else(|| nested.and_then(|n| text(n, "response"))),
    }
}

impl ManagementJob {
    /// Sessions investigation reads (C11). The binding is already bound, so
    /// a cached answer is only ever given to the current credential.
    async fn investigate(
        &self,
        binding: &ManagementBinding,
        request: &ManagementRequest,
        deadline: Instant,
    ) -> Result<ManagementResult, PageError> {
        let investigation = &self.source.investigation;
        investigation.ready().await;
        let result = match request {
            ManagementRequest::Session {
                session_id,
                app_id,
                app_name,
            } => {
                let key = session_key(session_id, app_id, app_name);
                if let Some(Some(session)) = investigation
                    .with(|c| crate::sessions::InvestigationCache::get(&c.sessions, &key).cloned())
                    && let Ok(session) = serde_json::from_value::<SessionRead>(session)
                {
                    return Ok(ManagementResult::Session { session });
                }
                let session = binding
                    .session(session_id, app_id, app_name, deadline)
                    .await?;
                if let Ok(value) = serde_json::to_value(&session) {
                    investigation.with(|c| c.put_session(key, value));
                }
                ManagementResult::Session { session }
            }
            ManagementRequest::Transaction {
                session_id,
                app_id,
                app_name,
                scan_id,
                sub_request,
            } => {
                let key = scan_key(scan_id, *sub_request);
                if let Some(Some(transaction)) = investigation.with(|c| {
                    crate::sessions::InvestigationCache::get(&c.transactions, &key).cloned()
                }) {
                    return Ok(ManagementResult::Transaction {
                        transaction: transaction.to_string(),
                    });
                }
                let value = binding
                    .transaction(
                        session_id,
                        app_id,
                        app_name,
                        scan_id,
                        *sub_request,
                        deadline,
                    )
                    .await?;
                let transaction = value.to_string();
                investigation.with(|c| c.put_transaction(key, value));
                ManagementResult::Transaction { transaction }
            }
            ManagementRequest::Content {
                scan_id,
                sub_request,
            } => {
                let key = scan_key(scan_id, *sub_request);
                let cached = investigation
                    .with(|c| crate::sessions::InvestigationCache::get(&c.contents, &key).cloned())
                    .flatten()
                    .map(|v| scan_content(&v));
                if let Some(content) = cached {
                    return Ok(ManagementResult::Content { content });
                }
                let report = binding.scan_report(scan_id, *sub_request, deadline).await?;
                let content = scan_content(&report);
                let kept =
                    serde_json::json!({"prompt": content.prompt, "response": content.response});
                investigation.with(|c| c.put_content(key, kept));
                ManagementResult::Content { content }
            }
            ManagementRequest::Search { query } => {
                let (kind, query) = crate::sessions::classify(query).ok_or(PageError::Invalid)?;
                ManagementResult::Search {
                    search: self.search(binding, kind, query, deadline).await?,
                }
            }
            _ => return Err(PageError::Invalid),
        };
        investigation.persist().await;
        Ok(result)
    }

    /// v1's bounded searches: a session ID over 30 days (40 pages of 100), a
    /// scan ID by its report (metadata only), a detector over 24 hours
    /// (1,000 sessions, stopping at 100 matches). The cache answers first.
    async fn search(
        &self,
        binding: &ManagementBinding,
        kind: crate::sessions::SearchKind,
        query: String,
        deadline: Instant,
    ) -> Result<SessionSearch, PageError> {
        use crate::sessions::{
            DETECTOR_MATCHES, DETECTOR_SCAN, SEARCH_PAGE, SESSION_SEARCH_PAGES, SearchKind,
        };
        let investigation = &self.source.investigation;
        let mut search = SessionSearch {
            kind,
            query: query.clone(),
            sessions: "[]".into(),
            scanned: 0,
            complete: false,
            match_limit: false,
            scan: None,
            cached: false,
        };
        match kind {
            SearchKind::SessionId => {
                let cached = investigation
                    .with(|c| {
                        c.sessions.iter().find_map(|(_, v)| {
                            let session: Value =
                                serde_json::from_str(v.get("session")?.as_str()?).ok()?;
                            (session.get("session_id")?.as_str()? == query).then_some(session)
                        })
                    })
                    .flatten();
                if let Some(session) = cached {
                    let mut row = session;
                    crate::sessions::strip_content(&mut row);
                    if let Some(map) = row.as_object_mut() {
                        map.remove("session_actions");
                    }
                    search.sessions = Value::Array(vec![row]).to_string();
                    search.complete = true;
                    search.cached = true;
                    return Ok(search);
                }
                let (found, scanned, complete) = binding
                    .walk_sessions(
                        30,
                        SEARCH_PAGE * SESSION_SEARCH_PAGES,
                        usize::MAX,
                        deadline,
                        |row| row.get("session_id").and_then(Value::as_str) == Some(query.as_str()),
                    )
                    .await?;
                search.complete = complete;
                search.scanned = scanned;
                search.sessions = Value::Array(found).to_string();
            }
            SearchKind::ScanId => {
                match binding.scan_report(&query, 0, deadline).await {
                    Ok(mut report) => {
                        crate::sessions::strip_content(&mut report);
                        search.scan = Some(report.to_string());
                        search.complete = true;
                    }
                    // The service says there is no such scan: absence is
                    // proven for this credential.
                    Err(PageError::Sdk(error))
                        if ManagementFailure::from_sdk(&error) == ManagementFailure::NotFound =>
                    {
                        search.complete = true;
                    }
                    Err(error) => return Err(error),
                }
            }
            SearchKind::Detector => {
                let limit = DETECTOR_MATCHES as usize;
                let (found, scanned, complete) = binding
                    .walk_sessions(1, DETECTOR_SCAN, limit, deadline, |row| {
                        crate::sessions::reports_detector(row, &query)
                    })
                    .await?;
                search.match_limit = found.len() >= limit;
                search.complete = complete;
                search.scanned = scanned;
                search.sessions = Value::Array(found).to_string();
            }
        }
        Ok(search)
    }

    pub fn into_work(self) -> Work {
        Box::new(move |claim| Box::pin(self.run(claim)))
    }

    fn leave(&self, result: ManagementResult) {
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some(result);
        }
    }

    fn fail(&self, failure: ManagementFailure, outcome: Outcome) -> Outcome {
        self.leave(ManagementResult::Failed { failure, outcome });
        outcome
    }

    async fn run(self, claim: Claim) -> Outcome {
        let binding = match self.source.bind().await {
            Ok(binding) => binding,
            Err(failure) => return self.fail(failure, Outcome::NotSent),
        };
        if claim.cancel.is_cancelled() {
            return self.fail(ManagementFailure::Cancelled, Outcome::NotSent);
        }
        let deadline = claim.deadline;
        let work = async {
            match &self.request {
                ManagementRequest::Verify => binding
                    .verify(deadline)
                    .await
                    .map(|()| ManagementResult::Verified)
                    .map_err(PageError::Sdk),
                ManagementRequest::Page {
                    resource,
                    offset,
                    limit,
                } => binding
                    .page(*resource, *offset, *limit, deadline)
                    .await
                    .map(|page| ManagementResult::Page { page }),
                ManagementRequest::Detail { resource, id } => binding
                    .detail(*resource, id, deadline)
                    .await
                    .map(|detail| ManagementResult::Detail { detail }),
                ManagementRequest::Overview => Ok(ManagementResult::Overview {
                    overview: runtime_overview(binding, deadline).await,
                }),
                ManagementRequest::RedTeam { window } => red_team_read(binding, *window, deadline)
                    .await
                    .map(|observation| ManagementResult::RedTeam { observation }),
                ManagementRequest::SupplyChain => supply_chain_read(binding, deadline)
                    .await
                    .map(|observation| ManagementResult::SupplyChain { observation }),
                ManagementRequest::Attacks { scan_id, page } => binding
                    .product_read(
                        Product::RedTeam,
                        &format!("/v1/report/static/{scan_id}/list-attacks"),
                        vec![
                            (
                                "skip".to_owned(),
                                (u32::from(*page) * crate::red_team::LIST_LIMIT).to_string(),
                            ),
                            ("limit".to_owned(), crate::red_team::LIST_LIMIT.to_string()),
                        ],
                        deadline,
                    )
                    .await
                    .and_then(|reply| {
                        crate::red_team::attacks(&reply, scan_id, *page).ok_or(PageError::Invalid)
                    })
                    .map(|attacks| ManagementResult::Attacks { attacks }),
                request => self.investigate(binding, request, deadline).await,
            }
        };
        tokio::pin!(work);
        let result = tokio::select! {
            result = &mut work => Some(result),
            () = claim.cancel.cancelled() => None,
        };
        match result {
            // Reads change nothing: a cancelled read is simply not shown.
            None => self.fail(ManagementFailure::Cancelled, Outcome::Unknown),
            Some(Ok(result)) => {
                self.leave(result);
                Outcome::Completed
            }
            Some(Err(PageError::Invalid)) => {
                self.fail(ManagementFailure::InvalidResponse, Outcome::Completed)
            }
            Some(Err(PageError::Sdk(error))) => {
                self.fail(ManagementFailure::from_sdk(&error), outcome_of(&error))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PROFILES: Paging = Paging::Offset {
        key: "ai_profiles",
        required: true,
    };
    const DEPLOYMENTS: Paging = Paging::Local {
        key: "deployment_profiles",
        required: true,
    };

    fn items(page: &ManagementPage) -> Vec<Value> {
        serde_json::from_str(&page.items).expect("items")
    }

    #[test]
    fn offset_pages_claim_completeness_only_from_the_service() {
        let page = |value: Value| normalize(ManagementResource::Profiles, 0, 2, value, PROFILES);
        // A cursor: more follows.
        let more = page(
            json!({"ai_profiles": [{"profile_id": "a"}, {"profile_id": "b"}], "next_offset": 2}),
        )
        .expect("page");
        assert_eq!((more.next_offset, more.complete), (Some(2), false));
        // A short page with a cursor: more still follows.
        let short =
            page(json!({"ai_profiles": [{"profile_id": "a"}], "next_offset": 1})).expect("page");
        assert_eq!((short.next_offset, short.complete), (Some(1), false));
        // A short page without a cursor: the last page.
        let last = page(json!({"ai_profiles": [{"profile_id": "a"}]})).expect("page");
        assert_eq!((last.next_offset, last.complete), (None, true));
        // A full page without a cursor proves nothing: the next page is asked.
        let full =
            page(json!({"ai_profiles": [{"profile_id": "a"}, {"profile_id": "b"}]})).expect("page");
        assert_eq!((full.next_offset, full.complete), (Some(2), false));
        // A cursor that does not advance, or too many items: invalid.
        assert!(
            normalize(
                ManagementResource::Profiles,
                4,
                2,
                json!({"ai_profiles": [], "next_offset": 4}),
                PROFILES
            )
            .is_err()
        );
        assert!(page(json!({"ai_profiles": [{}, {}, {}]})).is_err());
        // A zero cursor is no cursor (v1).
        let zero =
            page(json!({"ai_profiles": [{"profile_id": "a"}], "next_offset": 0})).expect("page");
        assert_eq!((zero.next_offset, zero.complete), (None, true));
        // The service's total decides what follows (v1 `pagination`).
        let counted =
            page(json!({"ai_profiles": [{}, {}], "pagination": {"total_items": 3}})).expect("page");
        assert_eq!(
            (counted.next_offset, counted.complete, counted.total),
            (Some(2), false, Some(3))
        );
        let ended =
            page(json!({"ai_profiles": [{}, {}], "pagination": {"total_items": 2}})).expect("page");
        assert_eq!((ended.next_offset, ended.complete), (None, true));
        // A required inventory without its array fails closed; an optional
        // one (applications, API keys, the DLP catalog) is empty (v1).
        assert!(page(json!({})).is_err());
        let optional = normalize(
            ManagementResource::ApiKeys,
            0,
            2,
            json!({}),
            Paging::Offset {
                key: "api_keys",
                required: false,
            },
        )
        .expect("page");
        assert_eq!((optional.count, optional.complete), (0, true));
        assert!(
            normalize(
                ManagementResource::DeploymentProfiles,
                0,
                2,
                json!({"status": "ok"}),
                DEPLOYMENTS
            )
            .is_err()
        );
    }

    #[test]
    fn local_and_spring_pages_slice_and_report_totals() {
        let all = json!({"deployment_profiles": [{"dp_name": "a"}, {"dp_name": "b"}, {"dp_name": "c"}], "status": "ok"});
        let first = normalize(
            ManagementResource::DeploymentProfiles,
            0,
            2,
            all.clone(),
            DEPLOYMENTS,
        )
        .expect("page");
        assert_eq!(
            (first.count, first.next_offset, first.complete, first.total),
            (2, Some(2), false, Some(3))
        );
        let second = normalize(
            ManagementResource::DeploymentProfiles,
            2,
            2,
            all,
            DEPLOYMENTS,
        )
        .expect("page");
        assert_eq!((second.count, second.complete), (1, true));
        let spring = normalize(
            ManagementResource::DataPatterns,
            0,
            2,
            json!({"content": [{"id": "p"}], "last": false, "totalElements": 9}),
            Paging::Spring,
        )
        .expect("page");
        assert_eq!(
            (spring.next_offset, spring.complete, spring.total),
            (Some(2), false, Some(9))
        );
        // `totalPages` without `last` (v1).
        let paged = normalize(
            ManagementResource::DataPatterns,
            2,
            2,
            json!({"content": [{"id": "p"}, {"id": "q"}], "totalPages": 2}),
            Paging::Spring,
        )
        .expect("page");
        assert_eq!((paged.next_offset, paged.complete), (None, true));
        assert!(
            normalize(
                ManagementResource::DataPatterns,
                0,
                2,
                json!({"items": []}),
                Paging::Spring
            )
            .is_err()
        );
    }

    #[test]
    fn secret_fields_never_leave_and_unknown_values_stay() {
        let page = normalize(
            ManagementResource::ApiKeys,
            0,
            5,
            json!({"api_keys": [{"api_key_id": "k1", "api_key": "SECRET-VALUE", "auth_code": "AUTH-CODE",
                                 "status": "SOME_FUTURE_STATUS", "nested": {"client_secret": "x", "kept": 1},
                                 "brand_new_field": {"x": 1}}]}),
            Paging::Offset { key: "api_keys", required: false },
        )
        .expect("page");
        assert!(
            !page.items.contains("SECRET-VALUE")
                && !page.items.contains("AUTH-CODE")
                && !page.items.contains("client_secret")
        );
        let item = &items(&page)[0];
        assert_eq!(item["status"], "SOME_FUTURE_STATUS");
        assert_eq!(item["brand_new_field"]["x"], 1);
        assert_eq!(item["nested"]["kept"], 1);
    }

    #[test]
    fn requests_are_bounded() {
        let page = |resource, offset, limit| {
            ManagementRequest::Page {
                resource,
                offset,
                limit,
            }
            .is_valid()
        };
        assert!(page(ManagementResource::Profiles, 25, 25));
        assert!(!page(ManagementResource::Profiles, 0, 0));
        assert!(!page(ManagementResource::Profiles, 0, 101));
        assert!(page(ManagementResource::DataPatterns, 50, 25));
        assert!(
            !page(ManagementResource::DataPatterns, 10, 25),
            "DLP pages start on a page boundary"
        );
        assert!(
            !ManagementRequest::Detail {
                resource: ManagementResource::Topics,
                id: String::new()
            }
            .is_valid()
        );
        assert!(tsg_is_valid("1234567890") && !tsg_is_valid("12a"));
        assert!(
            client_id_is_valid("svc@1234.iam.panserviceaccount.com") && !client_id_is_valid(" x")
        );
        assert_eq!(
            ManagementEndpoints::default().token_origin().as_deref(),
            Some("https://auth.apps.paloaltonetworks.com")
        );
    }

    #[test]
    fn identities_match_strings_and_numbers_only() {
        assert_eq!(identity(Some(&json!("p-1"))).as_deref(), Some("p-1"));
        assert_eq!(identity(Some(&json!(42))).as_deref(), Some("42"));
        assert_eq!(identity(Some(&json!(null))), None);
        assert_eq!(identity(Some(&json!({"id": "x"}))), None);
        assert_eq!(identity(None), None);
    }
}
