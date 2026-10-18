//! C02 context, credential and lifecycle contracts through the production
//! authority interface (dispatch/observe) with scripted vault, store and
//! picker adapters, plus native transport/SDK binding contracts.

// Shared with the sync_scan target, which uses replies this target does not.
#[allow(dead_code)]
mod support;
mod transport;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use desktop_core::context::ContextRef;
use desktop_core::fixtures::{self, Handles, ScriptedStore, VaultCall};
use desktop_core::runtime::{
    Claim, OperationLimits, OperationState, Outcome, Settlement, StopReason,
};
use desktop_core::store::{AppStore, PersistedState, StoredCredential};
use desktop_core::tenants::{Destination, Region, TenantId, TenantProfile};
use desktop_core::transport::TransportPolicy;
use desktop_core::vault::{RecordId, VaultError};
use desktop_core::{
    AuthorityHandle, Core, CoreConfig, Delivery, ImportView, Intent, Observation, Receipt, Refusal,
    ResetFailure, ResetStage, ScanState, ShellSnapshot, StorageHealth, Theme, ViewScope,
};
use serde_json::json;

use crate::support::eventually;

const KEY_A: &str = "synthetic-scan-key-aaaa";
const KEY_B: &str = "synthetic-scan-key-bbbb";

struct Desk {
    core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
}

impl Desk {
    fn new() -> Self {
        Self::with_store(ScriptedStore::default())
    }

    fn with_store(store: ScriptedStore) -> Self {
        Self::with(store, CoreConfig::default())
    }

    fn with(store: ScriptedStore, config: CoreConfig) -> Self {
        let (adapters, handles) = fixtures::adapters_with(store);
        let core = Core::new(config, adapters);
        core.start();
        let handle = core.operator();
        let session = handle.register_delivery_session().expect("session").nonce;
        Self {
            core,
            handle,
            session,
            sequence: 0,
            adapters: handles,
        }
    }

    /// The same storage and vault, as after an app restart.
    fn restart(self) -> Self {
        let state = self.adapters.store.saved().unwrap_or_default();
        self.restart_from(state)
    }

    /// A crash: only the last confirmed-durable document survives; the
    /// Keychain keeps everything that was written.
    fn crash_restart(self) -> Self {
        let state = self.adapters.store.durable().unwrap_or_default();
        self.restart_from(state)
    }

    fn restart_from(self, state: PersistedState) -> Self {
        let vault = Arc::clone(&self.adapters.vault);
        drop(self);
        let (mut adapters, handles) = fixtures::adapters_with(ScriptedStore::with_state(state));
        adapters.keychain = vault.clone();
        let core = Core::new(CoreConfig::default(), adapters);
        core.start();
        let handle = core.operator();
        let session = handle.register_delivery_session().expect("session").nonce;
        Self {
            core,
            handle,
            session,
            sequence: 0,
            adapters: Handles { vault, ..handles },
        }
    }

    async fn send(&mut self, intent: serde_json::Value) -> Result<Receipt, Refusal> {
        let intent: Intent = serde_json::from_value(intent).expect("intent");
        self.sequence += 1;
        self.handle
            .dispatch(
                Delivery {
                    session: self.session.clone(),
                    sequence: self.sequence,
                },
                intent,
            )
            .await
    }

    fn snap(&self) -> ShellSnapshot {
        match self.handle.observe(ViewScope::Shell).expect("observe") {
            Observation::Shell { snapshot, .. } => snapshot,
            _ => unreachable!("shell scope"),
        }
    }

    fn context(&self) -> String {
        self.snap()
            .active
            .expect("active")
            .context
            .as_str()
            .to_owned()
    }

    fn tenant(&self, label: &str) -> String {
        self.snap()
            .tenants
            .iter()
            .find(|t| t.label == label)
            .expect("tenant")
            .id
            .as_str()
            .to_owned()
    }

    fn scan_state(&self, label: &str) -> ScanState {
        self.snap()
            .tenants
            .iter()
            .find(|t| t.label == label)
            .expect("tenant")
            .scan
            .state
    }

    async fn create(&mut self, label: &str) -> String {
        self.send(json!({"kind": "createTenant", "profile": {
            "label": label, "tsgId": "1234567890",
            "destination": {"kind": "region", "region": "europe"}
        }}))
        .await
        .expect("created");
        self.tenant(label)
    }

    async fn configure(&mut self, mode: &str, key: &str) -> Result<Receipt, Refusal> {
        let context = self.context();
        self.send(json!({"kind": "configureScanCredential", "context": context, "mode": mode, "key": key}))
            .await
    }

    async fn settled(&self, label: &str) -> ScanState {
        assert!(
            eventually(|| self.scan_state(label) != ScanState::Storing).await,
            "write settled"
        );
        self.scan_state(label)
    }

    async fn ledger_empty(&self) -> bool {
        eventually(|| {
            self.adapters
                .store
                .saved()
                .is_some_and(|s| s.pending_cleanup.is_empty())
        })
        .await
    }
}

