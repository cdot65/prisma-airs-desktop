//! Repository-relative locations the controller reads and writes.

use std::path::{Path, PathBuf};

use crate::Subject;

/// Root of a Prisma AIRS Desktop v2 checkout.
#[derive(Clone, Debug)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Locates the checkout from the xtask crate's compile-time location.
    pub fn discover() -> Self {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        Self::new(
            manifest_dir
                .parent()
                .and_then(Path::parent)
                .unwrap_or(manifest_dir),
        )
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn join(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    pub fn manifest(&self) -> PathBuf {
        self.join("docs/architecture/checks.json")
    }

    pub fn toolchain_manifest(&self) -> PathBuf {
        self.join("docs/architecture/toolchain.json")
    }

    pub fn verdict_schema(&self) -> PathBuf {
        self.join("docs/architecture/review-verdict.schema.json")
    }

    /// Committed, redacted evidence for one subject.
    pub fn evidence_dir(&self, subject: Subject) -> PathBuf {
        self.join(format!("docs/planning/evidence/{subject}"))
    }

    /// Immutable review attempts for one subject.
    pub fn reviews_dir(&self, subject: Subject) -> PathBuf {
        self.join(format!("docs/planning/reviews/{subject}"))
    }

    /// Append-only completion ledger.
    pub fn completion_ledger(&self) -> PathBuf {
        self.join("docs/planning/evidence/completions.jsonl")
    }

    /// Transport handshake records.
    pub fn transport_dir(&self) -> PathBuf {
        self.join("docs/planning/reviews/transport")
    }
}
