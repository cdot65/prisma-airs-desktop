//! Runtime API key rotation (C31, screens/reviews-administration.md, I6, I7):
//! the admitted sequence additional key (`ApiKeys · Version`) → local
//! deployment acknowledgement → old key `Delete` with readback. The core owns
//! the state per tenant (persisted), drives it from the applied change
//! results it already settles, and raises the reminder as a distinct Inbox
//! item until the old key is read back absent. Two timestamps are stored
//! once and labelled distinctly: when the operator acknowledged the
//! deployment (a local statement) and when the old key was read back gone.
//! A failed issuance records nothing; an unknown one is a rotation whose key
//! is not confirmed until the key inventory shows it; a rejected or unknown
//! removal keeps the reminder and the completion time unset.

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

use super::change_domain::Readback;
use super::{Data, Refusal, State, StorageHealth};
use crate::changes::{ChangeAction, ChangeKind};
use crate::context::ContextRef;
use crate::features::{KeyRef, Rotation, RotationIssuance, RotationRemoval};
use crate::observation::{Fact, InboxDestination, InboxItem, Tone};
use crate::runtime::Outcome;
use crate::store::StoreError;
use crate::tenants::TenantId;

/// Where a rotation stands, as the panel shows it (derived, never stored).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RotationStage {
    /// The additional key's issuance settled unknown: nothing confirms a key
    /// exists until the key inventory is read back.
    IssuanceUnconfirmed,
    /// Issued; the one-time secret is still held for this context.
    Issued,
    /// Issued earlier; the secret is no longer available.
    Resumed,
    /// The operator stated the application was updated.
    Acknowledged,
    /// The old key's removal settled unknown: reconciled by readback.
    RemovalUnconfirmed,
    /// The old key was read back absent.
    Complete,
}

/// One rotation as the presentation shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RotationView {
    pub application: String,
    pub old_key: KeyRef,
    pub new_key: Option<KeyRef>,
    pub stage: RotationStage,
    /// The change result that issued the key, while this context holds its
    /// one-time secret (reveal through the host).
    pub issued_by: Option<String>,
    pub issued_at: u32,
    pub deployment_acknowledged_at: Option<u32>,
    pub old_key_removed_at: Option<u32>,
    /// Why the last removal was rejected, if it was.
    pub removal_rejected: Option<String>,
    /// What the service established is not yet saved on this Mac (storage
    /// failed); it is retried on every publication (C31).
    pub unsaved: bool,
}

/// What a settled change tells the rotation state (built by the change
/// domain from its record and output).
pub(super) struct SettledChange {
    pub(super) operation: String,
    pub(super) kind: ChangeKind,
    pub(super) action: ChangeAction,
    pub(super) target: String,
    /// The draft's item name (the new key a version names).
    pub(super) new_name: Option<String>,
    /// The reviewed item's name (the old key of a version) and the
    /// application the change named.
    pub(super) source_name: Option<String>,
    pub(super) application: Option<String>,
    pub(super) outcome: Outcome,
    pub(super) failure: Option<String>,
    pub(super) readback: Option<Readback>,
    pub(super) current: Option<String>,
}

/// The Inbox reminder's id for an application's rotation.
pub fn rotation_item_id(application: &str) -> String {
    format!("rotation-{application}")
}

