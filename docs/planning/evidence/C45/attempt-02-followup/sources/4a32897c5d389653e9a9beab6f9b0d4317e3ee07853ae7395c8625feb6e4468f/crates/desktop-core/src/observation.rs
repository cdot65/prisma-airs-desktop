//! Runtime Home/Inbox observations (C12): the last day's sessions chart and
//! the API key inventory's first page, summarized without fabricating zeros
//! or completeness, and the Inbox items and source gaps they support.
//!
//! Working v1 (74754c02, read-only): `bridge/src/runtime.ts` (chart
//! validation), `src/ui/pages/home/insights.rs` (the Runtime summary, Inbox
//! items, gaps and their order), `src/cache.rs` (freshness) and
//! `src/needs_attention.rs` (attention faces and holds). Gateway, Red
//! Teaming and Supply Chain sources belong to C16-C18.

use serde::Serialize;
use serde_json::Value;
use ts_rs::TS;

/// A read is fresh for ten minutes (v1 `FRESH_FOR`, strict).
pub const FRESH_SECS: u32 = 600;
/// Automatic refresh waits this long after a failed read (v1 back-off).
pub const RETRY_SECS: u32 = 600;
/// A snooze hides an item for 24 hours (v1 `SNOOZE_SECONDS`).
pub const SNOOZE_SECS: u32 = 86_400;
/// Keys expiring within this many days are Inbox items (inclusive, v1).
pub const EXPIRY_DAYS: i64 = 30;
/// Most chart buckets (v1).
pub const MAX_BUCKETS: usize = 2_048;
/// Most holds per tenant (v1 `ITEM_LIMIT`).
pub const HOLD_LIMIT: usize = 256;
/// The key inventory page read for expiry insights.
pub const KEY_PAGE: u32 = 100;

/// One detector's count in a chart bucket.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DetectorCount {
    pub detector: String,
    pub count: u32,
}

/// One chart bucket as the service reported it (boundaries kept).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ChartBucket {
    pub time: String,
    pub total: u32,
    pub violating: u32,
    /// Per-detector violation counts; `None` when the bucket reported no
    /// breakdown (or an empty one while violating) — unknown, never zero.
    pub detections: Option<Vec<DetectorCount>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeChart {
    pub bucket_seconds: u32,
    pub buckets: Vec<ChartBucket>,
}

/// The chart's totals; each `None` is unknown (never 0).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSummary {
    pub sessions: Option<u32>,
    pub violating: Option<u32>,
    /// `sessions - violating`; unknown when the totals are inconsistent.
    pub benign: Option<u32>,
    /// Detector violations summed over every bucket; unknown unless every
    /// bucket reported a usable breakdown.
    pub detection_events: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    Ok,
    Warn,
    Bad,
}

/// Where an Inbox item leads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InboxDestination {
    ApiKeys,
    Sessions,
    /// The AI Gateway's observation of the chosen workspace (C18).
    Gateway,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub label: String,
    pub value: String,
}

/// One Inbox insight (v1 `insights.rs`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub source: String,
    /// Needs action (otherwise an update).
    pub action: bool,
    pub tone: Tone,
    pub destination: InboxDestination,
    pub action_label: String,
    pub facts: Vec<Fact>,
    /// Violating share of sessions in per-mille, when known.
    pub per_mille: Option<u32>,
}

/// One Runtime overview read: each source's data or sanitized failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeOverview {
    /// Seconds since the Unix epoch.
    pub read_at: u32,
    pub chart: Option<RuntimeChart>,
    pub chart_failure: Option<crate::management::ManagementFailure>,
    pub summary: RuntimeSummary,
    /// Whether the key page was the whole inventory.
    pub keys_complete: bool,
    pub keys_failure: Option<crate::management::ManagementFailure>,
    pub items: Vec<InboxItem>,
    /// What the sources could not establish (sorted, deduplicated).
    pub gaps: Vec<String>,
    /// The chosen Gateway workspace's last day (C18); none when no
    /// workspace was chosen when the overview was read.
    pub gateway: Option<crate::gateway_observe::GatewayOverview>,
}

impl RuntimeOverview {
    /// The sessions chart established the day's activity: read, with
    /// consistent totals (an inconsistent chart is a gap, see `summarize`).
    pub fn chart_established(&self) -> bool {
        self.chart.is_some()
            && self.chart_failure.is_none()
            && self.summary.sessions.is_some()
            && self.summary.violating.is_some()
            && self.summary.benign.is_some()
    }

