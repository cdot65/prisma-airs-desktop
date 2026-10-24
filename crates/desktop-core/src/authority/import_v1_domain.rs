//! The reviewed v1 import inside the authority (C23, D050).
//!
//! Inspecting reads v1's folder only. Planning reads the selected protected
//! items and checks each item against v2: what is already there exactly is
//! `alreadyPresent`, what v2 holds differently under the same identity is a
//! `conflict` that is never overwritten, and anything that cannot be read or
//! mapped is `blocked` with its reason. v1's instance lock is held shared
//! from planning until the import settles, so v1 cannot run (and change its
//! work) meanwhile.
//!
//! Importing writes item by item through each v2 owner. Inside the section
//! that makes each owner write (the local-data writer, the API Client's
//! lane, the saved document), the receipt first notes what is about to be
//! written, durably; nothing is written that a confirmed receipt does not
//! name. Until the receipt commits (`applied`), every entry it names is
//! hidden from every view, so the import appears whole or not at all, and
//! what the import writes waits meanwhile. An item that cannot be written
//! rolls the whole import back; an import the app ended during is rolled
//! back at the next launch before anything else writes. A rollback removes
//! only what its receipt wrote and v2 still holds unchanged (D050).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use super::{Effect, Inner, Intent, Refusal, State, lock};
use crate::context::ReviewId;
use crate::import_v1::plan::{self, CategoryView, Payload, SourceState, Target, V1TenantView};
use crate::import_v1::receipt::{
    self, ItemState, MAX_RECEIPTS, RECEIPT_SCHEMA, Receipt, ReceiptItem, ReceiptState, Written,
};
use crate::import_v1::source::{self, SourceLock};
use crate::import_v1::{Category, CommitPoint, ItemStatus, Problem, V1Source, content_hash, parse};
use crate::tenants::TenantId;
use crate::wire::Theme;

// ------------------------------------------------------------------ views

/// A v1 tenant with the v2 tenants it may go into (the same TSG).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct V1TenantChoice {
    pub index: u32,
    pub label: String,
    pub tsg_id: Option<String>,
    pub session_only: bool,
    /// v2 tenants with the same TSG ID.
    pub matches: Vec<TenantId>,
}

