//! Native admission of a rendered request (C15, ADR 0004), decided before
//! any secret is read. The credential class comes from the references the
//! request would actually use, never from a collection or request identity:
//!
//! - anonymous: no secret; workbench: only workbench Keychain secrets;
//!   managed: a native tenant credential (`tenant.client_id`,
//!   `tenant.client_secret`, `tenant.api_key`). Mixing classes is refused.
//! - A managed credential may only go where it belongs: the OAuth client
//!   pair only into an `oauth2` grant whose token URL is an admitted token
//!   destination of the active tenant's management capability; the scan
//!   key only as the `x-pan-token` header to the tenant's scan origin.
//!   Managed requests use HTTPS and admitted API origins only.
//! - A managed request must be a known built-in catalogue operation: known
//!   reads are admitted, known mutations need an exact review, and any
//!   other method or path is denied, whatever it is presented as.
//! - Every destination passes the build's network mode (fixture builds:
//!   loopback only).
//! - Registered Prisma origins (the configured management, token and scan
//!   origins, known with or without an active tenant) are classified
//!   whatever the credential: a request there with a manual or workbench
//!   credential, or none, is still a known operation (mutations reviewed)
//!   or denied, and generic redirects never enter them.
//! - Managed and registered-origin requests carry no URL user-info and no
//!   Host or forwarding override in any header they would send, the editor's
//!   or the one their authentication generates. At a registered origin a
//!   request also carries no method override and at most one credential
//!   (one Authorization, Proxy-Authorization, Cookie or x-pan-token header,
//!   or an API key in the query).

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::sync::OnceLock;

use serde::Serialize;
use ts_rs::TS;

use super::render::{Auth, Piece, Plan, RenderProblem, Template};
use super::scm;

pub const CLIENT_ID: &str = "tenant.client_id";
pub const CLIENT_SECRET: &str = "tenant.client_secret";
pub const API_KEY: &str = "tenant.api_key";
/// The active tenant's TSG (not a secret; filled in as text).
pub const TSG_ID: &str = "tenant.tsg_id";
const MANAGED: [&str; 4] = [CLIENT_ID, CLIENT_SECRET, API_KEY, TSG_ID];
/// The legacy token path the built-in catalogue uses, at the token origin.
pub const CATALOGUE_TOKEN_PATH: &str = "/auth/v1/oauth2/access_token";
/// Headers whose presence makes a managed request's credential or method
/// ambiguous.
const AMBIGUOUS: [&str; 7] = [
    CREDENTIAL_HEADERS[0],
    CREDENTIAL_HEADERS[1],
    CREDENTIAL_HEADERS[2],
    CREDENTIAL_HEADERS[3],
    METHOD_OVERRIDES[0],
    METHOD_OVERRIDES[1],
    METHOD_OVERRIDES[2],
];

/// Headers that carry a credential.
const CREDENTIAL_HEADERS: [&str; 4] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-pan-token",
];

/// Headers that ask a server to treat the request as another method.
const METHOD_OVERRIDES: [&str; 3] = [
    "x-http-method-override",
    "x-http-method",
    "x-method-override",
];

/// Headers that would redirect a request to another virtual host behind
/// the admitted authority.
const HOST_OVERRIDES: [&str; 5] = [
    "host",
    "forwarded",
    "x-forwarded-host",
    "x-original-url",
    "x-rewrite-url",
];

/// Where API Client requests may go in this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Network {
    /// Any http(s) destination (production builds).
    Live,
    /// Loopback only, no proxy (fixture builds): literal loopback
    /// addresses, or `localhost`, which the client pins to 127.0.0.1 (no
    /// name is ever looked up).
    LoopbackOnly,
    /// Nothing is sent.
    Disabled,
}

impl Network {
    pub fn admits(self, url: &url::Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        match self {
            Self::Disabled => false,
            Self::Live => url.host().is_some(),
            Self::LoopbackOnly => match url.host() {
                Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
                Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
                Some(url::Host::Domain(name)) => name == "localhost",
                None => false,
            },
        }
    }
}

/// The credential class a request was admitted under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CredentialClass {
    Anonymous,
    Workbench,
    Managed,
}

