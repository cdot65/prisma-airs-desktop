//! Runtime Home/Inbox state inside the authority (C12, C30): each opened
//! tenant's last overview read (kept when a refresh fails, marked with its
//! failure, and kept across a switch away and back, I2), the finite outcomes
//! of its other products' Home reads, its read bookkeeping, and the
//! operator's attention holds per tenant. A healthy empty source, a failed
//! one and one not yet read stay distinct; "all clear" is claimed only from
//! a fresh read of both sources with nothing open and no gap.
//!
//! A read is admitted under a context; its reply is recorded only while that
//! context is current (a retired context's reply is withdrawn and never
//! shown), but the tenant's recorded read outlives the context it was made
//! under: switching A → B → A shows A's cached read again until a new one
//! lands.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Refusal, State};
use crate::context::ContextRef;
use crate::features::ProductId;
use crate::management::{ManagementFailure, ManagementResult};
use crate::observation::{FRESH_SECS, HOLD_LIMIT, InboxItem, RuntimeOverview, SNOOZE_SECS, level};
use crate::tenants::TenantId;

/// How the operator set an attention item aside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HoldKind {
    /// Hidden until its level rises above the acknowledged level.
    Acknowledge,
    /// Hidden for 24 hours.
    Snooze,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Hold {
    pub(super) id: String,
    pub(super) kind: HoldKind,
    pub(super) level: u8,
    pub(super) until: u32,
}

/// What Needs attention can say (v1 faces, Runtime only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum AttentionFace {
    /// Something needs attention.
    Items,
    /// Nothing has been read yet.
    FirstLaunch,
    /// A fresh, complete read of every source with nothing open.
    AllClear,
    /// Nothing open, but a source failed, is stale, partial or unknown.
    Gap,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct HeldItem {
    pub item: InboxItem,
    pub kind: HoldKind,
    /// When a snoozed item returns (seconds since the epoch), computed
    /// here (C30, I11); none for an acknowledgment.
    pub returns_at: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OverviewView {
    /// The last overview read for this tenant (kept when a refresh fails).
    pub read: Option<RuntimeOverview>,
    pub refreshing: bool,
    /// The last refresh failed (the shown read, if any, is older).
    pub failure: Option<ManagementFailure>,
    /// When the last refresh failed (seconds since the epoch).
    pub failed_at: Option<u32>,
    /// The read is younger than ten minutes.
    pub fresh: bool,
    /// Attention items not set aside (actionable, above normal).
    pub attention: Vec<InboxItem>,
    pub held: Vec<HeldItem>,
    pub face: AttentionFace,
}

/// The snooze the operator placed last in this session, announced once by
/// the presentation with the return time computed here (C30, I11).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct SnoozeReceipt {
    pub id: String,
    pub title: String,
    pub returns_at: u32,
    /// Counts snoozes placed on this tenant in this session: a change is a
    /// new snooze, however many snapshots repeat it.
    pub sequence: u32,
}

/// The finite outcome of one product's Home read (Red Team, model and
/// skill): which of its sources were established, by name, and the worst
/// failure. A listing counts as established only when its page is the
/// whole inventory (not capped, nothing beyond the first page).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProductOutcome {
    pub(super) read_at: u32,
    pub(super) sources_read: u32,
    pub(super) sources_total: u32,
    /// The sources not established, by name.
    pub(super) missing: Vec<String>,
    pub(super) needs_role: bool,
    pub(super) failure: Option<ManagementFailure>,
    pub(super) detail: String,
}

/// A product read that established nothing (every source failed, or the
/// read never reached the service): kept apart from the last outcome that
/// did establish something, which stays shown as older.
/// The overview's time-dependent state, compared by the deadline ticker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct OverviewSignature {
    pub(super) fresh: bool,
    pub(super) face: AttentionFace,
    pub(super) attention: Vec<String>,
    /// The holds in effect now (an expired snooze drops out).
    pub(super) holds: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FailedAttempt {
    pub(super) at: u32,
    pub(super) needs_role: bool,
    pub(super) failure: Option<ManagementFailure>,
    pub(super) detail: String,
}