/// How many entries of an item are new, already present or in conflict.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ItemCounts {
    pub new: u32,
    pub present: u32,
    pub conflict: u32,
    /// Read but not imported (v2 has no equivalent, or not a draft).
    pub skipped: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemView {
    pub index: u32,
    pub category: Category,
    pub label: String,
    /// The v2 tenant it goes into (its label), or none.
    pub target: Option<String>,
    pub status: ItemStatus,
    pub counts: ItemCounts,
    /// Why part of it is not imported (a tenant's v1 scan endpoint).
    pub partial: Option<Problem>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub review: ReviewId,
    pub items: Vec<PlanItemView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptItemView {
    pub category: Category,
    pub label: String,
    pub state: ItemState,
    pub written: u32,
    pub kept: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptView {
    pub id: String,
    #[ts(type = "number")]
    pub created: u64,
    pub state: ReceiptState,
    /// It stopped before it finished (found on launch, or failed).
    pub interrupted: bool,
    pub items: Vec<ReceiptItemView>,
    pub can_resume: bool,
    pub can_roll_back: bool,
}

/// What is under way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ImportActivity {
    Inspecting,
    Planning,
    Importing,
    Confirming,
    RollingBack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct V1ImportView {
    /// Whether this build can import (a v1 source and a receipt folder).
    pub available: bool,
    /// None until inspected.
    pub source: Option<SourceState>,
    pub tenants: Vec<V1TenantChoice>,
    pub categories: Vec<CategoryView>,
    /// v1 settings with no v2 equivalent (never read).
    pub not_mapped: Vec<String>,
    pub plan: Option<PlanView>,
    pub activity: Option<ImportActivity>,
    pub receipts: Vec<ReceiptView>,
    /// Receipts readable in neither copy (kept until the operator removes
    /// one): what an import writes is held meanwhile (D050).
    pub damaged_receipts: Vec<String>,
    /// Neither receipt folder can be read: what an import writes is held
    /// until it can.
    pub receipts_unreadable: bool,
    /// Why the last step failed.
    pub problem: Option<Problem>,
}

// --------------------------------------------------------------- intents

/// Where one v1 tenant's work goes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TargetChoice {
    New,
    Existing { tenant: TenantId },
    Skip,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TenantTarget {
    pub source: u32,
    pub target: TargetChoice,
}

// ------------------------------------------------------------------ state

struct Inspection {
    state: SourceState,
    tenants: Option<parse::V1Tenants>,
    views: Vec<V1TenantView>,
    categories: Vec<CategoryView>,
    not_mapped: Vec<String>,
}

struct PlanItem {
    view: PlanItemView,
    /// The v1 tenant's ID (re-read on resume).
    source_tenant: Option<String>,
    /// The v2 tenant it writes into (pre-issued for a new one).
    target: Option<TenantId>,
    source_hash: String,
    payload: Option<Payload>,
}

struct Review {
    id: ReviewId,
    categories: Vec<Category>,
    items: Vec<PlanItem>,
    lock: SourceLock,
}

#[derive(Default)]
pub(super) struct V1Import {
    inspection: Option<Inspection>,
    review: Option<Review>,
    activity: Option<ImportActivity>,
    lease: u64,
    receipts: Vec<Receipt>,
    damaged: Vec<String>,
    unreadable: bool,
    problem: Option<Problem>,
    /// Entries of an import not committed yet, or of a rollback under way:
    /// no view shows them (D050).
    staged: Vec<Written>,
    /// The launch rollback [`super::Core::start`] runs (its lease).
    recovery: Option<u64>,
    /// A fixture interruption: the commit point at which an import or
    /// rollback stops as if the app ended there, and how many times that
    /// point passes first.
    #[cfg(feature = "test-fixtures")]
    pub(super) interrupt_at: Option<(CommitPoint, u32)>,
}

/// Background work the effects run.
pub(super) enum Job {
    Inspect {
        lease: u64,
    },
    Plan {
        lease: u64,
        categories: Vec<Category>,
        targets: Vec<Target>,
    },
    Apply {
        lease: u64,
    },
    Resume {
        lease: u64,
        receipt: String,
    },
    RollBack {
        lease: u64,
        receipt: String,
        nonce: u64,
    },
    /// The launch rollback of imports the app ended during.
    Recover {
        lease: u64,
    },
}

impl State {
    fn v1_source(&self) -> Option<V1Source> {
        self.v1_source.clone()
    }

    pub(super) fn v1_import_view(&self) -> V1ImportView {
        let i = &self.v1_import;
        let tenants = i
            .inspection
            .as_ref()
            .map(|ins| {
                ins.views
                    .iter()
                    .map(|v| V1TenantChoice {
                        index: v.index,
                        label: v.label.clone(),
                        tsg_id: v.tsg_id.clone(),
                        session_only: v.session_only,
                        matches: self.tsg_matches(v.tsg_id.as_deref()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        V1ImportView {
            available: self.v1_source.is_some() && self.import_root.is_some(),
            source: i.inspection.as_ref().map(|ins| ins.state),
            tenants,
            categories: i
                .inspection
                .as_ref()
                .map(|ins| ins.categories.clone())
                .unwrap_or_default(),
            not_mapped: i
                .inspection
                .as_ref()
                .map(|ins| ins.not_mapped.clone())
                .unwrap_or_default(),
            plan: i.review.as_ref().map(|r| PlanView {
                review: r.id.clone(),
                items: r.items.iter().map(|item| item.view.clone()).collect(),
            }),
            activity: i.activity,
            receipts: i.receipts.iter().map(receipt_view).collect(),
            damaged_receipts: i.damaged.clone(),
            receipts_unreadable: i.unreadable,
            problem: i.problem,
        }
    }

    fn tsg_matches(&self, tsg: Option<&str>) -> Vec<TenantId> {
        let Some(tsg) = tsg else {
            return Vec::new();
        };
        self.data
            .tenants
            .iter()
            .filter(|t| t.profile.tsg_id.as_deref() == Some(tsg))
            .filter(|t| !self.v1_hides_tenant(&t.profile.id))
            .map(|t| t.profile.id.clone())
            .collect()
    }

    /// Receipts on disk, read at launch. One that did not settle (the app
    /// ended during its import or rollback) stays hidden and is rolled back
    /// by [`super::Core::start`]; what it wrote waits until then.
    pub(super) fn load_v1_receipts(&mut self) {
        let Some(root) = self.import_root.clone() else {
            return;
        };
        let mut inventory = receipt::load_all(&root);
        // The saved document's copies bind every receipt that has not
        // settled: one its files no longer give is taken from there (so what
        // it names stays hidden and is rolled back), and one whose files say
        // it settled is dropped from the document (the files are written
        // first, so they are newer).
        let before = self.data.clone();
        let mut stale = false;
        self.data
            .open_imports
            .retain(|o| match inventory.receipts.iter().find(|r| r.id == o.id) {
                Some(r) if r.terminal() => {
                    stale = true;
                    false
                }
                Some(_) => true,
                None => {
                    inventory.damaged.retain(|d| *d != o.id);
                    inventory.receipts.push(o.clone());
                    true
                }
            });
        if stale {
            let _ = self.save_with(&before);
        }
        inventory
            .receipts
            .sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
        self.v1_import.receipts = inventory.receipts;
        self.v1_import.damaged = inventory.damaged;
        self.v1_import.unreadable = inventory.unreadable;
        self.restage();
        if self.v1_import.receipts.iter().any(|r| !r.terminal()) {
            let lease = self.v1_lease(ImportActivity::RollingBack);
            self.v1_import.recovery = Some(lease);
        }
    }

    /// The launch rollback, once.
    pub(super) fn take_v1_recovery(&mut self) -> Option<Effect> {
        let lease = self.v1_import.recovery.take()?;
        Some(Effect::V1Import(Job::Recover { lease }))
    }

    /// Hides exactly what unsettled receipts name.
    fn restage(&mut self) {
        self.v1_import.staged = self
            .v1_import
            .receipts
            .iter()
            .filter(|r| !r.terminal())
            .flat_map(named)
            .collect();
    }

    fn staged(&self, f: impl Fn(&Written) -> bool) -> bool {
        self.v1_import.staged.iter().any(f)
    }

    pub(super) fn v1_hides_tenant(&self, tenant: &TenantId) -> bool {
        self.staged(|w| matches!(w, Written::Tenant { id, .. } if id == tenant.as_str()))
    }

    pub(super) fn v1_hides_case(&self, tenant: &TenantId, case: &str) -> bool {
        self.staged(
            |w| matches!(w, Written::Case { tenant: t, id, .. } if t == tenant.as_str() && id == case),
        )
    }

    pub(super) fn v1_hides_report(&self, tenant: &TenantId, scan: &str, sub_request: u32) -> bool {
        self.staged(|w| {
            matches!(w, Written::Report { tenant: t, scan_id, sub, .. }
                if t == tenant.as_str() && scan_id == scan && *sub == sub_request)
        })
    }

    pub(super) fn v1_hides_conversation(
        &self,
        tenant: &TenantId,
        scope: &str,
        record: &str,
    ) -> bool {
        self.staged(|w| {
            matches!(w, Written::Conversation { tenant: t, workspace, id, .. }
                if t == tenant.as_str() && workspace == scope && id == record)
        })
    }

    pub(super) fn v1_hides_recent(&self, collection: &Path) -> bool {
        self.staged(|w| matches!(w, Written::Recent { root, .. } if root == collection))
    }

    /// The signed-in account's draft for `attempt_id` (another account's
    /// drafts are its own).
    pub(super) fn v1_hides_draft(&self, attempt_id: &str) -> bool {
        let Some(learner) = &self.data.learner else {
            return false;
        };
        let digest = plan::learner_digest(&learner.account_id);
        self.staged(|w| {
            matches!(w, Written::Draft { account, attempt, .. }
                if attempt == attempt_id && *account == digest)
        })
    }

    /// The appearance views show: the one before an import not committed.
    pub(super) fn shown_appearance(&self) -> Theme {
        let now = self.appearance();
        self.v1_import
            .staged
            .iter()
            .find_map(|w| match w {
                Written::Appearance { before, after } if *after == now => Some(*before),
                _ => None,
            })
            .unwrap_or(now)
    }

    /// What an import writes waits (D050): while one, or its rollback,
    /// runs; until every receipt has settled (an import or rollback that
    /// did not finish, at launch or after, keeps it waiting until it does);
    /// and while a receipt cannot be read.
    pub(super) fn v1_import_holds(&self) -> bool {
        self.v1_import_busy() || self.v1_records_settled().is_err()
    }

    /// Every receipt has settled and can be read.
    fn v1_records_settled(&self) -> Result<(), Refusal> {
        let i = &self.v1_import;
        if i.unreadable || !i.damaged.is_empty() || i.receipts.iter().any(|r| !r.terminal()) {
            return Err(Refusal::Busy);
        }
        Ok(())
    }

    /// An import, resume or rollback is writing (reset waits for it).
    pub(super) fn v1_import_busy(&self) -> bool {
        matches!(
            self.v1_import.activity,
            Some(
                ImportActivity::Importing
                    | ImportActivity::RollingBack
                    | ImportActivity::Confirming
            )
        )
    }

    /// An imported tenant nothing else uses: no credential, sign-in,
    /// project, server or chosen workspace (its cases, reports and
    /// conversations are checked by the rollback).
    pub(super) fn v1_tenant_unused(&self, tenant: &TenantId) -> bool {
        self.tenant(tenant).is_some_and(|t| {
            t.scan == super::ScanCapability::Missing
                && t.management == super::ScanCapability::Missing
                && t.inference == super::ScanCapability::Missing
                && t.harness_sign_in.is_none()
                && t.harness_project.is_none()
                && t.harness_mcp.is_empty()
                && t.gateway_workspace.is_none()
        })
    }

    fn v1_idle(&self) -> Result<(), Refusal> {
        if self.v1_source.is_none() || self.import_root.is_none() {
            return Err(Refusal::CapabilityMissing);
        }
        if self.v1_import.activity.is_some() {
            return Err(Refusal::Busy);
        }
        self.require_storage()
    }

    fn v1_lease(&mut self, activity: ImportActivity) -> u64 {
        self.v1_import.lease += 1;
        self.v1_import.activity = Some(activity);
        self.v1_import.problem = None;
        self.v1_import.lease
    }

    pub(super) fn begin_v1_inspection(&mut self) -> Result<Effect, Refusal> {
        self.v1_idle()?;
        self.v1_import.review = None;
        let lease = self.v1_lease(ImportActivity::Inspecting);
        Ok(Effect::V1Import(Job::Inspect { lease }))
    }

    pub(super) fn begin_v1_plan(
        &mut self,
        categories: Vec<Category>,
        targets: Vec<TenantTarget>,
    ) -> Result<Effect, Refusal> {
        self.v1_idle()?;
        let inspection = self
            .v1_import
            .inspection
            .as_ref()
            .filter(|i| i.state == SourceState::Ready)
            .ok_or(Refusal::InvalidInput)?;
        let count = inspection.views.len();
        if categories.is_empty()
            || categories.len() > Category::ALL.len()
            || categories
                .iter()
                .enumerate()
                .any(|(i, c)| categories[..i].contains(c))
            || targets.len() > count
        {
            return Err(Refusal::InvalidInput);
        }
        let mut resolved = vec![Target::Skip; count];
        let mut seen = Vec::new();
        for t in targets {
            let i = usize::try_from(t.source).map_err(|_| Refusal::InvalidInput)?;
            if i >= count || seen.contains(&i) {
                return Err(Refusal::InvalidInput);
            }
            seen.push(i);
            resolved[i] = match t.target {
                TargetChoice::New => Target::New,
                TargetChoice::Skip => Target::Skip,
                TargetChoice::Existing { tenant } => {
                    // The same TSG only: never another account's tenant.
                    let view = &inspection.views[i];
                    if !self.tsg_matches(view.tsg_id.as_deref()).contains(&tenant) {
                        return Err(Refusal::InvalidInput);
                    }
                    Target::Existing(tenant)
                }
            };
        }
        self.v1_import.review = None;
        let lease = self.v1_lease(ImportActivity::Planning);
        Ok(Effect::V1Import(Job::Plan {
            lease,
            categories,
            targets: resolved,
        }))
    }

    pub(super) fn confirm_v1_import(
        &mut self,
        review: &ReviewId,
        items: &[u32],
    ) -> Result<Effect, Refusal> {
        if self.consumed_reviews.contains(review) {
            return Err(Refusal::ReviewConsumed);
        }
        self.v1_idle()?;
        self.v1_records_settled()?;
        let plan = self
            .v1_import
            .review
            .as_ref()
            .ok_or(Refusal::ReviewInvalid)?;
        if &plan.id != review {
            return Err(Refusal::ReviewInvalid);
        }
        if items.is_empty()
            || items
                .iter()
                .enumerate()
                .any(|(i, x)| items[..i].contains(x))
            || items.iter().any(|x| {
                !plan
                    .items
                    .iter()
                    .any(|p| p.view.index == *x && p.view.status == ItemStatus::Ready)
            })
        {
            return Err(Refusal::InvalidInput);
        }
        let open = self
            .v1_import
            .receipts
            .iter()
            .filter(|r| !r.terminal())
            .count();
        if self.v1_import.receipts.len() >= MAX_RECEIPTS || open > 0 {
            // A rollback is never made impossible to make room.
            return Err(Refusal::Busy);
        }
        // A new tenant's work needs its tenant: both or neither.
        let chosen = |p: &PlanItem| items.contains(&p.view.index);
        for p in plan.items.iter().filter(|p| chosen(p)) {
            if p.view.category != Category::Tenants
                && let Some(target) = &p.target
                && self.tenant(target).is_none()
                && !plan.items.iter().any(|t| {
                    t.view.category == Category::Tenants
                        && t.target.as_ref() == Some(target)
                        && chosen(t)
                })
            {
                return Err(Refusal::InvalidInput);
            }
        }
        if self.data.tenants.len()
            + plan
                .items
                .iter()
                .filter(|p| chosen(p) && p.view.category == Category::Tenants)
                .count()
            > super::MAX_TENANTS
        {
            return Err(Refusal::InvalidInput);
        }
        let mut plan = self.v1_import.review.take().ok_or(Refusal::ReviewInvalid)?;
        plan.items.retain(|p| chosen(p));
        self.consumed_reviews.push(review.clone());
        let lease = self.v1_lease(ImportActivity::Importing);
        self.v1_import.review = Some(plan);
        Ok(Effect::V1Import(Job::Apply { lease }))
    }

    /// Imports an import that was undone (it failed, or the app ended
    /// during it) again: v1 is read again and every item written anew.
    pub(super) fn resume_v1_import(&mut self, id: &str) -> Result<Effect, Refusal> {
        self.v1_idle()?;
        let receipt = self.v1_receipt(id)?;
        if !(receipt.state == ReceiptState::RolledBack && receipt.recovered) {
            return Err(Refusal::InvalidInput);
        }
        self.v1_records_settled()?;
        let lease = self.v1_lease(ImportActivity::Importing);
        Ok(Effect::V1Import(Job::Resume {
            lease,
            receipt: id.to_owned(),
        }))
    }

    pub(super) fn roll_back_v1_import(&mut self, id: &str) -> Result<Effect, Refusal> {
        self.v1_idle()?;
        let receipt = self.v1_receipt(id)?;
        if receipt.state == ReceiptState::RolledBack {
            return Err(Refusal::InvalidInput);
        }
        let lease = self.v1_lease(ImportActivity::Confirming);
        let nonce = self.lease();
        Ok(Effect::V1Import(Job::RollBack {
            lease,
            receipt: id.to_owned(),
            nonce,
        }))
    }

    pub(super) fn forget_v1_receipt(&mut self, id: &str) -> Result<(), Refusal> {
        self.v1_idle()?;
        // The operator's way past a receipt neither copy of which can be
        // read: what it imported stays as it is, no longer held.
        if self.v1_import.damaged.iter().any(|d| d == id) {
            let root = self.import_root.clone().ok_or(Refusal::CapabilityMissing)?;
            receipt::forget(&root, id).map_err(|_| Refusal::DeniedStore)?;
            self.v1_import.damaged.retain(|d| d != id);
            return Ok(());
        }
        if !self.v1_receipt(id)?.terminal() {
            return Err(Refusal::InvalidInput);
        }
        let root = self.import_root.clone().ok_or(Refusal::CapabilityMissing)?;
        receipt::forget(&root, id).map_err(|_| Refusal::DeniedStore)?;
        self.v1_import.receipts.retain(|r| r.id != id);
        Ok(())
    }

    pub(super) fn dismiss_v1_import(&mut self) -> Result<(), Refusal> {
        if self.v1_import.activity.is_some() {
            return Err(Refusal::Busy);
        }
        self.v1_import.inspection = None;
        self.v1_import.review = None;
        self.v1_import.problem = None;
        Ok(())
    }

    fn v1_receipt(&self, id: &str) -> Result<&Receipt, Refusal> {
        self.v1_import
            .receipts
            .iter()
            .find(|r| r.id == id)
            .ok_or(Refusal::InvalidInput)
    }

    fn v1_current(&self, lease: u64) -> bool {
        self.v1_import.lease == lease && self.v1_import.activity.is_some()
    }

    fn settle_v1(&mut self, lease: u64, problem: Option<Problem>) {
        if self.v1_import.lease == lease {
            self.v1_import.activity = None;
            self.v1_import.problem = problem;
        }
    }

    /// Publishes a receipt; an entry the import is writing right now stays
    /// staged as well.
    fn put_v1_receipt(&mut self, receipt: &Receipt) {
        match self
            .v1_import
            .receipts
            .iter_mut()
            .find(|r| r.id == receipt.id)
        {
            Some(entry) => *entry = receipt.clone(),
            None => self.v1_import.receipts.push(receipt.clone()),
        }
    }
}

/// Everything a receipt names as its own: what it wrote, and what an item
/// that stopped before it was noted applied was writing.
fn named(r: &Receipt) -> Vec<Written> {
    fn unconfirmed(i: &ReceiptItem) -> &[Written] {
        if matches!(i.state, ItemState::Writing | ItemState::Failed) {
            &i.intended
        } else {
            &[]
        }
    }
    let mut all: Vec<Written> = Vec::new();
    for w in r
        .items
        .iter()
        .flat_map(|i| i.written.iter().chain(unconfirmed(i)))
    {
        if !all.contains(w) {
            all.push(w.clone());
        }
    }
    all
}

fn receipt_view(r: &Receipt) -> ReceiptView {
    ReceiptView {
        id: r.id.clone(),
        created: r.created,
        state: r.state,
        interrupted: r.recovered || !r.terminal(),
        items: r
            .items
            .iter()
            .map(|i| ReceiptItemView {
                category: i.category,
                label: i.label.clone(),
                state: i.state,
                written: u32::try_from(i.written.len()).unwrap_or(u32::MAX),
                kept: i.kept,
            })
            .collect(),
        can_resume: r.state == ReceiptState::RolledBack && r.recovered,
        can_roll_back: r.state != ReceiptState::RolledBack,
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ------------------------------------------------------------------- jobs

pub(super) fn run(inner: &Arc<Inner>, job: Job) {
    let inner = Arc::clone(inner);
    tokio::spawn(async move {
        match job {
            Job::Inspect { lease } => inspect(&inner, lease).await,
            Job::Plan {
                lease,
                categories,
                targets,
            } => make_plan(&inner, lease, categories, targets).await,
            Job::Apply { lease } => apply(&inner, lease).await,
            Job::Resume { lease, receipt } => resume(&inner, lease, receipt).await,
            Job::RollBack {
                lease,
                receipt,
                nonce,
            } => roll_back(&inner, lease, receipt, nonce).await,
            Job::Recover { lease } => recover(&inner, lease).await,
        }
    });
}

fn changed(inner: &Inner, f: impl FnOnce(&mut State)) {
    if let Ok(mut state) = lock(inner) {
        f(&mut state);
        state.changed();
    }
}

async fn blocking<T: Send + 'static>(fallback: T, f: impl FnOnce() -> T + Send + 'static) -> T {
    tokio::task::spawn_blocking(f).await.unwrap_or(fallback)
}

async fn inspect(inner: &Arc<Inner>, lease: u64) {
    let Some(src) = lock(inner).ok().and_then(|s| s.v1_source()) else {
        return;
    };
    let inventory = blocking(None, move || Some(plan::inventory(&src))).await;
    changed(inner, |state| {
        if !state.v1_current(lease) {
            return;
        }
        let problem = inventory.is_none().then_some(Problem::AccessDenied);
        if let Some(inv) = inventory {
            let views = inv.tenant_views();
            state.v1_import.inspection = Some(Inspection {
                state: inv.state,
                tenants: inv.tenants,
                views,
                categories: inv.categories,
                not_mapped: inv.not_mapped,
            });
        }
        state.settle_v1(lease, problem);
    });
}

/// What planning needs of v2 (read under the lock, used off it).
struct DestinationFacts {
    learner: Option<String>,
    labels: Vec<(TenantId, String)>,
    appearance: Theme,
}

async fn make_plan(
    inner: &Arc<Inner>,
    lease: u64,
    categories: Vec<Category>,
    targets: Vec<Target>,
) {
    let facts = lock(inner).ok().and_then(|s| {
        let tenants = s.v1_import.inspection.as_ref()?.tenants.as_ref()?;
        Some((
            s.v1_source()?,
            parse::V1Tenants {
                tenants: tenants.tenants.clone(),
                retired: tenants.retired.clone(),
                gateway_items: tenants.gateway_items,
            },
            DestinationFacts {
                learner: s.data.learner.as_ref().map(|l| l.account_id.clone()),
                labels: s
                    .data
                    .tenants
                    .iter()
                    .map(|t| (t.profile.id.clone(), t.profile.label.clone()))
                    .collect(),
                appearance: s.appearance(),
            },
        ))
    });
    let Some((src, tenants, facts)) = facts else {
        changed(inner, |s| s.settle_v1(lease, Some(Problem::Missing)));
        return;
    };
    let read_src = src.clone();
    let read_targets = targets.clone();
    let learner = facts.learner.clone();
    let selected = categories.clone();
    let read = blocking(Err(Problem::AccessDenied), move || {
        let lock = source::lock(&read_src.root)?;
        let candidates = plan::read(
            &read_src,
            &tenants,
            &read_targets,
            &selected,
            learner.as_deref(),
        );
        Ok((lock, candidates, tenants))
    })
    .await;
    let (lock_held, candidates, tenants) = match read {
        Ok(read) => read,
        Err(p) => {
            changed(inner, |s| s.settle_v1(lease, Some(p)));
            return;
        }
    };
    // New tenants get their identity now, so the review, the receipt and a
    // resumed import all name the same tenant.
    let mut issued: Vec<Option<TenantId>> = vec![None; tenants.tenants.len()];
    for (i, t) in targets.iter().enumerate() {
        issued[i] = match t {
            Target::New => TenantId::issue(),
            Target::Existing(id) => Some(id.clone()),
            Target::Skip => None,
        };
    }
    let mut items = Vec::new();
    for (n, c) in candidates.into_iter().enumerate() {
        let target = c
            .tenant
            .and_then(|i| issued.get(usize::try_from(i).ok()?).cloned().flatten());
        let target_label = target.as_ref().map(|id| {
            facts
                .labels
                .iter()
                .find(|(t, _)| t == id)
                .map(|(_, l)| l.clone())
                .unwrap_or_else(|| format!("{} (new tenant)", c.label))
        });
        let source_tenant = c
            .tenant
            .and_then(|i| tenants.tenants.get(usize::try_from(i).ok()?))
            .map(|t| t.id.clone());
        let partial = match &c.payload {
            Ok(Payload::Tenant {
                endpoint_problem, ..
            }) => *endpoint_problem,
            _ => None,
        };
        let (status, counts, payload) = match c.payload {
            Err(p) => (ItemStatus::Blocked(p), ItemCounts::default(), None),
            Ok(payload) => {
                let checked = check(inner, &payload, target.as_ref(), &facts).await;
                (checked.0, checked.1, Some(payload))
            }
        };
        items.push(PlanItem {
            view: PlanItemView {
                index: u32::try_from(n).unwrap_or(u32::MAX),
                category: c.category,
                label: c.label,
                target: target_label,
                status,
                counts,
                partial,
            },
            source_tenant,
            target,
            source_hash: c.source_hash,
            payload,
        });
    }
    changed(inner, |state| {
        if !state.v1_current(lease) {
            return;
        }
        match ReviewId::issue() {
            Some(id) => {
                state.v1_import.review = Some(Review {
                    id,
                    categories,
                    items,
                    lock: lock_held,
                });
                state.settle_v1(lease, None);
            }
            None => state.settle_v1(lease, Some(Problem::AccessDenied)),
        }
    });
}

/// An item's status against v2 now: how many of its entries are new,
/// already present exactly, or in conflict.
async fn check(
    inner: &Arc<Inner>,
    payload: &Payload,
    target: Option<&TenantId>,
    facts: &DestinationFacts,
) -> (ItemStatus, ItemCounts) {
    let mut counts = ItemCounts::default();
    let tally = |counts: &ItemCounts| {
        if counts.new > 0 {
            ItemStatus::Ready
        } else if counts.conflict > 0 {
            ItemStatus::Conflict
        } else {
            ItemStatus::AlreadyPresent
        }
    };
    match payload {
        Payload::Tenant { .. } => {
            counts.new = 1;
            (ItemStatus::Ready, counts)
        }
        Payload::Appearance(theme) => {
            if *theme == facts.appearance {
                counts.present = 1;
            } else {
                counts.new = 1;
            }
            (tally(&counts), counts)
        }
        Payload::Cases(cases) => {
            let existing = match target {
                Some(t) => owners::cases(inner, t).await,
                None => Ok(Vec::new()),
            };
            match existing {
                Err(p) => (ItemStatus::Blocked(p), counts),
                Ok(existing) => {
                    for case in cases {
                        match existing.iter().find(|e| e.id == case.id) {
                            None => counts.new += 1,
                            Some(e) if content_hash(&rebound(case, target)) == content_hash(e) => {
                                counts.present += 1
                            }
                            Some(_) => counts.conflict += 1,
                        }
                    }
                    let fresh = existing.len() + counts.new as usize;
                    if fresh > crate::library::CASE_LIMIT {
                        return (ItemStatus::Blocked(Problem::DestinationFull), counts);
                    }
                    (tally(&counts), counts)
                }
            }
        }
        Payload::Reports { reports, unfit } => {
            counts.skipped = u32::try_from(*unfit).unwrap_or(u32::MAX);
            let existing = match target {
                Some(t) => owners::reports(inner, t).await,
                None => Ok(Vec::new()),
            };
            match existing {
                Err(p) => (ItemStatus::Blocked(p), counts),
                Ok(existing) => {
                    for r in reports {
                        match existing.iter().find(|e| {
                            e.scan_id == r.scan_id && e.scan_sub_req_id == r.scan_sub_req_id
                        }) {
                            None => counts.new += 1,
                            Some(e) if e == r => counts.present += 1,
                            Some(_) => counts.conflict += 1,
                        }
                    }
                    if existing.len() + counts.new as usize > crate::sessions::MAX_REPORTS {
                        return (ItemStatus::Blocked(Problem::DestinationFull), counts);
                    }
                    (tally(&counts), counts)
                }
            }
        }
        Payload::Drafts {
            drafts, skipped, ..
        } => {
            counts.skipped = u32::try_from(*skipped).unwrap_or(u32::MAX);
            for d in drafts {
                match owners::draft(inner, &d.attempt_id).await {
                    Err(p) => return (ItemStatus::Blocked(p), counts),
                    Ok(None) => counts.new += 1,
                    Ok(Some(existing)) if same_draft(&existing, d) => counts.present += 1,
                    Ok(Some(_)) => counts.conflict += 1,
                }
            }
            (tally(&counts), counts)
        }
        Payload::Conversations(snapshots) => {
            let Some(target) = target else {
                return (ItemStatus::Blocked(Problem::NoTarget), counts);
            };
            let mut per_scope: Vec<(String, usize)> = Vec::new();
            for s in snapshots {
                let Some(record) = imported_record(target, s) else {
                    counts.skipped += 1;
                    continue;
                };
                match owners::conversation(inner, target, &s.context.workspace, &record.id).await {
                    Err(p) => return (ItemStatus::Blocked(p), counts),
                    Ok((None, held)) => {
                        counts.new += 1;
                        match per_scope
                            .iter_mut()
                            .find(|(w, _)| w == &s.context.workspace)
                        {
                            Some((_, n)) => *n += 1,
                            None => per_scope.push((s.context.workspace.clone(), held + 1)),
                        }
                    }
                    Ok((Some(existing), _))
                        if conversation_hash(&existing) == conversation_hash(&record) =>
                    {
                        counts.present += 1
                    }
                    Ok((Some(_), _)) => counts.conflict += 1,
                }
            }
            if per_scope
                .iter()
                .any(|(_, n)| *n > crate::harness::history::MAX_CONVERSATIONS)
            {
                return (ItemStatus::Blocked(Problem::DestinationFull), counts);
            }
            (tally(&counts), counts)
        }
        Payload::ApiClient(candidates) => {
            for c in candidates {
                match owners::api_client_present(inner, c).await {
                    Err(p) => return (ItemStatus::Blocked(p), counts),
                    Ok(true) => counts.present += 1,
                    Ok(false) => counts.new += 1,
                }
            }
            (tally(&counts), counts)
        }
    }
}

/// A v1 case as it is kept for its v2 tenant.
fn rebound(case: &crate::library::TestCase, target: Option<&TenantId>) -> crate::library::TestCase {
    let mut case = case.clone();
    if let Some(t) = target {
        case.tenant = t.as_str().to_owned();
    }
    case
}

fn draft_value(d: &parse::V1Draft) -> Value {
    json!({ "attemptId": d.attempt_id, "baseVersion": d.base_version, "draft": d.draft })
}

fn same_draft(existing: &Value, d: &parse::V1Draft) -> bool {
    existing["attemptId"] == Value::String(d.attempt_id.clone())
        && existing["baseVersion"] == json!(d.base_version)
        && existing["draft"] == d.draft
}

/// A v1 conversation as a v2 record: its transcript only, inspect-only,
/// bound to nothing that could ever be a live binding.
fn imported_record(
    target: &TenantId,
    s: &parse::V1Snapshot,
) -> Option<crate::harness::history::Record> {
    use crate::harness::MAX_ENTRIES;
    use crate::harness::history::{Origin, Record, SCHEMA, SavedBinding, title};
    use crate::harness::{EntryState, HarnessEntry, HarnessRole};
    use parse::{V1EntryKind, V1EntryStatus};
    let mut entries = Vec::new();
    for e in &s.conversation.entries {
        let state = match e.status {
            V1EntryStatus::Completed => EntryState::Complete,
            V1EntryStatus::Failed => EntryState::Failed,
            _ => EntryState::Interrupted,
        };
        let (role, state, text) = match e.kind {
            V1EntryKind::User => (HarnessRole::Operator, state, e.text.clone()),
            V1EntryKind::Assistant => (HarnessRole::Assistant, state, e.text.clone()),
            V1EntryKind::Tool => (HarnessRole::Tool, state, e.text.clone()),
            V1EntryKind::Approval => (
                HarnessRole::Tool,
                EntryState::Declined,
                "Approval from v1 (not transferred)".to_owned(),
            ),
            V1EntryKind::Notice => continue,
        };
        entries.push(HarnessEntry {
            id: e.id.clone(),
            role,
            text,
            state,
        });
    }
    if entries.len() > MAX_ENTRIES {
        return None;
    }
    let first = entries
        .iter()
        .find(|e| e.role == HarnessRole::Operator)
        .map(|e| e.text.clone())
        .unwrap_or_default();
    let updated = s
        .conversation
        .updated_at
        .and_then(|t| u64::try_from(t).ok())
        .unwrap_or(0);
    Some(Record {
        schema: SCHEMA.to_owned(),
        id: s.conversation.id.clone(),
        title: title(s.conversation.title.as_deref().unwrap_or(&first)),
        created: updated,
        updated,
        binding: SavedBinding {
            tenant: crate::harness::protocol::derived_uuid("tenant", target.as_str()),
            workspace: s.context.workspace.clone(),
            credential: crate::harness::protocol::derived_uuid("imported-v1", &s.conversation.id),
            credential_epoch: 0,
            endpoint: String::new(),
            source: crate::harness::history::CredentialSource::WorkspaceKey,
            project: None,
            write: false,
            mcp: Vec::new(),
        },
        entries,
        usage: None,
        context: Value::Null,
        replay: Value::Null,
        origin: Some(Origin::ImportedV1),
    })
}

fn conversation_hash(r: &crate::harness::history::Record) -> String {
    content_hash(&(&r.id, &r.title, &r.entries, &r.binding.workspace))
}

// ---------------------------------------------------------------- receipt

/// Writes the receipt durably: its two files, each synced with its folder,
/// and then, while it has not settled, the copy in the saved document (its
/// commit binding, D050; removed once it settles). Anything less is
/// `notRecorded`, and nothing is written on it. The files are written first,
/// so they are never older than the document's copy.
fn store_durably(inner: &Inner, root: &Path, r: &Receipt) -> Result<(), Problem> {
    receipt::store(root, r).map_err(|_| Problem::NotRecorded)?;
    let mut s = lock(inner).map_err(|_| Problem::NotRecorded)?;
    let before = s.data.clone();
    let at = s.data.open_imports.iter().position(|o| o.id == r.id);
    match (r.terminal(), at) {
        (false, Some(i)) => s.data.open_imports[i] = r.clone(),
        (false, None) => s.data.open_imports.push(r.clone()),
        (true, Some(i)) => {
            s.data.open_imports.remove(i);
        }
        (true, None) => return Ok(()),
    }
    s.save_with(&before).map_err(|_| Problem::NotRecorded)?;
    if s.durability_unconfirmed {
        return Err(Problem::NotRecorded);
    }
    Ok(())
}

/// Writes the receipt durably and publishes it.
fn note(inner: &Inner, root: &Path, r: &Receipt) -> Result<(), Problem> {
    let stored = store_durably(inner, root, r);
    changed(inner, |s| s.put_v1_receipt(r));
    stored
}

#[cfg(feature = "test-fixtures")]
fn interrupted(inner: &Inner, point: CommitPoint) -> bool {
    lock(inner)
        .ok()
        .is_some_and(|mut s| match &mut s.v1_import.interrupt_at {
            Some((at, 0)) if *at == point => {
                s.v1_import.interrupt_at = None;
                true
            }
            Some((at, n)) if *at == point => {
                *n -= 1;
                false
            }
            _ => false,
        })
}

#[cfg(not(feature = "test-fixtures"))]
fn interrupted(_: &Inner, _: CommitPoint) -> bool {
    false
}

/// The receipt of an import under way, shared with the owner writes: each
/// notes what it is about to write, durably, inside the section that
/// writes it, so no other writer comes between the note and the write.
#[derive(Clone)]
pub(super) struct Journal {
    root: PathBuf,
    receipt: Arc<std::sync::Mutex<Receipt>>,
    item: usize,
}

impl Journal {
    fn new(root: PathBuf, receipt: Receipt) -> Self {
        Self {
            root,
            receipt: Arc::new(std::sync::Mutex::new(receipt)),
            item: 0,
        }
    }

    fn at(&self, item: usize) -> Self {
        Self {
            item,
            ..self.clone()
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Receipt) -> T) -> T {
        f(&mut self
            .receipt
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner))
    }

    fn receipt(&self) -> Receipt {
        self.with(|r| r.clone())
    }

    /// The receipt's ID (owner writes mark what they write with it).
    pub(super) fn id(&self) -> String {
        self.with(|r| r.id.clone())
    }

    /// Notes this item `writing` with `intended`, durably, before its owner
    /// write. `Err(None)`: a fixture interruption right after the note.
    pub(super) fn writing(
        &self,
        inner: &Inner,
        intended: &[Written],
    ) -> Result<(), Option<Problem>> {
        let r = self.with(|r| {
            let item = &mut r.items[self.item];
            item.state = ItemState::Writing;
            for w in intended {
                if !item.intended.contains(w) {
                    item.intended.push(w.clone());
                }
            }
            r.clone()
        });
        store_durably(inner, &self.root, &r).map_err(Some)?;
        if interrupted(inner, CommitPoint::Writing) {
            return Err(None);
        }
        Ok(())
    }
}

/// Stages entries an owner is about to write: hidden until the receipt
/// commits.
pub(super) fn stage(inner: &Inner, entries: &[Written]) {
    if let Ok(mut s) = lock(inner) {
        for w in entries {
            if !s.v1_import.staged.contains(w) {
                s.v1_import.staged.push(w.clone());
            }
        }
    }
}

async fn apply(inner: &Arc<Inner>, lease: u64) {
    let taken = lock(inner).ok().and_then(|mut s| {
        let review = s.v1_import.review.take()?;
        Some((review, s.import_root.clone()?, s.v1_source()?))
    });
    let Some((review, root, src)) = taken else {
        changed(inner, |s| s.settle_v1(lease, Some(Problem::Missing)));
        return;
    };
    let Some(id) = receipt::new_id() else {
        changed(inner, |s| s.settle_v1(lease, Some(Problem::AccessDenied)));
        return;
    };
    let r = Receipt {
        schema: RECEIPT_SCHEMA.to_owned(),
        id,
        created: now_secs(),
        state: ReceiptState::Applying,
        selected: review.categories.clone(),
        recovered: false,
        items: review
            .items
            .iter()
            .map(|p| ReceiptItem {
                index: p.view.index,
                category: p.view.category,
                label: p.view.label.clone(),
                target: p.target.as_ref().map(|t| t.as_str().to_owned()),
                source_tenant: p.source_tenant.clone(),
                source_hash: p.source_hash.clone(),
                state: ItemState::Planned,
                written: Vec::new(),
                intended: Vec::new(),
                kept: 0,
            })
            .collect(),
    };
    let payloads: Vec<Option<Payload>> = review.items.into_iter().map(|p| p.payload).collect();
    import(inner, lease, &root, &src, r, payloads).await;
    drop(review.lock);
}

/// Writes a receipt's items and commits it, or rolls it back whole.
async fn import(
    inner: &Arc<Inner>,
    lease: u64,
    root: &Path,
    src: &V1Source,
    r: Receipt,
    payloads: Vec<Option<Payload>>,
) {
    if let Err(p) = note(inner, root, &r) {
        // Nothing is written on a receipt that is not recorded.
        let problem = revert(inner, root, r, true).await;
        changed(inner, |s| s.settle_v1(lease, Some(problem.unwrap_or(p))));
        return;
    }
    if interrupted(inner, CommitPoint::Created) {
        return;
    }
    let journal = Journal::new(root.to_path_buf(), r);
    let outcome = write_items(inner, src, &journal, payloads).await;
    let mut r = journal.receipt();
    let failure = match outcome {
        // Interrupted (fixture): stop as if the app ended here.
        Err(None) => return,
        Err(Some(p)) => p,
        Ok(()) => {
            r.state = ReceiptState::Applied;
            match store_durably(inner, root, &r) {
                Ok(()) => {
                    // The commit: the receipt and everything it wrote show
                    // together.
                    changed(inner, |s| {
                        s.put_v1_receipt(&r);
                        s.restage();
                        s.settle_v1(lease, None);
                    });
                    return;
                }
                Err(p) => {
                    r.state = ReceiptState::Applying;
                    p
                }
            }
        }
    };
    let problem = revert(inner, root, r, true).await;
    changed(inner, |s| {
        s.settle_v1(lease, Some(problem.unwrap_or(failure)))
    });
}

/// Writes every item, stopping at the first that cannot be written
/// (`Err(Some)`: the import is rolled back) or at a fixture interruption
/// (`Err(None)`).
async fn write_items(
    inner: &Arc<Inner>,
    src: &V1Source,
    journal: &Journal,
    mut payloads: Vec<Option<Payload>>,
) -> Result<(), Option<Problem>> {
    let root = journal.root.clone();
    for i in 0..journal.with(|r| r.items.len()) {
        let (category, source_hash, target) = journal.with(|r| {
            let item = &r.items[i];
            (item.category, item.source_hash.clone(), item.target.clone())
        });
        let fail = |p: Problem| {
            journal.with(|r| r.items[i].state = ItemState::Failed);
            Err(Some(p))
        };
        let Some(payload) = payloads.get_mut(i).and_then(Option::take) else {
            return fail(Problem::SourceChanged);
        };
        // A file the item came from must still hold the reviewed bytes.
        if let Some(now) = plan::file_hash(src, category)
            && now.ok().as_deref() != Some(source_hash.as_str())
        {
            return fail(Problem::SourceChanged);
        }
        let target = target.as_deref().and_then(TenantId::parse);
        let written = owners::write(inner, &journal.at(i), payload, target.as_ref()).await;
        if interrupted(inner, CommitPoint::Written) {
            return Err(None);
        }
        let stop = match written {
            Err((_, None)) => return Err(None),
            Ok(done) => {
                journal.with(|r| {
                    r.items[i].written = done;
                    r.items[i].state = ItemState::Applied;
                });
                None
            }
            Err((done, Some(p))) => {
                journal.with(|r| {
                    r.items[i].written = done;
                    r.items[i].state = ItemState::Failed;
                });
                Some(p)
            }
        };
        if let Some(p) = stop {
            return Err(Some(p));
        }
        note(inner, &root, &journal.receipt()).map_err(Some)?;
        if interrupted(inner, CommitPoint::Applied) {
            return Err(None);
        }
    }
    Ok(())
}

/// Imports an undone import again: v1 is read again (each item must be the
/// same bytes) and every item is written anew.
async fn resume(inner: &Arc<Inner>, lease: u64, id: String) {
    let taken = lock(inner).ok().and_then(|s| {
        Some((
            s.v1_receipt(&id).ok()?.clone(),
            s.import_root.clone()?,
            s.v1_source()?,
            s.data.learner.as_ref().map(|l| l.account_id.clone()),
        ))
    });
    let Some((mut r, root, src, learner)) = taken else {
        changed(inner, |s| s.settle_v1(lease, Some(Problem::Missing)));
        return;
    };
    let read_src = src.clone();
    let wanted = r.clone();
    let read = blocking(Err(Problem::AccessDenied), move || {
        let lock = source::lock(&read_src.root)?;
        let bytes =
            source::read_file(&read_src.root, &[parse::TENANTS_FILE], parse::TENANTS_LIMIT)?;
        let tenants = parse::tenants(&bytes)?;
        let mut targets = vec![Target::Skip; tenants.tenants.len()];
        for item in &wanted.items {
            if let (Some(st), Some(t)) = (&item.source_tenant, &item.target)
                && let Some(i) = tenants.tenants.iter().position(|v| &v.id == st)
            {
                let id = TenantId::parse(t).ok_or(Problem::Corrupt)?;
                targets[i] = if wanted.items.iter().any(|x| {
                    x.category == Category::Tenants && x.target.as_deref() == Some(t.as_str())
                }) {
                    Target::New
                } else {
                    Target::Existing(id)
                };
            }
        }
        // The same categories as the review (endpoints change how a new
        // tenant is read).
        let mut categories = wanted.selected.clone();
        if categories.is_empty() {
            categories = wanted.items.iter().map(|i| i.category).collect();
            categories.sort();
            categories.dedup();
        }
        let candidates = plan::read(
            &read_src,
            &tenants,
            &targets,
            &categories,
            learner.as_deref(),
        );
        Ok((lock, candidates, tenants))
    })
    .await;
    let (held, candidates, tenants) = match read {
        Ok(read) => read,
        Err(p) => {
            changed(inner, |s| s.settle_v1(lease, Some(p)));
            return;
        }
    };
    // Each item's payload: the candidate of the same category and v1
    // tenant whose source bytes are unchanged.
    let mut payloads = Vec::new();
    let mut candidates: Vec<_> = candidates.into_iter().map(Some).collect();
    for item in &r.items {
        let found = candidates.iter_mut().find_map(|slot| {
            let c = slot.as_ref()?;
            let tenant = c
                .tenant
                .and_then(|i| tenants.tenants.get(usize::try_from(i).ok()?))
                .map(|t| t.id.clone());
            (c.category == item.category && tenant == item.source_tenant).then(|| slot.take())?
        });
        payloads.push(found.and_then(|c| {
            (c.source_hash == item.source_hash)
                .then_some(c.payload.ok())
                .flatten()
        }));
    }
    r.state = ReceiptState::Applying;
    r.recovered = false;
    for item in &mut r.items {
        item.state = ItemState::Planned;
        item.written.clear();
        item.intended.clear();
        item.kept = 0;
    }
    import(inner, lease, &root, &src, r, payloads).await;
    drop(held);
}

async fn roll_back(inner: &Arc<Inner>, lease: u64, id: String, nonce: u64) {
    let request = super::ConfirmationRequest {
        title: "Roll back this import?".to_owned(),
        message: "Everything this import added that is unchanged since is removed from v2. \
                  Anything changed or added since is kept. v1 is not affected."
            .to_owned(),
        confirm_label: "Roll Back".to_owned(),
        destructive: true,
    };
    let accepted = inner.adapters.confirmer.confirm(nonce, request).await;
    let taken = lock(inner).ok().and_then(|mut s| {
        if !s.v1_current(lease) {
            return None;
        }
        s.v1_import.activity = Some(ImportActivity::RollingBack);
        s.changed();
        Some((s.v1_receipt(&id).ok()?.clone(), s.import_root.clone()?))
    });
    if !accepted {
        changed(inner, |s| s.settle_v1(lease, None));
        return;
    }
    let Some((r, root)) = taken else {
        changed(inner, |s| s.settle_v1(lease, Some(Problem::Missing)));
        return;
    };
    // An import that never committed is marked for importing again.
    let undone = r.recovered || r.state == ReceiptState::Applying;
    let problem = revert(inner, &root, r, undone).await;
    changed(inner, |s| s.settle_v1(lease, problem));
}

/// The launch rollback: every receipt the app ended during, without asking
/// (nothing of an import shows before it commits, D050).
async fn recover(inner: &Arc<Inner>, lease: u64) {
    let taken = lock(inner).ok().and_then(|s| {
        Some((
            s.import_root.clone()?,
            s.v1_import
                .receipts
                .iter()
                .filter(|r| !r.terminal())
                .cloned()
                .collect::<Vec<_>>(),
        ))
    });
    let Some((root, open)) = taken else {
        changed(inner, |s| s.settle_v1(lease, Some(Problem::Missing)));
        return;
    };
    let mut problem = None;
    for r in open {
        let undone = r.recovered || r.state == ReceiptState::Applying;
        if let Some(p) = revert(inner, &root, r, undone).await {
            problem.get_or_insert(p);
        }
    }
    // Its writes restore the folders' permissions: what they hold now is
    // listed again.
    let inventory = tokio::task::spawn_blocking(move || receipt::load_all(&root))
        .await
        .unwrap_or_default();
    changed(inner, |s| {
        s.v1_import.unreadable = inventory.unreadable;
        s.v1_import.damaged = inventory
            .damaged
            .into_iter()
            .filter(|d| !s.v1_import.receipts.iter().any(|r| r.id == *d))
            .collect();
        s.settle_v1(lease, problem);
    });
}

/// Rolls a receipt back: each entry it wrote, and each it was writing when
/// it stopped, is removed while v2 still holds exactly that; one changed
/// since is kept. Its entries stay hidden while it runs and go together.
/// `undone`: the import never committed (it failed, or the app ended
/// during it), so it is marked for importing again. `Some`: why it could
/// not finish (it stays unsettled and hidden, and is tried again when
/// asked or at the next launch). `None` from a fixture interruption too.
async fn revert(inner: &Arc<Inner>, root: &Path, mut r: Receipt, undone: bool) -> Option<Problem> {
    let previous = (r.state, r.recovered);
    r.recovered |= undone;
    let committed = r.state == ReceiptState::Applied;
    // An item that stopped before it was noted applied: what it was writing
    // may have been written (nothing else writes those entries meanwhile).
    for item in &mut r.items {
        if matches!(item.state, ItemState::Writing | ItemState::Failed) {
            for w in std::mem::take(&mut item.intended) {
                if !item.written.contains(&w) {
                    item.written.push(w);
                }
            }
        }
    }
    if r.items.iter().all(|i| i.written.is_empty()) {
        // Nothing was written: settled at once. Should this note not be
        // recorded, the next launch settles the receipt the same way.
        for item in &mut r.items {
            if item.state == ItemState::Writing {
                item.state = ItemState::Reverted;
            }
        }
        r.state = ReceiptState::RolledBack;
        let _ = store_durably(inner, root, &r);
        changed(inner, |s| {
            s.put_v1_receipt(&r);
            s.restage();
        });
        return None;
    }
    r.state = ReceiptState::RollingBack;
    if let Err(p) = store_durably(inner, root, &r) {
        // Not recorded: nothing is removed. A committed import stays shown.
        if committed {
            (r.state, r.recovered) = previous;
        }
        changed(inner, |s| {
            s.put_v1_receipt(&r);
            s.restage();
        });
        return Some(p);
    }
    changed(inner, |s| {
        s.put_v1_receipt(&r);
        s.restage();
    });
    // Newest first, tenants last: a tenant's work goes before it does.
    let mut order: Vec<usize> = (0..r.items.len()).collect();
    order.reverse();
    order.sort_by_key(|i| r.items[*i].category == Category::Tenants);
    for i in order {
        let was_applied = r.items[i].state == ItemState::Applied;
        if r.items[i].written.is_empty() {
            if r.items[i].state == ItemState::Writing {
                r.items[i].state = ItemState::Reverted;
            }
            continue;
        }
        if !matches!(
            r.items[i].state,
            ItemState::Applied | ItemState::Writing | ItemState::Reverting | ItemState::Failed
        ) {
            continue;
        }
        r.items[i].state = ItemState::Reverting;
        if let Err(p) = note(inner, root, &r) {
            return Some(p);
        }
        if interrupted(inner, CommitPoint::Reverting) {
            return None;
        }
        let (mut removed, mut kept, mut gone) = (0u32, 0u32, 0u32);
        let mut remaining = Vec::new();
        for w in std::mem::take(&mut r.items[i].written) {
            match owners::revert(inner, &r.id, &w).await {
                Revert::Removed => removed += 1,
                Revert::Kept => kept += 1,
                Revert::Gone => gone += 1,
                Revert::Failed => remaining.push(w),
            }
        }
        if interrupted(inner, CommitPoint::Reverted) {
            return None;
        }
        r.items[i].kept += kept;
        r.items[i].state = if !remaining.is_empty() {
            ItemState::Reverting
        } else if kept > 0 {
            ItemState::KeptChanged
        } else if was_applied && removed == 0 && gone > 0 {
            ItemState::Gone
        } else {
            ItemState::Reverted
        };
        r.items[i].written = remaining;
        if let Err(p) = note(inner, root, &r) {
            return Some(p);
        }
    }
    let done = r
        .items
        .iter()
        .all(|i| !matches!(i.state, ItemState::Reverting | ItemState::Writing));
    if done {
        r.state = ReceiptState::RolledBack;
    }
    let stored = store_durably(inner, root, &r);
    changed(inner, |s| {
        s.put_v1_receipt(&r);
        s.restage();
    });
    stored.err().or((!done).then_some(Problem::AccessDenied))
}

pub(super) enum Revert {
    /// Removed (or restored) as the receipt wrote it.
    Removed,
    /// Changed since: left as it is.
    Kept,
    /// Not there (removed since, or never written).
    Gone,
    /// Could not be checked or removed now (tried again later).
    Failed,
}

mod owners;

/// Intents that write what an import writes: they wait (`busy`) while an
/// import or rollback runs, so nothing else writes those entries between a
/// receipt's note and its owner write, or before a launch rollback, and
/// nothing exports what is not committed (D050).
pub(super) fn waits_for_import(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::SetAppearance { .. }
            | Intent::CreateTenant { .. }
            | Intent::DeleteTenant { .. }
            | Intent::ConfirmProfileImport { .. }
            | Intent::SaveCase { .. }
            | Intent::DeleteCase { .. }
            | Intent::ImportCases { .. }
            | Intent::ImportExamples { .. }
            | Intent::CopyCase { .. }
            | Intent::SaveLibraryAgain { .. }
            | Intent::ExportCases { .. }
            | Intent::SaveReport { .. }
            | Intent::RemoveReport { .. }
            | Intent::UndoReport { .. }
            | Intent::ExportReports { .. }
            | Intent::Workbench { .. }
            | Intent::DeleteHarnessHistory { .. }
            | Intent::ExportHarnessHistory { .. }
    )
}

/// Admits the import intents (called from the authority's dispatch).
pub(super) fn admit(
    state: &mut State,
    intent: Intent,
    effects: &mut Vec<Effect>,
) -> Result<(), Refusal> {
    match intent {
        Intent::InspectV1Import => effects.push(state.begin_v1_inspection()?),
        Intent::PlanV1Import {
            categories,
            targets,
        } => effects.push(state.begin_v1_plan(categories, targets)?),
        Intent::ConfirmV1Import { review, items } => {
            effects.push(state.confirm_v1_import(&review, &items)?)
        }
        Intent::ResumeV1Import { receipt } => effects.push(state.resume_v1_import(&receipt)?),
        Intent::RollBackV1Import { receipt } => effects.push(state.roll_back_v1_import(&receipt)?),
        Intent::ForgetV1ImportReceipt { receipt } => state.forget_v1_receipt(&receipt)?,
        Intent::DismissV1Import => state.dismiss_v1_import()?,
        _ => return Err(Refusal::InvalidInput),
    }
    state.changed();
    Ok(())
}
