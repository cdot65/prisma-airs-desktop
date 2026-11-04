//! Gateway observation reads (C18, D044): the chosen workspace's usage
//! (requests, cost, tokens and latency over a window, and its busiest
//! models), its request logs page by page, and the rate- and usage-limit
//! policies that apply to it. Every read goes through the SDK's typed
//! telemetry and policy operations over the tenant's management OAuth
//! client and is bound to the workspace chosen natively (its slug for
//! telemetry, its ID for policies). Each source keeps its own data or its
//! sanitized failure; a total the service did not report is unknown, never
//! zero, and a reply the service marks over its quota is partial. Values
//! leave the core only as the allow-listed fields below: log metadata,
//! user identities, request URLs and policy conditions do not.

use scm_sdk::RequestOptions;
use scm_sdk::airs::gateway::models::{
    GatewayRateLimitsClientListOptions, GatewayUsageLimitsClientListOptions,
};
use scm_sdk::airs::gateway::{
    AIGatewayChartOptions, AIGatewayClient, AIGatewayGroupColumn, AIGatewayGroupDimension,
    AIGatewayGroupOptions, AIGatewayLogsOptions, AIGatewayWindowOptions,
};
use scm_sdk::data::Optional;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::gateway::WorkspaceScope;
use crate::management::{ManagementFailure, PageError};

/// Windows a usage or logs read may cover, in days.
pub const WINDOWS: [u8; 3] = [1, 7, 30];
/// Points one series may carry (v1).
pub const MAX_POINTS: usize = 2048;
/// Model rows kept, busiest first (v1).
pub const MAX_GROUPS: usize = 100;
/// Rows a group reply may carry before it is refused.
const MAX_GROUP_ROWS: usize = 200;
/// Log rows per page, and the pages one window may read (v1).
pub const LOG_PAGE: u64 = 25;
pub const LOG_PAGES: u32 = 40;
/// Policies one list may carry.
pub const MAX_POLICIES: usize = 200;
const MAX_TEXT: usize = 256;

/// A finite, non-negative measure as the service reported it (cents for
/// cost, milliseconds for latency, units for tokens).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, TS)]
#[ts(type = "number")]
pub struct Measure(f64);

// Only finite values are ever constructed.
impl Eq for Measure {}

impl Measure {
    fn of(value: &Value) -> Option<Self> {
        value
            .as_f64()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .map(Self)
    }

    pub fn value(self) -> f64 {
        self.0
    }
}

/// One series point: the service's bucket label and its value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub x: String,
    pub y: Measure,
}

/// One chart over the window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub points: Vec<Point>,
    /// The service's total (for latency, its mean); unknown when not
    /// reported, or when the reply is partial.
    pub total: Option<Measure>,
    /// Errors in the window (the requests chart only), when reported.
    pub errors: Option<Measure>,
    /// The service said the window is over its quota: what it returned is
    /// part of the window.
    pub partial: bool,
}

/// One model's share of the window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    pub provider: Option<String>,
    pub requests: Measure,
    pub cost: Option<Measure>,
    pub total_tokens: Option<Measure>,
    /// 0-1 as the service reported it.
    pub success_rate: Option<Measure>,
}

/// The busiest models in the window.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ModelGroups {
    pub rows: Vec<ModelUsage>,
    pub total: Option<Measure>,
    /// Every model the service returned is shown (none were cut).
    pub complete: bool,
    pub partial: bool,
}

/// The chosen workspace's usage over a window. Each source keeps its data
/// or its failure; a read in which none answered fails.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUsage {
    pub workspace: String,
    pub days: u8,
    pub requests: Option<Series>,
    pub requests_failure: Option<ManagementFailure>,
    /// In cents.
    pub cost: Option<Series>,
    pub cost_failure: Option<ManagementFailure>,
    pub tokens: Option<Series>,
    pub tokens_failure: Option<ManagementFailure>,
    /// Milliseconds.
    pub latency: Option<Series>,
    pub latency_failure: Option<ManagementFailure>,
    pub models: Option<ModelGroups>,
    pub models_failure: Option<ManagementFailure>,
}

/// One request log row (allow-listed).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LogRow {
    pub id: String,
    pub created_at: String,
    pub model: String,
    pub provider: Option<String>,
    pub status: u16,
    pub units: Option<Measure>,
    /// In cents.
    pub cost: Option<Measure>,
    pub trace_id: Option<String>,
    pub cache_status: Option<String>,
    pub api_key_id: Option<String>,
}