/// One tenant's Home reads, kept for the session.
pub(super) struct OverviewRecord {
    pub(super) tenant: TenantId,
    pub(super) read: Option<RuntimeOverview>,
    pub(super) failure: Option<ManagementFailure>,
    pub(super) failed_at: Option<u32>,
    /// Home read parts under way, each with the context it was admitted
    /// under (the overview and each other product). Per-operation
    /// bookkeeping, separate from the cached data: a part admitted under a
    /// context the tenant no longer has is withdrawn when it lands and never
    /// counts, however the tenant was switched meanwhile.
    parts: Vec<ContextRef>,
    pub(super) red_team: Option<ProductOutcome>,
    pub(super) model_skill: Option<ProductOutcome>,
    /// The latest attempt of each, when it established nothing (cleared by
    /// an attempt that did).
    pub(super) red_team_failed: Option<FailedAttempt>,
    pub(super) model_skill_failed: Option<FailedAttempt>,
    /// Completed reads of this tenant in this session.
    pub(super) generation: u32,
    /// The product a `recheckProduct` asked for, until its read lands.
    pub(super) rechecking: Option<ProductId>,
    snoozes: u32,
    pub(super) last_snooze: Option<SnoozeReceipt>,
}

impl OverviewRecord {
    fn new(tenant: TenantId) -> Self {
        Self {
            tenant,
            read: None,
            failure: None,
            failed_at: None,
            parts: Vec::new(),
            red_team: None,
            model_skill: None,
            red_team_failed: None,
            model_skill_failed: None,
            generation: 0,
            rechecking: None,
            snoozes: 0,
            last_snooze: None,
        }
    }

    /// Reading: a part admitted under the tenant's current context is
    /// still under way.
    pub(super) fn refreshing(&self, current: Option<&ContextRef>) -> bool {
        self.parts.iter().any(|c| Some(c) == current)
    }

    /// A product's read settled: an outcome that established at least one
    /// source replaces the retained outcome (and clears any failure); one
    /// that established nothing is kept as the latest failed attempt, the
    /// retained outcome staying shown as older.
    fn settle_product(&mut self, product: ProductId, outcome: ProductOutcome) {
        let (slot, failed) = match product {
            ProductId::RedTeam => (&mut self.red_team, &mut self.red_team_failed),
            ProductId::ModelSkill => (&mut self.model_skill, &mut self.model_skill_failed),
            ProductId::RuntimeSecurity | ProductId::Gateway => return,
        };
        if outcome.sources_read > 0 {
            *slot = Some(outcome);
            *failed = None;
        } else {
            *failed = Some(FailedAttempt {
                at: outcome.read_at,
                needs_role: outcome.needs_role,
                failure: outcome.failure,
                detail: outcome.detail,
            });
        }
    }

    /// One read part landed under `context`. A current part completes the
    /// read when no current part is left (one generation); a withdrawn
    /// (retired-context) part counts nothing, and a recheck it carried is
    /// simply over once nothing current remains.
    fn part_landed(&mut self, context: &ContextRef, current: Option<&ContextRef>) {
        if let Some(at) = self.parts.iter().position(|c| c == context) {
            self.parts.remove(at);
        }
        if self.refreshing(current) {
            return;
        }
        if Some(context) == current {
            self.generation = self.generation.wrapping_add(1);
        }
        self.rechecking = None;
    }
}

fn wall() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

/// A failure that means the client lacks a role, not that the source is down.
fn needs_role(failure: Option<ManagementFailure>) -> bool {
    matches!(
        failure,
        Some(ManagementFailure::Denied | ManagementFailure::Unauthorized)
    )
}

