//! Versioned P11 fixture preparation; never overwrites sealed preparation.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use xtask::{Repo, env::Environment, manifest::Manifest, native};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn reserve(repo: &Repo, args: &[String]) -> Result<fs::File> {
    let name = args.get(1).map(String::as_str).unwrap_or("");
    let attempt = name
        .strip_prefix("docs/planning/evidence/P11/native-build-")
        .and_then(|s| s.strip_suffix("-vm/native-prepare.json"))
        .and_then(|s| s.parse::<u32>().ok());
    if args.len() != 2
        || args[0] != "--out"
        || !attempt.is_some_and(|n| {
            n >= 5
                && name
                    == format!(
                        "docs/planning/evidence/P11/native-build-{n:02}-vm/native-prepare.json"
                    )
        })
    {
        return Err("expected --out docs/planning/evidence/P11/native-build-NN-vm/native-prepare.json (NN>=05)".into());
    }
    if repo.root().canonicalize()? != repo.root() {
        return Err("repository parents must not be symlinks".into());
    }
    let mut parent = repo.root().to_path_buf();
    for part in Path::new(name)
        .parent()
        .ok_or("missing output parent")?
        .components()
    {
        parent.push(part);
        match fs::symlink_metadata(&parent) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&parent)?,
            _ => return Err("output parents must be real directories".into()),
        }
    }
    Ok(fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(repo.join(name))?)
}

fn main() -> Result<()> {
    let repo = Repo::discover();
    // Reserve before environment probes, extraction or either build command.
    // A failed build leaves this incomplete attempt reserved for inspection.
    let mut output = reserve(&repo, &std::env::args().skip(1).collect::<Vec<_>>())?;
    let manifest = Manifest::load(&repo.manifest())?;
    let target = "aarch64-apple-darwin";
    let features = vec!["test-fixtures".to_owned()];
    if manifest.target != target || manifest.fixture_only_features != features {
        return Err("P11 requires the fixed ARM64 fixture recipe".into());
    }
    let env = Environment::prepare(&repo)?;
    xtask::harness::unpack(&repo)?;
    let commands: Vec<Vec<String>> = native::build_commands(target)
        .iter()
        .map(|argv| argv.iter().map(|s| (*s).to_owned()).collect())
        .collect();
    for argv in &commands {
        if !env.command(argv, repo.root())?.status()?.success() {
            return Err(format!("fixture build failed: {}", argv.join(" ")).into());
        }
    }
    let bundle = native::bundle_path(&repo, target, "Prisma AIRS Desktop Fixture");
    let identifier = native::plist_value(&bundle, "CFBundleIdentifier")?;
    if identifier != native::FIXTURE_IDENTIFIER {
        return Err(format!("unexpected fixture identifier: {identifier}").into());
    }
    let prepared = native::Prepared {
        subject: "P11".to_owned(),
        built_utc: desktop_core::sessions::rfc3339(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        ),
        bundle: bundle.strip_prefix(repo.root())?.display().to_string(),
        identifier,
        executable_sha256: hex::encode(Sha256::digest(fs::read(
            bundle.join("Contents/MacOS/prisma-airs-desktop"),
        )?)),
        bundle_sha256: xtask::fingerprint::tree_hash(&bundle)?,
        features,
        target: target.to_owned(),
        commands,
    };
    serde_json::to_writer_pretty(&mut output, &prepared)?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_paths_collisions_and_symlink_parents_refuse_before_build() -> Result<()> {
        let base = std::env::temp_dir().join(format!(
            "airs-p11-reserve-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        fs::create_dir(&base)?;
        let repo = Repo::new(base.canonicalize()?);
        for value in [
            "docs/planning/evidence/P11/native-prepare.json",
            "docs/planning/evidence/P11/native-build-04-vm/native-prepare.json",
            "docs/planning/evidence/P11/native-build-5-vm/native-prepare.json",
            "docs/planning/evidence/P11/native-build-0005-vm/native-prepare.json",
            "docs/planning/evidence/P11/../P11/native-build-05-vm/native-prepare.json",
            "/docs/planning/evidence/P11/native-build-05-vm/native-prepare.json",
        ] {
            assert!(reserve(&repo, &["--out".into(), value.into()]).is_err());
        }
        assert!(reserve(&repo, &[]).is_err());
        assert!(reserve(&repo, &["--out".into()]).is_err());
        assert!(reserve(&repo, &["--wrong".into(), "x".into()]).is_err());
        assert!(!repo.join("docs").exists());
        let value = "docs/planning/evidence/P11/native-build-05-vm/native-prepare.json";
        let args = vec!["--out".to_owned(), value.to_owned()];
        reserve(&repo, &args)?.write_all(b"owned-sentinel")?;
        assert!(reserve(&repo, &args).is_err());
        assert_eq!(fs::read(repo.join(value))?, b"owned-sentinel");
        drop(reserve(
            &repo,
            &["--out".into(), value.replace("05-vm", "06-vm")],
        )?);
        fs::remove_dir_all(repo.join("docs"))?;
        fs::create_dir(repo.join("outside"))?;
        std::os::unix::fs::symlink(repo.join("outside"), repo.join("docs"))?;
        assert!(reserve(&repo, &args).is_err());
        assert_eq!(fs::read_dir(repo.join("outside"))?.count(), 0);
        fs::remove_dir_all(&base)?;
        Ok(())
    }
}
