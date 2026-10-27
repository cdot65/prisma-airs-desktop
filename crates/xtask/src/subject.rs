//! Validated review subjects: chunks C01–C34 and phases P01–P09 (P09 = C27–C34, D058).

use std::fmt;

use crate::{Result, invalid};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Subject {
    Chunk(u8),
    Phase(u8),
}

impl Subject {
    /// The last registered chunk and phase (checks.json must define exactly these).
    pub const LAST_CHUNK: u8 = 34;
    pub const LAST_PHASE: u8 = 9;
    /// D058: the documentation-only chunks. Fixed in code so the policy (no
    /// native gate, no owner acceptance below 9/10) cannot be switched on for
    /// an application chunk or off for the specification from a manifest.
    pub const DOCS_ONLY_CHUNKS: &'static [u8] = &[27];

    /// Whether this subject is a documentation-only chunk (D058).
    pub fn is_docs_only(self) -> bool {
        matches!(self, Self::Chunk(n) if Self::DOCS_ONLY_CHUNKS.contains(&n))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let parse_num = |digits: &str, max: u8| -> Option<u8> {
            (digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()))
                .then(|| digits.parse::<u8>().ok())
                .flatten()
                .filter(|n| (1..=max).contains(n))
        };
        if let Some(n) = text
            .strip_prefix('C')
            .and_then(|rest| parse_num(rest, Self::LAST_CHUNK))
        {
            return Ok(Self::Chunk(n));
        }
        if let Some(n) = text
            .strip_prefix('P')
            .and_then(|rest| parse_num(rest, Self::LAST_PHASE))
        {
            return Ok(Self::Phase(n));
        }
        Err(invalid(format!(
            "not a valid chunk (C01-C34) or phase (P01-P09): {text:?}"
        )))
    }

    pub fn is_phase(self) -> bool {
        matches!(self, Self::Phase(_))
    }
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chunk(n) => write!(f, "C{n:02}"),
            Self::Phase(n) => write!(f, "P{n:02}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Subject;

    #[test]
    fn accepts_registered_range_only() {
        assert_eq!(Subject::parse("C01").unwrap(), Subject::Chunk(1));
        assert_eq!(Subject::parse("C26").unwrap(), Subject::Chunk(26));
        assert_eq!(Subject::parse("C27").unwrap(), Subject::Chunk(27));
        assert_eq!(Subject::parse("C34").unwrap(), Subject::Chunk(34));
        assert_eq!(Subject::parse("P08").unwrap(), Subject::Phase(8));
        assert_eq!(Subject::parse("P09").unwrap(), Subject::Phase(9));
        for bad in ["C00", "C35", "P00", "P10", "C7", "C027", "c27", "X01"] {
            assert!(Subject::parse(bad).is_err(), "{bad} must be refused");
        }
        assert_eq!(Subject::Chunk(34).to_string(), "C34");
        assert_eq!(Subject::Phase(9).to_string(), "P09");
    }

    #[test]
    fn only_the_specification_chunk_is_documentation_only() {
        assert!(Subject::Chunk(27).is_docs_only());
        for other in [
            Subject::Chunk(1),
            Subject::Chunk(26),
            Subject::Chunk(28),
            Subject::Chunk(34),
            Subject::Phase(9),
        ] {
            assert!(!other.is_docs_only(), "{other}");
        }
    }
}
