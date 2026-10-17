//! Tenant profiles: nonsecret identity and approved scan destination.
//!
//! A tenant profile carries a native-issued ID, an operator label, an optional
//! TSG identifier and the HTTPS origin its scan credential may be sent to. It
//! never carries a secret; credentials are separate records bound to the
//! profile's approved destination (see `vault`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Longest operator label, in characters.
pub const MAX_LABEL_CHARS: usize = 64;
/// Longest TSG identifier, in digits.
pub const MAX_TSG_DIGITS: usize = 32;
/// Longest custom origin, in bytes.
pub const MAX_ORIGIN_BYTES: usize = 2048;

/// Native-issued tenant identifier (`tn-` + 16 hex digits). Deserialization
/// accepts only that shape, so arbitrary text is never treated as an ID.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, TS)]
#[serde(transparent)]
pub struct TenantId(String);

impl<'de> Deserialize<'de> for TenantId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("not a tenant id"))
    }
}

impl TenantId {
    pub(crate) fn issue() -> Option<Self> {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).ok()?;
        Some(Self(format!("tn-{}", hex::encode(entropy))))
    }

    /// Accepts only the native-issued shape; other text is never an ID.
    pub fn parse(text: &str) -> Option<Self> {
        let hex_part = text.strip_prefix("tn-")?;
        (hex_part.len() == 16
            && hex_part
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// AIRS scan regions offered by the SDK (`scm_sdk` `Region::endpoint`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Region {
    UnitedStates,
    Europe,
    India,
    Singapore,
}

impl Region {
    pub fn origin(self) -> &'static str {
        match self {
            Region::UnitedStates => "https://service.api.aisecurity.paloaltonetworks.com",
            Region::Europe => "https://service-de.api.aisecurity.paloaltonetworks.com",
            Region::India => "https://service-in.api.aisecurity.paloaltonetworks.com",
            Region::Singapore => "https://service-sg.api.aisecurity.paloaltonetworks.com",
        }
    }
}

/// Where a tenant's scan requests (and therefore its scan key) may go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Destination {
    Region {
        region: Region,
    },
    /// An operator-reviewed custom HTTPS origin (`https://host[:port]`).
    Custom {
        origin: String,
    },
}

impl Destination {
    /// The exact origin credentials are bound to.
    pub fn origin(&self) -> &str {
        match self {
            Destination::Region { region } => region.origin(),
            Destination::Custom { origin } => origin,
        }
    }

    /// Validates a draft and normalizes a custom origin to its ASCII origin.
    pub fn validate(self) -> Option<Self> {
        match self {
            Destination::Region { .. } => Some(self),
            Destination::Custom { origin } => {
                normalize_origin(&origin).map(|origin| Destination::Custom { origin })
            }
        }
    }
}

/// HTTPS origin only: host required; no userinfo, path, query or fragment.
pub fn normalize_origin(text: &str) -> Option<String> {
    if text.is_empty() || text.len() > MAX_ORIGIN_BYTES || text.trim() != text {
        return None;
    }
    let url = url::Url::parse(text).ok()?;
    let plain = url.scheme() == "https"
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && (url.path().is_empty() || url.path() == "/");
    if !plain {
        return None;
    }
    let origin = url.origin().ascii_serialization();
    origin.starts_with("https://").then_some(origin)
}

/// The nonsecret profile fields an operator supplies or imports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileDraft {
    pub label: String,
    pub tsg_id: Option<String>,
    pub destination: Destination,
}

impl ProfileDraft {
    /// Validates and normalizes the draft; `None` when any field is invalid.
    pub fn validate(self) -> Option<Self> {
        let label = self.label.trim().to_owned();
        let label_ok = !label.is_empty()
            && label.chars().count() <= MAX_LABEL_CHARS
            && !label.chars().any(char::is_control);
        let tsg_id = match self.tsg_id.map(|t| t.trim().to_owned()) {
            None => None,
            Some(t) if t.is_empty() => None,
            Some(t) if t.len() <= MAX_TSG_DIGITS && t.bytes().all(|b| b.is_ascii_digit()) => {
                Some(t)
            }
            Some(_) => return None,
        };
        let destination = self.destination.validate()?;
        label_ok.then_some(Self {
            label,
            tsg_id,
            destination,
        })
    }
}

/// A stored tenant profile (nonsecret).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantProfile {
    pub id: TenantId,
    pub label: String,
    pub tsg_id: Option<String>,
    pub destination: Destination,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins_are_https_hosts_without_extra_parts() {
        assert_eq!(
            normalize_origin("https://Scan.Example.com:8443/").as_deref(),
            Some("https://scan.example.com:8443")
        );
        assert_eq!(
            normalize_origin("https://scan.example.com:443").as_deref(),
            Some("https://scan.example.com")
        );
        for bad in [
            "http://scan.example.com",
            "https://user:pw@scan.example.com",
            "https://scan.example.com/v1",
            "https://scan.example.com?x=1",
            "https://scan.example.com#f",
            " https://scan.example.com",
            "file:///etc/passwd",
            "https://",
            "",
        ] {
            assert_eq!(normalize_origin(bad), None, "{bad}");
        }
    }

    #[test]
    fn drafts_are_trimmed_and_bounded() {
        let draft = |label: &str, tsg: Option<&str>| ProfileDraft {
            label: label.to_owned(),
            tsg_id: tsg.map(str::to_owned),
            destination: Destination::Region {
                region: Region::Europe,
            },
        };
        let ok = draft("  Prod EU ", Some(" 1234567890 "))
            .validate()
            .expect("valid");
        assert_eq!(
            (ok.label.as_str(), ok.tsg_id.as_deref()),
            ("Prod EU", Some("1234567890"))
        );
        assert_eq!(draft("x", Some("")).validate().expect("valid").tsg_id, None);
        assert!(draft("", None).validate().is_none());
        assert!(draft(&"x".repeat(65), None).validate().is_none());
        assert!(draft("tab\there", None).validate().is_none());
        assert!(draft("ok", Some("12a")).validate().is_none());
    }

    #[test]
    fn tenant_ids_have_the_issued_shape_only() {
        let id = TenantId::issue().expect("entropy");
        assert_eq!(TenantId::parse(id.as_str()), Some(id));
        for bad in [
            "tn-",
            "tn-ABCDEF0123456789",
            "tn-0123456789abcdef0",
            "x-0123456789abcdef",
        ] {
            assert!(TenantId::parse(bad).is_none(), "{bad}");
        }
    }
}