struct Release(Arc<desktop_core::fixtures::ScriptedVault>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn redacted(snapshot: &ShellSnapshot) -> String {
    serde_json::to_string(snapshot).expect("json")
}

#[tokio::test]
async fn tenants_are_created_selected_and_contexts_never_revive() {
    let mut desk = Desk::new();
    assert!(desk.snap().active.is_none());
    let a = desk.create("Tenant A").await;
    let first = desk.context();
    assert_eq!(
        desk.snap().active.expect("active").tenant.as_str(),
        a,
        "first tenant becomes active"
    );
    let b = desk.create("Tenant B").await;
    assert_eq!(
        desk.context(),
        first,
        "creating another tenant keeps the context"
    );
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("B");
    let on_b = desk.context();
    desk.send(json!({"kind": "selectTenant", "tenant": a}))
        .await
        .expect("A again");
    let back_on_a = desk.context();
    assert!(
        first != on_b && on_b != back_on_a && first != back_on_a,
        "A→B→A issues a third reference"
    );
    let stale = desk
        .send(json!({"kind": "removeScanCredential", "context": first}))
        .await;
    assert_eq!(stale, Err(Refusal::StaleContext));
    let unknown = desk
        .send(json!({"kind": "selectTenant", "tenant": "tn-ffffffffffffffff"}))
        .await;
    assert_eq!(unknown, Err(Refusal::InvalidInput));
    let saved = desk.adapters.store.saved().expect("saved");
    assert_eq!(saved.tenants.len(), 2);
    assert_eq!(
        saved.active_tenant.as_ref().map(TenantId::as_str),
        Some(a.as_str())
    );
}

#[tokio::test]
async fn invalid_profiles_are_refused_and_nothing_is_stored() {
    let mut desk = Desk::new();
    for profile in [
        json!({"label": "", "destination": {"kind": "region", "region": "europe"}}),
        json!({"label": "x", "tsgId": "12ab", "destination": {"kind": "region", "region": "europe"}}),
        json!({"label": "x", "destination": {"kind": "custom", "origin": "http://plain.example.com"}}),
        json!({"label": "x", "destination": {"kind": "custom", "origin": "https://user:pw@scan.example.com"}}),
    ] {
        let refused = desk
            .send(json!({"kind": "createTenant", "profile": profile}))
            .await;
        assert_eq!(refused, Err(Refusal::InvalidInput));
    }
    assert!(desk.snap().tenants.is_empty());
    assert_eq!(desk.adapters.store.saves(), 0);
}

#[tokio::test]
async fn a_keychain_scan_key_is_stored_indexed_and_never_observed() {
    let mut desk = Desk::new();
    let a = desk.create("Tenant A").await;
    let before = desk.context();
    desk.configure("keychain", KEY_A).await.expect("admitted");
    assert_ne!(
        desk.context(),
        before,
        "credential replacement issues a new context"
    );
    assert_eq!(desk.settled("Tenant A").await, ScanState::Ready);
    let records = desk.adapters.vault.records();
    assert_eq!(records.len(), 1);
    assert_eq!(
        desk.adapters.vault.secret(&records[0]).as_deref(),
        Some(KEY_A.as_bytes())
    );
    assert!(
        desk.ledger_empty().await,
        "the write-ahead entry is committed"
    );
    let saved = desk.adapters.store.saved().expect("saved");
    assert_eq!(
        saved.credentials,
        vec![StoredCredential {
            record: records[0].clone(),
            tenant: TenantId::parse(&a).expect("id"),
            destination: Region::Europe.origin().to_owned(),
            class: desktop_core::store::CredentialClass::Scan,
            client_id: None,
        }]
    );
    let everything = format!("{} {:?}", redacted(&desk.snap()), saved);
    assert!(
        !everything.contains(KEY_A),
        "no key material in snapshots or saved state"
    );
}

#[tokio::test]
async fn unusable_keys_and_stale_contexts_store_nothing() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    for key in ["", "has\nnewline"] {
        assert_eq!(
            desk.configure("keychain", key).await,
            Err(Refusal::InvalidInput)
        );
    }
    assert_eq!(
        desk.send(
            json!({"kind": "configureScanCredential", "context": "cx-000000000000000000000000",
            "mode": "keychain", "key": KEY_A})
        )
        .await,
        Err(Refusal::StaleContext)
    );
    assert!(desk.adapters.vault.calls().is_empty());
}

#[tokio::test]
async fn session_only_keys_are_never_persisted_and_end_with_the_app() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.configure("session", KEY_A).await.expect("admitted");
    assert_eq!(desk.settled("Tenant A").await, ScanState::Ready);
    assert_eq!(desk.handle.session_credentials(), 1);
    assert!(
        desk.adapters.vault.calls().is_empty(),
        "the Keychain is not touched"
    );
    // Any later save must still leave the session key out of the document.
    desk.send(json!({"kind": "setAppearance", "theme": "dark"}))
        .await
        .expect("saved later");
    let saved = desk.adapters.store.saved().expect("saved");
    assert_eq!(
        saved.appearance,
        Theme::Dark,
        "a save happened after the key"
    );
    assert!(saved.credentials.is_empty() && saved.pending_cleanup.is_empty());
    let context = ContextRef::clone(&desk.snap().active.expect("active").context);
    let (_, secret) = desk
        .handle
        .read_scan_credential(&context)
        .await
        .expect("readable now");
    assert_eq!(secret.as_slice(), KEY_A.as_bytes());
    let desk = desk.restart();
    assert_eq!(
        desk.scan_state("Tenant A"),
        ScanState::Missing,
        "gone after restart"
    );
    assert_eq!(desk.handle.session_credentials(), 0);
}

#[tokio::test]
async fn sign_out_and_replacement_retire_the_old_record() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.configure("keychain", KEY_A).await.expect("first");
    desk.settled("Tenant A").await;
    let first = desk.adapters.vault.records();
    desk.configure("keychain", KEY_B).await.expect("replace");
    desk.settled("Tenant A").await;
    assert!(desk.ledger_empty().await);
    let second = desk.adapters.vault.records();
    assert_eq!(second.len(), 1);
    assert_ne!(
        first, second,
        "a new immutable record; the old one is deleted"
    );
    assert_eq!(
        desk.adapters.vault.secret(&second[0]).as_deref(),
        Some(KEY_B.as_bytes())
    );
    let context = desk.context();
    desk.send(json!({"kind": "removeScanCredential", "context": context}))
        .await
        .expect("sign out");
    assert_eq!(desk.scan_state("Tenant A"), ScanState::Missing);
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
    assert!(desk.ledger_empty().await);
}

