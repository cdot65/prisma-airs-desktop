//! Developer ID signing for the release (C26).
//!
//! Signing rewrites a Mach-O's bytes, and the app checks the bundled
//! Harness engine against the SHA-256 compiled from `resources/harness/
//! pin.json` before it starts it. So the engine is signed once, at sealing
//! time (`harness-sign`, then `tools/harness/seal.py --signed-engine`), and
//! its signed hash becomes the pin; the release signing below never signs
//! the engine again and refuses a bundle whose engine is not the pinned one.
//!
//! Release order (`release-sign`): verify the engine's signature and pin,
//! sign the main executable and then the app, notarize and staple the app,
//! build the DMG from the stapled app, sign it, notarize and staple it, and
//! assess both. Each stage's hashes are recorded; nothing is uploaded or
//! published beyond the notary submissions.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::{Result, invalid, io_err, now_utc, sha256_hex, write_json};

pub const SIGNING_SCHEMA: &str = "airs-v2/engine-signing/1";
pub const RELEASE_SCHEMA: &str = "airs-v2/release-signing/1";
/// The ad-hoc identity: no certificate, no timestamp (tests only).
pub const AD_HOC: &str = "-";

/// `codesign` arguments for one file: hardened runtime, a secure timestamp
/// for a real identity (an ad-hoc signature cannot be timestamped).
pub fn codesign_args(identity: &str, path: &Path) -> Vec<String> {
    let mut argv = vec![
        "/usr/bin/codesign".to_owned(),
        "--force".to_owned(),
        "--sign".to_owned(),
        identity.to_owned(),
        "--options".to_owned(),
        "runtime".to_owned(),
    ];
    argv.push(if identity == AD_HOC {
        "--timestamp=none".to_owned()
    } else {
        "--timestamp".to_owned()
    });
    argv.push(path.display().to_string());
    argv
}

/// Runs an argv with only the system tools on PATH. HOME is kept: the
/// signing identity and the notary profile live in the operator's Keychain.
fn run(argv: &[String]) -> Result<std::process::Output> {
    let (program, args) = argv.split_first().ok_or_else(|| invalid("empty command"))?;
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    if let Some(home) = std::env::var_os("HOME") {
        command.env("HOME", home);
    }
    let output = command.output().map_err(io_err(Path::new(program)))?;
    if !output.status.success() {
        return Err(invalid(format!(
            "{} failed with {}: {}",
            argv.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output)
}

fn file_sha256(path: &Path) -> Result<String> {
    Ok(sha256_hex(&std::fs::read(path).map_err(io_err(path))?))
}

/// `codesign --verify --strict` of a signed file or bundle.
pub fn verify(path: &Path) -> Result<()> {
    run(&[
        "/usr/bin/codesign".to_owned(),
        "--verify".to_owned(),
        "--strict".to_owned(),
        "--verbose=2".to_owned(),
        path.display().to_string(),
    ])
    .map(|_| ())
}

/// The signature's team identifier (`None` for an ad-hoc signature).
pub fn team_identifier(path: &Path) -> Result<Option<String>> {
    let output = run(&[
        "/usr/bin/codesign".to_owned(),
        "-dv".to_owned(),
        "--verbose=2".to_owned(),
        path.display().to_string(),
    ])?;
    let text = String::from_utf8_lossy(&output.stderr);
    Ok(text
        .lines()
        .find_map(|l| l.strip_prefix("TeamIdentifier="))
        .map(str::trim)
        .filter(|t| *t != "not set" && !t.is_empty())
        .map(str::to_owned))
}

/// What `harness-sign` produced: the unsigned provenance and the signed
/// bytes the reseal pins.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineSigning {
    pub schema: String,
    pub signed_utc: String,
    pub identity: String,
    pub team_identifier: Option<String>,
    pub options: String,
    pub timestamp: bool,
    pub unsigned_sha256: String,
    pub signed_sha256: String,
}

/// Copies the engine at `source` to `out/airs-harness`, signs and verifies
/// the copy, and writes `out/signing.json`. The source must be the pinned
/// unsigned engine (`expected_unsigned`); `out` must not be the committed
/// resources folder.
pub fn sign_engine(
    source: &Path,
    expected_unsigned: &str,
    out: &Path,
    identity: &str,
) -> Result<EngineSigning> {
    if identity.trim().is_empty() {
        return Err(invalid("a signing identity is required"));
    }
    if out.components().any(|c| c.as_os_str() == "resources") {
        return Err(invalid(
            "the signed engine is staged outside resources/; seal.py reseals it",
        ));
    }
    let unsigned = file_sha256(source)?;
    if unsigned != expected_unsigned {
        return Err(invalid(format!(
            "{} is not the pinned unsigned engine ({unsigned})",
            source.display()
        )));
    }
    std::fs::create_dir_all(out).map_err(io_err(out))?;
    let target = out.join("airs-harness");
    std::fs::copy(source, &target).map_err(io_err(&target))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .map_err(io_err(&target))?;
    }
    run(&codesign_args(identity, &target))?;
    verify(&target)?;
    let signed = file_sha256(&target)?;
    if signed == unsigned {
        return Err(invalid("signing left the engine's bytes unchanged"));
    }
    let record = EngineSigning {
        schema: SIGNING_SCHEMA.to_owned(),
        signed_utc: now_utc(),
        identity: identity.to_owned(),
        team_identifier: team_identifier(&target)?,
        options: "runtime".to_owned(),
        timestamp: identity != AD_HOC,
        unsigned_sha256: unsigned,
        signed_sha256: signed,
    };
    write_json(&out.join("signing.json"), &record)?;
    Ok(record)
}

/// One planned release step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub label: String,
    pub argv: Vec<String>,
}

