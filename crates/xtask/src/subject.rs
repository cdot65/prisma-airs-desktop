//! Validated review subjects: chunks C01–C26 and phases P01–P08.

use std::fmt;

use crate::{Result, invalid};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Subject {
    Chunk(u8),
    Phase(u8),
}

impl Subject {
    pub fn parse(text: &str) -> Result<Self> {
        let parse_num = |digits: &str, max: u8| -> Option<u8> {
            (digits.len() == 2 && digits.bytes().all(|b| b.is_ascii_digit()))
                .then(|| digits.parse::<u8>().ok())
                .flatten()
                .filter(|n| (1..=max).contains(n))
        };
        if let Some(n) = text.strip_prefix('C').and_then(|rest| parse_num(rest, 26)) {
            return Ok(Self::Chunk(n));
        }
        if let Some(n) = text.strip_prefix('P').and_then(|rest| parse_num(rest, 8)) {
            return Ok(Self::Phase(n));
        }
        Err(invalid(format!(
            "not a valid chunk (C01-C26) or phase (P01-P08): {text:?}"
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