/// One source of a product's read: its name, whether it was established
/// (answered, and for a listing the whole inventory), and its failure.
type Source = (&'static str, bool, Option<ManagementFailure>);

fn outcome_of(read_at: u32, sources: &[Source], detail: String) -> ProductOutcome {
    let sources_read = sources.iter().filter(|(_, ok, _)| *ok).count() as u32;
    let failure = sources.iter().find_map(|(_, _, f)| *f);
    ProductOutcome {
        read_at,
        sources_read,
        sources_total: sources.len() as u32,
        missing: sources
            .iter()
            .filter(|(_, ok, _)| !*ok)
            .map(|(name, _, _)| (*name).to_owned())
            .collect(),
        needs_role: sources.iter().any(|(_, _, f)| needs_role(*f)),
        failure,
        detail,
    }
}

/// A listing establishes its source only when the page is the inventory:
/// a capped count or rows beyond the page leave it unread coverage.
fn listed(complete: Option<bool>) -> bool {
    complete == Some(true)
}

/// The names of a product's sources (for a read that never reached them).
pub(super) fn source_names(product: ProductId) -> &'static [&'static str] {
    match product {
        ProductId::RuntimeSecurity => &["sessions chart", "API keys"],
        ProductId::Gateway => &["Gateway usage", "Gateway limits"],
        ProductId::RedTeam => &["Red Team statistics", "Red Team scan list"],
        ProductId::ModelSkill => &["model scan list", "skill statistics", "skill scan list"],
    }
}

fn all_missing(product: ProductId) -> Vec<String> {
    source_names(product)
        .iter()
        .map(|s| (*s).to_owned())
        .collect()
}

impl State {
    /// Seconds since the epoch (fixture builds may move it forward).
    pub(super) fn clock(&self) -> u32 {
        wall().saturating_add(self.clock_offset)
    }

    /// What the overview's time-dependent state looks like now: when it
    /// differs from the last publication, the deadline ticker publishes.
    /// Besides freshness, the face and the attention list, the holds in
    /// effect (a snooze ends at its return time whether or not its entry is
    /// actionable, C30 Inbox), so an update's snooze expiry is published too.
    pub(super) fn overview_signature(&self) -> OverviewSignature {
        let view = self.overview_view();
        let now = self.clock();
        let holds = self
            .active_holds()
            .iter()
            .filter(|h| match h.kind {
                HoldKind::Acknowledge => true,
                HoldKind::Snooze => now < h.until,
            })
            .map(|h| h.id.clone())
            .collect();
        OverviewSignature {
            fresh: view.fresh,
            face: view.face,
            attention: view.attention.iter().map(|i| i.id.clone()).collect(),
            holds,
        }
    }

    /// The active tenant's record, if any read was ever started for it.
    pub(super) fn active_record(&self) -> Option<&OverviewRecord> {
        let tenant = self.data.active.as_ref()?;
        self.overviews.iter().find(|r| &r.tenant == tenant)
    }

    fn record_for(&mut self, tenant: &TenantId) -> &mut OverviewRecord {
        let at = match self.overviews.iter().position(|r| &r.tenant == tenant) {
            Some(at) => at,
            None => {
                self.overviews.push(OverviewRecord::new(tenant.clone()));
                self.overviews.len() - 1
            }
        };
        &mut self.overviews[at]
    }

    /// A Home read part was admitted under `context` (the overview, or one
    /// product's read): the tenant reads until every current part lands.
    pub(super) fn read_part_started(&mut self, context: &ContextRef) {
        let Ok(tenant) = self.check_context(context) else {
            return;
        };
        self.record_for(&tenant).parts.push(context.clone());
    }

    pub(super) fn overview_started(&mut self, context: &ContextRef) {
        self.read_part_started(context);
    }

    /// A part admitted under a context that has since retired: its reply is
    /// withdrawn and the tenant stops waiting for it; nothing completed.
    fn retired_part_done(&mut self, context: &ContextRef) {
        let current = self.active_ref.clone();
        if let Some(record) = self
            .overviews
            .iter_mut()
            .find(|r| r.parts.iter().any(|c| c == context))
        {
            record.part_landed(context, current.as_ref());
        }
    }