/// The release inputs: the built app, where the DMG goes, the identity and
/// the notary Keychain profile.
#[derive(Clone, Debug)]
pub struct Release {
    pub app: PathBuf,
    pub executable: String,
    pub dmg: PathBuf,
    pub volume: String,
    pub identity: String,
    pub profile: String,
}

impl Release {
    fn engine(&self) -> PathBuf {
        self.app.join("Contents/Resources/harness/airs-harness")
    }

    fn app_zip(&self) -> PathBuf {
        self.dmg.with_extension("app.zip")
    }

    fn s(path: &Path) -> String {
        path.display().to_string()
    }

    /// Signing steps (no network): the main executable, then the app.
    pub fn sign_steps(&self) -> Vec<Step> {
        let exe = self.app.join("Contents/MacOS").join(&self.executable);
        vec![
            Step {
                label: "sign main executable".to_owned(),
                argv: codesign_args(&self.identity, &exe),
            },
            Step {
                label: "sign app".to_owned(),
                argv: codesign_args(&self.identity, &self.app),
            },
        ]
    }

    /// Notarize and staple the app, build and sign the DMG from it,
    /// notarize and staple the DMG, and assess both.
    pub fn notary_steps(&self) -> Vec<Step> {
        let notarize = |path: &Path| {
            vec![
                "/usr/bin/xcrun".to_owned(),
                "notarytool".to_owned(),
                "submit".to_owned(),
                Self::s(path),
                "--keychain-profile".to_owned(),
                self.profile.clone(),
                "--wait".to_owned(),
            ]
        };
        let staple = |path: &Path| {
            vec![
                "/usr/bin/xcrun".to_owned(),
                "stapler".to_owned(),
                "staple".to_owned(),
                Self::s(path),
            ]
        };
        let mut dmg_sign = codesign_args(&self.identity, &self.dmg);
        // A disk image carries no hardened-runtime flag.
        dmg_sign.retain(|a| a != "--options" && a != "runtime");
        vec![
            Step {
                label: "zip app for notarization".to_owned(),
                argv: vec![
                    "/usr/bin/ditto".to_owned(),
                    "-c".to_owned(),
                    "-k".to_owned(),
                    "--keepParent".to_owned(),
                    Self::s(&self.app),
                    Self::s(&self.app_zip()),
                ],
            },
            Step {
                label: "notarize app".to_owned(),
                argv: notarize(&self.app_zip()),
            },
            Step {
                label: "staple app".to_owned(),
                argv: staple(&self.app),
            },
            Step {
                label: "build dmg".to_owned(),
                argv: vec![
                    "/usr/bin/hdiutil".to_owned(),
                    "create".to_owned(),
                    "-volname".to_owned(),
                    self.volume.clone(),
                    "-srcfolder".to_owned(),
                    Self::s(&self.app),
                    "-ov".to_owned(),
                    "-format".to_owned(),
                    "UDZO".to_owned(),
                    Self::s(&self.dmg),
                ],
            },
            Step {
                label: "sign dmg".to_owned(),
                argv: dmg_sign,
            },
            Step {
                label: "notarize dmg".to_owned(),
                argv: notarize(&self.dmg),
            },
            Step {
                label: "staple dmg".to_owned(),
                argv: staple(&self.dmg),
            },
            Step {
                label: "assess app".to_owned(),
                argv: vec![
                    "/usr/sbin/spctl".to_owned(),
                    "-a".to_owned(),
                    "-vv".to_owned(),
                    "-t".to_owned(),
                    "exec".to_owned(),
                    Self::s(&self.app),
                ],
            },
            Step {
                label: "assess dmg".to_owned(),
                argv: vec![
                    "/usr/sbin/spctl".to_owned(),
                    "-a".to_owned(),
                    "-vv".to_owned(),
                    "-t".to_owned(),
                    "open".to_owned(),
                    "--context".to_owned(),
                    "context:primary-signature".to_owned(),
                    Self::s(&self.dmg),
                ],
            },
            Step {
                label: "validate dmg staple".to_owned(),
                argv: vec![
                    "/usr/bin/xcrun".to_owned(),
                    "stapler".to_owned(),
                    "validate".to_owned(),
                    Self::s(&self.dmg),
                ],
            },
        ]
    }