#[tokio::test]
async fn a_switch_during_a_write_leaves_the_stale_record_unattached_and_cleaned() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let b = desk.create("Tenant B").await;
    desk.adapters.vault.hold_writes();
    // Releases held writes even if an assertion fails, so a failing test
    // ends instead of waiting on a blocked vault call at shutdown.
    let _release = Release(Arc::clone(&desk.adapters.vault));
    desk.configure("keychain", KEY_A).await.expect("admitted");
    assert!(eventually(|| desk.adapters.vault.waiting() == 1).await);
    assert_eq!(
        desk.configure("keychain", KEY_B).await,
        Err(Refusal::Busy),
        "one write at a time"
    );
    // Sign out while the write is in flight, then switch tenants.
    let context = desk.context();
    desk.send(json!({"kind": "removeScanCredential", "context": context}))
        .await
        .expect("signed out");
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("switched");
    let late = desk
        .adapters
        .vault
        .calls()
        .into_iter()
        .find_map(|c| match c {
            VaultCall::Put(record) => Some(record),
            _ => None,
        })
        .expect("the held write");
    // While the write is in flight its write-ahead entry stays in the saved
    // ledger and cleanup does not touch it (a delete could precede the write).
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        desk.adapters
            .store
            .saved()
            .is_some_and(|s| s.pending_cleanup.contains(&late))
    );
    assert!(
        !desk
            .adapters
            .vault
            .calls()
            .contains(&VaultCall::Delete(late.clone()))
    );
    desk.adapters.vault.release();
    // The late write must land and then be deleted: checking for an empty
    // vault alone could pass before the write arrives.
    let landed_then_deleted = || {
        let calls = desk.adapters.vault.calls();
        let put = calls
            .iter()
            .position(|c| c == &VaultCall::Put(late.clone()));
        let deleted_after = calls
            .iter()
            .rposition(|c| c == &VaultCall::Delete(late.clone()));
        desk.adapters.vault.waiting() == 0
            && matches!((put, deleted_after), (Some(p), Some(d)) if d > p)
            && desk.adapters.vault.records().is_empty()
    };
    assert!(
        eventually(landed_then_deleted).await,
        "the late write is deleted after it lands"
    );
    assert!(desk.adapters.vault.secret(&late).is_none());
    assert_eq!(
        desk.scan_state("Tenant A"),
        ScanState::Missing,
        "never attached"
    );
    assert_eq!(
        desk.scan_state("Tenant B"),
        ScanState::Missing,
        "never attached elsewhere"
    );
    assert!(desk.ledger_empty().await);
}

#[tokio::test]
async fn locked_and_denied_stores_fail_explicitly_without_fallback() {
    for (failure, expected) in [
        (VaultError::Locked, VaultError::Locked),
        (VaultError::Denied, VaultError::Denied),
    ] {
        let mut desk = Desk::new();
        desk.create("Tenant A").await;
        desk.adapters.vault.fail_with(Some(failure));
        desk.configure("keychain", KEY_A).await.expect("admitted");
        assert_eq!(desk.settled("Tenant A").await, ScanState::Failed);
        let view = desk.snap().tenants[0].scan.clone();
        assert_eq!(view.failure, Some(expected));
        assert_eq!(desk.handle.session_credentials(), 0, "no session fallback");
        let saved = desk.adapters.store.saved().expect("saved");
        assert!(saved.credentials.is_empty());
        assert!(
            !format!("{saved:?}").contains(KEY_A),
            "no plaintext persisted"
        );
        // Reads of a Keychain credential map the same way.
        desk.adapters.vault.fail_with(None);
        desk.configure("keychain", KEY_A).await.expect("retry");
        assert_eq!(desk.settled("Tenant A").await, ScanState::Ready);
        desk.adapters.vault.fail_with(Some(failure));
        let context = ContextRef::clone(&desk.snap().active.expect("active").context);
        let read = desk.handle.read_scan_credential(&context).await.map(|_| ());
        let refusal = if failure == VaultError::Locked {
            Refusal::LockedStore
        } else {
            Refusal::DeniedStore
        };
        assert_eq!(read, Err(refusal));
    }
}

#[tokio::test]
async fn a_failed_save_refuses_and_changes_nothing() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let before = desk.snap();
    desk.adapters.store.fail_saves(true);
    let refused = desk
        .send(json!({"kind": "createTenant", "profile": {"label": "B", "destination": {"kind": "region", "region": "india"}}}))
        .await;
    assert_eq!(refused, Err(Refusal::DeniedStore));
    assert_eq!(
        desk.configure("keychain", KEY_A).await,
        Err(Refusal::DeniedStore)
    );
    let after = desk.snap();
    assert_eq!(after.tenants, before.tenants);
    assert_eq!(after.active, before.active, "no context change");
    assert!(desk.adapters.vault.calls().is_empty());
}

