//! API Client projects and documents (C14): v1's portable collections,
//! request documents with Rust-owned revisions, environments, workbench
//! secret references, schemas, imports, the read-only Git view, profile
//! adoption and the built-in Strata Cloud Manager collection. Nothing in
//! this module sends a request; execution and its admission arrive in C15.

pub mod admission;
pub mod cookies;
pub mod curl;
pub mod document;
pub mod environments;
pub mod execute;
pub mod git;
pub mod history;
pub mod insomnia;
pub mod profile;
pub mod project;
pub mod render;
pub mod schema;
pub mod scm;

#[cfg(test)]
pub(crate) mod testdir {
    /// A temporary folder removed when dropped.
    pub struct Dir(std::path::PathBuf);

    impl Dir {
        pub fn new() -> Self {
            let mut n = [0u8; 8];
            getrandom::fill(&mut n).expect("fixture");
            let p = std::env::temp_dir().join(format!("wb-{}", hex::encode(n)));
            std::fs::create_dir_all(&p).expect("fixture");
            Self(p.canonicalize().expect("fixture"))
        }

        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
