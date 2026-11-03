//! L19 belongs to the real wizard, never the general C25 live runner.

use serde_json::json;
use xtask::live;

use super::common::{Browser, Fixture, designate, manifest, secrets};

fn wizard_only() -> serde_json::Value {
    let mut value = manifest();
    designate(
        &mut value,
        "L19",
        json!({
            "environment": "test-b",
            "credentials": {"client_secret": super::common::keychain("wizard-not-resolved")},
            "nonsecret": {"client_id": "wizard@example.test", "tsg_id": "1852583913", "region": "us"},
            "permitted_operations": [live::WIZARD_VERIFY_OPERATION],
            "budget": {"reads": 2}
        }),
    );
    value
}

#[tokio::test]
async fn an_optional_wizard_designation_never_enters_default_c25_execution() {
    let (_, result) = Fixture::new()
        .run(wizard_only(), &[], &secrets(&[]), Browser::idle())
        .await;
    let record = result.expect("L19 must not be dispatched by the default C25 run");
    assert!(record.families.is_empty());
}

#[tokio::test]
async fn explicitly_selecting_l19_refuses_before_resolving_its_secret() {
    let (_, result) = Fixture::new()
        .run(wizard_only(), &["L19"], &secrets(&[]), Browser::idle())
        .await;
    let error = result.expect_err("L19 requires the native first-run wizard");
    assert!(
        error
            .to_string()
            .contains("L19 requires the native first-run wizard")
    );
}
