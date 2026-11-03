//! Package gates. Neither kind uploads or publishes anything.
//!
//! * preview: only completed chunks may be advertised; every other feature
//!   must stay visibly unavailable in the build.
//! * release-candidate: requires C01–C25 completed; C26 adds the signed,
//!   notarized, stapled and installed-byte validators.

use crate::complete::{is_completed, read_ledger};
use crate::{Repo, Result, Subject, invalid};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Preview,
    ReleaseCandidate,
}

impl Kind {
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "preview" => Ok(Self::Preview),
            "release-candidate" => Ok(Self::ReleaseCandidate),
            other => Err(invalid(format!("unknown package kind {other}"))),
        }
    }
}

/// Completed chunks, in order, from the append-only ledger.
pub fn completed_chunks(repo: &Repo) -> Result<Vec<Subject>> {
    let entries = read_ledger(&repo.completion_ledger())?;
    Ok((1..=Subject::LAST_CHUNK)
        .map(Subject::Chunk)
        .filter(|s| is_completed(repo, &entries, *s))
        .collect())
}

pub fn gate(repo: &Repo, kind: Kind) -> Result<Vec<Subject>> {
    let completed = completed_chunks(repo)?;
    match kind {
        Kind::Preview => {
            if completed.is_empty() {
                return Err(invalid(
                    "a preview package needs at least one completed chunk",
                ));
            }
            // Completion must be a contiguous prefix: no skipped predecessor.
            for (i, subject) in completed.iter().enumerate() {
                if *subject != Subject::Chunk(i as u8 + 1) {
                    return Err(invalid("completed chunks are not a contiguous prefix"));
                }
            }
            Ok(completed)
        }
        Kind::ReleaseCandidate => {
            let missing: Vec<String> = (1..=25u8)
                .map(Subject::Chunk)
                .filter(|s| !completed.contains(s))
                .map(|s| s.to_string())
                .collect();
            if !missing.is_empty() {
                return Err(invalid(format!(
                    "release-candidate requires C01-C25 completed; missing {}",
                    missing.join(", ")
                )));
            }
            verify_current_release_phases(repo)?;
            Ok(completed)
        }
    }
}

/// P09/P10 actual independent phase proofs supplement the existing C01-C25
/// ledger. C26 remains open so its own release implementation is not circular.
pub fn verify_current_release_phases(repo: &Repo) -> Result<()> {
    let output = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "tools/p10/review_gate.py", "release-prerequisites"])
        .current_dir(repo.root())
        .env_clear()
        .env("PATH", crate::git_path())
        .output()
        .map_err(crate::io_err(&repo.join("tools/p10/review_gate.py")))?;
    if !output.status.success() {
        return Err(invalid(format!(
            "release requires genuine audited P09 and P10 full-9 phase proofs: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}
