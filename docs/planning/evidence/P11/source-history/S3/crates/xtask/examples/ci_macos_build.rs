//! Internal build-only adapter. It neither enters C26 nor writes gate evidence.
use sha2::{Digest, Sha256};
use std::path::Path;
use xtask::{Repo, env::Environment};

fn digest(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(hex::encode(Sha256::digest(std::fs::read(path)?)))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() != 1 {
        return Err("this build-only adapter accepts no arguments".into());
    }
    let repo = Repo::discover();
    let env = Environment::prepare(&repo)?
        .with("CARGO_HOME", &std::env::var("CARGO_HOME")?)
        .with("NPM_CONFIG_USERCONFIG", "/dev/null")
        .with("NPM_CONFIG_GLOBALCONFIG", "/dev/null")
        .with("XDG_CONFIG_HOME", &std::env::var("XDG_CONFIG_HOME")?);
    // Reuse the reviewed archive, full inventory and compiled-pin verification.
    xtask::harness::unpack(&repo)?;
    let commands = [
        vec!["pnpm", "--filter", "@prisma-airs/desktop", "build"],
        vec![
            "pnpm",
            "--filter",
            "@prisma-airs/desktop",
            "exec",
            "tauri",
            "build",
            "--ci",
            "--no-sign",
            "--target",
            "aarch64-apple-darwin",
            "--bundles",
            "app",
            "--config",
            "src-tauri/tauri.harness.conf.json",
            "--",
            "--locked",
        ],
    ];
    for argv in &commands {
        let args: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
        if !env.command(&args, repo.root())?.status()?.success() {
            return Err(format!("build failed: {}", argv.join(" ")).into());
        }
    }
    let app = xtask::native::bundle_path(&repo, "aarch64-apple-darwin", "Prisma AIRS Desktop");
    if xtask::native::plist_value(&app, "CFBundleIdentifier")? != "io.cdot.airs-desktop"
        || xtask::native::plist_value(&app, "CFBundleShortVersionString")? != "1.0.0"
    {
        return Err("unexpected production identity/version".into());
    }
    let engine = app.join("Contents/Resources/harness/airs-harness");
    let engine_sha256 = digest(&engine)?;
    if engine_sha256 != desktop_core::harness::engine::pin().binary_sha256 {
        return Err("bundled Harness differs from the verified signed input".into());
    }
    let output = repo.join("target/ci-macos-build");
    std::fs::create_dir_all(&output)?;
    let receipt = serde_json::json!({
        "schema": "airs-ci-macos-build/1",
        "kind": "internal-unsigned-production-build-only",
        "target": "aarch64-apple-darwin",
        "app": app.strip_prefix(repo.root())?.to_string_lossy(),
        "app_tree_sha256": xtask::fingerprint::tree_hash(&app)?,
        "executable_sha256": digest(&app.join("Contents/MacOS/prisma-airs-desktop"))?,
        "engine_sha256": engine_sha256,
        "toolchain_sha256": env.toolchain_hash(),
        "tools": env.tools,
        "commands": commands,
        "outer_app_signing": "skipped (--no-sign); not notarized",
        "harness_signing": "inherited historical signature in exact pinned resource",
        "qualification": "none; no C26 evidence, installed acceptance or publication"
    });
    std::fs::write(
        output.join("build.json"),
        format!("{}\n", serde_json::to_string_pretty(&receipt)?),
    )?;
    Ok(())
}
