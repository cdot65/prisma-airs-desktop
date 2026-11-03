//! Workspace MCP servers for the Harness (C22, D049): the Gateway catalog
//! as the desktop reads it, each server's revision and engine namespace,
//! and a conversation's fixed server scope with the bearer tokens it is
//! bootstrapped with.
//!
//! The engine (9331a58) takes at most eight servers in its one bootstrap
//! frame, each a `{name, serverId, url, accessToken}`; it never signs in or
//! stores a token itself, calls each server over Streamable HTTP at its
//! Gateway-facing URL with the bearer held in memory, and asks the desktop
//! before every tool call. Grants and revisions are not part of the
//! engine's context, so the desktop checks them itself.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use ts_rs::TS;
use url::Url;
use zeroize::Zeroizing;

/// Servers in one conversation (the engine's bound).
pub const MAX_SERVERS: usize = 8;
/// Catalog pages read (zero-based, 100 each).
pub const MAX_PAGES: u32 = 5;
pub const PAGE_SIZE: u32 = 100;
/// The token a server without a current sign-in is bootstrapped with: its
/// place in the scope (and so the engine context) stays the same, and the
/// server fails to start, so its tools are unavailable while inference
/// goes on.
pub const SIGNED_OUT_TOKEN: &str = "airs-desktop-mcp-sign-in-expired";
/// The engine's bounds on an access token.
pub const TOKEN_MIN: usize = 16;
pub const TOKEN_MAX: usize = 16 * 1024;
const NAME_MAX: usize = 32;
const RESERVED: [&str; 2] = ["airs_desktop", "codex_apps"];
const SUFFIX_WIDTHS: [usize; 5] = [8, 12, 16, 20, 24];
const URL_MAX: usize = 2048;
const TEXT_MAX: usize = 256;

/// One server of the chosen workspace's catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CatalogServer {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// The Gateway-facing URL, canonical (never the upstream server's).
    pub url: String,
    pub status: String,
    /// What the desktop binds a sign-in and a conversation to.
    pub revision: String,
    /// The engine namespace (`mcp__{namespace}__{tool}`).
    pub namespace: String,
    /// The engine can take it (its ID fits the engine's form).
    pub supported: bool,
}

/// Whether `id` fits the engine's `serverId`.
pub fn supported_id(id: &str) -> bool {
    (1..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The URL as the engine stores it: HTTPS, a host, no credentials, query
/// or fragment, canonical.
pub fn canonical_url(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > URL_MAX || !text.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let url = Url::parse(text).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none())
    .then(|| url.to_string())
    .filter(|u| u.len() <= URL_MAX)
}

/// A server's revision: what changes when its identity, address or state
/// in the catalog does.
pub fn revision(id: &str, slug: &str, url: &str, status: &str, updated: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"prisma-airs-desktop\0workspace-mcp-revision/v1");
    for part in [id, slug, url, status, updated] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    hex::encode(h.finalize())
}

fn base_name(slug: &str) -> String {
    let mut name: String = slug
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
        name.insert_str(0, "m_");
    }
    name.truncate(NAME_MAX);
    name
}

/// Engine namespaces for `(id, slug)` pairs, in the same order: the slug
/// made a name; a name shared by several servers, or reserved, gets a
/// suffix from its server's ID for every member of the group, wide enough
/// to tell them apart, so the result does not depend on catalog order.
pub fn namespaces(servers: &[(String, String)]) -> Vec<String> {
    let bases: Vec<String> = servers.iter().map(|(_, slug)| base_name(slug)).collect();
    let mut names = bases.clone();
    for (i, base) in bases.iter().enumerate() {
        let group: Vec<usize> = (0..bases.len()).filter(|&j| &bases[j] == base).collect();
        if group.len() == 1 && !RESERVED.contains(&base.as_str()) {
            continue;
        }
        let digests: Vec<String> = group
            .iter()
            .map(|&j| hex::encode(Sha256::digest(servers[j].0.as_bytes())))
            .collect();
        let width = SUFFIX_WIDTHS
            .iter()
            .copied()
            .find(|&w| {
                let mut seen: Vec<&str> = digests.iter().map(|d| &d[..w]).collect();
                seen.sort_unstable();
                seen.windows(2).all(|p| p[0] != p[1])
            })
            .unwrap_or(24);
        let digest = hex::encode(Sha256::digest(servers[i].0.as_bytes()));
        let mut stem = base.clone();
        stem.truncate(NAME_MAX - 1 - width);
        names[i] = format!("{stem}_{}", &digest[..width]);
    }
    names
}