/// Why a Send was refused (nothing was sent).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SendRefusal {
    Render {
        problem: RenderProblem,
    },
    /// Not an absolute http(s) URL, or a secret in its origin.
    Url,
    /// This build does not send to that destination.
    Network,
    /// Workbench secrets and tenant credentials in one request.
    MixedCredentials,
    /// A `tenant.*` reference that names no native tenant credential.
    UnknownManagedReference {
        reference: String,
    },
    /// A tenant credential somewhere it may not go.
    ManagedPlacement,
    /// Operator headers that would override or obscure a tenant
    /// credential or the method.
    AmbiguousCredential,
    /// No active tenant with the credential this request needs.
    NoTenant,
    /// The token URL is not the tenant credential's token destination.
    TokenDestination,
    /// The request URL is not where the tenant credential may go.
    RequestDestination,
    /// Tenant credentials go only over HTTPS.
    NotHttps,
    /// A tenant-credential request that is no known catalogue operation.
    UnknownEndpoint,
    /// A secret has no usable value on this Mac.
    Secret {
        reference: String,
        failure: SecretFailure,
    },
    Body {
        problem: BodyProblem,
    },
    /// Four requests are already running.
    Busy,
    /// The document changed since the snapshot was taken.
    Stale,
    /// Review outcomes for tenant-credential changes.
    ReviewMissing,
    ReviewStale,
    ReviewExpired,
    ReviewConsumed,
    Declined,
    /// The private API Client store is unavailable (nothing recorded).
    Private,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum SecretFailure {
    Missing,
    Locked,
    Denied,
    /// Empty, surrounding whitespace or a line break (v1).
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum BodyProblem {
    /// A file is missing, outside the collection or not a regular file.
    Missing,
    /// Over 64 MiB per file or 128 MiB in all.
    TooLarge,
    /// A stored-file body has unsaved edits (Save first).
    Unsaved,
}

/// Where the active tenant's credentials may go (built natively from the
/// configured endpoints and the tenant's stored capabilities).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Destinations {
    /// Exact token URLs the management client secret may be sent to.
    pub token_urls: Vec<String>,
    /// The token origin (the catalogue's auth host operations).
    pub token_origin: Option<String>,
    /// API origins management bearer tokens may be sent to.
    pub api_origins: BTreeSet<String>,
    /// The tenant's scan origin (the scan key's destination).
    pub scan_origin: Option<String>,
    /// The management capability is ready.
    pub management: bool,
    pub tsg: Option<String>,
    /// Every registered Prisma origin, independent of the active tenant.
    pub registered: Registered,
}

/// The registered Prisma origins and the catalogue classes each serves,
/// keyed by host and port (whatever the scheme, so plain HTTP to a
/// registered host is classified too).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Registered(std::collections::BTreeMap<String, BTreeSet<OriginClass>>);

impl Registered {
    fn key(url: &url::Url) -> Option<String> {
        Some(format!(
            "{}:{}",
            url.host_str()?.to_ascii_lowercase(),
            url.port_or_known_default()?
        ))
    }

    /// Registers `base`'s origin as serving `class`.
    pub fn add(&mut self, base: &str, class: OriginClass) {
        if let Some(key) = url::Url::parse(base).ok().as_ref().and_then(Self::key) {
            self.0.entry(key).or_default().insert(class);
        }
    }

    /// The classes `url`'s origin serves, if it is registered.
    pub fn classes(&self, url: &url::Url) -> Option<&BTreeSet<OriginClass>> {
        Self::key(url).and_then(|k| self.0.get(&k))
    }

    pub fn contains(&self, url: &url::Url) -> bool {
        self.classes(url).is_some()
    }
}

/// A known catalogue operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub index: usize,
    pub name: String,
    pub method: String,
    pub mutation: bool,
}

/// What admission decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Admitted {
    pub class: CredentialClass,
    /// The request URL as admitted (query rows not yet appended).
    pub url: url::Url,
    /// For a managed request: its catalogue operation.
    pub operation: Option<Operation>,
}