#[tokio::test]
async fn restart_restores_profiles_credentials_and_appearance_and_finishes_cleanup() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.configure("keychain", KEY_A).await.expect("stored");
    desk.settled("Tenant A").await;
    desk.send(json!({"kind": "setAppearance", "theme": "dark"}))
        .await
        .expect("dark");
    // A record left by an interrupted earlier write (in the ledger).
    let orphan = RecordId::parse("cr-00000000000000aa").expect("id");
    desk.adapters.vault.plant(&orphan, b"stale");
    let mut state = desk.adapters.store.saved().expect("saved");
    state.pending_cleanup.push(orphan.clone());
    desk.adapters.store.save(&state).expect("save");
    let desk = desk.restart();
    let snap = desk.snap();
    assert_eq!(snap.appearance, Theme::Dark);
    assert_eq!(desk.scan_state("Tenant A"), ScanState::Ready);
    assert!(snap.active.is_some());
    assert!(
        eventually(|| desk.adapters.vault.secret(&orphan).is_none()).await,
        "ledgered orphan deleted at start"
    );
    assert!(desk.ledger_empty().await);
    let context = ContextRef::clone(&snap.active.expect("active").context);
    let (origin, secret) = desk
        .handle
        .read_scan_credential(&context)
        .await
        .expect("read");
    assert_eq!(
        (origin.as_str(), secret.as_slice()),
        (Region::Europe.origin(), KEY_A.as_bytes())
    );
}

#[tokio::test]
async fn endpoint_changes_are_reviewed_and_never_carry_the_old_key() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.configure("keychain", KEY_A).await.expect("stored");
    desk.settled("Tenant A").await;
    let context = desk.context();
    let custom = "https://airs-proxy.example.com:8443";
    desk.send(json!({"kind": "reviewEndpointChange", "context": context,
        "destination": {"kind": "custom", "origin": format!("{custom}/")}}))
        .await
        .expect("review");
    let review = desk.snap().endpoint_review.expect("review");
    assert_eq!(review.from_origin, Region::Europe.origin());
    assert_eq!(review.to_origin, custom);
    assert!(review.removes_scan_credential);
    assert_eq!(
        desk.scan_state("Tenant A"),
        ScanState::Ready,
        "nothing changes before confirmation"
    );
    let id = review.review.clone();
    let wrong = desk
        .send(json!({"kind": "confirmEndpointChange", "context": "cx-000000000000000000000000", "review": id}))
        .await;
    assert_eq!(wrong, Err(Refusal::StaleContext));
    desk.send(json!({"kind": "confirmEndpointChange", "context": context, "review": id}))
        .await
        .expect("confirmed");
    let snap = desk.snap();
    assert_eq!(snap.tenants[0].origin, custom);
    assert_eq!(
        snap.tenants[0].scan.state,
        ScanState::Missing,
        "the old key is not rebound"
    );
    assert_ne!(
        snap.active.expect("active").context.as_str(),
        context,
        "prior context retired"
    );
    assert!(
        eventually(|| desk.adapters.vault.records().is_empty()).await,
        "old key deleted"
    );
    let reused = desk
        .send(json!({"kind": "confirmEndpointChange", "context": desk.context(), "review": id}))
        .await;
    assert_eq!(reused, Err(Refusal::ReviewConsumed));
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(saved.credentials.is_empty());
    assert_eq!(saved.tenants[0].destination.origin(), custom);
}

#[tokio::test]
async fn a_context_change_discards_a_pending_endpoint_review() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let b = desk.create("Tenant B").await;
    let context = desk.context();
    desk.send(json!({"kind": "reviewEndpointChange", "context": context,
        "destination": {"kind": "region", "region": "singapore"}}))
        .await
        .expect("review");
    let id = desk.snap().endpoint_review.expect("review").review;
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("switch");
    assert!(desk.snap().endpoint_review.is_none());
    let confirm = desk
        .send(json!({"kind": "confirmEndpointChange", "context": desk.context(), "review": id}))
        .await;
    assert_eq!(confirm, Err(Refusal::ReviewInvalid));
}

#[tokio::test]
async fn profile_import_previews_whitelisted_metadata_and_creates_keyless_tenants() {
    let mut desk = Desk::new();
    desk.create("lab").await;
    let source = json!({
        "version": 1,
        "tenants": [
            {"name": "prod", "configPath": "/Users/someone/.config/airs/prod.json", "tsgId": "1111111111"},
            {"name": "lab", "configPath": "/tmp/lab.json", "tsgId": "2222", "airsApiKey": "synthetic-embedded-secret"},
            {"name": "", "configPath": "/tmp/x.json"}
        ]
    });
    desk.adapters.picker.reply(Some(Ok((
        "/Users/someone/tenants.json".into(),
        serde_json::to_vec(&source).expect("json"),
    ))));
    desk.send(json!({"kind": "chooseProfileImport"}))
        .await
        .expect("sheet");
    assert!(eventually(|| matches!(desk.snap().import, ImportView::Review { .. })).await);
    let ImportView::Review {
        review,
        file_name,
        candidates,
        discarded,
        ..
    } = desk.snap().import
    else {
        unreachable!()
    };
    assert_eq!(file_name, "tenants.json", "no directory path");
    assert_eq!(candidates.len(), 3);
    assert!(candidates[1].existing_label);
    assert!(candidates[2].problem.is_some());
    assert_eq!((discarded.secret_fields, discarded.file_references), (1, 3));
    let shown = redacted(&desk.snap());
    assert!(!shown.contains("synthetic-embedded-secret") && !shown.contains("/tmp/lab.json"));
    assert_eq!(
        desk.send(json!({"kind": "confirmProfileImport", "review": review, "selected": [2]}))
            .await,
        Err(Refusal::InvalidInput),
        "an invalid entry cannot be imported"
    );
    desk.send(json!({"kind": "confirmProfileImport", "review": review, "selected": [0, 1]}))
        .await
        .expect("imported");
    let snap = desk.snap();
    let labels: Vec<_> = snap.tenants.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(
        labels,
        vec!["lab", "prod", "lab"],
        "the existing tenant is unchanged"
    );
    assert!(
        snap.tenants
            .iter()
            .all(|t| t.scan.state == ScanState::Missing),
        "no credential imported"
    );
    assert!(matches!(snap.import, ImportView::Idle));
    assert_eq!(
        desk.send(json!({"kind": "confirmProfileImport", "review": review, "selected": [0]}))
            .await,
        Err(Refusal::ReviewConsumed)
    );
    assert!(desk.adapters.vault.calls().is_empty());
}