    /// Records a settled overview read for its context (a retired context's
    /// read is never kept).
    pub(super) fn overview_settled(&mut self, context: &ContextRef, result: ManagementResult) {
        if self.active_ref.as_ref() != Some(context) {
            self.retired_part_done(context);
            return;
        }
        let Some(tenant) = self.data.active.clone() else {
            return;
        };
        let clock = self.clock();
        let current = self.active_ref.clone();
        let record = self.record_for(&tenant);
        // A completed read of the current context: the presentation may
        // announce it (C30, I4), and a recheck is answered by it.
        record.part_landed(context, current.as_ref());
        match result {
            // Every source failed: a failed refresh. The last read stays
            // (marked by the failure); with none, the failed read is shown.
            ManagementResult::Overview { overview }
                if overview.chart.is_none()
                    && overview.keys_failure.is_some()
                    && overview
                        .gateway
                        .as_ref()
                        .is_none_or(|g| g.usage_failure.is_some() && g.policies.is_none()) =>
            {
                record.failure = overview.chart_failure.or(overview.keys_failure);
                record.failed_at = Some(clock);
                if record.read.is_none() {
                    record.read = Some(overview);
                }
            }
            ManagementResult::Overview { overview } => {
                record.failure = None;
                record.failed_at = None;
                let complete = overview.chart.is_some()
                    && overview.keys_complete
                    && overview.keys_failure.is_none();
                // A Gateway item's hold ends only when a read of its own
                // workspace established the day's requests and errors and
                // no longer reports it (C18); a read of another workspace
                // says nothing about it.
                let gateway_known = overview
                    .gateway
                    .as_ref()
                    .filter(|g| g.activity_known())
                    .map(|g| crate::observation::gateway_item_id(&g.workspace));
                let reported: Vec<String> = overview.items.iter().map(|i| i.id.clone()).collect();
                record.read = Some(overview);
                // v1: a complete fresh answer that no longer reports an item
                // ends its hold (a Runtime item by the Runtime sources, a
                // Gateway item by its own workspace's read).
                if let Some((_, holds)) = self.holds.iter_mut().find(|(t, _)| t == &tenant) {
                    holds.retain(|h| {
                        reported.contains(&h.id)
                            // A setup warning (C29) is not reported by reads:
                            // its hold stays until the operator lifts it.
                            || h.id.starts_with("vw-")
                            || if h.id.starts_with("gateway-") {
                                gateway_known.as_ref() != Some(&h.id)
                            } else {
                                !complete
                            }
                    });
                }
            }
            ManagementResult::Failed { failure, .. } => {
                record.failure = Some(failure);
                record.failed_at = Some(clock);
            }
            _ => {
                record.failure = Some(ManagementFailure::InvalidResponse);
                record.failed_at = Some(clock);
            }
        }
    }