fn key_ref(item: &Value) -> Option<KeyRef> {
    let id = item.get("api_key_id").and_then(Value::as_str)?.to_owned();
    let name = item
        .get("api_key_name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    Some(KeyRef { id, name })
}

fn application_of(item: &Value) -> Option<String> {
    item.get("cust_app")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

impl State {
    fn rotations_mut(&mut self, tenant: &TenantId) -> Option<&mut Vec<Rotation>> {
        self.data
            .tenants
            .iter_mut()
            .find(|t| &t.profile.id == tenant)
            .map(|t| &mut t.rotations)
    }

    /// Applies a settled Runtime change to the active tenant's rotations
    /// (C31): a key version that read back starts one (replacing an earlier
    /// rotation of the same application), a version that settled unknown
    /// starts an unconfirmed one, a failed version records nothing; a delete
    /// of a rotation's old key completes it when read back absent, keeps it
    /// with the reason when rejected, and marks it unconfirmed when unknown.
    pub(super) fn rotation_settled(&mut self, tenant: &TenantId, change: SettledChange) {
        if change.kind != ChangeKind::ApiKeys {
            return;
        }
        let clock = self.clock();
        let before = self.data.clone();
        let Some(rotations) = self.rotations_mut(tenant) else {
            return;
        };
        let changed = match change.action {
            ChangeAction::Version => {
                let current = change
                    .current
                    .as_deref()
                    .and_then(|c| serde_json::from_str::<Value>(c).ok());
                let found = change.outcome == Outcome::Completed
                    && change.failure.is_none()
                    && change.readback == Some(Readback::Found);
                let new_key = current.as_ref().and_then(key_ref).filter(|_| found);
                let application = current
                    .as_ref()
                    .and_then(application_of)
                    .or_else(|| change.application.clone());
                let sent = matches!(change.outcome, Outcome::Completed | Outcome::Unknown);
                match (sent, application) {
                    // Sent: the rotation starts (or restarts). Confirmed only
                    // when the readback found the key the review named; a
                    // completed write whose readback did not establish it,
                    // or an unknown one, is an unconfirmed issuance that only
                    // a key of that exact name can confirm later.
                    (true, Some(application)) => {
                        rotations.retain(|r| r.application != application);
                        let confirmed = new_key.is_some();
                        rotations.push(Rotation {
                            application,
                            old_key: KeyRef {
                                id: change.target.clone(),
                                name: change.source_name.clone().unwrap_or_default(),
                            },
                            new_key,
                            issuance: if confirmed {
                                RotationIssuance::Confirmed
                            } else {
                                RotationIssuance::Unconfirmed
                            },
                            issued_by: Some(change.operation.clone()),
                            pending_name: change.new_name.clone(),
                            issued_at: clock,
                            deployment_acknowledged_at: None,
                            removal: RotationRemoval::Pending,
                            old_key_removed_at: None,
                        });
                        true
                    }
                    // Refused or not sent: nothing issued, nothing kept.
                    _ => false,
                }
            }
            ChangeAction::Delete => {
                let Some(rotation) = rotations.iter_mut().find(|r| r.old_key.id == change.target)
                else {
                    return;
                };
                match (change.outcome, change.failure.as_deref(), change.readback) {
                    (Outcome::Completed, None, Some(Readback::Absent)) => {
                        rotation.removal = RotationRemoval::Done;
                        rotation.old_key_removed_at = Some(clock);
                    }
                    (Outcome::Completed, None, _) | (Outcome::Unknown, _, _) => {
                        // Sent, but the key was not read back gone: reconciled
                        // by a later whole-inventory read; the reminder stays.
                        rotation.removal = RotationRemoval::Unconfirmed;
                    }
                    (_, failure, _) => {
                        rotation.removal = RotationRemoval::Rejected {
                            reason: failure.unwrap_or("not sent").to_owned(),
                        };
                    }
                }
                true
            }
            _ => false,
        };
        if changed {
            self.keep_rotation_fact(&before);
        }
    }

    /// A complete key inventory page settles what rotations could not
    /// confirm: an unconfirmed issuance is confirmed by a key of the
    /// application other than the old one; an unconfirmed removal is done
    /// once the old key is absent from the whole inventory.
    pub(super) fn rotation_reconcile(&mut self, tenant: &TenantId, items: &[Value], whole: bool) {
        let clock = self.clock();
        let before = self.data.clone();
        let Some(rotations) = self.rotations_mut(tenant) else {
            return;
        };
        let mut changed = false;
        for rotation in rotations.iter_mut() {
            // Only the key this attempt named confirms its issuance: a key the
            // application already had says nothing about it.
            if rotation.issuance == RotationIssuance::Unconfirmed
                && let Some(name) = rotation.pending_name.clone()
            {
                let found = items.iter().find(|k| {
                    application_of(k).as_deref() == Some(rotation.application.as_str())
                        && k.get("api_key_name").and_then(Value::as_str) == Some(name.as_str())
                        && key_ref(k).is_some_and(|key| key.id != rotation.old_key.id)
                });
                if let Some(key) = found.and_then(key_ref) {
                    rotation.new_key = Some(key);
                    rotation.issuance = RotationIssuance::Confirmed;
                    changed = true;
                }
            }
            // Absence is established only by the whole inventory in one read.
            if rotation.removal == RotationRemoval::Unconfirmed && whole {
                let present = items
                    .iter()
                    .any(|k| key_ref(k).is_some_and(|key| key.id == rotation.old_key.id));
                if !present {
                    rotation.removal = RotationRemoval::Done;
                    rotation.old_key_removed_at = Some(clock);
                    changed = true;
                }
            }
        }
        if changed {
            self.keep_rotation_fact(&before);
        }
    }

    /// A detail read walked the whole key inventory without this key: a
    /// removal that was sent but not read back gone is now established.
    pub(super) fn rotation_key_absent(&mut self, tenant: &TenantId, id: &str) {
        let clock = self.clock();
        let before = self.data.clone();
        let Some(rotations) = self.rotations_mut(tenant) else {
            return;
        };
        let mut changed = false;
        for rotation in rotations
            .iter_mut()
            .filter(|r| r.old_key.id == id && r.removal == RotationRemoval::Unconfirmed)
        {
            rotation.removal = RotationRemoval::Done;
            rotation.old_key_removed_at = Some(clock);
            changed = true;
        }
        if changed {
            self.keep_rotation_fact(&before);
        }
    }

    /// The operator's statement that the application now uses the new key
    /// (a local fact, timestamped once).
    pub(super) fn acknowledge_deployment(
        &mut self,
        context: &ContextRef,
        application: &str,
    ) -> Result<Option<(String, String)>, Refusal> {
        let tenant = self.check_context(context)?;
        let clock = self.clock();
        let rotations = self.rotations_mut(&tenant).ok_or(Refusal::StaleContext)?;
        let rotation = rotations
            .iter_mut()
            .find(|r| r.application == application)
            .ok_or(Refusal::InvalidInput)?;
        if rotation.new_key.is_none() || rotation.removal == RotationRemoval::Done {
            return Err(Refusal::InvalidInput);
        }
        if rotation.deployment_acknowledged_at.is_none() {
            rotation.deployment_acknowledged_at = Some(clock);
        }
        // The operator says the application has the key: once this is
        // recorded, its one-time copy is no longer needed here (I6).
        Ok(rotation
            .issued_by
            .clone()
            .map(|operation| (operation, rotation.old_key.id.clone())))
    }

    /// Saves a rotation fact the service established. Unlike an intent's
    /// save, a failure keeps the fact in memory (it happened remotely and
    /// must not be forgotten), reports it, and every later publication
    /// retries the save; nothing remote is replayed.
    pub(super) fn keep_rotation_fact(&mut self, _before: &Data) {
        let saved = match self.storage {
            StorageHealth::Ready => self.store.save(&self.persisted()),
            _ => Err(StoreError::Unavailable),
        };
        match saved {
            Ok(()) => {
                self.durability_unconfirmed = false;
                self.rotation_unsaved = false;
            }
            Err(StoreError::Unconfirmed) => {
                self.durability_unconfirmed = true;
                self.rotation_unsaved = false;
            }
            Err(_) => self.rotation_unsaved = true,
        }
    }

    /// Retries an unsaved rotation fact (each publication; C31).
    pub(super) fn retry_rotation_save(&mut self) {
        if self.rotation_unsaved {
            let data = self.data.clone();
            self.keep_rotation_fact(&data);
        }
    }

    /// The active tenant's rotations as the presentation shows them.
    pub(super) fn rotations_view(&self) -> Vec<RotationView> {
        let Some(tenant) = self.data.active.as_ref().and_then(|id| self.tenant(id)) else {
            return Vec::new();
        };
        tenant
            .rotations
            .iter()
            .map(|r| {
                let secret = r
                    .issued_by
                    .as_deref()
                    .is_some_and(|op| self.issuance_held(op, &r.old_key.id));
                let stage = match (
                    &r.removal,
                    r.deployment_acknowledged_at,
                    r.new_key.is_some(),
                ) {
                    (RotationRemoval::Done, _, _) => RotationStage::Complete,
                    (RotationRemoval::Unconfirmed, _, _) => RotationStage::RemovalUnconfirmed,
                    (_, _, false) => RotationStage::IssuanceUnconfirmed,
                    (_, Some(_), true) => RotationStage::Acknowledged,
                    (_, None, true) if secret => RotationStage::Issued,
                    (_, None, true) => RotationStage::Resumed,
                };
                RotationView {
                    application: r.application.clone(),
                    old_key: r.old_key.clone(),
                    new_key: r.new_key.clone(),
                    stage,
                    // Offered only while the core holds the key and the stage
                    // is one that may reveal it (issued, or issued but not yet
                    // confirmed by readback).
                    issued_by: r.issued_by.clone().filter(|_| {
                        secret
                            && matches!(
                                stage,
                                RotationStage::Issued | RotationStage::IssuanceUnconfirmed
                            )
                    }),
                    issued_at: r.issued_at,
                    deployment_acknowledged_at: r.deployment_acknowledged_at,
                    old_key_removed_at: r.old_key_removed_at,
                    unsaved: self.rotation_unsaved,
                    removal_rejected: match &r.removal {
                        RotationRemoval::Rejected { reason } => Some(reason.clone()),
                        _ => None,
                    },
                }
            })
            .collect()
    }

    /// The reminder items of rotations in progress (I7): one per application
    /// with a confirmed new key and the old key not yet read back gone.
    pub(super) fn rotation_items(&self) -> Vec<InboxItem> {
        let Some(tenant) = self.data.active.as_ref().and_then(|id| self.tenant(id)) else {
            return Vec::new();
        };
        tenant
            .rotations
            .iter()
            .filter(|r| r.new_key.is_some() && r.removal != RotationRemoval::Done)
            .map(|r| InboxItem {
                id: rotation_item_id(&r.application),
                title: format!(
                    "Update {} with the new key, then remove the old key",
                    r.application
                ),
                detail: format!(
                    "An additional key was issued for {}. Once the application uses it, finish the rotation by removing the old key {}.",
                    r.application, r.old_key.name
                ),
                source: "API keys · rotation".to_owned(),
                action: true,
                tone: Tone::Warn,
                destination: InboxDestination::ApiKeys,
                action_label: "Finish rotation…".to_owned(),
                facts: vec![
                    Fact {
                        label: "Application".to_owned(),
                        value: r.application.clone(),
                    },
                    Fact {
                        label: "New key".to_owned(),
                        value: r.new_key.as_ref().map(|k| k.name.clone()).unwrap_or_default(),
                    },
                    Fact {
                        label: "Old key".to_owned(),
                        value: r.old_key.name.clone(),
                    },
                ],
                per_mille: None,
            })
            .collect()
    }
}
