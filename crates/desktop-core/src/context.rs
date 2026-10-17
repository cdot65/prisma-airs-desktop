//! Opaque native references and transient entered secrets.
//!
//! A [`ContextRef`] names one issued view of the active tenant and its
//! credential/endpoint generation. A fresh one is issued on every selection
//! and every generation change, from a monotonically increasing epoch, so an
//! old reference never becomes current again (A→B→A issues a third value).
//! References are lookup values compared by equality; they grant nothing.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroize;

fn bounded_ascii(text: &str, prefix: &str, max: usize) -> bool {
    text.len() <= max
        && text.starts_with(prefix)
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A native-issued context reference.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(transparent)]
pub struct ContextRef(String);

impl ContextRef {
    pub(crate) fn issue(salt: &[u8; 16], epoch: u64) -> Self {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"prisma-airs-desktop-v2/context");
        hasher.update(salt);
        hasher.update(epoch.to_be_bytes());
        Self(format!("cx-{}", hex::encode(&hasher.finalize()[..12])))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ContextRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        bounded_ascii(&text, "cx-", 64)
            .then_some(Self(text))
            .ok_or_else(|| serde::de::Error::custom("not a context reference"))
    }
}

/// A native-issued review reference (endpoint change or import preview).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(transparent)]
pub struct ReviewId(String);

impl ReviewId {
    pub(crate) fn issue() -> Option<Self> {
        let mut entropy = [0u8; 8];
        getrandom::fill(&mut entropy).ok()?;
        Some(Self(format!("rv-{}", hex::encode(entropy))))
    }
}

impl<'de> Deserialize<'de> for ReviewId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        bounded_ascii(&text, "rv-", 64)
            .then_some(Self(text))
            .ok_or_else(|| serde::de::Error::custom("not a review reference"))
    }
}

/// A secret typed by the operator, carried only for the intent that stores
/// it. Formatting is redacted and the text is zeroized when dropped.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct EnteredSecret(#[ts(type = "string")] String);

impl EnteredSecret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl PartialEq for EnteredSecret {
    fn eq(&self, other: &Self) -> bool {
        use sha2::{Digest, Sha256};
        Sha256::digest(self.0.as_bytes()) == Sha256::digest(other.0.as_bytes())
    }
}

impl Eq for EnteredSecret {}

impl std::fmt::Debug for EnteredSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EnteredSecret([REDACTED])")
    }
}

impl Drop for EnteredSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_are_unique_per_epoch_and_bounded_on_input() {
        let salt = [7u8; 16];
        assert_ne!(ContextRef::issue(&salt, 1), ContextRef::issue(&salt, 2));
        assert_eq!(ContextRef::issue(&salt, 3), ContextRef::issue(&salt, 3));
        assert!(serde_json::from_str::<ContextRef>("\"cx-abc\"").is_ok());
        assert!(serde_json::from_str::<ContextRef>(&format!("\"cx-{}\"", "a".repeat(80))).is_err());
        assert!(serde_json::from_str::<ContextRef>("\"cx-a b\"").is_err());
        assert!(serde_json::from_str::<ReviewId>("\"cx-abc\"").is_err());
    }

    #[test]
    fn entered_secrets_never_format_their_value() {
        let secret: EnteredSecret = serde_json::from_str("\"synthetic-key-123\"").expect("parse");
        assert_eq!(format!("{secret:?}"), "EnteredSecret([REDACTED])");
        assert_eq!(secret.expose(), "synthetic-key-123");
    }
}