impl Admitted {
    pub fn needs_review(&self) -> bool {
        self.operation.as_ref().is_some_and(|o| o.mutation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OriginClass {
    Api,
    Scan,
    Auth,
}

struct Pattern {
    index: usize,
    name: String,
    method: String,
    class: OriginClass,
    /// `None`: one non-empty segment (a `{{variable}}`).
    segments: Vec<Option<String>>,
}

fn patterns() -> &'static [Pattern] {
    static BUILT: OnceLock<Vec<Pattern>> = OnceLock::new();
    BUILT.get_or_init(|| {
        let catalog = scm::catalog();
        catalog
            .requests
            .iter()
            .enumerate()
            .filter_map(|(index, e)| {
                let rest = e.url.strip_prefix("{{")?;
                let (base, suffix) = rest.split_once("}}")?;
                let class = match base {
                    "auth_host" => OriginClass::Auth,
                    "airs_scan_base" => OriginClass::Scan,
                    _ => OriginClass::Api,
                };
                let prefix = url::Url::parse(catalog.variables.get(base)?).ok()?;
                let mut segments: Vec<Option<String>> = prefix
                    .path()
                    .split('/')
                    .filter(|s| !s.is_empty())
                    .map(|s| Some(s.to_owned()))
                    .collect();
                segments.extend(
                    suffix
                        .split('/')
                        .filter(|s| !s.is_empty())
                        .map(|s| (!s.contains("{{")).then(|| s.to_owned())),
                );
                Some(Pattern {
                    index,
                    name: format!("{}/{}", e.folder, e.name),
                    method: e.method.clone(),
                    class,
                    segments,
                })
            })
            .collect()
    })
}

/// The number of catalogue operations the matcher knows (all 193).
pub fn known_operations() -> usize {
    patterns().len()
}

/// A path with no encoded separators or dots, no empty, dot or parameter
/// segments: one spelling per operation.
fn plain_path(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    if ["%2e", "%2f", "%5c", "\\", ";"]
        .iter()
        .any(|bad| lower.contains(bad))
    {
        return false;
    }
    let Some((_, after)) = lower.split_once("://") else {
        return false;
    };
    let after = after.split(['?', '#']).next().unwrap_or_default();
    let Some((_, path)) = after.split_once('/') else {
        return true;
    };
    path.split('/').all(|s| !matches!(s, "" | "." | ".."))
}

fn classify(method: &str, class: OriginClass, path: &str) -> Option<Operation> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    patterns()
        .iter()
        .filter(|p| p.method == method && p.class == class && p.segments.len() == segments.len())
        .find(|p| {
            p.segments
                .iter()
                .zip(&segments)
                .all(|(want, got)| want.as_deref().is_none_or(|w| w == *got))
        })
        .map(|p| Operation {
            index: p.index,
            name: p.name.clone(),
            method: p.method.clone(),
            mutation: p.method != "GET",
        })
}

/// The URL with every secret replaced by a placeholder (for parsing only)
/// and whether its origin is free of secrets.
fn parse_url(url: &Template) -> Result<url::Url, SendRefusal> {
    let mut text = String::new();
    let mut origin_plain = None;
    for piece in &url.0 {
        match piece {
            Piece::Text(t) => text.push_str(t),
            Piece::Secret(_) => {
                origin_plain.get_or_insert(text.clone());
                text.push_str("secret");
            }
        }
    }
    let parsed = url::Url::parse(&text).map_err(|_| SendRefusal::Url)?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        return Err(SendRefusal::Url);
    }
    if let Some(prefix) = origin_plain {
        // Everything up to the path must be written out, never a secret.
        let after = prefix.split_once("://").map_or("", |(_, r)| r);
        if !after.contains(['/', '?', '#']) {
            return Err(SendRefusal::Url);
        }
    }
    Ok(parsed)
}

fn origin(url: &url::Url) -> String {
    url.origin().ascii_serialization()
}

/// The names (lower-cased) of every header the request would send: the
/// editor's and the one its authentication generates.
fn effective_headers(plan: &Plan) -> Vec<String> {
    let mut names: Vec<String> = plan
        .headers
        .iter()
        .map(|(n, _)| n.to_ascii_lowercase())
        .collect();
    match &plan.auth {
        Auth::Basic { .. } | Auth::Bearer { .. } | Auth::OAuth2 { .. } => {
            names.push("authorization".to_owned());
        }
        Auth::ApiKey {
            name, query: false, ..
        } => names.push(name.to_ascii_lowercase()),
        Auth::ApiKey { query: true, .. } | Auth::None => {}
    }
    names
}

/// URL user-info or a Host/forwarding header, the editor's or a generated
/// one: another credential or another virtual host than the admitted
/// authority.
fn ambiguous_authority(plan: &Plan, url: &url::Url) -> bool {
    !url.username().is_empty()
        || url.password().is_some()
        || effective_headers(plan)
            .iter()
            .any(|n| HOST_OVERRIDES.contains(&n.as_str()))
}

/// At a registered origin: an ambiguous authority, a method override, or
/// more than one credential (headers, the editor's or generated, and an API
/// key in the query), so the method and account the review names are the
/// ones the service sees.
fn ambiguous_at_registered(plan: &Plan, url: &url::Url) -> bool {
    let names = effective_headers(plan);
    let credentials = names
        .iter()
        .filter(|n| CREDENTIAL_HEADERS.contains(&n.as_str()))
        .count()
        + usize::from(matches!(plan.auth, Auth::ApiKey { query: true, .. }));
    ambiguous_authority(plan, url)
        || names.iter().any(|n| METHOD_OVERRIDES.contains(&n.as_str()))
        || credentials > 1
}

/// The catalogue operation `plan` is at a registered origin serving
/// `classes` (one plain spelling of a known method and path), or denial.
fn registered_operation(
    plan: &Plan,
    url: &url::Url,
    classes: &BTreeSet<OriginClass>,
) -> Result<Operation, SendRefusal> {
    if !plan.url.plain().is_some_and(|t| plain_path(&t)) {
        return Err(SendRefusal::UnknownEndpoint);
    }
    classes
        .iter()
        .find_map(|class| classify(&plan.method, *class, url.path()))
        .ok_or(SendRefusal::UnknownEndpoint)
}

