//! Inbox, reads and the tenant-switch guard (C30, screens/inbox-reads.md,
//! invariants I1, I2, I4, I11, I12). Everything here is derived per tenant
//! from scopes the core already owns: the Runtime overview read (its items,
//! sources and failures), the operator's holds, the tenant's chosen
//! products and its setup warnings (C29). The core computes the snooze
//! return time, the per-product read status and whether a switch may
//! proceed; the presentation renders them.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::observation_domain::{
    FailedAttempt, HoldKind, OverviewView, ProductOutcome, SnoozeReceipt, product_sources,
    source_names,
};
use super::{PendingDecisionRef, Refusal, State, SwitchBlocked};
use crate::context::{ContextRef, ReviewId};
use crate::features::{FeatureId, ProductId, SetupWarning};
use crate::management::{ManagementFailure, ManagementResource};
use crate::observation::{Fact, InboxDestination, InboxItem, Tone};
use crate::tenants::TenantId;

/// How a blocked switch is resolved (I1): the pending conversation is
/// ended and kept (`archive`) or ended and dropped (`discard`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SwitchResolution {
    Archive {
        context: ContextRef,
        review: ReviewId,
    },
    Discard {
        context: ContextRef,
        review: ReviewId,
    },
}

/// Where an Inbox entry came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InboxKind {
    /// A Runtime overview item (sessions, keys).
    Runtime,
    /// The chosen Gateway workspace's day (C18).
    Gateway,
    /// A warning recorded at tenant setup (C29).
    SetupWarning,
    /// A key rotation in progress (C31, I7).
    Rotation,
}

/// An operator's hold on an entry, with the core-computed return time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HoldView {
    pub kind: HoldKind,
    /// When a snoozed entry returns (seconds since the epoch); none for an
    /// acknowledgment, which ends when the item's level rises.
    pub returns_at: Option<u32>,
}

/// Where an entry's action leads (C30): the destination, and within it the
/// product, the inventory and the item the presentation selects on arrival.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InboxTarget {
    pub feature: FeatureId,
    pub product: Option<ProductId>,
    /// The Administration inventory to open (API keys for a key item).
    pub resource: Option<ManagementResource>,
    /// The item's name within that inventory, for the filter.
    pub item: Option<String>,
}

/// One Inbox entry of the active tenant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InboxEntry {
    pub id: String,
    pub kind: InboxKind,
    /// Needs action (otherwise an update).
    pub needs_action: bool,
    pub severity: Tone,
    pub title: String,
    pub detail: String,
    pub source: String,
    pub product: Option<ProductId>,
    /// Where the action leads, with the context it carries.
    pub target: Option<InboxTarget>,
    pub action_label: Option<String>,
    pub facts: Vec<Fact>,
    /// When it was observed or recorded (seconds since the epoch).
    pub when: u32,
    pub hold: Option<HoldView>,
}

/// The read as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReadState {
    /// Nothing has been read for this tenant yet.
    Pending,
    Reading,
    Done,
    /// The last read failed; an older read may be shown.
    Failed,
}

/// One product's part of the read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ProductReadState {
    Pending,
    Ok,
    /// Some of its sources were not read.
    Incomplete,
    /// A source refused the client for permissions.
    NeedsRole,
    Failed,
    /// Nothing reads this product on Home; its destination does.
    NotRead,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProductRead {
    pub product: ProductId,
    pub state: ProductReadState,
    pub sources_read: u32,
    pub sources_total: u32,
    pub detail: Option<String>,
}

/// The active tenant's read status (I4), derived from the overview read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReadStatus {
    pub state: ReadState,
    pub last_read_at: Option<u32>,
    /// When the last read failed (seconds since the epoch); with
    /// `last_read_at`, an older read is shown.
    pub failed_at: Option<u32>,
    /// When the latest attempt settled, established or not (seconds since
    /// the epoch): the time a completion is announced with when nothing was
    /// established yet.
    pub settled_at: Option<u32>,
    pub products: Vec<ProductRead>,
    /// Sources the last read could not establish, by name.
    pub sources_missing: Vec<String>,
    /// Counts completed reads of this tenant in this session: a change is
    /// a completion the presentation may announce.
    pub generation: u32,
    /// The product a `recheckProduct` asked for, until the next read lands.
    pub rechecking: Option<ProductId>,
}