#[tokio::test]
async fn cancelled_or_unsupported_import_leaves_tenants_unchanged() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let before = desk.snap().tenants;
    desk.adapters.picker.reply(None);
    desk.send(json!({"kind": "chooseProfileImport"}))
        .await
        .expect("sheet");
    assert!(eventually(|| matches!(desk.snap().import, ImportView::Idle)).await);
    desk.adapters
        .picker
        .reply(Some(Ok(("notes.txt".into(), b"just text".to_vec()))));
    desk.send(json!({"kind": "chooseProfileImport"}))
        .await
        .expect("sheet");
    assert!(eventually(|| matches!(desk.snap().import, ImportView::Failed { .. })).await);
    assert_eq!(desk.snap().tenants, before);
    assert_eq!(desk.adapters.picker.opened(), 2);
}

#[tokio::test]
async fn reset_purges_the_namespace_and_removes_the_document() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.configure("keychain", KEY_A).await.expect("stored");
    desk.settled("Tenant A").await;
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset admitted");
    assert!(
        eventually(|| desk.snap().tenants.is_empty() && desk.snap().storage == StorageHealth::Ready)
            .await
    );
    assert!(desk.adapters.vault.records().is_empty());
    assert!(desk.adapters.vault.calls().contains(&VaultCall::Purge));
    assert!(
        desk.adapters.store.saved().is_none(),
        "the saved document is removed"
    );
    let snap = desk.snap();
    assert!(snap.active.is_none() && snap.reset_failure.is_none());
}

#[tokio::test]
async fn a_failed_purge_retires_keys_durably_and_cleanup_finishes_after_restart() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.configure("keychain", KEY_A).await.expect("stored");
    desk.settled("Tenant A").await;
    let record = desk.adapters.vault.records()[0].clone();
    let old = ContextRef::clone(&desk.snap().active.expect("active").context);
    desk.adapters.vault.fail_with(Some(VaultError::Locked));
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset admitted");
    assert!(eventually(|| desk.snap().reset_failure.is_some()).await);
    let snap = desk.snap();
    assert_eq!(
        snap.reset_failure,
        Some(ResetFailure {
            stage: ResetStage::Credentials,
            reason: VaultError::Locked
        })
    );
    assert_eq!(snap.tenants.len(), 1, "profiles are kept");
    assert_eq!(
        snap.tenants[0].scan.state,
        ScanState::Missing,
        "never shown as saved"
    );
    let fresh = snap.active.expect("a fresh context").context;
    assert_ne!(fresh, old, "no earlier reference revives");
    assert_eq!(
        desk.handle.read_scan_credential(&old).await.map(|_| ()),
        Err(Refusal::StaleContext)
    );
    let saved = desk.adapters.store.saved().expect("saved");
    assert!(
        saved.credentials.is_empty(),
        "the retirement was saved first"
    );
    assert!(
        saved.pending_cleanup.contains(&record),
        "the key stays tracked for cleanup"
    );
    desk.adapters.vault.fail_with(None);
    let desk = desk.restart();
    assert!(desk.ledger_empty().await, "cleanup resumes after restart");
    assert!(desk.adapters.vault.records().is_empty());
    assert_eq!(desk.scan_state("Tenant A"), ScanState::Missing);
}