/// One page of the chosen workspace's request logs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayLogs {
    pub workspace: String,
    pub days: u8,
    pub page: u32,
    pub trace: Option<String>,
    pub status: Option<u16>,
    pub rows: Vec<LogRow>,
    /// The service's total for the filters, when reported.
    pub total: Option<u32>,
    /// More pages follow (by the total, or a full page without one) and
    /// may be read.
    pub more: bool,
    /// More rows follow (or may follow) but this is the last page read
    /// (the 1,000-row cap): the list is not complete.
    pub capped: bool,
    pub partial: bool,
}

/// One limit policy (allow-listed; its conditions are not shown).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct LimitPolicy {
    pub id: String,
    pub name: Option<String>,
    pub status: String,
    /// What it limits (the service's type: cost, tokens, requests).
    pub limit_type: Option<String>,
    /// What it is applied by (the service's group-by keys).
    pub group_by: Vec<String>,
    /// Usage limits: the credit limit (cents), alert threshold (percent)
    /// and reset period.
    pub credit_limit: Option<Measure>,
    pub alert_threshold: Option<Measure>,
    pub periodic_reset: Option<String>,
    pub next_reset_at: Option<String>,
    /// Rate limits: requests or tokens per unit.
    pub value: Option<Measure>,
    pub unit: Option<String>,
    pub target: Option<String>,
}

/// One policy list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PolicyList {
    pub policies: Vec<LimitPolicy>,
    pub total: Option<u32>,
    /// The total, when reported, is no more than the policies received.
    pub complete: bool,
}

/// The policies that apply to the chosen workspace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayLimits {
    pub workspace: String,
    pub usage: Option<PolicyList>,
    pub usage_failure: Option<ManagementFailure>,
    pub rate: Option<PolicyList>,
    pub rate_failure: Option<ManagementFailure>,
}

/// A Gateway observation read the presentation asks for (scoped).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ObserveRead {
    Usage {
        days: u8,
    },
    Logs {
        days: u8,
        page: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        trace: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        status: Option<u16>,
    },
    Limits,
}

impl ObserveRead {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Usage { days } => WINDOWS.contains(days),
            Self::Logs {
                days,
                page,
                trace,
                status,
            } => {
                WINDOWS.contains(days)
                    && *page < LOG_PAGES
                    && trace.as_deref().is_none_or(trace_is_valid)
                    && status.is_none_or(|s| (100..=599).contains(&s))
            }
            Self::Limits => true,
        }
    }
}

/// A trace ID the logs may be filtered by (v1: 1-128 of `[a-z0-9_.:-]`).
pub fn trace_is_valid(trace: &str) -> bool {
    (1..=128).contains(&trace.len())
        && trace
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}

fn text(value: Option<&Value>, max: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control))
        .map(str::to_owned)
}

/// A reported total: the number; zero when an empty, complete reply
/// reports none; otherwise unknown. Any other value is malformed.
fn reported(value: Option<&Value>, rows: usize, partial: bool) -> Result<Option<Measure>, ()> {
    match value {
        None | Some(Value::Null) => Ok((rows == 0 && !partial).then_some(Measure(0.0))),
        Some(v) => Measure::of(v).map(Some).ok_or(()),
    }
}

/// A reply that says it did not succeed is a failure, whatever it carries.
fn failed(reply: &Value) -> bool {
    reply.get("success") == Some(&Value::Bool(false))
        || reply.get("error").is_some_and(|e| !e.is_null())
        || reply.get("errors").is_some_and(|e| !e.is_null())
}