/// The Inbox scope (per tenant, I2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InboxView {
    pub items: Vec<InboxEntry>,
    pub read: ReadStatus,
    /// The last snooze placed on this tenant in this session (I11).
    pub last_snooze: Option<SnoozeReceipt>,
}

fn product_label(product: ProductId) -> &'static str {
    match product {
        ProductId::RuntimeSecurity => "AI Runtime Security",
        ProductId::Gateway => "AI Gateway",
        ProductId::RedTeam => "AI Red Teaming",
        ProductId::ModelSkill => "Model and skill scans",
    }
}

fn warning_destination(product: ProductId) -> InboxDestination {
    match product {
        ProductId::Gateway => InboxDestination::Gateway,
        _ => InboxDestination::ApiKeys,
    }
}

/// Where a setup warning is fixed (C30): the scan key and the management
/// client live in Settings, a Gateway workspace in AI Gateway, the Red Team
/// and model/skill services in AI Red Teaming.
fn warning_target(product: ProductId) -> (InboxTarget, &'static str) {
    let (feature, label) = match product {
        ProductId::RuntimeSecurity => (FeatureId::Settings, "Open Settings"),
        ProductId::Gateway => (FeatureId::Gateway, "Open AI Gateway"),
        ProductId::RedTeam | ProductId::ModelSkill => (FeatureId::RedTeam, "Open AI Red Teaming"),
    };
    (
        InboxTarget {
            feature,
            product: Some(product),
            resource: None,
            item: None,
        },
        label,
    )
}

/// Where a Runtime overview item's action leads: an API key item opens the
/// API keys inventory filtered to that key.
fn item_target(item: &InboxItem) -> InboxTarget {
    match item.destination {
        InboxDestination::ApiKeys => InboxTarget {
            feature: FeatureId::Administration,
            product: Some(ProductId::RuntimeSecurity),
            resource: Some(ManagementResource::ApiKeys),
            item: item
                .facts
                .iter()
                .find(|f| f.label == "Key")
                .map(|f| f.value.clone()),
        },
        InboxDestination::Sessions => InboxTarget {
            feature: FeatureId::Sessions,
            product: Some(ProductId::RuntimeSecurity),
            resource: None,
            item: None,
        },
        InboxDestination::Gateway => InboxTarget {
            feature: FeatureId::Gateway,
            product: Some(ProductId::Gateway),
            resource: None,
            item: None,
        },
    }
}

/// A setup warning as an attention item (C29 → C30): the item id is the
/// warning's `vw-<n>`, so holds address it like any other item.
pub(super) fn setup_warning_item(w: &SetupWarning) -> InboxItem {
    InboxItem {
        id: w.id.clone(),
        title: format!("{}: setup warning", product_label(w.product)),
        detail: w.reason.clone(),
        source: "Setup · verify".to_owned(),
        action: true,
        tone: Tone::Warn,
        destination: warning_destination(w.product),
        action_label: warning_target(w.product).1.to_owned(),
        facts: vec![Fact {
            label: "Product".to_owned(),
            value: product_label(w.product).to_owned(),
        }],
        per_mille: None,
    }
}

fn source_state(failure: Option<ManagementFailure>) -> ProductReadState {
    match failure {
        None => ProductReadState::Ok,
        Some(ManagementFailure::Denied | ManagementFailure::Unauthorized) => {
            ProductReadState::NeedsRole
        }
        Some(_) => ProductReadState::Failed,
    }
}

fn pending(product: ProductId) -> ProductRead {
    ProductRead {
        product,
        state: ProductReadState::Pending,
        sources_read: 0,
        sources_total: product_sources(product),
        detail: None,
    }
}

fn needs_role_failure(failure: Option<ManagementFailure>) -> bool {
    matches!(
        failure,
        Some(ManagementFailure::Denied | ManagementFailure::Unauthorized)
    )
}

fn combined(needs_role: bool, read_count: u32, total: u32) -> ProductReadState {
    if needs_role {
        ProductReadState::NeedsRole
    } else if read_count == total {
        ProductReadState::Ok
    } else if read_count == 0 {
        ProductReadState::Failed
    } else {
        ProductReadState::Incomplete
    }
}