    /// The key inventory established every key's expiration: read, whole,
    /// and nothing unreported (an unreported expiration is a gap, see
    /// `key_items`).
    pub fn keys_established(&self) -> bool {
        self.keys_failure.is_none()
            && self.keys_complete
            && !self
                .gaps
                .iter()
                .any(|g| g.ends_with(" unreported") || g.contains("unreported"))
    }
}

fn count(value: Option<&Value>) -> Option<u32> {
    value
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

/// The Inbox item a Gateway workspace's errors raise.
pub fn gateway_item_id(workspace: &str) -> String {
    format!("gateway-errors-{workspace}")
}

/// The Gateway's Inbox item and gaps (C18): an item when the chosen
/// workspace answered with errors in the last day (bad at one error in
/// ten); a gap for each source that could not be read or was partial.
pub fn gateway_items(
    gateway: &crate::gateway_observe::GatewayOverview,
) -> (Option<InboxItem>, Vec<String>) {
    let mut gaps = Vec::new();
    if gateway.usage_failure.is_some() {
        gaps.push("Gateway activity unavailable".to_owned());
    } else if gateway.partial {
        gaps.push("Gateway activity partial (over the service's quota)".to_owned());
    } else if !gateway.activity_known() {
        gaps.push("Gateway requests or errors not reported".to_owned());
    }
    if gateway.limits_failure.is_some() || gateway.policies.is_none() {
        gaps.push("Gateway limit policies unavailable".to_owned());
    }
    let item = match (gateway.requests, gateway.errors) {
        (Some(requests), Some(errors)) if errors.value() > 0.0 => {
            let (requests, errors) = (requests.value(), errors.value());
            let bad = requests > 0.0 && errors * 10.0 >= requests;
            Some(InboxItem {
                id: gateway_item_id(&gateway.workspace),
                title: format!("Gateway errors in {}", gateway.workspace_name),
                detail: format!(
                    "{errors:.0} of {requests:.0} Gateway requests in the last day failed."
                ),
                source: "AI Gateway".to_owned(),
                action: true,
                tone: if bad { Tone::Bad } else { Tone::Warn },
                destination: InboxDestination::Gateway,
                action_label: "Open Gateway logs".to_owned(),
                facts: vec![
                    Fact {
                        label: "Requests".to_owned(),
                        value: format!("{requests:.0}"),
                    },
                    Fact {
                        label: "Errors".to_owned(),
                        value: format!("{errors:.0}"),
                    },
                ],
                per_mille: (requests > 0.0)
                    .then(|| ((errors / requests) * 1000.0).round().min(1000.0) as u32),
            })
        }
        _ => None,
    };
    (item, gaps)
}

/// v1's chart contract: an empty reply is an empty hourly chart; otherwise
/// a bucket size of at least one second, at most 2,048 buckets, each with a
/// parseable time of at most 64 characters and non-negative integer totals.
pub fn parse_chart(value: Option<Value>) -> Option<RuntimeChart> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Some(RuntimeChart {
            bucket_seconds: 3600,
            buckets: Vec::new(),
        });
    };
    let bucket_seconds = count(value.get("bucket_size_seconds")).filter(|n| *n >= 1)?;
    let raw = value.get("buckets").and_then(Value::as_array)?;
    if raw.len() > MAX_BUCKETS {
        return None;
    }
    let mut buckets = Vec::with_capacity(raw.len());
    for b in raw {
        let time = b.get("time").and_then(Value::as_str)?;
        if time.len() > 64 || parse_instant(time).is_none() {
            return None;
        }
        let total = count(b.get("total"))?;
        let violating = count(b.get("violating"))?;
        let detections = match b.get("detection_type_violation_breakdown") {
            Some(Value::Array(entries)) if entries.is_empty() && violating > 0 => None,
            Some(Value::Array(entries)) => entries
                .iter()
                .map(|e| {
                    let detector = e.get("detection_type").and_then(Value::as_str)?;
                    let counts = e.get("violation_breakdown")?;
                    let n = match count(counts.get("total")) {
                        Some(total) => total,
                        None => ["critical", "high", "medium", "low"]
                            .iter()
                            .map(|k| count(counts.get(*k)))
                            .try_fold(0u32, |acc, n| acc.checked_add(n?))?,
                    };
                    Some(DetectorCount {
                        detector: detector.to_owned(),
                        count: n,
                    })
                })
                .collect::<Option<Vec<_>>>(),
            _ => None,
        };
        buckets.push(ChartBucket {
            time: time.to_owned(),
            total,
            violating,
            detections,
        });
    }
    Some(RuntimeChart {
        bucket_seconds,
        buckets,
    })
}