    /// Records the finite outcome of one product's Home read (C30, I4): how
    /// many of its sources answered, whether a role is missing. A retired
    /// context's reply is withdrawn.
    pub(super) fn product_read_settled(
        &mut self,
        context: &ContextRef,
        product: ProductId,
        result: &ManagementResult,
    ) {
        if self.active_ref.as_ref() != Some(context) {
            self.retired_part_done(context);
            return;
        }
        let Some(tenant) = self.data.active.clone() else {
            return;
        };
        let clock = self.clock();
        let outcome = match (product, result) {
            (ProductId::RedTeam, ManagementResult::RedTeam { observation: o }) => outcome_of(
                o.read_at,
                &[
                    (
                        "Red Team statistics",
                        o.statistics.is_some(),
                        o.statistics_failure,
                    ),
                    (
                        "Red Team scan list",
                        listed(o.scans.as_ref().map(|s| s.complete)),
                        o.scans_failure,
                    ),
                ],
                match o.scans.as_ref() {
                    Some(s) if s.complete => format!("{} scans listed", s.rows.len()),
                    Some(s) => format!(
                        "{} of {} scans listed",
                        s.rows.len(),
                        s.total.map_or("more".to_owned(), |t| t.to_string())
                    ),
                    None => "no scan list".to_owned(),
                },
            ),
            (ProductId::ModelSkill, ManagementResult::SupplyChain { observation: o }) => {
                outcome_of(
                    o.read_at,
                    &[
                        (
                            "model scan list",
                            listed(o.models.as_ref().map(|m| m.complete)),
                            o.models_failure,
                        ),
                        (
                            "skill statistics",
                            o.skill_statistics.is_some(),
                            o.skill_statistics_failure,
                        ),
                        (
                            "skill scan list",
                            listed(o.skills.as_ref().map(|s| s.complete)),
                            o.skills_failure,
                        ),
                    ],
                    format!(
                        "{} models, {} skills{}",
                        o.models.as_ref().map_or(0, |m| m.rows.len()),
                        o.skills.as_ref().map_or(0, |s| s.rows.len()),
                        if o.models.as_ref().is_some_and(|m| !m.complete)
                            || o.skills.as_ref().is_some_and(|k| !k.complete)
                        {
                            " (first page; more exist)"
                        } else {
                            ""
                        }
                    ),
                )
            }
            (_, ManagementResult::Failed { failure, .. }) => ProductOutcome {
                read_at: clock,
                sources_read: 0,
                sources_total: product_sources(product),
                missing: all_missing(product),
                needs_role: needs_role(Some(*failure)),
                failure: Some(*failure),
                detail: format!("{failure:?}"),
            },
            _ => ProductOutcome {
                read_at: clock,
                sources_read: 0,
                sources_total: product_sources(product),
                missing: all_missing(product),
                needs_role: false,
                failure: Some(ManagementFailure::InvalidResponse),
                detail: "The service's answer was not understood.".to_owned(),
            },
        };
        let current = self.active_ref.clone();
        let record = self.record_for(&tenant);
        record.part_landed(context, current.as_ref());
        record.settle_product(product, outcome);
    }

    /// A product's read was asked for but not admitted: a finite, named
    /// failure rather than a part that never lands.
    pub(super) fn product_read_refused(
        &mut self,
        tenant: &TenantId,
        product: ProductId,
        refusal: Refusal,
    ) {
        let clock = self.clock();
        let record = self.record_for(tenant);
        let outcome = ProductOutcome {
            read_at: clock,
            sources_read: 0,
            sources_total: product_sources(product),
            missing: all_missing(product),
            needs_role: false,
            failure: None,
            detail: format!("The read was not admitted ({refusal:?})."),
        };
        record.settle_product(product, outcome);
    }

    pub(super) fn set_rechecking(&mut self, tenant: &TenantId, product: Option<ProductId>) {
        self.record_for(tenant).rechecking = product;
    }

    /// The items the current read reports, plus the active tenant's setup
    /// warnings (C29), which holds address like any other item.
    fn current_items(&self, context: &ContextRef) -> Vec<InboxItem> {
        let Ok(tenant) = self.check_context(context) else {
            return Vec::new();
        };
        let mut items: Vec<InboxItem> = self
            .overviews
            .iter()
            .find(|r| r.tenant == tenant)
            .and_then(|r| r.read.as_ref())
            .map(|r| r.items.clone())
            .unwrap_or_default();
        if self.active_ref.as_ref() == Some(context) {
            items.extend(self.setup_warning_items());
        }
        items
    }

    /// The active tenant's holds.
    pub(super) fn active_holds(&self) -> &[Hold] {
        self.data
            .active
            .as_ref()
            .and_then(|t| self.holds.iter().find(|(h, _)| h == t))
            .map_or(&[], |(_, h)| h.as_slice())
    }

