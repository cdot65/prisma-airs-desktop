//! C26 signed-artifact controls on a synthetic bundle, with ad-hoc
//! signatures (no certificate, no network): the release signing refuses an
//! engine that is not the pin before it signs anything, never changes the
//! engine's bytes, records each stage's hashes, and a signed bundle no
//! longer verifies once any of its files changes.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use xtask::signing::{AD_HOC, Release, execute, sign_engine, verify};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .canonicalize()
        .expect("temp")
        .join(format!(
            "airs-v2-signed-artifact-{name}-{}",
            std::process::id()
        ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch");
    dir
}

fn sha256(path: &Path) -> String {
    hex::encode(Sha256::digest(std::fs::read(path).expect("read")))
}

/// A Mach-O to sign: a copy of a system tool.
fn macho(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    std::fs::copy("/usr/bin/true", path).expect("copy");
}

/// A minimal app bundle whose engine is signed at "sealing" (ad hoc) and
/// whose pin is that signed engine's hash.
fn bundle(dir: &Path) -> (PathBuf, String) {
    let app = dir.join("Fake.app");
    std::fs::create_dir_all(app.join("Contents/Resources")).expect("bundle");
    std::fs::write(
        app.join("Contents/Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>fake</string>
<key>CFBundleIdentifier</key><string>io.cdot.prisma-airs-desktop.v2.signed-artifact-test</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#,
    )
    .expect("plist");
    std::fs::write(
        app.join("Contents/Resources/notice.txt"),
        b"synthetic resource\n",
    )
    .expect("resource");
    macho(&app.join("Contents/MacOS/fake"));
    let unsigned = dir.join("unsigned/airs-harness");
    macho(&unsigned);
    let staged = dir.join("staged");
    let signed = sign_engine(&unsigned, &sha256(&unsigned), &staged, AD_HOC).expect("engine");
    let engine = app.join("Contents/Resources/harness/airs-harness");
    std::fs::create_dir_all(engine.parent().expect("parent")).expect("dir");
    std::fs::copy(staged.join("airs-harness"), &engine).expect("bundle engine");
    assert_eq!(sha256(&engine), signed.signed_sha256);
    (app, signed.signed_sha256)
}

fn release(app: &Path, dir: &Path) -> Release {
    Release {
        app: app.to_path_buf(),
        executable: "fake".to_owned(),
        dmg: dir.join("Fake.dmg"),
        volume: "Fake".to_owned(),
        identity: AD_HOC.to_owned(),
        profile: "prisma-airs-desktop".to_owned(),
    }
}

#[test]
fn an_engine_that_is_not_the_pin_is_refused_before_anything_is_signed() {
    let dir = scratch("unpinned");
    let (app, _) = bundle(&dir);
    let exe = app.join("Contents/MacOS/fake");
    let before = sha256(&exe);
    let err = execute(&release(&app, &dir), &"0".repeat(64), false).expect_err("refused");
    assert!(err.to_string().contains("not the pinned engine"), "{err}");
    assert_eq!(sha256(&exe), before, "the executable was not signed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn signing_records_every_stage_and_never_changes_the_engine() {
    let dir = scratch("stages");
    let (app, pin) = bundle(&dir);
    let record = execute(&release(&app, &dir), &pin, false).expect("signed");
    verify(&app).expect("the signed app verifies");
    assert_eq!(
        record.before_signing.engine_sha256.as_deref(),
        Some(pin.as_str())
    );
    assert_eq!(
        record.after_signing.engine_sha256.as_deref(),
        Some(pin.as_str())
    );
    assert_ne!(
        record.before_signing.executable_sha256, record.after_signing.executable_sha256,
        "the main executable was signed"
    );
    assert_ne!(
        record.before_signing.app_tree_sha256,
        record.after_signing.app_tree_sha256
    );
    assert!(!record.notarized);
    let labels: Vec<&str> = record.steps.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["sign main executable", "sign app"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_signed_bundle_rejects_a_changed_resource_or_engine() {
    let dir = scratch("tamper");
    let (app, pin) = bundle(&dir);
    execute(&release(&app, &dir), &pin, false).expect("signed");
    verify(&app).expect("verifies before the change");

    // A changed resource breaks the app's sealed resources.
    let resource = app.join("Contents/Resources/notice.txt");
    std::fs::write(&resource, b"changed\n").expect("tamper");
    assert!(verify(&app).is_err(), "a changed resource must not verify");
    std::fs::write(&resource, b"synthetic resource\n").expect("restore");
    verify(&app).expect("verifies again once restored");

    // A changed engine no longer matches the pin and does not verify.
    let engine = app.join("Contents/Resources/harness/airs-harness");
    let mut bytes = std::fs::read(&engine).expect("engine");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&engine, &bytes).expect("tamper engine");
    assert_ne!(sha256(&engine), pin);
    assert!(verify(&engine).is_err() || verify(&app).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