/// One chart reply (`{success, data: {records, total, isQuotaExceeded}}`).
pub fn series(reply: &Value, latency: bool) -> Option<Series> {
    if failed(reply) {
        return None;
    }
    let data = match reply.get("data") {
        None | Some(Value::Null) => {
            return Some(Series {
                points: Vec::new(),
                total: (!latency).then_some(Measure(0.0)),
                errors: None,
                partial: false,
            });
        }
        Some(Value::Object(data)) => data,
        Some(_) => return None,
    };
    let partial = match data.get("isQuotaExceeded") {
        Some(Value::Bool(b)) => *b,
        None => false,
        Some(_) => return None,
    };
    let records = match data.get("records") {
        Some(Value::Array(records)) => records,
        None | Some(Value::Null) => &Vec::new(),
        Some(_) => return None,
    };
    if records.len() > MAX_POINTS {
        return None;
    }
    let points = records
        .iter()
        .map(|r| {
            Some(Point {
                x: text(r.get("x"), 64)?,
                y: Measure::of(r.get("y")?)?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let total = if partial {
        None
    } else if latency {
        match data.get("total") {
            None | Some(Value::Null) => None,
            Some(v) => Some(Measure::of(v)?),
        }
    } else {
        reported(data.get("total"), points.len(), partial).ok()?
    };
    let errors = match data.get("total_llm_error_count") {
        None | Some(Value::Null) => None,
        Some(v) => Some(Measure::of(v)?),
    };
    Some(Series {
        points,
        total,
        errors: errors.filter(|_| !partial),
        partial,
    })
}

/// One `logs/groups/model` reply: the busiest models first.
pub fn model_groups(reply: &Value) -> Option<ModelGroups> {
    if failed(reply) {
        return None;
    }
    let partial = match reply.get("is_quota_exceeded") {
        Some(Value::Bool(b)) => *b,
        None => false,
        Some(_) => return None,
    };
    let rows = match reply.get("data") {
        Some(Value::Array(rows)) => rows,
        None | Some(Value::Null) => &Vec::new(),
        Some(_) => return None,
    };
    if rows.len() > MAX_GROUP_ROWS {
        return None;
    }
    let optional = |row: &Value, key: &str| match row.get(key) {
        None | Some(Value::Null) => Some(None),
        Some(v) => Measure::of(v).map(Some),
    };
    let mut models = rows
        .iter()
        .map(|row| {
            let model = match row.get("model")? {
                Value::String(s) => text(Some(&Value::String(s.clone())), 128)?,
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            Some(ModelUsage {
                model,
                provider: text(row.get("ai_org").or_else(|| row.get("provider")), 64),
                requests: Measure::of(row.get("requests")?)?,
                cost: optional(row, "cost")?,
                total_tokens: optional(row, "total_tokens")?,
                success_rate: optional(row, "success_rate")?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    models.sort_by(|a, b| b.requests.0.total_cmp(&a.requests.0));
    let complete = models.len() <= MAX_GROUPS;
    let total = reported(reply.get("total"), models.len(), partial).ok()?;
    models.truncate(MAX_GROUPS);
    Some(ModelGroups {
        rows: models,
        total: total.filter(|_| !partial),
        complete,
        partial,
    })
}

/// One logs page. Every row must belong to the chosen workspace (and, when
/// filtered, carry the trace asked for); IDs are unique.
pub fn logs_page(
    reply: &Value,
    slug: &str,
    days: u8,
    page: u32,
    trace: Option<&str>,
    status: Option<u16>,
) -> Option<GatewayLogs> {
    if failed(reply) {
        return None;
    }
    let empty = Map::new();
    let data = match reply.get("data") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(data)) => data,
        Some(_) => return None,
    };
    let partial = match data.get("isQuotaExceeded") {
        Some(Value::Bool(b)) => *b,
        None => false,
        Some(_) => return None,
    };
    let records = match data.get("records") {
        Some(Value::Array(records)) => records,
        None | Some(Value::Null) => &Vec::new(),
        Some(_) => return None,
    };
    if records.len() > LOG_PAGE as usize {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let rows = records
        .iter()
        .map(|r| {
            if r.get("workspace_slug").and_then(Value::as_str) != Some(slug) {
                return None;
            }
            let trace_id = text(r.get("trace_id"), 4096).filter(|t| trace_is_valid(t));
            if trace.is_some() && trace_id.as_deref() != trace {
                return None;
            }
            let code = r
                .get("response_status_code")
                .and_then(Value::as_u64)
                .filter(|c| (100..=599).contains(c))
                .and_then(|c| u16::try_from(c).ok())?;
            if status.is_some_and(|s| s != code) {
                return None;
            }
            let id = text(r.get("id"), 128)?;
            if !seen.insert(id.clone()) {
                return None;
            }
            let measure = |key: &str| r.get(key).and_then(Measure::of);
            Some(LogRow {
                id,
                created_at: text(r.get("created_at"), 64)?,
                model: text(r.get("ai_model"), 128).unwrap_or_default(),
                provider: text(r.get("ai_org"), 64),
                status: code,
                units: measure("total_units"),
                cost: measure("cost"),
                trace_id,
                cache_status: text(r.get("cache_status"), 32),
                api_key_id: text(r.get("api_key_id"), 128),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let total = match data.get("total") {
        None | Some(Value::Null) => None,
        Some(v) => Some(u32::try_from(v.as_u64()?).ok()?),
    };
    let received = u64::from(page) * LOG_PAGE + rows.len() as u64;
    let following = match total.filter(|_| !partial) {
        Some(total) => u64::from(total) > received,
        None => rows.len() as u64 == LOG_PAGE,
    };
    let more = following && page + 1 < LOG_PAGES;
    Some(GatewayLogs {
        workspace: slug.to_owned(),
        days,
        page,
        trace: trace.map(str::to_owned),
        status,
        rows,
        total: total.filter(|_| !partial),
        more,
        capped: following && !more,
        partial,
    })
}

/// One policy list (`{object, data, total}`); every policy must name the
/// chosen workspace.
pub fn policies(reply: &Value, workspace: &str, usage: bool) -> Option<PolicyList> {
    let rows = match reply.get("data") {
        Some(Value::Array(rows)) => rows,
        None | Some(Value::Null) => &Vec::new(),
        Some(_) => return None,
    };
    if rows.len() > MAX_POLICIES {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let policies = rows
        .iter()
        .map(|p| {
            if p.get("workspace_id").and_then(Value::as_str) != Some(workspace) {
                return None;
            }
            let id = text(p.get("id"), 128)?;
            if !seen.insert(id.clone()) {
                return None;
            }
            let measure = |key: &str| match p.get(key) {
                None | Some(Value::Null) => Some(None),
                Some(v) => Measure::of(v).map(Some),
            };
            let group_by = match p.get("group_by") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Array(keys)) if keys.len() <= 16 => keys
                    .iter()
                    .map(|k| text(k.get("key").or(Some(k)), 64))
                    .collect::<Option<Vec<_>>>()?,
                Some(_) => return None,
            };
            Some(LimitPolicy {
                id,
                name: text(p.get("name"), MAX_TEXT),
                status: text(p.get("status"), 32)?,
                limit_type: text(p.get("type"), 32),
                group_by,
                credit_limit: if usage {
                    measure("credit_limit")?
                } else {
                    None
                },
                alert_threshold: if usage {
                    measure("alert_threshold")?
                } else {
                    None
                },
                periodic_reset: usage.then(|| text(p.get("periodic_reset"), 32)).flatten(),
                next_reset_at: usage
                    .then(|| text(p.get("next_usage_reset_at"), 64))
                    .flatten(),
                value: if usage { None } else { measure("value")? },
                unit: (!usage).then(|| text(p.get("unit"), 32)).flatten(),
                target: (!usage).then(|| text(p.get("target"), 64)).flatten(),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let total = match reply.get("total") {
        None | Some(Value::Null) => None,
        Some(v) => Some(u32::try_from(v.as_u64()?).ok()?),
    };
    if total.is_some_and(|t| (t as usize) < policies.len()) {
        return None;
    }
    Some(PolicyList {
        complete: total.is_none_or(|t| t as usize == policies.len()),
        policies,
        total,
    })
}

fn json<T: Serialize>(reply: &T) -> Result<Value, PageError> {
    serde_json::to_value(reply).map_err(|_| PageError::Invalid)
}

fn failure_of(error: &PageError) -> ManagementFailure {
    match error {
        PageError::Invalid => ManagementFailure::InvalidResponse,
        PageError::Sdk(e) => ManagementFailure::from_sdk(e),
    }
}

fn window(scope: &WorkspaceScope, days: u8) -> AIGatewayWindowOptions {
    let mut window = AIGatewayWindowOptions::new(scope.slug.clone());
    window.days = Some(f64::from(days));
    window
}

fn chart(scope: &WorkspaceScope, days: u8) -> Result<AIGatewayChartOptions, PageError> {
    AIGatewayChartOptions::new(window(scope, days)).map_err(PageError::from)
}

fn split<T>(read: Result<T, PageError>) -> (Option<T>, Option<ManagementFailure>) {
    match read {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(failure_of(&e))),
    }
}

async fn usage(
    gw: &AIGatewayClient,
    scope: &WorkspaceScope,
    days: u8,
    options: Option<&RequestOptions>,
) -> Result<GatewayUsage, PageError> {
    let telemetry = gw.telemetry();
    let params = chart(scope, days)?;
    let parsed = |reply: Result<Value, PageError>, latency: bool| {
        reply.and_then(|v| series(&v, latency).ok_or(PageError::Invalid))
    };
    let requests = parsed(
        match telemetry.requests(&params, options).await {
            Ok(r) => json(&r),
            Err(e) => Err(e.into()),
        },
        false,
    );
    let cost = parsed(
        match telemetry.cost(&params, options).await {
            Ok(r) => json(&r),
            Err(e) => Err(e.into()),
        },
        false,
    );
    let tokens = parsed(
        match telemetry.tokens(&params, options).await {
            Ok(r) => json(&r),
            Err(e) => Err(e.into()),
        },
        false,
    );
    let latency = parsed(
        match telemetry.latency(&params, options).await {
            Ok(r) => json(&r),
            Err(e) => Err(e.into()),
        },
        true,
    );
    let mut groups = AIGatewayGroupOptions::new(params.clone());
    groups.columns = Some(vec![
        AIGatewayGroupColumn::Cost,
        AIGatewayGroupColumn::TotalTokens,
        AIGatewayGroupColumn::SuccessRate,
    ]);
    let models = match telemetry
        .group_by(AIGatewayGroupDimension::Model, &groups, options)
        .await
    {
        Ok(r) => json(&r).and_then(|v| model_groups(&v).ok_or(PageError::Invalid)),
        Err(e) => Err(e.into()),
    };
    if [&requests, &cost, &tokens, &latency]
        .iter()
        .all(|r| r.is_err())
        && models.is_err()
    {
        return Err(requests.err().unwrap_or(PageError::Invalid));
    }
    let (requests, requests_failure) = split(requests);
    let (cost, cost_failure) = split(cost);
    let (tokens, tokens_failure) = split(tokens);
    let (latency, latency_failure) = split(latency);
    let (models, models_failure) = split(models);
    Ok(GatewayUsage {
        workspace: scope.id.clone(),
        days,
        requests,
        requests_failure,
        cost,
        cost_failure,
        tokens,
        tokens_failure,
        latency,
        latency_failure,
        models,
        models_failure,
    })
}

async fn logs(
    gw: &AIGatewayClient,
    scope: &WorkspaceScope,
    read: (u8, u32, Option<&str>, Option<u16>),
    options: Option<&RequestOptions>,
) -> Result<GatewayLogs, PageError> {
    let (days, page, trace, status) = read;
    let mut params = AIGatewayLogsOptions::new(window(scope, days));
    params.page_size = Some(LOG_PAGE);
    params.current_page = Some(u64::from(page));
    params.trace_id = trace.map(str::to_owned);
    params.status_code = status.map(u64::from);
    let reply = json(&gw.telemetry().logs(&params, options).await?)?;
    logs_page(&reply, &scope.slug, days, page, trace, status).ok_or(PageError::Invalid)
}

async fn limits(
    gw: &AIGatewayClient,
    scope: &WorkspaceScope,
    options: Option<&RequestOptions>,
) -> Result<GatewayLimits, PageError> {
    let size = || Optional::Value(scm_sdk::data::FiniteNumber::from(MAX_POLICIES as u32));
    let usage = match GatewayUsageLimitsClientListOptions::builder()
        .workspace_id(Optional::Value(scope.id.clone()))
        .page_size(size())
        .build()
    {
        Ok(query) => match gw.usage_limits().list(&query, options).await {
            Ok(r) => json(&r).and_then(|v| policies(&v, &scope.id, true).ok_or(PageError::Invalid)),
            Err(e) => Err(e.into()),
        },
        Err(e) => Err(e.into()),
    };
    let rate = match GatewayRateLimitsClientListOptions::builder()
        .workspace_id(Optional::Value(scope.id.clone()))
        .page_size(size())
        .build()
    {
        Ok(query) => match gw.rate_limits().list(&query, options).await {
            Ok(r) => {
                json(&r).and_then(|v| policies(&v, &scope.id, false).ok_or(PageError::Invalid))
            }
            Err(e) => Err(e.into()),
        },
        Err(e) => Err(e.into()),
    };
    if usage.is_err() && rate.is_err() {
        return Err(usage.err().unwrap_or(PageError::Invalid));
    }
    let (usage, usage_failure) = split(usage);
    let (rate, rate_failure) = split(rate);
    Ok(GatewayLimits {
        workspace: scope.id.clone(),
        usage,
        usage_failure,
        rate,
        rate_failure,
    })
}

/// The chosen workspace's last day for Home and the Inbox (C18): its
/// requests, errors and cost, and how many limit policies apply. Each
/// source keeps its data or failure.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct GatewayOverview {
    pub workspace: String,
    pub workspace_name: String,
    pub requests: Option<Measure>,
    pub errors: Option<Measure>,
    /// In cents.
    pub cost: Option<Measure>,
    /// The service said the day is over its quota.
    pub partial: bool,
    pub usage_failure: Option<ManagementFailure>,
    /// Usage and rate-limit policies that apply, when both lists were read.
    pub policies: Option<u32>,
    pub limits_failure: Option<ManagementFailure>,
}

impl GatewayOverview {
    /// Whether the day's requests and errors are both known: read, whole
    /// and reported (an absent total or error count is unknown, not zero).
    pub fn activity_known(&self) -> bool {
        self.usage_failure.is_none()
            && !self.partial
            && self.requests.is_some()
            && self.errors.is_some()
    }
}

/// Reads the chosen workspace's last day for the overview.
pub(crate) async fn overview(
    gw: &AIGatewayClient,
    scope: &WorkspaceScope,
    options: Option<&RequestOptions>,
) -> GatewayOverview {
    let telemetry = gw.telemetry();
    let (requests, cost) = match chart(scope, 1) {
        Ok(params) => {
            let requests = match telemetry.requests(&params, options).await {
                Ok(r) => json(&r).and_then(|v| series(&v, false).ok_or(PageError::Invalid)),
                Err(e) => Err(e.into()),
            };
            let cost = match telemetry.cost(&params, options).await {
                Ok(r) => json(&r).and_then(|v| series(&v, false).ok_or(PageError::Invalid)),
                Err(e) => Err(e.into()),
            };
            (requests, cost)
        }
        Err(e) => (Err(e), Err(PageError::Invalid)),
    };
    let usage_failure = requests
        .as_ref()
        .err()
        .or(cost.as_ref().err())
        .map(failure_of);
    let partial =
        requests.as_ref().is_ok_and(|s| s.partial) || cost.as_ref().is_ok_and(|s| s.partial);
    let limits = limits(gw, scope, options).await;
    let (policies, limits_failure) = match &limits {
        Ok(l) => match (&l.usage, &l.rate) {
            (Some(u), Some(r)) if u.complete && r.complete => (
                u32::try_from(u.policies.len() + r.policies.len()).ok(),
                None,
            ),
            _ => (None, l.usage_failure.or(l.rate_failure)),
        },
        Err(e) => (None, Some(failure_of(e))),
    };
    GatewayOverview {
        workspace: scope.id.clone(),
        workspace_name: scope.name.clone(),
        requests: requests.as_ref().ok().and_then(|s| s.total),
        errors: requests.as_ref().ok().and_then(|s| s.errors),
        cost: cost.as_ref().ok().and_then(|s| s.total),
        partial,
        usage_failure,
        policies,
        limits_failure,
    }
}

/// What an observation read produced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ObserveResult {
    Usage { usage: Box<GatewayUsage> },
    Logs { logs: Box<GatewayLogs> },
    Limits { limits: Box<GatewayLimits> },
}

/// Runs one admitted observation read of the chosen workspace.
pub(crate) async fn read(
    gw: &AIGatewayClient,
    read: &ObserveRead,
    scope: &WorkspaceScope,
    options: Option<&RequestOptions>,
) -> Result<ObserveResult, PageError> {
    if !read.is_valid() {
        return Err(PageError::Invalid);
    }
    match read {
        ObserveRead::Usage { days } => {
            usage(gw, scope, *days, options)
                .await
                .map(|usage| ObserveResult::Usage {
                    usage: Box::new(usage),
                })
        }
        ObserveRead::Logs {
            days,
            page,
            trace,
            status,
        } => logs(
            gw,
            scope,
            (*days, *page, trace.as_deref(), *status),
            options,
        )
        .await
        .map(|logs| ObserveResult::Logs {
            logs: Box::new(logs),
        }),
        ObserveRead::Limits => {
            limits(gw, scope, options)
                .await
                .map(|limits| ObserveResult::Limits {
                    limits: Box::new(limits),
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WS: &str = "16f7e90d-382a-4e78-b577-1b01eb5f8297";

    #[test]
    fn charts_report_only_what_the_service_did() {
        let reply = |data: Value| json!({"success": true, "data": data});
        let s = series(
            &reply(
                json!({"records": [{"x": "2026-10-20T00:00:00Z", "y": 4}], "total": 4,
                "total_llm_error_count": 1, "isQuotaExceeded": false}),
            ),
            false,
        )
        .expect("series");
        assert_eq!(s.total, Some(Measure(4.0)));
        assert_eq!(s.errors, Some(Measure(1.0)));
        // An empty window without totals is zero; with rows it is unknown.
        let empty = series(
            &reply(json!({"records": [], "total": null, "isQuotaExceeded": false})),
            false,
        )
        .expect("empty");
        assert_eq!(empty.total, Some(Measure(0.0)));
        let unknown = series(
            &reply(
                json!({"records": [{"x": "a", "y": 1}], "total": null, "isQuotaExceeded": false}),
            ),
            false,
        )
        .expect("unknown");
        assert_eq!(unknown.total, None);
        // Over quota: partial, and no total is claimed.
        let partial = series(
            &reply(json!({"records": [{"x": "a", "y": 1}], "total": 9, "isQuotaExceeded": true})),
            false,
        )
        .expect("partial");
        assert!(partial.partial && partial.total.is_none());
        // Latency without requests has no mean.
        assert_eq!(
            series(&reply(json!({"records": [], "total": null})), true)
                .expect("l")
                .total,
            None
        );
        // Malformed or failed replies are refused.
        for bad in [
            json!({"success": false, "data": {"records": []}}),
            reply(json!({"records": [{"x": "a", "y": -1}]})),
            reply(json!({"records": [{"x": "a"}]})),
            reply(json!({"records": [], "total": "4"})),
            reply(json!({"records": "x"})),
            reply(json!({"records": [], "isQuotaExceeded": "no"})),
        ] {
            assert!(series(&bad, false).is_none(), "{bad}");
        }
        let many: Vec<Value> = (0..=MAX_POINTS)
            .map(|i| json!({"x": i.to_string(), "y": 1}))
            .collect();
        assert!(series(&reply(json!({"records": many})), false).is_none());
    }

    #[test]
    fn model_groups_rank_by_requests_and_never_invent_totals() {
        let reply = json!({"object": "list", "is_quota_exceeded": false, "total": 3, "data": [
            {"object": "group", "model": "gpt-4o", "ai_org": "openai", "requests": 2, "cost": 30},
            {"object": "group", "model": "claude", "provider": "anthropic", "requests": 9},
            {"object": "group", "model": "mini", "requests": 5, "success_rate": 0.98},
        ]});
        let groups = model_groups(&reply).expect("groups");
        let order: Vec<_> = groups.rows.iter().map(|r| r.model.as_str()).collect();
        assert_eq!(order, ["claude", "mini", "gpt-4o"]);
        assert_eq!(groups.rows[2].provider.as_deref(), Some("openai"));
        assert_eq!(groups.rows[0].cost, None);
        assert!(groups.complete && groups.total == Some(Measure(3.0)));
        assert!(model_groups(&json!({"data": [{"model": "x"}]})).is_none());
        assert!(model_groups(&json!({"data": [{"model": {"a": 1}, "requests": 1}]})).is_none());
    }

    #[test]
    fn logs_belong_to_the_chosen_workspace_and_carry_no_metadata() {
        let row = |id: &str, slug: &str| {
            json!({"id": id, "workspace_slug": slug, "ai_model": "gpt-4o", "ai_org": "openai",
                "response_status_code": 200, "total_units": 12, "cost": 0.2, "trace_id": "t-1",
                "created_at": "2026-10-20T00:00:00Z", "_user": "person@example.com",
                "metadata_key": ["k"], "metadata_value": ["CANARY-meta"],
                "request_url": "https://x/v1/chat?secret=CANARY-url"})
        };
        let reply = |rows: Vec<Value>, total: Value| {
            json!({"success": true, "data": {"records": rows, "total": total, "capturedTotal": 0,
                "isQuotaExceeded": false}})
        };
        let page = logs_page(
            &reply(vec![row("a", "team-a"), row("b", "team-a")], json!(30)),
            "team-a",
            7,
            0,
            None,
            None,
        )
        .expect("page");
        assert_eq!(page.rows.len(), 2);
        assert!(page.more, "30 rows reported, 2 received");
        let shown = serde_json::to_string(&page).expect("json");
        assert!(!shown.contains("CANARY") && !shown.contains("person@example.com"));
        // Another workspace's row, a repeated ID, or a row without the
        // filtered trace or status makes the page invalid.
        assert!(
            logs_page(
                &reply(vec![row("a", "team-b")], json!(1)),
                "team-a",
                7,
                0,
                None,
                None
            )
            .is_none()
        );
        assert!(
            logs_page(
                &reply(vec![row("a", "team-a"), row("a", "team-a")], json!(2)),
                "team-a",
                7,
                0,
                None,
                None
            )
            .is_none()
        );
        assert!(
            logs_page(
                &reply(vec![row("a", "team-a")], json!(1)),
                "team-a",
                7,
                0,
                Some("t-2"),
                None
            )
            .is_none()
        );
        assert!(
            logs_page(
                &reply(vec![row("a", "team-a")], json!(1)),
                "team-a",
                7,
                0,
                None,
                Some(500)
            )
            .is_none()
        );
        let last = logs_page(
            &reply(vec![row("a", "team-a")], json!(1)),
            "team-a",
            7,
            0,
            None,
            None,
        )
        .expect("last");
        assert!(!last.more && !last.capped);
        // The last page read with more reported is capped, never complete.
        let full: Vec<Value> = (0..LOG_PAGE)
            .map(|i| row(&format!("r{i}"), "team-a"))
            .collect();
        let capped = logs_page(
            &reply(full.clone(), json!(5000)),
            "team-a",
            7,
            LOG_PAGES - 1,
            None,
            None,
        )
        .expect("capped");
        assert!(!capped.more && capped.capped);
        let unknown = logs_page(
            &reply(full, Value::Null),
            "team-a",
            7,
            LOG_PAGES - 1,
            None,
            None,
        )
        .expect("no total");
        assert!(
            !unknown.more && unknown.capped,
            "a full last page may have more"
        );
    }

    #[test]
    fn policies_name_the_chosen_workspace_and_hide_their_conditions() {
        let reply = json!({"object": "list", "total": 1, "data": [{
            "id": "ul-1", "name": "Monthly", "status": "active", "workspace_id": WS,
            "organisation_id": "1234567890", "credit_limit": 5000, "alert_threshold": 80,
            "periodic_reset": "monthly", "next_usage_reset_at": "2026-11-01T00:00:00Z",
            "conditions": [{"key": "api_key", "value": "CANARY-condition"}],
            "group_by": [{"key": "api_key"}], "created_at": "x", "last_updated_at": "y"}]});
        let list = policies(&reply, WS, true).expect("policies");
        assert!(list.complete);
        assert_eq!(list.policies[0].credit_limit, Some(Measure(5000.0)));
        assert_eq!(list.policies[0].group_by, ["api_key"]);
        assert!(
            !serde_json::to_string(&list)
                .expect("json")
                .contains("CANARY")
        );
        let foreign = json!({"data": [{"id": "x", "status": "active", "workspace_id": "other"}]});
        assert!(policies(&foreign, WS, true).is_none());
        let short =
            json!({"total": 0, "data": [{"id": "x", "status": "active", "workspace_id": WS}]});
        assert!(
            policies(&short, WS, false).is_none(),
            "a total below the rows"
        );
    }

    #[test]
    fn reads_are_bounded() {
        assert!(ObserveRead::Usage { days: 7 }.is_valid());
        assert!(!ObserveRead::Usage { days: 2 }.is_valid());
        assert!(
            !ObserveRead::Logs {
                days: 7,
                page: LOG_PAGES,
                trace: None,
                status: None
            }
            .is_valid()
        );
        assert!(
            !ObserveRead::Logs {
                days: 7,
                page: 0,
                trace: Some("a b".into()),
                status: None
            }
            .is_valid()
        );
        assert!(
            !ObserveRead::Logs {
                days: 7,
                page: 0,
                trace: None,
                status: Some(42)
            }
            .is_valid()
        );
        assert!(
            ObserveRead::Logs {
                days: 1,
                page: 3,
                trace: Some("t-1".into()),
                status: Some(446)
            }
            .is_valid()
        );
    }
}