/// A product's finite outcome as its read entry, naming each source it
/// did not establish.
fn product_read(
    product: ProductId,
    o: &ProductOutcome,
    sources_missing: &mut Vec<String>,
) -> ProductRead {
    let state = combined(o.needs_role, o.sources_read, o.sources_total);
    sources_missing.extend(o.missing.iter().cloned());
    ProductRead {
        product,
        state,
        sources_read: o.sources_read,
        sources_total: o.sources_total,
        detail: Some(match (state, o.failure) {
            (ProductReadState::Ok, _) => o.detail.clone(),
            (_, Some(f)) => format!("{f:?}: {}", o.detail),
            (_, None) => o.detail.clone(),
        }),
    }
}

/// A product whose latest attempt established nothing: its failure, with
/// every source named missing and the retained older read (if any) named.
fn failed_product(
    product: ProductId,
    f: &FailedAttempt,
    kept: Option<&ProductOutcome>,
    sources_missing: &mut Vec<String>,
) -> ProductRead {
    let names = source_names(product);
    sources_missing.extend(names.iter().map(|n| (*n).to_owned()));
    let older = kept.map_or(String::new(), |o| {
        format!("; the read from {} ({}) is kept", o.read_at, o.detail)
    });
    ProductRead {
        product,
        state: if f.needs_role {
            ProductReadState::NeedsRole
        } else {
            ProductReadState::Failed
        },
        sources_read: 0,
        sources_total: names.len() as u32,
        detail: Some(match f.failure {
            Some(failure) => format!("{failure:?}: {}{older}", f.detail),
            None => format!("{}{older}", f.detail),
        }),
    }
}

impl State {
    /// The decision the active tenant's conversation is waiting for, if any.
    pub(super) fn pending_decision(&self) -> Option<PendingDecisionRef> {
        let conversation = self.live_bound_harness()?;
        let approvals = self.bound_approvals();
        let first = approvals.first()?;
        Some(PendingDecisionRef {
            conversation: conversation.as_str().to_owned(),
            decision: first.id.clone(),
        })
    }

    /// Admits a tenant switch, or refuses it while a decision is pending
    /// (I1): the refusal carries the guard the presentation renders.
    pub(super) fn guard_switch(
        &mut self,
        to: &TenantId,
        resolution: Option<SwitchResolution>,
        target_context: Option<&ContextRef>,
    ) -> Result<(), Refusal> {
        if let Some(resolution) = resolution {
            let (context, review) = match &resolution {
                SwitchResolution::Archive { context, review }
                | SwitchResolution::Discard { context, review } => (context, review),
            };
            // A resolution names one native refusal, never whichever
            // conversation happens to be pending when a lost delivery retries.
            let blocked = self.switch_blocked.as_ref().ok_or(Refusal::StaleContext)?;
            if self.active_ref.as_ref() != Some(context)
                || &blocked.context != context
                || &blocked.review != review
                || &blocked.to != to
                || blocked.target_context.as_ref() != target_context
                || Some(&blocked.pending) != self.pending_decision().as_ref()
            {
                return Err(Refusal::StaleContext);
            }
            let id = self.live_bound_harness().ok_or(Refusal::StaleContext)?;
            match resolution {
                SwitchResolution::Archive { .. } => self.end_harness(&id)?,
                SwitchResolution::Discard { .. } => self.discard_harness(&id)?,
            }
            self.switch_blocked = None;
            return Ok(());
        }
        let Some(pending) = self.pending_decision() else {
            self.switch_blocked = None;
            return Ok(());
        };
        self.switch_blocked = Some(SwitchBlocked {
            to: to.clone(),
            pending,
            completion: target_context.is_some(),
            context: self.active_ref.clone().ok_or(Refusal::StaleContext)?,
            review: ReviewId::issue().ok_or(Refusal::ShuttingDown)?,
            target_context: target_context.cloned(),
        });
        Err(Refusal::DecisionPending)
    }

    /// The active tenant's setup warnings as items.
    pub(super) fn setup_warning_items(&self) -> Vec<InboxItem> {
        self.data
            .active
            .as_ref()
            .and_then(|id| self.tenant(id))
            .map(|t| t.setup_warnings.iter().map(setup_warning_item).collect())
            .unwrap_or_default()
    }

