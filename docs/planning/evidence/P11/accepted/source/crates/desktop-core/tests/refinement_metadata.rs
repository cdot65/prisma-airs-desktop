#![cfg(feature = "test-fixtures")]

use desktop_core::features::TenantEnvironment;
use desktop_core::fixtures::{self, ScriptedStore};
use desktop_core::store::PersistedState;
use desktop_core::tenants::{Destination, Region, TenantId, TenantProfile};
use desktop_core::{
    Core, CoreConfig, Delivery, Intent, Observation, Refusal, ShellSnapshot, ViewScope,
};
use serde_json::json;

fn profile(id: &str, label: &str) -> TenantProfile {
    TenantProfile {
        id: TenantId::parse(id).expect("fixture id"),
        label: label.to_owned(),
        tsg_id: Some("1234567890".to_owned()),
        destination: Destination::Region {
            region: Region::Europe,
        },
        environment: TenantEnvironment::Production,
    }
}
fn snapshot(core: &Core) -> ShellSnapshot {
    match core.operator().observe(ViewScope::Shell).expect("snapshot") {
        Observation::Shell { snapshot, .. } => snapshot,
        _ => panic!("shell expected"),
    }
}
fn state() -> PersistedState {
    let a = profile("tn-0123456789abcdef", "A");
    let b = profile("tn-fedcba9876543210", "B");
    PersistedState {
        tenants: vec![a.clone(), b],
        active_tenant: Some(a.id),
        ..PersistedState::default()
    }
}
#[tokio::test]
async fn metadata_edit_targets_only_the_selected_profile_without_rebinding_identity() {
    let (adapters, handles) = fixtures::adapters_with(ScriptedStore::with_state(state()));
    let core = Core::new(
        CoreConfig {
            fixture_tenant_creation: false,
            ..CoreConfig::default()
        },
        adapters,
    );
    core.start();
    let before = snapshot(&core);
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    let target = before.tenants[1].id.clone();
    handle
        .dispatch(
            Delivery {
                session,
                sequence: 1,
            },
            Intent::EditTenant {
                tenant: target.clone(),
                label: "Renamed B".to_owned(),
                environment: TenantEnvironment::Staging,
            },
        )
        .await
        .expect("metadata edit");
    let after = snapshot(&core);
    assert_eq!(
        after.active, before.active,
        "metadata never switches the active tenant"
    );
    assert_eq!(after.tenants[0], before.tenants[0]);
    let changed = &after.tenants[1];
    assert_eq!(changed.id, target);
    assert_eq!(changed.label, "Renamed B");
    assert_eq!(changed.environment, TenantEnvironment::Staging);
    assert_eq!(changed.tsg_id, before.tenants[1].tsg_id);
    assert_eq!(changed.destination, before.tenants[1].destination);
    assert_eq!(changed.management, before.tenants[1].management);
    assert_eq!(changed.scan, before.tenants[1].scan);
    assert_eq!(
        handles.store.saved().expect("saved").tenants[1].label,
        "Renamed B"
    );
}
#[tokio::test]
async fn metadata_edit_rolls_back_when_private_persistence_fails() {
    let (adapters, handles) = fixtures::adapters_with(ScriptedStore::with_state(state()));
    let core = Core::new(
        CoreConfig {
            fixture_tenant_creation: false,
            ..CoreConfig::default()
        },
        adapters,
    );
    core.start();
    let before = snapshot(&core);
    handles.store.fail_saves(true);
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    assert!(
        handle
            .dispatch(
                Delivery {
                    session,
                    sequence: 1
                },
                Intent::EditTenant {
                    tenant: before.tenants[0].id.clone(),
                    label: "Not saved".to_owned(),
                    environment: TenantEnvironment::Other,
                }
            )
            .await
            .is_err()
    );
    let after = snapshot(&core);
    assert_eq!(after.tenants, before.tenants);
    assert_eq!(after.active, before.active);
    assert_eq!(
        handles.store.saved().expect("old saved state").tenants[0].label,
        "A"
    );
}
#[tokio::test]
async fn metadata_edit_rejects_invalid_labels_and_has_no_tsg_or_endpoint_fields() {
    let (adapters, _) = fixtures::adapters_with(ScriptedStore::with_state(state()));
    let core = Core::new(
        CoreConfig {
            fixture_tenant_creation: false,
            ..CoreConfig::default()
        },
        adapters,
    );
    core.start();
    let before = snapshot(&core);
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    assert_eq!(
        handle
            .dispatch(
                Delivery {
                    session,
                    sequence: 1
                },
                Intent::EditTenant {
                    tenant: before.tenants[0].id.clone(),
                    label: "   ".to_owned(),
                    environment: TenantEnvironment::Other,
                }
            )
            .await,
        Err(Refusal::InvalidInput)
    );
    for (field, value) in [
        ("tsgId", json!("9999999999")),
        ("destination", json!({"kind":"region", "region":"india"})),
    ] {
        let mut intent = json!({"kind":"editTenant", "tenant":before.tenants[0].id, "label":"A", "environment":"production"});
        intent[field] = value;
        assert!(
            serde_json::from_value::<Intent>(intent).is_err(),
            "unreviewed identity field {field}"
        );
    }
    assert_eq!(snapshot(&core).tenants, before.tenants);
}