    /// Every step, in order.
    pub fn plan(&self) -> Vec<Step> {
        let mut steps = self.sign_steps();
        steps.extend(self.notary_steps());
        steps
    }
}

/// Hashes recorded at each stage of a release signing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stages {
    pub app_tree_sha256: Option<String>,
    pub executable_sha256: Option<String>,
    pub engine_sha256: Option<String>,
    pub dmg_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseRecord {
    pub schema: String,
    pub started_utc: String,
    pub finished_utc: String,
    pub identity: String,
    pub team_identifier: Option<String>,
    pub profile: String,
    pub steps: Vec<Step>,
    pub notarized: bool,
    pub before_signing: Stages,
    pub after_signing: Stages,
    pub stapled: Stages,
}

fn stages(release: &Release, dmg: bool) -> Result<Stages> {
    let exe = release.app.join("Contents/MacOS").join(&release.executable);
    Ok(Stages {
        app_tree_sha256: Some(crate::fingerprint::tree_hash(&release.app)?),
        executable_sha256: Some(file_sha256(&exe)?),
        engine_sha256: Some(file_sha256(&release.engine())?),
        dmg_sha256: if dmg && release.dmg.exists() {
            Some(file_sha256(&release.dmg)?)
        } else {
            None
        },
    })
}

/// Signs `release` and, with `notarize`, notarizes, staples and assesses
/// it. Refuses before signing anything when the bundled engine is not the
/// pinned (`engine_pin`) signed engine or its signature does not verify.
pub fn execute(release: &Release, engine_pin: &str, notarize: bool) -> Result<ReleaseRecord> {
    let started = now_utc();
    let engine = release.engine();
    let engine_hash = file_sha256(&engine)?;
    if engine_hash != engine_pin {
        return Err(invalid(format!(
            "the bundled engine ({engine_hash}) is not the pinned engine ({engine_pin}); \
             sign it at sealing time (harness-sign, seal.py --signed-engine) and rebuild"
        )));
    }
    verify(&engine).map_err(|e| {
        invalid(format!(
            "the bundled engine's signature does not verify: {e}"
        ))
    })?;
    let before = stages(release, false)?;
    let mut steps = release.sign_steps();
    for step in &steps {
        run(&step.argv)?;
    }
    verify(&release.app)?;
    let after = stages(release, false)?;
    if after.engine_sha256 != before.engine_sha256 {
        return Err(invalid("signing the app changed the engine's bytes"));
    }
    let mut stapled = Stages::default();
    if notarize {
        let notary = release.notary_steps();
        for step in &notary {
            run(&step.argv)?;
        }
        steps.extend(notary);
        stapled = stages(release, true)?;
    }
    Ok(ReleaseRecord {
        schema: RELEASE_SCHEMA.to_owned(),
        started_utc: started,
        finished_utc: now_utc(),
        identity: release.identity.clone(),
        team_identifier: team_identifier(&release.app)?,
        profile: release.profile.clone(),
        steps,
        notarized: notarize,
        before_signing: before,
        after_signing: after,
        stapled,
    })
}