    /// The products the active tenant chose (Runtime Security until a setup
    /// chooses more).
    pub(super) fn selected_products(&self, tenant: &TenantId) -> Vec<ProductId> {
        self.tenant(tenant)
            .map(|t| {
                if t.products.is_empty() {
                    vec![ProductId::RuntimeSecurity]
                } else {
                    t.products.clone()
                }
            })
            .unwrap_or_default()
    }

    /// The per-product read status (I4) from the overview read, the other
    /// products' finite outcomes and the tenant's chosen products. Coverage
    /// a read did not establish is named in `sources_missing`, so nothing
    /// unread is ever announced as complete.
    pub(super) fn read_status(&self, overview: &OverviewView) -> ReadStatus {
        let products = self
            .data
            .active
            .as_ref()
            .map(|id| self.selected_products(id))
            .unwrap_or_default();
        let record = self.active_record();
        // A refresh in which no source answered is shown by the overview for
        // its gaps (C12) but established nothing: it is not a read.
        let read = overview.read.as_ref().filter(|r| {
            !(overview.failure.is_some() && r.chart.is_none() && r.keys_failure.is_some())
        });
        let outcomes_read = record.is_some_and(|r| r.red_team.is_some() || r.model_skill.is_some());
        // The latest failed attempts across the chosen products; a missing
        // role is a settled attempt reported on the product, not a failed read.
        let attempts: Vec<&FailedAttempt> = record
            .map(|r| {
                [
                    (ProductId::RedTeam, r.red_team_failed.as_ref()),
                    (ProductId::ModelSkill, r.model_skill_failed.as_ref()),
                ]
                .into_iter()
                .filter(|(p, _)| products.contains(p))
                .filter_map(|(_, f)| f)
                .collect()
            })
            .unwrap_or_default();
        let product_failures: Vec<&FailedAttempt> =
            attempts.iter().copied().filter(|f| !f.needs_role).collect();
        let failed_at = overview
            .failed_at
            .into_iter()
            .chain(product_failures.iter().map(|f| f.at))
            .max();
        // Attempted: something settled for this tenant, established or not.
        let attempted =
            read.is_some() || outcomes_read || overview.failure.is_some() || !attempts.is_empty();
        // A failed read is failed, however little it established: never
        // "pending" (it was attempted) and never "done".
        let state = if overview.refreshing {
            ReadState::Reading
        } else if overview.failure.is_some() || !product_failures.is_empty() {
            ReadState::Failed
        } else if !attempted {
            ReadState::Pending
        } else {
            ReadState::Done
        };
        let failed_without_data = |product: ProductId, sources_missing: &mut Vec<String>| {
            let names = source_names(product);
            sources_missing.extend(names.iter().map(|n| (*n).to_owned()));
            ProductRead {
                product,
                state: if needs_role_failure(overview.failure) {
                    ProductReadState::NeedsRole
                } else {
                    ProductReadState::Failed
                },
                sources_read: 0,
                sources_total: names.len() as u32,
                detail: overview.failure.map(|f| format!("{f:?}")),
            }
        };
        let mut sources_missing = Vec::new();
        let mut per_product = Vec::new();
        for product in products.clone() {
            let entry = match product {
                ProductId::RuntimeSecurity => match read {
                    None if overview.failure.is_some() => {
                        failed_without_data(product, &mut sources_missing)
                    }
                    None => pending(product),
                    Some(r) => {
                        // Established means read and whole: a chart with
                        // inconsistent totals or a key page with an unreported
                        // expiration is unread coverage, not a complete read.
                        let chart = if r.chart_failure.is_some() {
                            source_state(r.chart_failure)
                        } else if r.chart_established() {
                            ProductReadState::Ok
                        } else {
                            ProductReadState::Incomplete
                        };
                        let keys = if r.keys_failure.is_some() {
                            source_state(r.keys_failure)
                        } else if r.keys_established() {
                            ProductReadState::Ok
                        } else {
                            ProductReadState::Incomplete
                        };
                        if chart != ProductReadState::Ok {
                            sources_missing.push("sessions chart".to_owned());
                        }
                        if keys != ProductReadState::Ok {
                            sources_missing.push("API keys".to_owned());
                        }
                        let read_count = u32::from(chart == ProductReadState::Ok)
                            + u32::from(keys == ProductReadState::Ok);
                        let state = combined(
                            chart == ProductReadState::NeedsRole
                                || keys == ProductReadState::NeedsRole,
                            read_count,
                            2,
                        );
                        ProductRead {
                            product,
                            state,
                            sources_read: read_count,
                            sources_total: 2,
                            detail: (state != ProductReadState::Ok).then(|| {
                                r.chart_failure.or(r.keys_failure).map_or_else(
                                    || {
                                        r.gaps
                                            .iter()
                                            .find(|g| {
                                                g.contains("unreported") || g.starts_with("Runtime")
                                            })
                                            .cloned()
                                            .unwrap_or_else(|| {
                                                "the key inventory was not complete".to_owned()
                                            })
                                    },
                                    |f| format!("{f:?}"),
                                )
                            }),
                        }
                    }
                },
                ProductId::Gateway => match read {
                    None if overview.failure.is_some() => {
                        failed_without_data(product, &mut sources_missing)
                    }
                    None => pending(product),
                    // A chosen product without a workspace is unread coverage,
                    // not a complete read.
                    Some(r) if r.gateway.is_none() => {
                        sources_missing.push("Gateway workspace (none chosen)".to_owned());
                        ProductRead {
                            product,
                            state: ProductReadState::Incomplete,
                            sources_read: 0,
                            sources_total: 2,
                            detail: Some(
                                "No Gateway workspace chosen; choose one in AI Gateway.".to_owned(),
                            ),
                        }
                    }
                    Some(r) => {
                        let g = r.gateway.as_ref().expect("checked");
                        // Usage is established only when the day's requests
                        // and errors are both known (read, whole, reported).
                        let usage = if g.usage_failure.is_some() {
                            source_state(g.usage_failure)
                        } else if g.activity_known() {
                            ProductReadState::Ok
                        } else {
                            ProductReadState::Incomplete
                        };
                        let limits = if g.limits_failure.is_some() {
                            source_state(g.limits_failure)
                        } else if g.policies.is_some() {
                            ProductReadState::Ok
                        } else {
                            ProductReadState::Incomplete
                        };
                        if usage != ProductReadState::Ok {
                            sources_missing.push("Gateway usage".to_owned());
                        }
                        if limits != ProductReadState::Ok {
                            sources_missing.push("Gateway limits".to_owned());
                        }
                        let read_count = u32::from(usage == ProductReadState::Ok)
                            + u32::from(limits == ProductReadState::Ok);
                        let state = combined(
                            usage == ProductReadState::NeedsRole
                                || limits == ProductReadState::NeedsRole,
                            read_count,
                            2,
                        );
                        ProductRead {
                            product,
                            state,
                            sources_read: read_count,
                            sources_total: 2,
                            detail: (state != ProductReadState::Ok)
                                .then(|| format!("workspace {}", g.workspace_name)),
                        }
                    }
                },
                ProductId::RedTeam | ProductId::ModelSkill => {
                    let (outcome, failed) = record.map_or((None, None), |r| match product {
                        ProductId::RedTeam => (r.red_team.as_ref(), r.red_team_failed.as_ref()),
                        _ => (r.model_skill.as_ref(), r.model_skill_failed.as_ref()),
                    });
                    match (outcome, failed) {
                        (None, None) => pending(product),
                        // The latest attempt established nothing: reported as
                        // such, with the retained read (if any) named as older.
                        (kept, Some(f)) => failed_product(product, f, kept, &mut sources_missing),
                        (Some(o), None) => product_read(product, o, &mut sources_missing),
                    }
                }
            };
            per_product.push(entry);
        }
        // Once anything was attempted, a chosen product nothing has reached
        // yet is unread coverage like any other (I4): named, so no completion
        // is announced as complete over it.
        if attempted {
            for entry in per_product
                .iter()
                .filter(|p| p.state == ProductReadState::Pending)
            {
                sources_missing.extend(source_names(entry.product).iter().map(|n| (*n).to_owned()));
            }
        }
        let last_read_at = read
            .map(|r| r.read_at)
            .into_iter()
            .chain(record.and_then(|r| r.red_team.as_ref()).map(|o| o.read_at))
            .chain(
                record
                    .and_then(|r| r.model_skill.as_ref())
                    .map(|o| o.read_at),
            )
            .max();
        let settled_at = last_read_at
            .into_iter()
            .chain(overview.failed_at)
            .chain(attempts.iter().map(|f| f.at))
            .max();
        ReadStatus {
            state,
            last_read_at,
            failed_at,
            settled_at,
            products: per_product,
            sources_missing,
            generation: record.map_or(0, |r| r.generation),
            rechecking: record.and_then(|r| r.rechecking),
        }
    }