#[tokio::test]
async fn a_partial_purge_and_a_failed_document_removal_report_their_stage() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let b = desk.create("Tenant B").await;
    desk.configure("keychain", KEY_A).await.expect("A");
    desk.settled("Tenant A").await;
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("B");
    desk.configure("keychain", KEY_B).await.expect("B key");
    desk.settled("Tenant B").await;
    assert_eq!(desk.adapters.vault.records().len(), 2);
    desk.adapters
        .vault
        .fail_purge_after(Some((1, VaultError::Denied)));
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| desk.snap().reset_failure.is_some()).await);
    let snap = desk.snap();
    assert_eq!(
        snap.reset_failure.map(|f| f.stage),
        Some(ResetStage::Credentials)
    );
    assert!(
        snap.tenants
            .iter()
            .all(|t| t.scan.state == ScanState::Missing),
        "no tenant claims a deleted or retired key"
    );
    // The interrupted purge is finished by the durable cleanup ledger.
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
    assert!(desk.ledger_empty().await);
    // A fresh context after the failed reset accepts a new key, so the next
    // reset has a credential to delete.
    desk.configure("keychain", KEY_A)
        .await
        .expect("a new key after a failed reset");
    assert_eq!(desk.settled("Tenant B").await, ScanState::Ready);
    // The purge now succeeds but removing the document fails.
    desk.adapters.vault.fail_purge_after(None);
    desk.adapters.store.fail_resets(true);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| desk.snap().reset_failure.is_some()).await);
    assert_eq!(
        desk.snap().reset_failure.map(|f| f.stage),
        Some(ResetStage::Document)
    );
    assert!(
        desk.adapters.vault.records().is_empty(),
        "credentials were deleted"
    );
    desk.adapters.store.fail_resets(false);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("retry");
    assert!(
        eventually(|| desk.snap().tenants.is_empty() && desk.snap().reset_failure.is_none()).await
    );
    assert!(desk.adapters.store.saved().is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_waits_for_in_flight_writes_and_retires_authority_at_admission() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let old = ContextRef::clone(&desk.snap().active.expect("active").context);
    desk.adapters.vault.hold_writes();
    let _release = Release(Arc::clone(&desk.adapters.vault));
    desk.configure("keychain", KEY_A).await.expect("held write");
    assert!(eventually(|| desk.adapters.vault.waiting() == 1).await);
    let held = desk.context();
    let held_ref = ContextRef::clone(&desk.snap().active.expect("active").context);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset admitted");
    let snap = desk.snap();
    assert_eq!(snap.storage, StorageHealth::Resetting);
    assert!(snap.active.is_none(), "no context while resetting");
    for context in [&old, &held_ref] {
        assert_eq!(
            desk.handle.read_scan_credential(context).await.map(|_| ()),
            Err(Refusal::StaleContext)
        );
        let admitted = desk.handle.admit_fixture_operation(
            context,
            1,
            Box::new(|_| Box::pin(async { Outcome::Completed })),
        );
        assert_eq!(admitted.err(), Some(Refusal::StaleContext));
    }
    assert_eq!(
        desk.send(json!({"kind": "configureScanCredential", "context": held, "mode": "session", "key": KEY_B}))
            .await,
        Err(Refusal::StaleContext)
    );
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !desk.adapters.vault.calls().contains(&VaultCall::Purge),
        "no purge while a write is in flight"
    );
    assert!(
        desk.adapters
            .store
            .saved()
            .is_some_and(|s| s.credentials.is_empty() && !s.pending_cleanup.is_empty()),
        "the retirement and the held record are saved before anything is deleted"
    );
    desk.adapters.vault.release();
    assert!(
        eventually(|| desk.snap().storage == StorageHealth::Ready && desk.snap().tenants.is_empty())
            .await
    );
    let calls = desk.adapters.vault.calls();
    let put = calls.iter().position(|c| matches!(c, VaultCall::Put(_)));
    let purge = calls.iter().position(|c| c == &VaultCall::Purge);
    assert!(
        matches!((put, purge), (Some(p), Some(q)) if q > p),
        "the write settled before the purge"
    );
    assert!(
        desk.adapters.vault.records().is_empty(),
        "nothing survives the reset"
    );
    assert!(desk.adapters.store.saved().is_none());
}

/// Every record in the vault is owned by the given durable document: either
/// indexed as a credential or listed for cleanup.
fn owned_by(state: &PersistedState, records: &[RecordId]) -> bool {
    records.iter().all(|r| {
        state.credentials.iter().any(|c| &c.record == r) || state.pending_cleanup.contains(r)
    })
}

#[tokio::test]
async fn a_first_key_is_written_only_after_a_confirmed_write_ahead() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.adapters.store.unconfirmed_saves(true);
    desk.configure("keychain", KEY_A).await.expect("admitted");
    let view = desk.snap().tenants[0].scan.clone();
    assert_eq!(
        (view.state, view.failure),
        (ScanState::Failed, Some(VaultError::Unavailable)),
        "not stored, and not shown as stored"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !desk
            .adapters
            .vault
            .calls()
            .iter()
            .any(|c| matches!(c, VaultCall::Put(_))),
        "no secret is written without durable ownership"
    );
    let desk = desk.crash_restart();
    assert!(desk.adapters.vault.records().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replacement_is_written_only_after_a_confirmed_write_ahead() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.configure("keychain", KEY_A).await.expect("first key");
    assert_eq!(desk.settled("Tenant A").await, ScanState::Ready);
    assert!(desk.ledger_empty().await);
    desk.adapters.store.unconfirmed_saves(true);
    desk.configure("keychain", KEY_B).await.expect("admitted");
    assert_eq!(desk.scan_state("Tenant A"), ScanState::Failed);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let puts = desk
        .adapters
        .vault
        .calls()
        .iter()
        .filter(|c| matches!(c, VaultCall::Put(_)))
        .count();
    assert_eq!(puts, 1, "the replacement secret was never written");
    let durable = desk.adapters.store.durable().expect("durable");
    let desk = desk.crash_restart();
    assert!(owned_by(&durable, &desk.adapters.vault.records()));
    assert_eq!(
        desk.scan_state("Tenant A"),
        ScanState::Ready,
        "after the crash the last durable state (the first key) is what remains"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_confirming_retry_lets_the_write_proceed() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.adapters.store.unconfirm_next(1);
    desk.configure("keychain", KEY_A).await.expect("admitted");
    assert_eq!(desk.settled("Tenant A").await, ScanState::Ready);
    assert!(
        !desk.snap().durability_unconfirmed,
        "the retry confirmed it"
    );
    let durable = desk.adapters.store.durable().expect("durable");
    assert!(owned_by(&durable, &desk.adapters.vault.records()));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unconfirmed_commit_after_a_landed_write_stays_owned_by_the_ledger() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.adapters.vault.hold_writes();
    let _release = Release(Arc::clone(&desk.adapters.vault));
    desk.configure("keychain", KEY_A)
        .await
        .expect("confirmed write-ahead");
    assert!(eventually(|| desk.adapters.vault.waiting() == 1).await);
    desk.adapters.store.unconfirmed_saves(true);
    desk.adapters.vault.release();
    assert_eq!(desk.settled("Tenant A").await, ScanState::Ready);
    let durable = desk.adapters.store.durable().expect("durable");
    assert!(
        owned_by(&durable, &desk.adapters.vault.records()),
        "the landed secret is owned by the last durable ledger"
    );
    let desk = desk.crash_restart();
    assert!(
        eventually(|| desk.adapters.vault.records().is_empty()).await,
        "after the crash, cleanup deletes the uncommitted secret"
    );
    assert_eq!(desk.scan_state("Tenant A"), ScanState::Missing);
}

#[tokio::test]
async fn an_unavailable_store_refuses_reset_and_touches_nothing() {
    let mut desk = Desk::with_store(ScriptedStore::unavailable());
    assert_eq!(desk.snap().storage, StorageHealth::Unavailable);
    assert_eq!(
        desk.send(json!({"kind": "resetLocalData"})).await,
        Err(Refusal::DeniedStore)
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        desk.adapters.vault.calls().is_empty(),
        "another instance's keys are not purged"
    );
}

#[tokio::test]
async fn saves_are_refused_only_when_nothing_was_written() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    desk.adapters.store.fail_saves(true);
    let refused = desk
        .send(
            json!({"kind": "createTenant", "profile": {"label": "Refused",
            "destination": {"kind": "region", "region": "india"}}}),
        )
        .await;
    assert_eq!(refused, Err(Refusal::DeniedStore));
    desk.adapters.store.fail_saves(false);
    // A replacement whose durability is unconfirmed is kept and reported.
    desk.adapters.store.unconfirmed_saves(true);
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Written",
        "destination": {"kind": "region", "region": "india"}}}),
    )
    .await
    .expect("written, durability unconfirmed");
    let snap = desk.snap();
    assert!(snap.durability_unconfirmed);
    assert!(snap.tenants.iter().any(|t| t.label == "Written"));
    desk.adapters.store.unconfirmed_saves(false);
    desk.send(json!({"kind": "setAppearance", "theme": "dark"}))
        .await
        .expect("confirmed save");
    assert!(!desk.snap().durability_unconfirmed);
    let desk = desk.restart();
    let labels: Vec<String> = desk
        .snap()
        .tenants
        .iter()
        .map(|t| t.label.clone())
        .collect();
    assert_eq!(
        labels,
        vec!["Tenant A", "Written"],
        "the refused change never appears later"
    );
}

