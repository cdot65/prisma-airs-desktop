//! Generated TypeScript wire declarations (ts-rs) and their drift check.

use std::path::Path;

use crate::{Repo, Result, invalid, io_err, random_hex};

pub const GENERATED_DIR: &str = "packages/desktop-contracts/src/generated";

fn files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(dir)
        .map_err(io_err(dir))?
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "ts") {
            let bytes = std::fs::read(&path).map_err(io_err(&path))?;
            out.push((entry.file_name().to_string_lossy().into_owned(), bytes));
        }
    }
    out.sort();
    Ok(out)
}

/// Regenerates into a scratch directory; writes (or with `check`, compares).
pub fn run(repo: &Repo, check: bool) -> Result<()> {
    let scratch = std::env::temp_dir().join(format!("airs-v2-bindings-{}", random_hex(6)?));
    desktop_core::wire::export_typescript(&scratch)
        .map_err(|e| invalid(format!("ts-rs export failed: {e}")))?;
    let fresh = files(&scratch)?;
    let target = repo.join(GENERATED_DIR);
    let result = if check {
        let committed = files(&target)?;
        if committed == fresh {
            Ok(())
        } else {
            let names =
                |v: &[(String, Vec<u8>)]| v.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
            Err(invalid(format!(
                "generated bindings differ from Rust DTOs; run `cargo run -p xtask -- bindings` (committed {:?}, fresh {:?})",
                names(&committed),
                names(&fresh)
            )))
        }
    } else {
        if target.is_dir() {
            std::fs::remove_dir_all(&target).map_err(io_err(&target))?;
        }
        std::fs::create_dir_all(&target).map_err(io_err(&target))?;
        for (name, bytes) in &fresh {
            let path = target.join(name);
            std::fs::write(&path, bytes).map_err(io_err(&path))?;
        }
        Ok(())
    };
    let _ = std::fs::remove_dir_all(&scratch);
    result
}