/// v1's Runtime summary: sums over the buckets (any overflow is unknown),
/// benign by subtraction (an underflow is unknown and a gap), detector
/// events only when every bucket reported a usable breakdown.
pub fn summarize(chart: &RuntimeChart) -> (RuntimeSummary, Vec<String>) {
    let mut gaps = Vec::new();
    let sum = |f: &dyn Fn(&ChartBucket) -> u32| {
        chart
            .buckets
            .iter()
            .try_fold(0u32, |acc, b| acc.checked_add(f(b)))
    };
    let sessions = sum(&|b| b.total);
    let violating = sum(&|b| b.violating);
    let benign = match (sessions, violating) {
        (Some(s), Some(v)) => {
            let benign = s.checked_sub(v);
            if benign.is_none() {
                gaps.push(
                    "Runtime activity has inconsistent totals; benign count unavailable".to_owned(),
                );
            }
            benign
        }
        _ => None,
    };
    let detection_events = chart.buckets.iter().try_fold(0u32, |acc, b| {
        let events = b
            .detections
            .as_ref()?
            .iter()
            .try_fold(0u32, |a, d| a.checked_add(d.count))?;
        acc.checked_add(events)
    });
    (
        RuntimeSummary {
            sessions,
            violating,
            benign,
            detection_events,
        },
        gaps,
    )
}