#[tokio::test]
async fn a_corrupt_store_is_reported_untouched_until_reset() {
    let mut desk = Desk::with_store(ScriptedStore::corrupt());
    assert_eq!(desk.snap().storage, StorageHealth::Corrupt);
    let refused = desk
        .send(json!({"kind": "createTenant", "profile": {"label": "A", "destination": {"kind": "region", "region": "europe"}}}))
        .await;
    assert_eq!(refused, Err(Refusal::DeniedStore));
    assert_eq!(
        desk.adapters.store.saves(),
        0,
        "nothing overwritten implicitly"
    );
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| desk.snap().storage == StorageHealth::Ready).await);
    desk.create("A").await;
}

#[tokio::test]
async fn delivery_gaps_changed_duplicates_eviction_and_rotation_never_replay() {
    let config = CoreConfig {
        retained_results: 2,
        ..CoreConfig::default()
    };
    let mut desk = Desk::with(ScriptedStore::default(), config);
    let session = desk.session.clone();
    let deliver = |sequence: u64| Delivery {
        session: session.clone(),
        sequence,
    };
    let create = |label: &str| -> Intent {
        serde_json::from_value(json!({"kind": "createTenant", "profile": {"label": label,
            "destination": {"kind": "region", "region": "europe"}}}))
        .expect("intent")
    };
    let first = desk
        .handle
        .dispatch(deliver(1), create("one"))
        .await
        .expect("1");
    assert_eq!(
        desk.handle.dispatch(deliver(1), create("one")).await,
        Ok(first),
        "identical duplicate"
    );
    assert_eq!(
        desk.handle.dispatch(deliver(1), create("changed")).await,
        Err(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.handle.dispatch(deliver(3), create("gap")).await,
        Err(Refusal::SequenceGap)
    );
    desk.handle
        .dispatch(deliver(2), create("two"))
        .await
        .expect("2");
    desk.handle
        .dispatch(deliver(3), create("three"))
        .await
        .expect("3");
    assert_eq!(
        desk.handle.dispatch(deliver(1), create("one")).await,
        Err(Refusal::AlreadyProcessed),
        "evicted, never re-run"
    );
    assert_eq!(
        desk.snap().tenants.len(),
        3,
        "no duplicate tenant from any redelivery"
    );
    // A context outlives delivery-session rotation (reload or exhaustion).
    let context = desk.context();
    let rotated = desk
        .handle
        .register_delivery_session()
        .expect("rotate")
        .nonce;
    assert_eq!(
        desk.handle
            .dispatch(deliver(4), create("old session"))
            .await,
        Err(Refusal::StaleContext)
    );
    desk.session = rotated;
    desk.sequence = 0;
    desk.send(json!({"kind": "removeScanCredential", "context": context}))
        .await
        .expect("same context after rotation");
}

#[tokio::test]
async fn the_last_sequence_is_usable_and_exhaustion_requires_rotation() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    // Sequences only advance by one, so a delivery cannot jump to the end;
    // exhaustion is handled by registering a new session (rotation).
    let handle = desk.handle.clone();
    let session = handle.register_delivery_session().expect("session").nonce;
    let theme = |t: &str| -> Intent {
        serde_json::from_value(json!({"kind": "setAppearance", "theme": t})).expect("intent")
    };
    handle
        .dispatch(
            Delivery {
                session: session.clone(),
                sequence: 1,
            },
            theme("dark"),
        )
        .await
        .expect("1");
    assert_eq!(
        handle
            .dispatch(
                Delivery {
                    session: session.clone(),
                    sequence: u64::MAX
                },
                theme("light")
            )
            .await,
        Err(Refusal::SequenceGap),
        "a jump to the end is a gap, not a fast-forward"
    );
    let next = handle.register_delivery_session().expect("rotation");
    assert_eq!(next.next_sequence, 1);
    handle
        .dispatch(
            Delivery {
                session: next.nonce,
                sequence: 1,
            },
            theme("light"),
        )
        .await
        .expect("fresh session");
}