/// Fills the tenant's TSG (plain text) into every `tenant.tsg_id` slot.
pub fn fill_tsg(plan: &mut Plan, tsg: &str) {
    let fill = |t: &mut Template| {
        let mut out = Template::default();
        for piece in std::mem::take(&mut t.0) {
            match piece {
                Piece::Secret(r) if r == TSG_ID => match out.0.last_mut() {
                    Some(Piece::Text(last)) => last.push_str(tsg),
                    _ => out.0.push(Piece::Text(tsg.to_owned())),
                },
                Piece::Text(text) => match out.0.last_mut() {
                    Some(Piece::Text(last)) => last.push_str(&text),
                    _ => out.0.push(Piece::Text(text)),
                },
                other => out.0.push(other),
            }
        }
        *t = out;
    };
    fill(&mut plan.url);
    for (n, v) in &mut plan.query {
        fill(n);
        fill(v);
    }
    for (_, v) in &mut plan.headers {
        fill(v);
    }
    match &mut plan.body {
        super::render::Body::Text { text, .. } => fill(text),
        super::render::Body::Form(rows) => {
            for (n, v) in rows {
                fill(n);
                fill(v);
            }
        }
        super::render::Body::Multipart(parts) => {
            for p in parts {
                if let super::render::PartValue::Text(t) = &mut p.value {
                    fill(t);
                }
            }
        }
        _ => {}
    }
    match &mut plan.auth {
        Auth::Basic { username, .. } => fill(username),
        Auth::OAuth2 {
            token_url,
            client_id,
            scope,
            ..
        } => {
            fill(token_url);
            fill(client_id);
            fill(scope);
        }
        _ => {}
    }
}

/// Whether `t` holds exactly one piece: that secret.
fn only(t: &Template, reference: &str) -> bool {
    matches!(t.0.as_slice(), [Piece::Secret(r)] if r == reference)
}

