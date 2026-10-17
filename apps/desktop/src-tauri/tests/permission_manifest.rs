//! Production permission manifest, capability grants, CSP and fixture
//! exclusion. These read the shipped configuration files and the ACL that
//! tauri-build generated from them; nothing here relaxes production settings.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn json(path: impl AsRef<Path>) -> Value {
    let path = crate_dir().join(path);
    serde_json::from_slice(
        &std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
    )
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn set(values: &Value) -> BTreeSet<String> {
    values
        .as_array()
        .expect("array")
        .iter()
        .map(|v| v.as_str().expect("string").to_owned())
        .collect()
}

#[test]
fn only_the_two_application_commands_are_registered_and_granted() {
    let acl = json("gen/schemas/acl-manifests.json");
    let app = &acl["__app-acl__"]["permissions"];
    let allowed: BTreeSet<String> = app
        .as_object()
        .expect("app permissions")
        .values()
        .flat_map(|p| set(&p["commands"]["allow"]))
        .collect();
    let expected: BTreeSet<String> = desktop_host::APP_COMMANDS
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    assert_eq!(
        allowed, expected,
        "app ACL commands must be exactly the two host commands"
    );

    let capabilities = json("gen/schemas/capabilities.json");
    let caps = capabilities.as_object().expect("capabilities");
    assert_eq!(
        caps.keys().collect::<Vec<_>>(),
        vec!["main-window"],
        "one capability only"
    );
    let main = &caps["main-window"];
    assert_eq!(set(&main["windows"]), BTreeSet::from(["main".to_owned()]));
    assert_eq!(set(&main["webviews"]), BTreeSet::from(["main".to_owned()]));
    assert_eq!(main["local"], Value::Bool(true));
    assert!(
        main.get("remote").is_none(),
        "no remote URL may hold a capability"
    );
    assert_eq!(
        set(&main["permissions"]),
        BTreeSet::from([
            "allow-desktop-dispatch".to_owned(),
            "allow-desktop-observe".to_owned()
        ]),
        "no core/plugin permission (event, window, webview, path, shell, fs, http) is granted"
    );
}

#[test]
fn capability_directory_holds_only_the_reviewed_grant() {
    let entries: Vec<String> = std::fs::read_dir(crate_dir().join("capabilities"))
        .expect("capabilities dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(entries, vec!["main.json".to_owned()]);
    let source = json("capabilities/main.json");
    assert_eq!(set(&source["permissions"]).len(), 2);
}

#[test]
fn production_csp_permits_packaged_assets_and_ipc_only() {
    let conf = json("tauri.conf.json");
    let csp = conf["app"]["security"]["csp"]
        .as_object()
        .expect("csp object");
    let directive = |name: &str| {
        csp.get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(directive("default-src"), "'self'");
    assert_eq!(directive("script-src"), "'self'");
    assert_eq!(directive("connect-src"), "ipc: http://ipc.localhost");
    assert_eq!(directive("object-src"), "'none'");
    assert_eq!(directive("frame-src"), "'none'");
    assert_eq!(directive("base-uri"), "'none'");
    for (name, value) in csp {
        let value = value.as_str().unwrap_or_default();
        assert!(
            !value.contains("unsafe-"),
            "{name} relaxes script/style execution: {value}"
        );
        for token in value.split_whitespace() {
            assert_ne!(token, "*", "{name} allows any origin");
            let remote = token.starts_with("https:")
                || token.starts_with("http:")
                || token.starts_with("ws");
            let ipc_bridge = name == "connect-src" && token == "http://ipc.localhost";
            assert!(
                !remote || ipc_bridge,
                "{name} allows a network origin: {token}"
            );
        }
    }
    let security = &conf["app"]["security"];
    assert_eq!(
        security["dangerousDisableAssetCspModification"],
        Value::Bool(false)
    );
    assert_eq!(security["freezePrototype"], Value::Bool(true));
    assert_eq!(
        conf["app"]["withGlobalTauri"],
        Value::Bool(false),
        "no global __TAURI__ object"
    );
    assert_eq!(
        conf["app"]["windows"],
        Value::Array(vec![]),
        "the host creates the only window with its navigation policy"
    );
    assert!(
        conf["build"].get("devUrl").is_none(),
        "production config has no dev server origin"
    );
}

#[test]
fn identities_are_independent_and_fixture_config_relaxes_nothing() {
    let conf = json("tauri.conf.json");
    assert_eq!(conf["identifier"], "io.cdot.prisma-airs-desktop.v2");
    assert_eq!(conf["productName"], "Prisma AIRS Desktop v2");
    let fixture = json("tauri.fixture.conf.json");
    let keys: BTreeSet<&str> = fixture
        .as_object()
        .expect("fixture")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        BTreeSet::from(["identifier", "productName"]),
        "fixture overlay may only rename the identity"
    );
    assert_eq!(
        fixture["identifier"],
        "io.cdot.prisma-airs-desktop.v2.fixture"
    );
    assert_ne!(fixture["identifier"], conf["identifier"]);
    assert!(
        !conf["identifier"]
            .as_str()
            .unwrap_or_default()
            .contains("prisma-airs-desktop\""),
        "v1 identity is not reused"
    );
}

#[test]
fn shipping_builds_exclude_fixture_features_and_commands() {
    let manifest = std::fs::read_to_string(crate_dir().join("Cargo.toml")).expect("Cargo.toml");
    assert!(
        manifest.contains("default = []"),
        "desktop-host enables no default features"
    );
    let features = manifest
        .split("test-fixtures = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("test-fixtures feature");
    for item in [
        "desktop-core/test-fixtures",
        "tauri/test",
        "dep:objc2",
        "dep:objc2-web-kit",
        "dep:objc2-app-kit",
        "dep:objc2-foundation",
        "dep:block2",
    ] {
        assert!(features.contains(item), "test-fixtures must enable {item}");
    }
    for dep in [
        "objc2 =",
        "objc2-foundation =",
        "objc2-app-kit =",
        "objc2-web-kit =",
        "block2 =",
    ] {
        let line = manifest
            .lines()
            .find(|l| l.starts_with(dep))
            .expect("capture dependency");
        assert!(
            line.contains("optional = true"),
            "{dep} must only be pulled in by test-fixtures"
        );
    }
    let checks = json("../../../docs/architecture/checks.json");
    let artifact: Vec<String> = checks["artifact_build"]
        .as_array()
        .expect("artifact argv")
        .iter()
        .map(|v| v.as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(
        !artifact
            .iter()
            .any(|a| a.contains("fixture") || a == "--features"),
        "artifact build must not enable fixtures: {artifact:?}"
    );
    assert_eq!(
        checks["requested_production_features"],
        Value::Array(vec![])
    );
    let build_rs = std::fs::read_to_string(crate_dir().join("build.rs")).expect("build.rs");
    assert!(
        build_rs.contains("&[\"desktop_dispatch\", \"desktop_observe\"]"),
        "build.rs registers exactly the two commands"
    );
    let lib = std::fs::read_to_string(crate_dir().join("src/lib.rs")).expect("lib.rs");
    assert!(
        lib.contains("generate_handler![desktop_dispatch, desktop_observe]"),
        "handler registers exactly the two commands"
    );
}