    /// The active tenant's Inbox (I2): overview items with their holds and
    /// the tenant's setup warnings; nothing of another tenant.
    pub(super) fn inbox_view(&self, overview: &OverviewView) -> InboxView {
        let now = self.clock();
        let holds = self.active_holds();
        let mut items = Vec::new();
        let read_at = overview.read.as_ref().map_or(now, |r| r.read_at);
        let hold_of = |item: &InboxItem| -> Option<HoldView> {
            holds.iter().find_map(|h| {
                if h.id != item.id {
                    return None;
                }
                match h.kind {
                    HoldKind::Acknowledge if crate::observation::level(item) <= h.level => {
                        Some(HoldView {
                            kind: HoldKind::Acknowledge,
                            returns_at: None,
                        })
                    }
                    HoldKind::Snooze if now < h.until => Some(HoldView {
                        kind: HoldKind::Snooze,
                        returns_at: Some(h.until),
                    }),
                    _ => None,
                }
            })
        };
        for item in overview.read.iter().flat_map(|r| r.items.iter()) {
            let (kind, product) = if item.id.starts_with("gateway-") {
                (InboxKind::Gateway, Some(ProductId::Gateway))
            } else {
                (InboxKind::Runtime, Some(ProductId::RuntimeSecurity))
            };
            items.push(InboxEntry {
                id: item.id.clone(),
                kind,
                needs_action: item.action,
                severity: item.tone,
                title: item.title.clone(),
                detail: item.detail.clone(),
                source: item.source.clone(),
                product,
                target: Some(item_target(item)),
                action_label: Some(item.action_label.clone()),
                facts: item.facts.clone(),
                when: read_at,
                hold: hold_of(item),
            });
        }
        if let Some(tenant) = self.data.active.as_ref().and_then(|id| self.tenant(id)) {
            for w in &tenant.setup_warnings {
                let item = setup_warning_item(w);
                let (target, label) = warning_target(w.product);
                items.push(InboxEntry {
                    id: w.id.clone(),
                    kind: InboxKind::SetupWarning,
                    needs_action: true,
                    severity: Tone::Warn,
                    title: item.title.clone(),
                    detail: w.reason.clone(),
                    source: item.source.clone(),
                    product: Some(w.product),
                    target: Some(target),
                    action_label: Some(label.to_owned()),
                    facts: item.facts.clone(),
                    when: w.recorded_secs,
                    hold: hold_of(&item),
                });
            }
        }
        for item in self.rotation_items() {
            items.push(InboxEntry {
                id: item.id.clone(),
                kind: InboxKind::Rotation,
                needs_action: true,
                severity: Tone::Warn,
                title: item.title.clone(),
                detail: item.detail.clone(),
                source: item.source.clone(),
                product: Some(ProductId::RuntimeSecurity),
                target: Some(InboxTarget {
                    feature: FeatureId::Administration,
                    product: Some(ProductId::RuntimeSecurity),
                    resource: Some(ManagementResource::ApiKeys),
                    item: item
                        .facts
                        .iter()
                        .find(|f| f.label == "Old key")
                        .map(|f| f.value.clone()),
                }),
                action_label: Some(item.action_label.clone()),
                facts: item.facts.clone(),
                when: self
                    .data
                    .active
                    .as_ref()
                    .and_then(|id| self.tenant(id))
                    .and_then(|t| {
                        t.rotations
                            .iter()
                            .find(|r| {
                                super::rotation_domain::rotation_item_id(&r.application) == item.id
                            })
                            .map(|r| r.issued_at)
                    })
                    .unwrap_or(read_at),
                hold: hold_of(&item),
            });
        }
        InboxView {
            items,
            read: self.read_status(overview),
            last_snooze: self.active_record().and_then(|r| r.last_snooze.clone()),
        }
    }
}