/// Admits `plan` (its `tenant.tsg_id` slots already filled) for `network`
/// and the active tenant's `destinations`, before any secret is read.
pub fn admit(
    plan: &Plan,
    network: Network,
    destinations: &Destinations,
) -> Result<Admitted, SendRefusal> {
    let references = plan.references();
    if let Some(unknown) = references
        .iter()
        .find(|r| r.starts_with(scm::MANAGED_PREFIX) && !MANAGED.contains(&r.as_str()))
    {
        return Err(SendRefusal::UnknownManagedReference {
            reference: unknown.clone(),
        });
    }
    if references.contains(TSG_ID) {
        // Filled before admission when a tenant is active.
        return Err(SendRefusal::NoTenant);
    }
    let managed: BTreeSet<&str> = references
        .iter()
        .map(String::as_str)
        .filter(|r| r.starts_with(scm::MANAGED_PREFIX))
        .collect();
    let workbench = references.len() > managed.len();
    if !managed.is_empty() && workbench {
        return Err(SendRefusal::MixedCredentials);
    }
    let url = parse_url(&plan.url)?;
    if !network.admits(&url) {
        return Err(SendRefusal::Network);
    }
    if managed.is_empty() {
        // A registered Prisma origin is classified whatever the credential
        // (a literal header, a collection secret or none).
        let operation = match destinations.registered.classes(&url) {
            Some(classes) => {
                if ambiguous_at_registered(plan, &url) {
                    return Err(SendRefusal::AmbiguousCredential);
                }
                Some(registered_operation(plan, &url, classes)?)
            }
            None => None,
        };
        return Ok(Admitted {
            class: if workbench {
                CredentialClass::Workbench
            } else {
                CredentialClass::Anonymous
            },
            url,
            operation,
        });
    }
    // ---- a tenant credential: placement, destinations, operation ----
    let oauth = matches!(&plan.auth, Auth::OAuth2 { secret, client_id, .. }
        if secret == CLIENT_SECRET && client_id.secrets().all(|r| r == CLIENT_ID));
    let key_header = plan
        .headers
        .iter()
        .filter(|(n, v)| n.eq_ignore_ascii_case("x-pan-token") && only(v, API_KEY))
        .count();
    let key_auth = matches!(&plan.auth, Auth::ApiKey { name, query: false, secret }
        if name.eq_ignore_ascii_case("x-pan-token") && secret == API_KEY);
    let scan = key_header + usize::from(key_auth) == 1;
    // Every slot holding a tenant credential must be one of those places.
    let mut placed = BTreeSet::new();
    if oauth {
        placed.insert(CLIENT_SECRET);
        if let Auth::OAuth2 { client_id, .. } = &plan.auth
            && client_id.secrets().next().is_some()
        {
            placed.insert(CLIENT_ID);
        }
    }
    if scan {
        placed.insert(API_KEY);
    }
    let stray = |t: &Template| t.secrets().any(|r| r.starts_with(scm::MANAGED_PREFIX));
    let mut elsewhere = stray(&plan.url)
        || plan.query.iter().any(|(n, v)| stray(n) || stray(v))
        || plan
            .headers
            .iter()
            .any(|(n, v)| stray(v) && !(n.eq_ignore_ascii_case("x-pan-token") && only(v, API_KEY)));
    elsewhere |= match &plan.body {
        super::render::Body::Text { text, .. } => stray(text),
        super::render::Body::Form(rows) => rows.iter().any(|(n, v)| stray(n) || stray(v)),
        super::render::Body::Multipart(parts) => parts
            .iter()
            .any(|p| matches!(&p.value, super::render::PartValue::Text(t) if stray(t))),
        _ => false,
    };
    elsewhere |= match &plan.auth {
        Auth::Basic { username, secret } => {
            stray(username) || secret.starts_with(scm::MANAGED_PREFIX)
        }
        Auth::Bearer { secret } => secret.starts_with(scm::MANAGED_PREFIX),
        Auth::ApiKey { secret, .. } => secret.starts_with(scm::MANAGED_PREFIX) && !key_auth,
        Auth::OAuth2 {
            token_url,
            scope,
            client_id,
            secret,
        } => {
            stray(token_url)
                || stray(scope)
                || (!oauth && (secret.starts_with(scm::MANAGED_PREFIX) || stray(client_id)))
        }
        Auth::None => false,
    };
    if elsewhere || managed.iter().any(|r| !placed.contains(r)) || (oauth && scan) {
        return Err(SendRefusal::ManagedPlacement);
    }
    // Nothing may override or obscure the credential or the method.
    let ambiguous = plan.headers.iter().any(|(n, v)| {
        AMBIGUOUS.contains(&n.to_ascii_lowercase().as_str())
            && !(scan && n.eq_ignore_ascii_case("x-pan-token") && only(v, API_KEY))
    });
    if ambiguous || ambiguous_authority(plan, &url) {
        return Err(SendRefusal::AmbiguousCredential);
    }
    if url.scheme() != "https" {
        return Err(SendRefusal::NotHttps);
    }
    let request_origin = origin(&url);
    let class = if oauth {
        if !destinations.management {
            return Err(SendRefusal::NoTenant);
        }
        let Auth::OAuth2 { token_url, .. } = &plan.auth else {
            return Err(SendRefusal::ManagedPlacement);
        };
        let token = token_url
            .plain()
            .and_then(|t| url::Url::parse(&t).ok())
            .ok_or(SendRefusal::TokenDestination)?;
        if token.scheme() != "https"
            || !network.admits(&token)
            || !destinations
                .token_urls
                .iter()
                .any(|admitted| url::Url::parse(admitted).ok().as_ref() == Some(&token))
        {
            return Err(SendRefusal::TokenDestination);
        }
        if destinations.api_origins.contains(&request_origin) {
            OriginClass::Api
        } else if destinations.token_origin.as_deref() == Some(request_origin.as_str()) {
            OriginClass::Auth
        } else {
            return Err(SendRefusal::RequestDestination);
        }
    } else {
        match &destinations.scan_origin {
            None => return Err(SendRefusal::NoTenant),
            Some(scan) if *scan == request_origin => OriginClass::Scan,
            Some(_) => return Err(SendRefusal::RequestDestination),
        }
    };
    // The URL as written (before normalization) must be one plain spelling.
    if !plan.url.plain().is_some_and(|t| plain_path(&t)) {
        return Err(SendRefusal::UnknownEndpoint);
    }
    let operation =
        classify(&plan.method, class, url.path()).ok_or(SendRefusal::UnknownEndpoint)?;
    Ok(Admitted {
        class: CredentialClass::Managed,
        url,
        operation: Some(operation),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::document::Fields;
    use crate::workbench::environments::Layers;
    use crate::workbench::render::render;

    const ORIGIN: &str = "https://127.0.0.1:9443";

    fn destinations() -> Destinations {
        let mut registered = Registered::default();
        for class in [OriginClass::Api, OriginClass::Auth] {
            registered.add(ORIGIN, class);
        }
        registered.add("https://127.0.0.2:9443", OriginClass::Scan);
        Destinations {
            registered,
            token_urls: vec![
                format!("{ORIGIN}/oauth2/access_token"),
                format!("{ORIGIN}{CATALOGUE_TOKEN_PATH}"),
            ],
            token_origin: Some(ORIGIN.into()),
            api_origins: [ORIGIN.to_owned()].into(),
            scan_origin: Some("https://127.0.0.2:9443".into()),
            management: true,
            tsg: Some("1234567890".into()),
        }
    }

    fn layers() -> Layers {
        Layers {
            collection: Some(format!(
                "format_version = 1\nid = \"c\"\n[variables]\nscm_base = \"{ORIGIN}\"\ntoken_url = \"{ORIGIN}/oauth2/access_token\"\nclient_id = {{ secret = \"tenant.client_id\" }}\nclient_secret = {{ secret = \"tenant.client_secret\" }}\ntsg_id = {{ secret = \"tenant.tsg_id\" }}\nkey = {{ secret = \"tenant.api_key\" }}\nmine = {{ secret = \"dev-token\" }}\n"
            )),
            ..Layers::default()
        }
    }

    const OAUTH: &str = "[auth]\nkind = \"oauth2\"\ntoken_url = \"{{token_url}}\"\nclient_id = \"{{client_id}}\"\nclient_secret = \"{{client_secret}}\"\nscope = \"tsg_id:{{tsg_id}}\"\n";

    fn admitted(
        method: &str,
        url: &str,
        headers: &str,
        settings: &str,
    ) -> Result<Admitted, SendRefusal> {
        let fields = Fields {
            method: method.into(),
            url: url.into(),
            headers: headers.into(),
            settings: settings.into(),
            ..Fields::default()
        };
        let mut plan =
            render(&fields, &layers(), None).map_err(|problem| SendRefusal::Render { problem })?;
        fill_tsg(&mut plan, "1234567890");
        admit(&plan, Network::LoopbackOnly, &destinations())
    }

    #[test]
    fn known_reads_pass_known_mutations_need_review_and_others_are_denied() {
        assert_eq!(known_operations(), 193);
        let read = admitted("GET", "{{scm_base}}/iam/v1/access_policies", "", OAUTH).expect("read");
        assert_eq!(read.class, CredentialClass::Managed);
        assert!(!read.needs_review());
        let delete = admitted(
            "DELETE",
            "{{scm_base}}/iam/v1/access_policies/abc",
            "",
            OAUTH,
        )
        .expect("mutation");
        assert!(delete.needs_review());
        // The same path presented as a read, an unknown path, HEAD.
        for (method, url) in [
            ("GET", "{{scm_base}}/iam/v1/access_policies/abc/extra"),
            ("GET", "{{scm_base}}/iam/v1/not-a-thing"),
            ("HEAD", "{{scm_base}}/iam/v1/access_policies"),
            ("PATCH", "{{scm_base}}/iam/v1/access_policies/abc"),
        ] {
            assert_eq!(
                admitted(method, url, "", OAUTH),
                Err(SendRefusal::UnknownEndpoint),
                "{method} {url}"
            );
        }
        // Ambiguous spellings of a known path.
        for url in [
            "{{scm_base}}/iam/v1/./access_policies",
            "{{scm_base}}/iam/v1/x/../access_policies",
            "{{scm_base}}/iam/v1/access%2Fpolicies",
            "{{scm_base}}/iam//v1/access_policies",
            "{{scm_base}}/iam/v1/access_policies;x=1",
        ] {
            assert_eq!(
                admitted("GET", url, "", OAUTH),
                Err(SendRefusal::UnknownEndpoint),
                "{url}"
            );
        }
    }

    #[test]
    fn tenant_credentials_go_only_to_their_destinations() {
        let other_token = OAUTH.replace("{{token_url}}", "https://127.0.0.1:9443/elsewhere");
        assert_eq!(
            admitted(
                "GET",
                "{{scm_base}}/iam/v1/access_policies",
                "",
                &other_token
            ),
            Err(SendRefusal::TokenDestination)
        );
        assert_eq!(
            admitted(
                "GET",
                "https://127.0.0.9:9443/iam/v1/access_policies",
                "",
                OAUTH
            ),
            Err(SendRefusal::RequestDestination)
        );
        assert_eq!(
            admitted(
                "GET",
                "http://127.0.0.1:9443/iam/v1/access_policies",
                "",
                OAUTH
            ),
            Err(SendRefusal::NotHttps)
        );
        // The client secret anywhere but the grant.
        assert_eq!(
            admitted(
                "GET",
                "{{scm_base}}/iam/v1/access_policies",
                "X-Leak: {{client_secret}}",
                ""
            ),
            Err(SendRefusal::ManagedPlacement)
        );
        assert_eq!(
            admitted(
                "GET",
                "{{scm_base}}/iam/v1/access_policies",
                "",
                "[auth]\nkind = \"bearer\"\nsecret = \"tenant.client_secret\"\n"
            ),
            Err(SendRefusal::ManagedPlacement)
        );
        // An operator Authorization header beside the grant.
        assert_eq!(
            admitted(
                "GET",
                "{{scm_base}}/iam/v1/access_policies",
                "Authorization: Bearer x",
                OAUTH
            ),
            Err(SendRefusal::AmbiguousCredential)
        );
        assert_eq!(
            admitted(
                "POST",
                "{{scm_base}}/iam/v1/access_policies",
                "X-HTTP-Method-Override: GET",
                OAUTH
            ),
            Err(SendRefusal::AmbiguousCredential)
        );
        // Workbench secrets never ride along.
        assert_eq!(
            admitted(
                "GET",
                "{{scm_base}}/iam/v1/access_policies",
                "X-Mine: {{mine}}",
                OAUTH
            ),
            Err(SendRefusal::MixedCredentials)
        );
        assert_eq!(
            admitted(
                "GET",
                "{{scm_base}}/x",
                "",
                "[auth]\nkind = \"bearer\"\nsecret = \"tenant.other\"\n"
            ),
            Err(SendRefusal::UnknownManagedReference {
                reference: "tenant.other".into()
            })
        );
        // The scan key: only x-pan-token to the scan origin.
        let scan = admitted(
            "POST",
            "https://127.0.0.2:9443/v1/scan/sync/request",
            "x-pan-token: {{key}}",
            "",
        )
        .expect("scan");
        assert!(scan.needs_review());
        assert_eq!(
            admitted(
                "POST",
                "{{scm_base}}/v1/scan/sync/request",
                "x-pan-token: {{key}}",
                ""
            ),
            Err(SendRefusal::RequestDestination)
        );
    }

    #[test]
    fn registered_origins_are_classified_whatever_the_credential() {
        let unmanaged = |method: &str, url: &str, headers: &str, settings: &str| {
            let r = admitted(method, url, headers, settings);
            let mut plain = destinations();
            plain.management = false;
            plain.token_urls.clear();
            plain.api_origins.clear();
            plain.scan_origin = None;
            // Without an active tenant the origins are still registered.
            let fields = Fields {
                method: method.into(),
                url: url.into(),
                headers: headers.into(),
                settings: settings.into(),
                ..Fields::default()
            };
            let plan = render(&fields, &layers(), None).expect("render");
            assert_eq!(
                admit(&plan, Network::LoopbackOnly, &plain),
                r,
                "{method} {url}"
            );
            r
        };
        // A manual Authorization header, a collection secret, none: a known
        // mutation needs review under its own class.
        for (headers, settings, class) in [
            (
                "Authorization: Bearer manual",
                "",
                CredentialClass::Anonymous,
            ),
            ("X-Mine: {{mine}}", "", CredentialClass::Workbench),
            (
                "",
                "[auth]\nkind = \"basic\"\nusername = \"u\"\nsecret = \"dev-token\"\n",
                CredentialClass::Workbench,
            ),
            ("", "", CredentialClass::Anonymous),
        ] {
            let a = unmanaged(
                "DELETE",
                "{{scm_base}}/iam/v1/access_policies/abc",
                headers,
                settings,
            )
            .expect("admitted");
            assert_eq!(a.class, class);
            assert!(a.needs_review(), "{headers} {settings}");
        }
        // Plain HTTP to a registered host and port is still that origin.
        assert!(
            unmanaged(
                "DELETE",
                "http://127.0.0.1:9443/iam/v1/access_policies/abc",
                "",
                ""
            )
            .expect("admitted")
            .needs_review()
        );
        let read = unmanaged("GET", "{{scm_base}}/iam/v1/access_policies", "", "").expect("read");
        assert!(read.operation.is_some() && !read.needs_review());
        for (method, url) in [
            ("GET", "{{scm_base}}/iam/v1/not-a-thing"),
            ("POST", "https://127.0.0.2:9443/v1/unknown"),
            ("GET", "{{scm_base}}/iam/v1/./access_policies"),
        ] {
            assert_eq!(
                unmanaged(method, url, "Authorization: Bearer manual", ""),
                Err(SendRefusal::UnknownEndpoint),
                "{method} {url}"
            );
        }
        // Elsewhere: ordinary operator-directed HTTP.
        let other = unmanaged(
            "DELETE",
            "https://127.0.0.1:9444/iam/v1/access_policies/abc",
            "",
            "",
        )
        .expect("unregistered");
        assert!(other.operation.is_none() && !other.needs_review());
    }

    #[test]
    fn host_overrides_and_url_user_info_are_refused() {
        for headers in [
            "Host: evil.example",
            "HOST: 127.0.0.1:9443",
            "X-Forwarded-Host: x",
            "Forwarded: host=x",
        ] {
            assert_eq!(
                admitted("GET", "{{scm_base}}/iam/v1/access_policies", headers, OAUTH),
                Err(SendRefusal::AmbiguousCredential),
                "{headers}"
            );
            assert_eq!(
                admitted("GET", "{{scm_base}}/iam/v1/access_policies", headers, ""),
                Err(SendRefusal::AmbiguousCredential),
                "registered, unmanaged: {headers}"
            );
        }
        for url in [
            "https://u:p@127.0.0.1:9443/iam/v1/access_policies",
            "https://u@127.0.0.1:9443/iam/v1/access_policies",
        ] {
            assert_eq!(
                admitted("GET", url, "", OAUTH),
                Err(SendRefusal::AmbiguousCredential),
                "{url}"
            );
            assert_eq!(
                admitted("GET", url, "", ""),
                Err(SendRefusal::AmbiguousCredential),
                "registered, unmanaged: {url}"
            );
        }
        // Unregistered generic HTTP keeps its user-info and headers.
        assert!(admitted("GET", "http://u:p@127.0.0.1:8080/x", "Host: other", "").is_ok());
    }

    #[test]
    fn generated_headers_overrides_and_extra_credentials_are_refused_at_registered_origins() {
        let read = "{{scm_base}}/iam/v1/access_policies";
        let key = |name: &str| {
            format!("[auth]\nkind = \"api-key\"\nname = \"{name}\"\nsecret = \"dev-token\"\n")
        };
        let basic = "[auth]\nkind = \"basic\"\nusername = \"u\"\nsecret = \"dev-token\"\n";
        let query_key = "[auth]\nkind = \"api-key\"\nname = \"key\"\nlocation = \"query\"\nsecret = \"dev-token\"\n";
        for (headers, settings) in [
            // A generated Host or forwarding header.
            ("", key("Host")),
            ("", key("X-Forwarded-Host")),
            // Method overrides, literal or generated.
            ("X-HTTP-Method-Override: DELETE", String::new()),
            ("x-method-override: DELETE", String::new()),
            ("", key("X-HTTP-Method")),
            // Two credentials, whatever their source.
            (
                "Authorization: Bearer one\nAuthorization: Bearer two",
                String::new(),
            ),
            ("Authorization: Bearer manual", basic.to_owned()),
            ("Cookie: sid=1", key("Authorization")),
            ("X-Pan-Token: literal", query_key.to_owned()),
            (
                "Authorization: Bearer manual\nProxy-Authorization: Basic x",
                String::new(),
            ),
        ] {
            assert_eq!(
                admitted("GET", read, headers, &settings),
                Err(SendRefusal::AmbiguousCredential),
                "{headers} {settings}"
            );
        }
        // A generated Host header is refused for a managed request too.
        assert_eq!(
            admitted(
                "GET",
                read,
                "",
                &key("Host").replace("dev-token", "tenant.api_key")
            ),
            Err(SendRefusal::ManagedPlacement),
            "a tenant key only as x-pan-token"
        );
        // One credential from any single source is kept.
        for (headers, settings) in [
            ("Authorization: Bearer manual", String::new()),
            ("", basic.to_owned()),
            ("", key("X-Api-Key")),
            ("", query_key.to_owned()),
            ("X-Pan-Token: literal", String::new()),
        ] {
            assert!(
                admitted("GET", read, headers, &settings).is_ok(),
                "{headers} {settings}"
            );
        }
        // Unregistered HTTP is the operator's own.
        assert!(
            admitted(
                "GET",
                "http://127.0.0.1:8080/x",
                "X-HTTP-Method-Override: DELETE\nAuthorization: Bearer a",
                basic
            )
            .is_ok()
        );
        assert!(admitted("GET", "http://127.0.0.1:8080/x", "", &key("Host")).is_ok());
    }

    #[test]
    fn ordinary_requests_keep_their_class_and_the_network_mode() {
        let anon = admitted("DELETE", "http://127.0.0.1:8080/anything", "", "").expect("anonymous");
        assert_eq!(anon.class, CredentialClass::Anonymous);
        let mine =
            admitted("GET", "http://127.0.0.1:8080/x", "X-Mine: {{mine}}", "").expect("workbench");
        assert_eq!(mine.class, CredentialClass::Workbench);
        assert_eq!(
            admitted("GET", "http://example.com/x", "", ""),
            Err(SendRefusal::Network)
        );
        assert_eq!(
            admitted("GET", "http://{{mine}}/x", "", ""),
            Err(SendRefusal::Url)
        );
        assert!(Network::Live.admits(&url::Url::parse("https://example.com/").expect("fixture")));
        let loopback =
            |u: &str| Network::LoopbackOnly.admits(&url::Url::parse(u).expect("fixture"));
        assert!(
            loopback("https://localhost:1/")
                && loopback("http://[::1]/")
                && loopback("http://127.0.0.9/")
        );
        assert!(
            !loopback("http://localhost.example/")
                && !loopback("http://10.0.0.1/")
                && !loopback("file:///x")
        );
        assert!(!Network::Disabled.admits(&url::Url::parse("http://127.0.0.1/").expect("fixture")));
    }
}