#[cfg(test)]
mod tests {
    use super::{AD_HOC, Release, codesign_args, execute, file_sha256, sign_engine, verify};
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "airs-v2-signing-{name}-{}-{}",
            std::process::id(),
            crate::random_hex(4).expect("random")
        ));
        std::fs::create_dir_all(&dir).expect("scratch");
        dir
    }

    /// A Mach-O to sign: a copy of a system tool.
    fn macho(path: &Path) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::copy("/usr/bin/true", path).expect("copy");
    }

    fn release(app: &Path, dir: &Path, identity: &str) -> Release {
        Release {
            app: app.to_path_buf(),
            executable: "fake".to_owned(),
            dmg: dir.join("Fake.dmg"),
            volume: "Fake".to_owned(),
            identity: identity.to_owned(),
            profile: "prisma-airs-desktop".to_owned(),
        }
    }

    #[test]
    fn a_real_identity_is_timestamped_and_an_ad_hoc_one_is_not() {
        let p = Path::new("/x/airs-harness");
        let real = codesign_args("Developer ID Application: Example (TEAM123456)", p);
        assert_eq!(
            real[1..],
            [
                "--force",
                "--sign",
                "Developer ID Application: Example (TEAM123456)",
                "--options",
                "runtime",
                "--timestamp",
                "/x/airs-harness"
            ]
        );
        assert!(codesign_args(AD_HOC, p).contains(&"--timestamp=none".to_owned()));
    }

    #[test]
    fn the_release_plan_signs_inside_out_never_re_signs_the_engine_and_staples_both() {
        let dir = Path::new("/rel");
        let app = dir.join("Fake.app");
        let plan = release(&app, dir, "Developer ID Application: Example (TEAM123456)").plan();
        let labels: Vec<&str> = plan.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "sign main executable",
                "sign app",
                "zip app for notarization",
                "notarize app",
                "staple app",
                "build dmg",
                "sign dmg",
                "notarize dmg",
                "staple dmg",
                "assess app",
                "assess dmg",
                "validate dmg staple"
            ]
        );
        assert!(
            plan.iter()
                .all(|s| !s.argv.iter().any(|a| a.ends_with("airs-harness"))),
            "the engine is signed at sealing time only"
        );
        assert_eq!(
            plan[0].argv.last().map(String::as_str),
            Some("/rel/Fake.app/Contents/MacOS/fake")
        );
        let notarize = &plan[3].argv;
        assert!(
            notarize
                .windows(2)
                .any(|w| w == ["--keychain-profile", "prisma-airs-desktop"])
        );
        assert!(notarize.contains(&"--wait".to_owned()));
        assert!(!plan[6].argv.contains(&"runtime".to_owned()));
    }

    #[test]
    fn the_engine_is_signed_once_from_its_pinned_unsigned_bytes() {
        let dir = scratch("engine");
        let source = dir.join("src/airs-harness");
        macho(&source);
        let unsigned = file_sha256(&source).expect("hash");
        let out = dir.join("signed");
        assert!(sign_engine(&source, &"0".repeat(64), &out, AD_HOC).is_err());
        assert!(sign_engine(&source, &unsigned, &dir.join("resources/harness"), AD_HOC).is_err());
        let record = sign_engine(&source, &unsigned, &out, AD_HOC).expect("signed");
        assert_eq!(record.unsigned_sha256, unsigned);
        assert_ne!(record.signed_sha256, unsigned);
        assert_eq!(
            file_sha256(&out.join("airs-harness")).expect("hash"),
            record.signed_sha256
        );
        assert_eq!(
            file_sha256(&source).expect("hash"),
            unsigned,
            "source untouched"
        );
        assert_eq!((record.team_identifier, record.timestamp), (None, false));
        verify(&out.join("airs-harness")).expect("verifies");
        assert!(out.join("signing.json").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn release_signing_keeps_the_pinned_engine_and_refuses_another() {
        let dir = scratch("release");
        let app = dir.join("Fake.app");
        std::fs::create_dir_all(app.join("Contents")).expect("bundle");
        std::fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>fake</string>
<key>CFBundleIdentifier</key><string>io.cdot.airs-v2.signing-test</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
        )
        .expect("plist");
        macho(&app.join("Contents/MacOS/fake"));
        let staged = dir.join("staged");
        let source = dir.join("src/airs-harness");
        macho(&source);
        let unsigned = file_sha256(&source).expect("hash");
        let signed = sign_engine(&source, &unsigned, &staged, AD_HOC).expect("engine");
        let engine = app.join("Contents/Resources/harness/airs-harness");
        std::fs::create_dir_all(engine.parent().expect("parent")).expect("dir");
        std::fs::copy(staged.join("airs-harness"), &engine).expect("engine");
        let rel = release(&app, &dir, AD_HOC);
        let exe_before = file_sha256(&app.join("Contents/MacOS/fake")).expect("hash");

        // An engine other than the pinned one is refused before anything is signed.
        assert!(execute(&rel, &unsigned, false).is_err());
        assert_eq!(
            file_sha256(&app.join("Contents/MacOS/fake")).expect("hash"),
            exe_before
        );

        let record = execute(&rel, &signed.signed_sha256, false).expect("signed");
        assert!(!record.notarized);
        assert_eq!(
            record.before_signing.engine_sha256.as_deref(),
            Some(signed.signed_sha256.as_str())
        );
        assert_eq!(
            record.after_signing.engine_sha256,
            record.before_signing.engine_sha256
        );
        assert_ne!(
            record.after_signing.executable_sha256,
            record.before_signing.executable_sha256
        );
        assert_ne!(
            record.after_signing.app_tree_sha256,
            record.before_signing.app_tree_sha256
        );
        assert_eq!(record.steps.len(), 2);
        verify(&app).expect("app verifies");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

pub const BUILD_SCHEMA: &str = "airs-v2/release-build/1";

/// The unsigned release build C26 signs: what was built, from which source.
#[derive(Debug, Serialize, Deserialize)]
pub struct BuildRecord {
    pub schema: String,
    pub built_utc: String,
    /// The repository's source fingerprint the build was made from.
    pub source_fingerprint: String,
    pub cargo_lock_sha256: String,
    pub identifier: String,
    pub app: String,
    pub app_tree_sha256: String,
    pub executable_sha256: String,
    /// The bundled engine: it must be the compiled pin (signed at sealing).
    pub engine_sha256: String,
    pub dmg: String,
    pub dmg_sha256: String,
    pub commands: Vec<Vec<String>>,
}

/// Builds the production app and DMG with the sealed (signed) engine and
/// records them as `evidence/C26/release-build.json`. Refuses a build whose
/// bundled engine is not the compiled pin or whose signature does not verify.
pub fn release_build(
    repo: &crate::Repo,
    manifest: &crate::manifest::Manifest,
    env: &crate::env::Environment,
) -> Result<BuildRecord> {
    let built = crate::budgets::build_production(repo, env, manifest.target.as_str())?;
    let pin = desktop_core::harness::engine::pin();
    let engine = built
        .app
        .join("Contents/Resources/harness")
        .join(&pin.executable);
    let engine_bytes = std::fs::read(&engine).map_err(io_err(&engine))?;
    let engine_sha256 = sha256_hex(&engine_bytes);
    if engine_sha256 != pin.binary_sha256 {
        return Err(invalid(format!(
            "the bundled engine {engine_sha256} is not the compiled pin {}",
            pin.binary_sha256
        )));
    }
    verify(&engine)?;
    let executable = built
        .app
        .join("Contents/MacOS")
        .join(crate::native::plist_value(
            &built.app,
            "CFBundleExecutable",
        )?);
    let lock = repo.join("Cargo.lock");
    let relative = |p: &Path| {
        p.strip_prefix(repo.root())
            .unwrap_or(p)
            .display()
            .to_string()
    };
    let record = BuildRecord {
        schema: BUILD_SCHEMA.to_owned(),
        built_utc: now_utc(),
        source_fingerprint: crate::fingerprint::source(repo)?,
        cargo_lock_sha256: sha256_hex(&std::fs::read(&lock).map_err(io_err(&lock))?),
        identifier: built.identifier,
        app: relative(&built.app),
        app_tree_sha256: crate::fingerprint::tree_hash(&built.app)?,
        executable_sha256: sha256_hex(&std::fs::read(&executable).map_err(io_err(&executable))?),
        engine_sha256,
        dmg: relative(&built.dmg),
        dmg_sha256: sha256_hex(&std::fs::read(&built.dmg).map_err(io_err(&built.dmg))?),
        commands: built.commands,
    };
    write_json(
        &repo
            .evidence_dir(crate::Subject::Chunk(26))
            .join("release-build.json"),
        &record,
    )?;
    Ok(record)
}
