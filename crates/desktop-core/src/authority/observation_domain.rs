//! Runtime Home/Inbox state inside the authority (C12): the active
//! context's last overview read (kept when a refresh fails, marked with its
//! failure), its freshness, and the operator's attention holds per tenant.
//! A healthy empty source, a failed one and one not yet read stay distinct;
//! "all clear" is claimed only from a fresh read of both sources with
//! nothing open and no gap.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{Refusal, State};
use crate::context::ContextRef;
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
    id: String,
    kind: HoldKind,
    level: u8,
    until: u32,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OverviewView {
    /// The last overview read for this context (kept when a refresh fails).
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

pub(super) struct OverviewRecord {
    context: ContextRef,
    read: Option<RuntimeOverview>,
    refreshing: bool,
    failure: Option<ManagementFailure>,
    failed_at: Option<u32>,
}

fn wall() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX))
}

impl State {
    /// Seconds since the epoch (fixture builds may move it forward).
    pub(super) fn clock(&self) -> u32 {
        wall().saturating_add(self.clock_offset)
    }

    /// What the overview's time-dependent state looks like now: when it
    /// differs from the last publication, the deadline ticker publishes.
    pub(super) fn overview_signature(&self) -> (bool, AttentionFace, Vec<String>) {
        let view = self.overview_view();
        (
            view.fresh,
            view.face,
            view.attention.iter().map(|i| i.id.clone()).collect(),
        )
    }

    fn overview_record(&mut self, context: &ContextRef) -> &mut OverviewRecord {
        if self.overview.as_ref().is_none_or(|o| &o.context != context) {
            self.overview = Some(OverviewRecord {
                context: context.clone(),
                read: None,
                refreshing: false,
                failure: None,
                failed_at: None,
            });
        }
        self.overview.as_mut().expect("just set")
    }

    pub(super) fn overview_started(&mut self, context: &ContextRef) {
        self.overview_record(context).refreshing = true;
    }

    /// Records a settled overview read for its context (a retired context's
    /// read is never kept).
    pub(super) fn overview_settled(&mut self, context: &ContextRef, result: ManagementResult) {
        if self.active_ref.as_ref() != Some(context) {
            return;
        }
        let tenant = self.data.active.clone();
        let clock = self.clock();
        let record = self.overview_record(context);
        record.refreshing = false;
        match result {
            // Every source failed: a failed refresh. The last read stays
            // (marked by the failure); with none, the failed read is shown.
            ManagementResult::Overview { overview }
                if overview.chart.is_none() && overview.keys_failure.is_some() =>
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
                let reported: Vec<String> = overview.items.iter().map(|i| i.id.clone()).collect();
                record.read = Some(overview);
                // v1: a complete fresh answer that no longer reports an item
                // ends its hold.
                if complete
                    && let Some(tenant) = tenant
                    && let Some((_, holds)) = self.holds.iter_mut().find(|(t, _)| t == &tenant)
                {
                    holds.retain(|h| reported.contains(&h.id));
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

    fn current_items(&self, context: &ContextRef) -> Vec<InboxItem> {
        self.overview
            .as_ref()
            .filter(|o| &o.context == context)
            .and_then(|o| o.read.as_ref())
            .map(|r| r.items.clone())
            .unwrap_or_default()
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
                self.holds.push((tenant, Vec::new()));
                &mut self.holds.last_mut().expect("pushed").1
            }
        };
        entry.retain(|h| h.id != id);
        if entry.len() >= HOLD_LIMIT {
            return Err(Refusal::Busy);
        }
        entry.push(Hold {
            id: id.to_owned(),
            kind,
            level: level(&item),
            until: clock.saturating_add(SNOOZE_SECS),
        });
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
    }

    pub(super) fn overview_view(&self) -> OverviewView {
        let now = self.clock();
        let record = self
            .overview
            .as_ref()
            .filter(|o| Some(&o.context) == self.active_ref.as_ref());
        let read = record.and_then(|r| r.read.clone());
        let fresh = read
            .as_ref()
            .is_some_and(|r| now.saturating_sub(r.read_at) < FRESH_SECS);
        let holds: &[Hold] = self
            .data
            .active
            .as_ref()
            .and_then(|t| self.holds.iter().find(|(h, _)| h == t))
            .map_or(&[], |(_, h)| h.as_slice());
        let mut attention = Vec::new();
        let mut held = Vec::new();
        for item in read.iter().flat_map(|r| r.items.iter()) {
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
            refreshing: record.is_some_and(|r| r.refreshing),
            failure,
            failed_at: record.and_then(|r| r.failed_at),
            fresh,
            attention,
            held,
            face,
        }
    }
}