fn waiting_work(started: Arc<AtomicUsize>) -> desktop_core::runtime::Work {
    Box::new(move |claim: Claim| {
        started.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            claim.cancel.cancelled().await;
            Outcome::Unknown
        })
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn work_is_bounded_and_retired_with_its_context_without_replay() {
    let config = CoreConfig {
        operations: OperationLimits {
            max_active: 1,
            max_queued: 2,
            ..OperationLimits::default()
        },
        ..CoreConfig::default()
    };
    let mut desk = Desk::with(ScriptedStore::default(), config);
    desk.create("Tenant A").await;
    let b = desk.create("Tenant B").await;
    let context = ContextRef::clone(&desk.snap().active.expect("active").context);
    let started = Arc::new(AtomicUsize::new(0));
    let claimed = desk
        .handle
        .admit_fixture_operation(&context, 10, waiting_work(started.clone()))
        .expect("claimed");
    let queued = desk
        .handle
        .admit_fixture_operation(&context, 10, waiting_work(started.clone()))
        .expect("queued");
    desk.handle
        .admit_fixture_operation(&context, 10, waiting_work(started.clone()))
        .expect("queued");
    assert_eq!(
        desk.handle
            .admit_fixture_operation(&context, 10, waiting_work(started.clone()))
            .err(),
        Some(Refusal::Busy),
        "a full queue refuses; nothing accepted is dropped"
    );
    assert!(eventually(|| started.load(Ordering::SeqCst) == 1).await);
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("switch");
    assert!(
        eventually(|| desk.handle.operation_state(&claimed)
            == Some(OperationState::Settled(Settlement {
                outcome: Outcome::Unknown,
                stop: Some(StopReason::ContextRetired)
            })))
        .await
    );
    assert_eq!(
        desk.handle.operation_state(&queued),
        Some(OperationState::Settled(Settlement {
            outcome: Outcome::NotSent,
            stop: Some(StopReason::ContextRetired)
        }))
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        started.load(Ordering::SeqCst),
        1,
        "retired work never starts or replays"
    );
    assert_eq!(
        desk.handle
            .admit_fixture_operation(&context, 1, waiting_work(started.clone()))
            .err(),
        Some(Refusal::StaleContext)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_renderer_reload_keeps_unsettled_work_observable() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let context = ContextRef::clone(&desk.snap().active.expect("active").context);
    let operation = desk
        .handle
        .admit_fixture_operation(
            &context,
            1,
            Box::new(|_claim| {
                Box::pin(async {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Outcome::Completed
                })
            }),
        )
        .expect("admitted");
    let reloaded = desk.handle.register_delivery_session().expect("reload");
    desk.session = reloaded.nonce;
    desk.sequence = 0;
    assert!(
        eventually(|| desk.handle.operation_state(&operation)
            == Some(OperationState::Settled(Settlement {
                outcome: Outcome::Completed,
                stop: None
            })))
        .await,
        "work continues across reload and is observed by reference"
    );
    assert!(
        desk.handle
            .cancel_operation(&operation)
            .map(|f| !f)
            .unwrap_or(false),
        "already settled"
    );
}

#[tokio::test]
async fn a_transport_policy_change_retires_contexts() {
    let mut desk = Desk::new();
    desk.create("Tenant A").await;
    let before = desk.context();
    desk.core
        .set_transport_policy(TransportPolicy {
            revision: 2,
            ..TransportPolicy::default()
        })
        .expect("policy");
    let after = desk.context();
    assert_ne!(before, after);
    assert_eq!(
        desk.send(json!({"kind": "removeScanCredential", "context": before}))
            .await,
        Err(Refusal::StaleContext)
    );
}

#[tokio::test]
async fn deleting_a_tenant_cleans_its_credential_and_context() {
    let mut desk = Desk::new();
    let a = desk.create("Tenant A").await;
    desk.configure("keychain", KEY_A).await.expect("stored");
    desk.settled("Tenant A").await;
    desk.send(json!({"kind": "deleteTenant", "tenant": a}))
        .await
        .expect("deleted");
    let snap = desk.snap();
    assert!(snap.tenants.is_empty() && snap.active.is_none());
    assert!(eventually(|| desk.adapters.vault.records().is_empty()).await);
    assert!(desk.ledger_empty().await);
}

#[test]
fn persisted_state_rejects_dangling_references() {
    let id = TenantId::parse("tn-0123456789abcdef").expect("id");
    let state = PersistedState {
        tenants: vec![TenantProfile {
            id: id.clone(),
            label: "A".into(),
            tsg_id: None,
            destination: Destination::Region {
                region: Region::India,
            },
        }],
        credentials: vec![StoredCredential {
            record: RecordId::parse("cr-0123456789abcdef").expect("record"),
            tenant: id,
            // Bound to another origin than the profile's.
            destination: Region::Europe.origin().into(),
            class: desktop_core::store::CredentialClass::Scan,
            client_id: None,
        }],
        ..PersistedState::default()
    };
    assert!(!state.is_consistent());
}