/// Seconds since the Unix epoch of an RFC 3339 instant (`Z` or an offset,
/// optional fractional seconds); `None` when it is not one.
pub fn parse_instant(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let s = text.get(r)?;
        s.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| s.parse().ok())
            .flatten()
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (h, mi, s) = (num(11..13)?, num(14..16)?, num(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    let mut rest = &text[19..];
    if let Some(frac) = rest.strip_prefix('.') {
        let digits = frac.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        rest = &frac[digits..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6
            && matches!(rest.as_bytes()[0], b'+' | b'-')
            && rest.as_bytes()[3] == b':' =>
        {
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6)?.parse().ok()?;
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };
    // Days from civil (Howard Hinnant).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + s - offset)
}

fn plural(n: i64, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// v1's key-expiry insights from the first key page: one item per active
/// key whose expiration is within 30 days (inclusive), and a gap for active
/// keys whose expiration is missing or unparseable.
pub fn key_items(keys: &[Value], complete: bool, now: i64) -> (Vec<InboxItem>, Vec<String>) {
    let source = if complete {
        "Runtime Security API keys".to_owned()
    } else {
        "Runtime Security API keys · partial inventory".to_owned()
    };
    let mut items = Vec::new();
    let mut unreported = 0;
    for key in keys {
        let text = |k: &str| key.get(k).and_then(Value::as_str).unwrap_or_default();
        let status = text("status").to_ascii_lowercase();
        if key.get("revoked").and_then(Value::as_bool) == Some(true)
            || key.get("removed").and_then(Value::as_bool) == Some(true)
            || matches!(status.as_str(), "revoked" | "disabled")
        {
            continue;
        }
        let Some(expires) = parse_instant(text("expiration")) else {
            unreported += 1;
            continue;
        };
        let days = (expires - now).div_euclid(86_400);
        if days > EXPIRY_DAYS {
            continue;
        }
        let name = [text("api_key_name"), text("name"), text("api_key_id")]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("API key")
            .to_owned();
        let id = [text("api_key_id"), text("id"), text("api_key_name")]
            .into_iter()
            .find(|s| !s.is_empty())
            .unwrap_or("key");
        let expired = expires < now;
        let title = if expired {
            let ago = (now - expires).div_euclid(86_400).max(1);
            format!("{name} expired {} ago", plural(ago, "day"))
        } else if days == 0 {
            format!("{name} expires today")
        } else {
            format!("{name} expires in {}", plural(days, "day"))
        };
        items.push(InboxItem {
            id: format!("runtime-key:{id}"),
            title,
            detail: "Review this key and rotate or replace it before its clients lose access. This insight reads expiration metadata only; opening it never retrieves a secret.".to_owned(),
            source: source.clone(),
            action: true,
            tone: if expired { Tone::Bad } else { Tone::Warn },
            destination: InboxDestination::ApiKeys,
            action_label: "Manage API keys".to_owned(),
            facts: vec![
                Fact { label: "Key".into(), value: name.clone() },
                Fact { label: "Expires".into(), value: text("expiration").to_owned() },
                Fact { label: "Source".into(), value: source.clone() },
            ],
            per_mille: None,
        });
    }
    let mut gaps = Vec::new();
    if unreported > 0 {
        gaps.push(format!(
            "{source}: {} unreported",
            plural(unreported, "key expiration")
        ));
    }
    if !complete {
        gaps.push("Runtime Security API keys · partial inventory".to_owned());
    }
    (items, gaps)
}

/// v1's Runtime activity insight from the chart summary.
pub fn activity_item(summary: &RuntimeSummary) -> Option<InboxItem> {
    let sessions = summary.sessions?;
    let violating = summary.violating?;
    let fact = |n: Option<u32>| n.map_or_else(|| "Unreported".to_owned(), |n| n.to_string());
    Some(InboxItem {
        id: "runtime-activity".to_owned(),
        title: if violating > 0 {
            format!(
                "{violating} violating session{} to investigate",
                if violating == 1 { "" } else { "s" }
            )
        } else {
            "Runtime activity available".to_owned()
        },
        detail: "Sessions and violations Runtime Security reported for the last 24 hours."
            .to_owned(),
        source: "Runtime Security · last 24h".to_owned(),
        action: violating > 0,
        tone: if violating > 0 { Tone::Warn } else { Tone::Ok },
        destination: InboxDestination::Sessions,
        action_label: "Inspect sessions".to_owned(),
        facts: vec![
            Fact {
                label: "Sessions".into(),
                value: sessions.to_string(),
            },
            Fact {
                label: "Violating".into(),
                value: violating.to_string(),
            },
            Fact {
                label: "Benign".into(),
                value: fact(summary.benign),
            },
        ],
        // Only a consistent pair has a share; inconsistent totals are a gap.
        per_mille: (sessions > 0 && violating <= sessions)
            .then(|| u32::try_from(u64::from(violating) * 1000 / u64::from(sessions)).ok())
            .flatten(),
    })
}

/// v1's order: bad first, then items needing action, then the rest, each by
/// identifier.
pub fn sort_items(items: &mut [InboxItem]) {
    items.sort_by(|a, b| {
        (b.tone == Tone::Bad, b.action, &a.id).cmp(&(a.tone == Tone::Bad, a.action, &b.id))
    });
}

/// An attention level (v1: bad is critical, ok is normal, otherwise warn).
pub fn level(item: &InboxItem) -> u8 {
    match item.tone {
        Tone::Bad => 2,
        Tone::Warn => 1,
        Tone::Ok => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_792_000_000;

    fn iso(secs: i64) -> String {
        crate::sessions::rfc3339(u64::try_from(secs).expect("positive"))
    }

    #[test]
    fn the_chart_contract_follows_v1() {
        assert_eq!(parse_chart(None).map(|c| c.buckets.len()), Some(0));
        let ok = json!({"bucket_size_seconds": 3600, "buckets": [
            {"time": "2026-10-19T00:00:00Z", "total": 5, "violating": 2,
             "detection_type_violation_breakdown": [
                {"detection_type": "pi", "violation_breakdown": {"total": 2}},
                {"detection_type": "dlp", "violation_breakdown": {"high": 1, "low": 0, "medium": 0, "critical": 0}}]}]});
        let chart = parse_chart(Some(ok)).expect("chart");
        assert_eq!(chart.buckets[0].detections.as_ref().map(Vec::len), Some(2));
        for bad in [
            json!({"bucket_size_seconds": 0, "buckets": []}),
            json!({"bucket_size_seconds": 3600}),
            json!({"bucket_size_seconds": 3600, "buckets": [{"time": "yesterday", "total": 1, "violating": 0}]}),
            json!({"bucket_size_seconds": 3600, "buckets": [{"time": "2026-10-19T00:00:00Z", "total": -1, "violating": 0}]}),
            json!({"bucket_size_seconds": 3600, "buckets": [{"time": "2026-10-19T00:00:00Z", "total": 1.5, "violating": 0}]}),
        ] {
            assert_eq!(parse_chart(Some(bad.clone())), None, "{bad}");
        }
        let many: Vec<Value> = (0..=MAX_BUCKETS)
            .map(|_| json!({"time": "2026-10-19T00:00:00Z", "total": 0, "violating": 0}))
            .collect();
        assert_eq!(
            parse_chart(Some(json!({"bucket_size_seconds": 60, "buckets": many}))),
            None
        );
    }

    #[test]
    fn summaries_never_fabricate_benign_or_detector_counts() {
        let bucket = |total, violating, detections: Option<Vec<DetectorCount>>| ChartBucket {
            time: "2026-10-19T00:00:00Z".into(),
            total,
            violating,
            detections,
        };
        let chart = RuntimeChart {
            bucket_seconds: 3600,
            buckets: vec![
                bucket(
                    10,
                    3,
                    Some(vec![DetectorCount {
                        detector: "pi".into(),
                        count: 4,
                    }]),
                ),
                bucket(5, 0, Some(vec![])),
            ],
        };
        let (summary, gaps) = summarize(&chart);
        assert_eq!(
            summary,
            RuntimeSummary {
                sessions: Some(15),
                violating: Some(3),
                benign: Some(12),
                detection_events: Some(4)
            }
        );
        assert!(gaps.is_empty());
        // A bucket without a breakdown makes detector events unknown.
        let partial = RuntimeChart {
            bucket_seconds: 3600,
            buckets: vec![bucket(4, 1, None)],
        };
        assert_eq!(summarize(&partial).0.detection_events, None);
        // Inconsistent totals: benign is unknown and a gap says why.
        let inconsistent = RuntimeChart {
            bucket_seconds: 3600,
            buckets: vec![bucket(1, 2, Some(vec![]))],
        };
        let (summary, gaps) = summarize(&inconsistent);
        assert_eq!(summary.benign, None);
        assert_eq!(
            gaps,
            ["Runtime activity has inconsistent totals; benign count unavailable"]
        );
        // An empty breakdown while violating is unknown, not zero.
        let raw = json!({"bucket_size_seconds": 3600, "buckets": [
            {"time": "2026-10-19T00:00:00Z", "total": 2, "violating": 1, "detection_type_violation_breakdown": []}]});
        let chart = parse_chart(Some(raw)).expect("chart");
        assert_eq!(summarize(&chart).0.detection_events, None);
    }

    #[test]
    fn key_expiry_is_inclusive_at_thirty_days_and_skips_inactive_keys() {
        let keys = vec![
            json!({"api_key_id": "k30", "api_key_name": "Thirty", "expiration": iso(NOW + 30 * 86_400 + 60)}),
            json!({"api_key_id": "k31", "api_key_name": "ThirtyOne", "expiration": iso(NOW + 31 * 86_400 + 60)}),
            json!({"api_key_id": "k0", "api_key_name": "Today", "expiration": iso(NOW + 3600)}),
            json!({"api_key_id": "kx", "api_key_name": "Gone", "expiration": iso(NOW - 2 * 86_400 - 60)}),
            json!({"api_key_id": "kr", "api_key_name": "Revoked", "revoked": true, "expiration": iso(NOW)}),
            json!({"api_key_id": "kd", "api_key_name": "Disabled", "status": "DISABLED", "expiration": iso(NOW)}),
            json!({"api_key_id": "ku", "api_key_name": "Unknown"}),
        ];
        let (mut items, gaps) = key_items(&keys, true, NOW);
        sort_items(&mut items);
        let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Gone expired 2 days ago",
                "Today expires today",
                "Thirty expires in 30 days"
            ]
        );
        assert_eq!(items[0].tone, Tone::Bad);
        assert_eq!(
            gaps,
            ["Runtime Security API keys: 1 key expiration unreported"]
        );
        let (_, gaps) = key_items(&[], false, NOW);
        assert_eq!(gaps, ["Runtime Security API keys · partial inventory"]);
    }

    #[test]
    fn activity_needs_known_totals() {
        assert_eq!(activity_item(&RuntimeSummary::default()), None);
        let item = activity_item(&RuntimeSummary {
            sessions: Some(8),
            violating: Some(2),
            benign: None,
            detection_events: None,
        })
        .expect("item");
        assert_eq!(item.title, "2 violating sessions to investigate");
        assert_eq!(item.per_mille, Some(250));
        assert_eq!(item.facts[2].value, "Unreported");
        let quiet = activity_item(&RuntimeSummary {
            sessions: Some(0),
            violating: Some(0),
            benign: Some(0),
            detection_events: Some(0),
        })
        .expect("item");
        assert_eq!(
            (quiet.title.as_str(), quiet.action, quiet.per_mille),
            ("Runtime activity available", false, None)
        );
        // Inconsistent totals have no share (the summary reports the gap).
        let inconsistent = activity_item(&RuntimeSummary {
            sessions: Some(1),
            violating: Some(2),
            benign: None,
            detection_events: None,
        })
        .expect("item");
        assert_eq!(inconsistent.per_mille, None);
    }

    #[test]
    fn instants_parse_offsets_and_reject_other_text() {
        assert_eq!(parse_instant("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_instant("1970-01-01T01:00:00+01:00"), Some(0));
        assert_eq!(
            parse_instant("2026-10-19T10:00:00.123Z"),
            parse_instant("2026-10-19T10:00:00Z")
        );
        for bad in [
            "",
            "2026-10-19",
            "2026-13-01T00:00:00Z",
            "2026-10-19T10:00:00",
            "soon",
        ] {
            assert_eq!(parse_instant(bad), None, "{bad}");
        }
    }
}