    /// Acknowledges or snoozes an attention item the current read reports.
    pub(super) fn hold_insight(
        &mut self,
        context: &ContextRef,
        id: &str,
        kind: HoldKind,
    ) -> Result<(), Refusal> {
        let tenant = self.check_context(context)?;
        let item = self
            .current_items(context)
            .into_iter()
            .find(|i| i.id == id)
            .ok_or(Refusal::InvalidInput)?;
        let clock = self.clock();
        let entry = match self.holds.iter().position(|(t, _)| t == &tenant) {
            Some(i) => &mut self.holds[i].1,
            None => {
                self.holds.push((tenant.clone(), Vec::new()));
                &mut self.holds.last_mut().expect("pushed").1
            }
        };
        entry.retain(|h| h.id != id);
        if entry.len() >= HOLD_LIMIT {
            return Err(Refusal::Busy);
        }
        let until = clock.saturating_add(SNOOZE_SECS);
        entry.push(Hold {
            id: id.to_owned(),
            kind,
            level: level(&item),
            until,
        });
        if kind == HoldKind::Snooze {
            // The receipt the presentation announces once (C30, I11).
            let record = self.record_for(&tenant);
            record.snoozes = record.snoozes.wrapping_add(1);
            record.last_snooze = Some(SnoozeReceipt {
                id: id.to_owned(),
                title: item.title,
                returns_at: until,
                sequence: record.snoozes,
            });
        }
        Ok(())
    }

    pub(super) fn restore_insight(
        &mut self,
        context: &ContextRef,
        id: &str,
    ) -> Result<(), Refusal> {
        let tenant = self.check_context(context)?;
        let holds = self
            .holds
            .iter_mut()
            .find(|(t, _)| t == &tenant)
            .ok_or(Refusal::InvalidInput)?;
        let before = holds.1.len();
        holds.1.retain(|h| h.id != id);
        (holds.1.len() < before)
            .then_some(())
            .ok_or(Refusal::InvalidInput)
    }

    pub(super) fn forget_holds(&mut self, tenant: &TenantId) {
        self.holds.retain(|(t, _)| t != tenant);
        self.overviews.retain(|r| &r.tenant != tenant);
    }

    pub(super) fn overview_view(&self) -> OverviewView {
        let now = self.clock();
        let record = self.active_record();
        let read = record.and_then(|r| r.read.clone());
        let fresh = read
            .as_ref()
            .is_some_and(|r| now.saturating_sub(r.read_at) < FRESH_SECS);
        let holds: &[Hold] = self.active_holds();
        let mut attention = Vec::new();
        let mut held = Vec::new();
        let warnings = self.setup_warning_items();
        for item in read
            .iter()
            .flat_map(|r| r.items.iter())
            .chain(warnings.iter())
        {
            if !(item.action && level(item) > 0) {
                continue;
            }
            let hold = holds.iter().find(|h| {
                h.id == item.id
                    && match h.kind {
                        HoldKind::Acknowledge => level(item) <= h.level,
                        HoldKind::Snooze => now < h.until,
                    }
            });
            match hold {
                Some(h) => held.push(HeldItem {
                    item: item.clone(),
                    kind: h.kind,
                    returns_at: (h.kind == HoldKind::Snooze).then_some(h.until),
                }),
                None => attention.push(item.clone()),
            }
        }
        let failure = record.and_then(|r| r.failure);
        let face = if !attention.is_empty() {
            AttentionFace::Items
        } else if read.is_none() {
            AttentionFace::FirstLaunch
        } else if fresh
            && failure.is_none()
            && read.as_ref().is_some_and(|r| {
                r.chart.is_some()
                    && r.keys_complete
                    && r.keys_failure.is_none()
                    && r.gaps.is_empty()
            })
        {
            AttentionFace::AllClear
        } else {
            AttentionFace::Gap
        };
        OverviewView {
            read,
            refreshing: record.is_some_and(|r| r.refreshing(self.active_ref.as_ref())),
            failure,
            failed_at: record.and_then(|r| r.failed_at),
            fresh,
            attention,
            held,
            face,
        }
    }
}

/// How many sources a product's Home read has.
pub(super) fn product_sources(product: ProductId) -> u32 {
    match product {
        ProductId::RuntimeSecurity | ProductId::Gateway | ProductId::RedTeam => 2,
        ProductId::ModelSkill => 3,
    }
}