fn text(value: &Value, key: &str) -> Option<String> {
    value[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= TEXT_MAX && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

/// The catalog page's servers of `workspace` (others are dropped, as are
/// servers without a usable URL and repeated IDs); `None` when the reply
/// is not a catalog page. The upstream URL is never kept.
pub fn project(reply: &Value, workspace: &str) -> Option<Vec<(CatalogServer, String)>> {
    let rows = reply["data"].as_array()?;
    let mut out: Vec<(CatalogServer, String)> = Vec::new();
    for row in rows {
        if row["workspace_id"].as_str() != Some(workspace) {
            continue;
        }
        let (Some(id), Some(slug), Some(url)) = (
            text(row, "id"),
            text(row, "slug"),
            row["url"].as_str().and_then(canonical_url),
        ) else {
            continue;
        };
        if out.iter().any(|(s, _)| s.id == id) {
            continue;
        }
        let status = text(row, "status").unwrap_or_default();
        let updated = text(row, "last_updated_at").unwrap_or_default();
        out.push((
            CatalogServer {
                revision: revision(&id, &slug, &url, &status, &updated),
                name: text(row, "name").unwrap_or_else(|| slug.clone()),
                supported: supported_id(&id),
                namespace: String::new(),
                id,
                slug,
                url,
                status,
            },
            updated,
        ));
    }
    Some(out)
}

/// Gives each server its namespace (over the whole catalog).
pub fn name_all(servers: &mut [CatalogServer]) {
    let pairs: Vec<(String, String)> = servers
        .iter()
        .map(|s| (s.id.clone(), s.slug.clone()))
        .collect();
    for (s, n) in servers.iter_mut().zip(namespaces(&pairs)) {
        s.namespace = n;
    }
}

/// One server of a conversation's fixed scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ScopeServer {
    /// The engine namespace.
    pub name: String,
    pub server_id: String,
    pub url: String,
    pub revision: String,
    /// The sign-in it was granted under, if any (nonsecret).
    pub grant: Option<String>,
}

/// A scope's bootstrap entries with each server's token (or the signed-out
/// placeholder), in scope order.
pub fn bootstrap_entries(scope: &[ScopeServer], tokens: &[Option<Zeroizing<String>>]) -> Value {
    Value::Array(
        scope
            .iter()
            .zip(tokens)
            .map(|(s, t)| {
                let token = t
                    .as_ref()
                    .filter(|t| valid_token(t))
                    .map_or(SIGNED_OUT_TOKEN, |t| t.as_str());
                json!({"name": s.name, "serverId": s.server_id, "url": s.url, "accessToken": token})
            })
            .collect(),
    )
}

/// Whether the engine can take `token`.
pub fn valid_token(token: &str) -> bool {
    (TOKEN_MIN..=TOKEN_MAX).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}

/// What the running engine was given: a change means a restart.
pub fn fingerprint(scope: &[ScopeServer], tokens: &[Option<Zeroizing<String>>]) -> String {
    let mut h = Sha256::new();
    for (s, t) in scope.iter().zip(tokens) {
        h.update((s.server_id.len() as u64).to_be_bytes());
        h.update(s.server_id.as_bytes());
        let token = t.as_ref().map_or("", |t| t.as_str());
        h.update((token.len() as u64).to_be_bytes());
        h.update(token.as_bytes());
    }
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_https_canonical_and_never_carry_credentials() {
        assert_eq!(
            canonical_url("https://mcp.example/team/mcp").as_deref(),
            Some("https://mcp.example/team/mcp")
        );
        assert_eq!(canonical_url("https://h").as_deref(), Some("https://h/"));
        for bad in [
            "http://mcp.example/x",
            "https://user:pw@mcp.example/x",
            "https://mcp.example/x?token=1",
            "https://mcp.example/x#f",
            "not a url",
            "",
        ] {
            assert_eq!(canonical_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn namespaces_are_engine_names_and_independent_of_order() {
        let a = ("id-a".to_owned(), "Team Tools".to_owned());
        let b = ("id-b".to_owned(), "team-tools".to_owned());
        let c = ("id-c".to_owned(), "9lives".to_owned());
        let d = ("id-d".to_owned(), "airs_desktop".to_owned());
        let names = namespaces(&[a.clone(), b.clone(), c.clone(), d.clone()]);
        let again = namespaces(&[d, c, b, a]);
        assert_eq!(names[0], again[3]);
        assert_eq!(names[1], again[2]);
        assert!(
            names[0].starts_with("team_tools_") && names[0] != names[1],
            "{names:?}"
        );
        assert_eq!(names[2], "m_9lives");
        assert!(names[3].starts_with("airs_desktop_"));
        for n in &names {
            assert!(
                n.len() <= 32 && n.starts_with(|c: char| c.is_ascii_lowercase()),
                "{n}"
            );
            assert!(
                n.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            );
        }
    }

    #[test]
    fn the_catalog_keeps_only_the_workspaces_usable_servers() {
        let reply = json!({"data": [
            {"id": "s1", "slug": "tools", "name": "Tools", "url": "https://gw.example/tools/mcp",
             "status": "active", "last_updated_at": "t1", "workspace_id": "w1",
             "mcp_integration_url": "https://upstream.example/secret"},
            {"id": "s1", "slug": "dup", "url": "https://gw.example/dup/mcp", "workspace_id": "w1"},
            {"id": "s2", "slug": "other", "url": "https://gw.example/o/mcp", "workspace_id": "w2"},
            {"id": "s3", "slug": "plain", "url": "http://gw.example/p/mcp", "workspace_id": "w1"},
            {"id": "bad id!", "slug": "odd", "url": "https://gw.example/odd/mcp", "workspace_id": "w1"},
        ]});
        let servers = project(&reply, "w1").expect("page");
        let ids: Vec<&str> = servers.iter().map(|(s, _)| s.id.as_str()).collect();
        assert_eq!(ids, ["s1", "bad id!"]);
        assert!(servers[0].0.supported && !servers[1].0.supported);
        assert!(
            !serde_json::to_string(&servers[0].0)
                .expect("json")
                .contains("upstream")
        );
        // The revision follows identity, address, state and update time.
        let r = revision(
            "s1",
            "tools",
            "https://gw.example/tools/mcp",
            "active",
            "t1",
        );
        assert_eq!(servers[0].0.revision, r);
        assert_ne!(
            r,
            revision(
                "s1",
                "tools",
                "https://gw.example/tools/mcp",
                "active",
                "t2"
            )
        );
        assert_ne!(
            r,
            revision(
                "s1",
                "tools",
                "https://gw.example/tools/mcp2",
                "active",
                "t1"
            )
        );
        assert_eq!(project(&json!({"rows": []}), "w1"), None);
    }

    #[test]
    fn a_scope_is_bootstrapped_with_tokens_or_the_signed_out_placeholder() {
        let scope = vec![
            ScopeServer {
                name: "tools".into(),
                server_id: "s1".into(),
                url: "https://gw.example/tools/mcp".into(),
                revision: "r1".into(),
                grant: Some("g1".into()),
            },
            ScopeServer {
                name: "docs".into(),
                server_id: "s2".into(),
                url: "https://gw.example/docs/mcp".into(),
                revision: "r2".into(),
                grant: None,
            },
        ];
        let token = Zeroizing::new("a-bearer-token-of-enough-length".to_owned());
        let tokens = vec![Some(token.clone()), None];
        let entries = bootstrap_entries(&scope, &tokens);
        assert_eq!(entries[0]["accessToken"], token.as_str());
        assert_eq!(entries[1]["accessToken"], SIGNED_OUT_TOKEN);
        assert_eq!(entries[1]["name"], "docs");
        assert!(valid_token(SIGNED_OUT_TOKEN));
        // A short token is never sent: the server reads as signed out.
        let short = vec![Some(Zeroizing::new("short".to_owned())), None];
        assert_eq!(
            bootstrap_entries(&scope, &short)[0]["accessToken"],
            SIGNED_OUT_TOKEN
        );
        assert_ne!(fingerprint(&scope, &tokens), fingerprint(&scope, &short));
    }
}
