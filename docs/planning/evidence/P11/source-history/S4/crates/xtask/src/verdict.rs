//! Strict parsing of reviewer verdicts (docs/architecture/review-verdict.schema.json).
//!
//! The JSON Schema is given to the reviewer; the controller enforces the same
//! shape with `deny_unknown_fields`, integer scores in 1..=10 and an explicit
//! recommendation. A verdict that does not parse is a failed attempt.

use serde::{Deserialize, Serialize};

use crate::fingerprint::Fingerprints;
use crate::manifest::DIMENSIONS;
use crate::{Result, invalid};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scores {
    pub architecture: u8,
    pub behavior: u8,
    pub authority_security: u8,
    pub validation: u8,
    pub overall: u8,
}

impl Scores {
    pub fn named(&self) -> [(&'static str, u8); 5] {
        [
            (DIMENSIONS[0], self.architecture),
            (DIMENSIONS[1], self.behavior),
            (DIMENSIONS[2], self.authority_security),
            (DIMENSIONS[3], self.validation),
            (DIMENSIONS[4], self.overall),
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Recommendation {
    Pass,
    Fail,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Verdict {
    pub request_id: String,
    pub attempt_id: String,
    pub subject: String,
    pub fingerprints: Fingerprints,
    pub scores: Scores,
    pub required_fixes: Vec<String>,
    pub blockers: Vec<String>,
    pub evidence_gaps: Vec<String>,
    pub optional_suggestions: Vec<String>,
    pub limitations: Vec<String>,
    pub inspected_files: Vec<String>,
    pub recommendation: Recommendation,
}

impl Verdict {
    /// Parses verdict bytes, rejecting any deviation from the schema.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let verdict: Self = serde_json::from_slice(bytes)
            .map_err(|e| invalid(format!("verdict does not match the strict schema: {e}")))?;
        for (name, score) in verdict.scores.named() {
            if !(1..=10).contains(&score) {
                return Err(invalid(format!(
                    "verdict score {name}={score} is outside 1..=10"
                )));
            }
        }
        if verdict.request_id.is_empty() || verdict.attempt_id.is_empty() {
            return Err(invalid("verdict request_id/attempt_id must be non-empty"));
        }
        Ok(verdict)
    }

    /// Reasons this verdict alone cannot pass the gate (empty when it can).
    pub fn threshold_failures(&self, minimum: u8) -> Vec<String> {
        let mut reasons: Vec<String> = self
            .scores
            .named()
            .iter()
            .filter(|(_, score)| *score < minimum)
            .map(|(name, score)| format!("score {name}={score} is below {minimum}"))
            .collect();
        if !self.required_fixes.is_empty() {
            reasons.push(format!(
                "{} required fixes remain",
                self.required_fixes.len()
            ));
        }
        if !self.blockers.is_empty() {
            reasons.push(format!("{} blockers remain", self.blockers.len()));
        }
        if !self.evidence_gaps.is_empty() {
            reasons.push(format!("{} evidence gaps remain", self.evidence_gaps.len()));
        }
        if self.recommendation != Recommendation::Pass {
            reasons.push("reviewer recommendation is fail".to_owned());
        }
        reasons
    }
}
