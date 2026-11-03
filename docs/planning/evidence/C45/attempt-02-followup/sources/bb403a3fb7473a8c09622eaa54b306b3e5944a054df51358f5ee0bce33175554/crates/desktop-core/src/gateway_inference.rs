//! Gateway inference from the Playground (C18, D044): one streamed chat
//! completion or Responses run through the SDK's `RuntimeClient`,
//! authenticated only by the
//! workspace inference key the operator stored for the chosen workspace and
//! sent only to the runtime endpoint stored with it. The management client
//! is never offered for inference, a run is never retried or replayed, and
//! nothing falls back to a direct provider when a route fails. Output is
//! bounded (v1: 32 KiB of prompt, 64 KiB of output, 256 tokens) and every
//! chunk is checked against the SDK's chunk contract; the first malformed
//! chunk ends the run. Stopping a run ends the stream on this Mac; it does
//! not establish that nothing was charged.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

use scm_sdk::airs::gateway::RuntimeClient;
use scm_sdk::airs::gateway::models::{
    GatewayChatCompletionRequest, GatewayInferenceInputCreateResponse,
};
use scm_sdk::auth::Authentication;
use scm_sdk::transport::Transport;
use scm_sdk::{
    Client, Endpoint, ErrorKind, GatewayRouting, Limits, OriginPolicy, RequestOptions, Secret,
    Session,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::runtime::Outcome;

/// Largest system text plus prompt (v1).
pub const MAX_INPUT_BYTES: usize = 32 * 1024;
/// Output kept from one run (v1); the stream is closed when it is reached.
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;
/// Output tokens asked for (v1).
pub const MAX_TOKENS: u32 = 256;
/// Largest single stream event.
pub const MAX_EVENT_BYTES: usize = 64 * 1024;
/// Longest a run may take, from admission.
pub const RUN_DEADLINE: Duration = Duration::from_secs(120);
/// Longest the stream may be silent.
const IDLE: Duration = Duration::from_secs(30);
const MAX_ENDPOINT: usize = 2048;
const MAX_MODEL: usize = 256;

/// Which inference API a run uses (both streamed, D045).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InferenceApi {
    /// `/chat/completions` (v1's only API): ends at `[DONE]`.
    #[default]
    Chat,
    /// `/responses`: ends at its completed, incomplete or failed event.
    /// Sent with `store: false`, so the Gateway keeps no response to
    /// continue from.
    Responses,
}

/// What the operator asks one run to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceRun {
    #[serde(default)]
    pub api: InferenceApi,
    /// The model as the Gateway names it (for example `@openai-prod/gpt-4o`).
    pub model: String,
    /// The routing configuration to apply (its ID); none sends the model
    /// as named, without a route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub route: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub system: Option<String>,
    pub prompt: String,
    /// A file of the run's workspace to attach (Responses only, D045): it
    /// must be in the files last read for that workspace with this key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file: Option<String>,
}

fn plain(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t' && c != '\r')
}

impl InferenceRun {
    pub fn is_valid(&self) -> bool {
        let input = self.prompt.len() + self.system.as_deref().map_or(0, str::len);
        (1..=MAX_MODEL).contains(&self.model.len())
            && !self.model.chars().any(char::is_control)
            && self.model.trim() == self.model
            && self.route.as_deref().is_none_or(crate::gateway::is_id)
            && !self.prompt.trim().is_empty()
            && input <= MAX_INPUT_BYTES
            && plain(&self.prompt)
            && self.system.as_deref().is_none_or(plain)
            && self
                .file
                .as_deref()
                .is_none_or(|f| crate::gateway::is_id(f) && self.api == InferenceApi::Responses)
    }

    /// The streamed request: chat is v1's shape (system then user
    /// message); Responses carries the system text as its instructions,
    /// and an attached file as an `input_file` beside the prompt. Either
    /// asks for at most [`MAX_TOKENS`]. Chat sends the cap as
    /// `max_completion_tokens`: current OpenAI models refuse `max_tokens`
    /// (found by the C25 live run, HTTP 400 `unsupported_parameter`).
    fn request(&self) -> Option<Request> {
        let system = self.system.as_deref().filter(|s| !s.trim().is_empty());
        match self.api {
            InferenceApi::Chat => {
                let mut messages = Vec::new();
                if let Some(system) = system {
                    messages.push(json!({"role": "system", "content": system}));
                }
                messages.push(json!({"role": "user", "content": self.prompt}));
                serde_json::from_value(json!({
                    "model": self.model,
                    "messages": messages,
                    "stream": true,
                    "max_completion_tokens": MAX_TOKENS,
                }))
                .ok()
                .map(Request::Chat)
            }
            InferenceApi::Responses => {
                let input = match &self.file {
                    None => json!(self.prompt),
                    Some(file) => json!([{"role": "user", "content": [
                        {"type": "input_text", "text": self.prompt},
                        {"type": "input_file", "file_id": file},
                    ]}]),
                };
                let mut body = json!({
                    "model": self.model,
                    "input": input,
                    "stream": true,
                    "store": false,
                    "max_output_tokens": MAX_TOKENS,
                });
                if let Some(system) = system {
                    body["instructions"] = json!(system);
                }
                serde_json::from_value(body).ok().map(Request::Responses)
            }
        }
    }
}

enum Request {
    Chat(GatewayChatCompletionRequest),
    Responses(GatewayInferenceInputCreateResponse),
}

/// A runtime endpoint the inference key may be sent to: an `https` URL
/// with a host and no credentials, query or fragment (v1).
pub fn endpoint_is_valid(text: &str) -> bool {
    if text.len() > MAX_ENDPOINT || text.chars().any(char::is_control) {
        return false;
    }
    let Ok(url) = url::Url::parse(text) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str().is_some_and(|h| !h.is_empty())
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && Endpoint::parse(text).is_ok()
}

/// Where a stored inference key may be used: the workspace it belongs to
/// and the runtime endpoint it may be sent to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InferenceTarget {
    pub workspace: crate::gateway::ListedWorkspace,
    pub endpoint: String,
}

impl InferenceTarget {
    pub fn is_valid(&self) -> bool {
        endpoint_is_valid(&self.endpoint)
            && crate::gateway::is_id(&self.workspace.id)
            && self.workspace.name.len() <= 256
            && self.workspace.slug.len() <= 256
            && self.workspace.scope_name.len() <= 256
    }
}

/// Why a run did not complete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum InferenceFailure {
    /// A guardrail blocked the request or its response (HTTP 446).
    Blocked,
    /// The inference key was refused (401).
    Unauthorized,
    /// The key may not call this model or route (403).
    Denied,
    /// No such model, route or endpoint (404).
    NotFound,
    /// The request was refused otherwise (another 4xx).
    Rejected,
    RateLimited,
    /// The Gateway or its provider failed (5xx).
    Unavailable,
    Timeout,
    Network,
    /// A redirect or an origin other than the stored endpoint.
    Destination,
    /// A chunk did not match the contract, or a stream event was too large.
    InvalidResponse,
    /// The model failed while producing a Responses run (its failed or
    /// error event).
    ModelFailed,
    CredentialLocked,
    CredentialDenied,
    CredentialMissing,
    Configuration,
}

impl InferenceFailure {
    pub(crate) fn from_sdk(error: &scm_sdk::Error) -> Self {
        match (error.kind(), error.status()) {
            (_, Some(446)) => Self::Blocked,
            (ErrorKind::Authentication, _) | (ErrorKind::Http, Some(401)) => Self::Unauthorized,
            (ErrorKind::Http, Some(403)) => Self::Denied,
            (ErrorKind::Http, Some(404)) => Self::NotFound,
            (ErrorKind::Http, Some(429)) => Self::RateLimited,
            (ErrorKind::Http, Some(s)) if s >= 500 => Self::Unavailable,
            (ErrorKind::Http, _) => Self::Rejected,
            (ErrorKind::Deadline, _) => Self::Timeout,
            (ErrorKind::Transport | ErrorKind::Io, _) => Self::Network,
            (ErrorKind::Destination, _) => Self::Destination,
            (ErrorKind::Configuration, _) => Self::Configuration,
            _ => Self::InvalidResponse,
        }
    }
}

/// Token counts the stream reported (none is unknown, never zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt: Option<u32>,
    pub completion: Option<u32>,
    pub total: Option<u32>,
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum RunEnd {
    /// The stream finished (`[DONE]`).
    Completed,
    /// The Gateway ended the answer before it was complete: a Responses
    /// `response.incomplete` event, or a chat finish of `length` or
    /// `content_filter`. Its reason is the run's `finish`.
    Incomplete,
    /// The output reached [`MAX_OUTPUT_BYTES`] and the stream was closed.
    Truncated,
    /// The operator stopped it, or the workspace, tenant or key changed.
    Stopped,
    Failed,
}

/// What one run has produced so far (shared with its job).
#[derive(Clone, Debug, Default)]
pub struct RunProgress {
    pub output: String,
    /// The request was handed to the transport (it may have been charged).
    pub sent: bool,
    /// The Gateway answered (response headers or the first chunk).
    pub answered: bool,
    pub model: Option<String>,
    pub response_id: Option<String>,
    pub finish: Option<String>,
    /// The Gateway reported the answer cut short (see [`RunEnd::Incomplete`]).
    pub cut: bool,
    pub usage: Option<TokenUsage>,
    pub end: Option<RunEnd>,
    pub failure: Option<InferenceFailure>,
    /// What the SDK established when the stream could not be opened.
    pub opening: Option<scm_sdk::RemoteOutcome>,
    pub chunks: u32,
}

pub type ProgressSlot = Arc<Mutex<RunProgress>>;

/// Everything a run needs, resolved at admission (the key is read in the
/// job, off the lock).
pub struct RunInputs {
    pub endpoint: String,
    pub trace: String,
    pub run: InferenceRun,
    pub key: Box<dyn FnOnce() -> Result<Zeroizing<String>, InferenceFailure> + Send>,
    pub transport: Arc<dyn Transport>,
    pub progress: ProgressSlot,
    /// Woken after each batch of output so a snapshot is published.
    pub pulse: Arc<Notify>,
}

fn stream_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_retries = 0;
    limits.operation_timeout = RUN_DEADLINE;
    limits.stream_handshake_timeout = IDLE;
    limits.stream_idle_timeout = IDLE;
    limits.stream_session_timeout = Some(RUN_DEADLINE);
    limits.max_event_bytes = MAX_EVENT_BYTES;
    limits
}

/// Where a runtime request is sent.
pub(crate) struct Target<'a> {
    pub(crate) endpoint: &'a str,
    pub(crate) trace: &'a str,
    pub(crate) route: Option<&'a str>,
    pub(crate) transport: Arc<dyn Transport>,
}

/// A runtime client for `target` with the inference key: only that
/// endpoint's origin, never retried, with the trace (and route) headers.
pub(crate) fn client(
    target: &Target<'_>,
    key: &str,
    session: Session,
    limits: Limits,
) -> Result<RuntimeClient, InferenceFailure> {
    let endpoint = Endpoint::parse(target.endpoint).map_err(|_| InferenceFailure::Configuration)?;
    let secret = Secret::new(key).map_err(|_| InferenceFailure::CredentialMissing)?;
    let auth =
        Authentication::runtime_key(secret).map_err(|_| InferenceFailure::CredentialMissing)?;
    let client = Client::builder(endpoint.clone())
        .authentication(auth)
        .transport(Arc::clone(&target.transport))
        .destination_policy(Arc::new(OriginPolicy::new([endpoint])))
        .limits(limits)
        .session(session)
        .build()
        .map_err(|_| InferenceFailure::Configuration)?;
    let mut routing = GatewayRouting::new()
        .header("x-portkey-trace-id", target.trace)
        .map_err(|_| InferenceFailure::Configuration)?;
    if let Some(route) = target.route {
        routing = routing
            .header("x-portkey-config", route)
            .map_err(|_| InferenceFailure::Configuration)?;
    }
    RuntimeClient::new(client)
        .and_then(|rt| rt.with_routing(routing))
        .map_err(|_| InferenceFailure::Configuration)
}

pub(crate) fn short(value: Option<&Value>, max: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|v| v.len() <= max && !v.chars().any(char::is_control))
        .map(str::to_owned)
}

/// Appends `text` up to the output bound (false once it is reached).
fn append(progress: &mut RunProgress, text: &str) -> bool {
    let room = MAX_OUTPUT_BYTES.saturating_sub(progress.output.len());
    if text.len() <= room {
        progress.output.push_str(text);
        return true;
    }
    let mut cut = room;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    progress.output.push_str(&text[..cut]);
    false
}

/// How one Responses event moved the run.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Going,
    /// The output reached its bound.
    Full,
    /// The model failed (its failed or error event).
    Failed,
}

/// Takes one Responses event: text deltas are appended; the response's
/// identity, model, usage and end come from its lifecycle events. Tool,
/// audio and search events are not asked for and carry no output here.
fn take_response(progress: &mut RunProgress, event: &Value) -> Step {
    progress.answered = true;
    progress.chunks = progress.chunks.saturating_add(1);
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(response) = event.get("response").filter(|r| r.is_object()) {
        if let Some(id) = short(response.get("id"), 128) {
            progress.response_id = Some(id);
        }
        if let Some(model) = short(response.get("model"), MAX_MODEL) {
            progress.model = Some(model);
        }
        if let Some(usage) = response.get("usage").filter(|u| u.is_object()) {
            let count = |k: &str| {
                usage
                    .get(k)
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
            };
            progress.usage = Some(TokenUsage {
                prompt: count("input_tokens"),
                completion: count("output_tokens"),
                total: count("total_tokens"),
            });
        }
    }
    match kind {
        "response.output_text.delta" | "response.refusal.delta" => {
            let text = event
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if append(progress, text) {
                Step::Going
            } else {
                Step::Full
            }
        }
        "response.completed" => {
            progress.finish = Some("completed".into());
            Step::Going
        }
        "response.incomplete" => {
            let reason = event
                .get("response")
                .and_then(|r| r.get("incomplete_details"))
                .and_then(|d| short(d.get("reason"), 32));
            progress.finish = Some(reason.unwrap_or_else(|| "incomplete".into()));
            progress.cut = true;
            Step::Going
        }
        "response.failed" | "error" => Step::Failed,
        _ => Step::Going,
    }
}

/// Takes one chunk: its text is appended up to the output bound (returns
/// false once the bound is reached).
fn take(progress: &mut RunProgress, chunk: &Value) -> bool {
    progress.answered = true;
    progress.chunks = progress.chunks.saturating_add(1);
    if progress.model.is_none() {
        progress.model = chunk
            .get("model")
            .and_then(Value::as_str)
            .filter(|m| m.len() <= MAX_MODEL)
            .map(str::to_owned);
    }
    if progress.response_id.is_none() {
        progress.response_id = chunk
            .get("id")
            .and_then(Value::as_str)
            .filter(|i| i.len() <= 128 && !i.chars().any(char::is_control))
            .map(str::to_owned);
    }
    if let Some(usage) = chunk.get("usage").filter(|u| u.is_object()) {
        let count = |k: &str| {
            usage
                .get(k)
                .and_then(Value::as_u64)
                .and_then(|v| u32::try_from(v).ok())
        };
        progress.usage = Some(TokenUsage {
            prompt: count("prompt_tokens"),
            completion: count("completion_tokens"),
            total: count("total_tokens"),
        });
    }
    let choice = chunk.get("choices").and_then(|c| c.get(0));
    if let Some(finish) = choice
        .and_then(|c| c.get("finish_reason"))
        .and_then(Value::as_str)
        .filter(|f| f.len() <= 32)
    {
        progress.finish = Some(finish.to_owned());
        progress.cut = matches!(finish, "length" | "content_filter");
    }
    let text = choice
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    append(progress, text)
}

/// One open stream of either API.
enum Opened {
    Chat(
        scm_sdk::streaming::ContractStream<
            scm_sdk::airs::gateway::models::GatewayChatCompletionChunk,
        >,
    ),
    Responses(
        scm_sdk::streaming::ContractStream<
            scm_sdk::airs::gateway::models::GatewayInferenceResponseStreamEvent,
        >,
    ),
}

/// What the next event did.
enum Next {
    Going,
    Full,
    Failed(InferenceFailure),
    Done,
}

impl Opened {
    fn close(&mut self) {
        match self {
            Self::Chat(s) => s.close(),
            Self::Responses(s) => s.close(),
        }
    }

    async fn next(&mut self, progress: &ProgressSlot) -> Next {
        let value = match self {
            Self::Chat(s) => s
                .next()
                .await
                .map(|c| c.map(|c| serde_json::to_value(&c).map_err(|_| ()))),
            Self::Responses(s) => s
                .next()
                .await
                .map(|e| e.map(|e| serde_json::to_value(&e).map_err(|_| ()))),
        };
        let value = match value {
            Ok(None) => return Next::Done,
            Ok(Some(Ok(value))) => value,
            Ok(Some(Err(()))) => return Next::Failed(InferenceFailure::InvalidResponse),
            Err(error) => return Next::Failed(InferenceFailure::from_sdk(&error)),
        };
        let Ok(mut p) = progress.lock() else {
            return Next::Failed(InferenceFailure::InvalidResponse);
        };
        match self {
            Self::Chat(_) => {
                if take(&mut p, &value) {
                    Next::Going
                } else {
                    Next::Full
                }
            }
            Self::Responses(_) => match take_response(&mut p, &value) {
                Step::Going => Next::Going,
                Step::Full => Next::Full,
                Step::Failed => Next::Failed(InferenceFailure::ModelFailed),
            },
        }
    }
}

fn settle(progress: &ProgressSlot, end: RunEnd, failure: Option<InferenceFailure>) -> Outcome {
    let Ok(mut p) = progress.lock() else {
        return Outcome::Unknown;
    };
    p.end = Some(end);
    p.failure = failure;
    match end {
        RunEnd::Completed | RunEnd::Incomplete | RunEnd::Truncated => Outcome::Completed,
        RunEnd::Stopped | RunEnd::Failed if !p.sent => Outcome::NotSent,
        RunEnd::Failed if p.answered => Outcome::Completed,
        // The SDK's own finding, never one derived from the failure's label
        // (a 5xx is not a refusal: the Gateway may have run it).
        RunEnd::Failed => match p.opening {
            Some(scm_sdk::RemoteOutcome::NotSent) => Outcome::NotSent,
            Some(scm_sdk::RemoteOutcome::Rejected) => Outcome::Rejected,
            _ => Outcome::Unknown,
        },
        RunEnd::Stopped => Outcome::Unknown,
    }
}

/// Runs one admitted inference: reads the key, opens the stream and takes
/// its chunks until it ends, the output bound is reached, the run is
/// stopped or its deadline passes. Never retried.
pub async fn run(inputs: RunInputs, cancel: CancellationToken, deadline: Instant) -> Outcome {
    let progress = Arc::clone(&inputs.progress);
    let pulse = Arc::clone(&inputs.pulse);
    let done = |end, failure| {
        let outcome = settle(&progress, end, failure);
        pulse.notify_one();
        outcome
    };
    let Some(request) = inputs.run.request() else {
        return done(RunEnd::Failed, Some(InferenceFailure::Configuration));
    };
    let RunInputs {
        endpoint,
        trace,
        run,
        key,
        transport,
        ..
    } = inputs;
    let key = match tokio::task::spawn_blocking(key).await {
        Ok(Ok(key)) => key,
        Ok(Err(failure)) => return done(RunEnd::Failed, Some(failure)),
        Err(_) => return done(RunEnd::Failed, Some(InferenceFailure::CredentialMissing)),
    };
    let session = Session::new();
    let target = Target {
        endpoint: &endpoint,
        trace: &trace,
        route: run.route.as_deref(),
        transport,
    };
    let runtime = match client(&target, &key, session.clone(), stream_limits()) {
        Ok(runtime) => runtime,
        Err(failure) => return done(RunEnd::Failed, Some(failure)),
    };
    drop(key);
    let remaining = deadline.saturating_duration_since(Instant::now());
    let Some(options) = RequestOptions::with_timeout(remaining.max(Duration::from_millis(1)))
        .and_then(|o| o.with_retries(0))
        .ok()
    else {
        return done(RunEnd::Failed, Some(InferenceFailure::Configuration));
    };
    if cancel.is_cancelled() {
        return done(RunEnd::Stopped, None);
    }
    if let Ok(mut p) = progress.lock() {
        p.sent = true;
    }
    let open = async {
        match &request {
            Request::Chat(body) => runtime
                .create_chat_completion_stream(body, Some(&options))
                .await
                .map(Opened::Chat),
            Request::Responses(body) => runtime
                .create_response_stream(body, Some(&options))
                .await
                .map(Opened::Responses),
        }
    };
    let opened = tokio::select! {
        opened = open => opened,
        () = cancel.cancelled() => {
            session.cancel();
            return done(RunEnd::Stopped, None);
        }
    };
    let mut stream = match opened {
        Ok(stream) => stream,
        Err(error) => {
            if let Ok(mut p) = progress.lock() {
                p.opening = Some(error.remote_outcome());
                p.sent = error.remote_outcome() != scm_sdk::RemoteOutcome::NotSent;
            }
            return done(RunEnd::Failed, Some(InferenceFailure::from_sdk(&error)));
        }
    };
    if let Ok(mut p) = progress.lock() {
        p.answered = true;
    }
    pulse.notify_one();
    loop {
        let next = tokio::select! {
            next = stream.next(&progress) => next,
            () = cancel.cancelled() => {
                stream.close();
                session.cancel();
                return done(RunEnd::Stopped, None);
            }
        };
        match next {
            Next::Going => pulse.notify_one(),
            Next::Full => {
                stream.close();
                session.cancel();
                return done(RunEnd::Truncated, None);
            }
            Next::Done => {
                let cut = progress.lock().is_ok_and(|p| p.cut);
                return done(
                    if cut {
                        RunEnd::Incomplete
                    } else {
                        RunEnd::Completed
                    },
                    None,
                );
            }
            Next::Failed(failure) => {
                stream.close();
                return done(RunEnd::Failed, Some(failure));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(prompt: &str) -> InferenceRun {
        InferenceRun {
            api: InferenceApi::Chat,
            model: "@openai-prod/gpt-4o".into(),
            route: None,
            system: None,
            prompt: prompt.into(),
            file: None,
        }
    }

    #[test]
    fn runs_and_endpoints_are_bounded() {
        assert!(run("hello").is_valid());
        assert!(!run("  ").is_valid());
        assert!(!run(&"x".repeat(MAX_INPUT_BYTES + 1)).is_valid());
        assert!(!run("bell\u{7}").is_valid());
        assert!(
            !InferenceRun {
                model: " m".into(),
                ..run("p")
            }
            .is_valid()
        );
        assert!(
            !InferenceRun {
                route: Some("a b".into()),
                ..run("p")
            }
            .is_valid()
        );
        assert!(
            InferenceRun {
                route: Some("cfg-1".into()),
                system: Some("be brief".into()),
                ..run("p")
            }
            .is_valid()
        );
        let body = |api| {
            match (InferenceRun {
                api,
                system: Some("be brief".into()),
                ..run("hi")
            })
            .request()
            .expect("request")
            {
                Request::Chat(r) => serde_json::to_value(r),
                Request::Responses(r) => serde_json::to_value(r),
            }
            .expect("json")
        };
        let request = body(InferenceApi::Chat);
        assert_eq!(request["stream"], true);
        assert_eq!(request["max_completion_tokens"], MAX_TOKENS);
        assert!(request.get("max_tokens").is_none_or(Value::is_null));
        assert_eq!(request["messages"][0]["role"], "system");
        assert_eq!(request["messages"][1]["content"], "hi");
        let request = body(InferenceApi::Responses);
        assert_eq!(request["stream"], true);
        assert_eq!(request["store"], false, "the Gateway keeps nothing");
        assert_eq!(request["max_output_tokens"], MAX_TOKENS);
        assert_eq!(request["instructions"], "be brief");
        assert_eq!(request["input"], "hi");
        let attached = InferenceRun {
            api: InferenceApi::Responses,
            file: Some("file-abc123".into()),
            ..run("hi")
        };
        assert!(attached.is_valid());
        let Some(Request::Responses(r)) = attached.request() else {
            panic!("a Responses request");
        };
        let request = serde_json::to_value(r).expect("json");
        assert_eq!(
            request["input"],
            json!([{"role": "user", "content": [
                {"type": "input_text", "text": "hi"},
                {"type": "input_file", "file_id": "file-abc123"}]}])
        );
        assert!(
            !InferenceRun {
                file: Some("file-abc123".into()),
                ..run("hi")
            }
            .is_valid(),
            "only a Responses run carries a file"
        );
        assert!(
            !InferenceRun {
                file: Some("../x".into()),
                ..attached.clone()
            }
            .is_valid()
        );
        assert!(endpoint_is_valid("https://gateway.example.com/v1"));
        for bad in [
            "http://gateway.example.com/v1",
            "https://user:pw@gateway.example.com/v1",
            "https://gateway.example.com/v1?key=x",
            "https://gateway.example.com/v1#x",
            "gateway.example.com",
        ] {
            assert!(!endpoint_is_valid(bad), "{bad}");
        }
    }

    #[test]
    fn output_is_bounded_on_a_character_boundary() {
        let mut p = RunProgress::default();
        let chunk = |text: &str| json!({"id": "c1", "model": "gpt-4o", "choices": [{"delta": {"content": text}}]});
        assert!(take(&mut p, &chunk("hello ")));
        assert_eq!(p.model.as_deref(), Some("gpt-4o"));
        p.output = "x".repeat(MAX_OUTPUT_BYTES - 1);
        assert!(
            !take(&mut p, &chunk("é")),
            "a two-byte character does not fit"
        );
        assert_eq!(p.output.len(), MAX_OUTPUT_BYTES - 1);
        let mut p = RunProgress::default();
        assert!(take(
            &mut p,
            &json!({"choices": [{"delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 5, "total_tokens": 8}})
        ));
        assert_eq!(p.finish.as_deref(), Some("stop"));
        assert!(!p.cut);
        assert_eq!(p.usage.and_then(|u| u.total), Some(8));
        for (finish, cut) in [("length", true), ("content_filter", true), ("stop", false)] {
            let mut p = RunProgress::default();
            take(
                &mut p,
                &json!({"choices": [{"delta": {}, "finish_reason": finish}]}),
            );
            assert_eq!(p.cut, cut, "{finish}");
        }
    }

    #[test]
    fn responses_events_carry_text_identity_usage_and_end() {
        let mut p = RunProgress::default();
        let created = json!({"type": "response.created", "sequence_number": 0,
            "response": {"id": "resp_1", "model": "gpt-4o", "status": "in_progress"}});
        assert_eq!(take_response(&mut p, &created), Step::Going);
        assert_eq!(p.response_id.as_deref(), Some("resp_1"));
        let delta = |text: &str| {
            json!({"type": "response.output_text.delta", "delta": text,
            "item_id": "msg_1", "output_index": 0, "content_index": 0})
        };
        assert_eq!(take_response(&mut p, &delta("Hel")), Step::Going);
        assert_eq!(take_response(&mut p, &delta("lo")), Step::Going);
        assert_eq!(
            take_response(
                &mut p,
                &json!({"type": "response.function_call_arguments.delta", "delta": "{}"})
            ),
            Step::Going
        );
        assert_eq!(p.output, "Hello", "only text deltas are output");
        let completed = json!({"type": "response.completed", "response": {"id": "resp_1",
            "model": "gpt-4o-2024", "usage": {"input_tokens": 2, "output_tokens": 3, "total_tokens": 5}}});
        assert_eq!(take_response(&mut p, &completed), Step::Going);
        assert_eq!(p.finish.as_deref(), Some("completed"));
        assert!(!p.cut, "a completed response is whole");
        assert_eq!(p.model.as_deref(), Some("gpt-4o-2024"));
        assert_eq!(p.usage.and_then(|u| u.total), Some(5));
        let mut p = RunProgress::default();
        let incomplete = json!({"type": "response.incomplete", "response": {"id": "r",
            "incomplete_details": {"reason": "max_output_tokens"}}});
        assert_eq!(take_response(&mut p, &incomplete), Step::Going);
        assert_eq!(p.finish.as_deref(), Some("max_output_tokens"));
        assert!(p.cut, "an incomplete response is cut short");
        assert_eq!(
            take_response(
                &mut p,
                &json!({"type": "response.failed", "response": {"id": "r"}})
            ),
            Step::Failed
        );
        p.output = "x".repeat(MAX_OUTPUT_BYTES - 1);
        assert_eq!(take_response(&mut p, &delta("é")), Step::Full);
        assert_eq!(p.output.len(), MAX_OUTPUT_BYTES - 1);
    }

    #[test]
    fn outcomes_follow_what_was_sent_and_answered() {
        let slot = || ProgressSlot::default();
        let s = slot();
        assert_eq!(settle(&s, RunEnd::Stopped, None), Outcome::NotSent);
        let s = slot();
        s.lock().expect("p").sent = true;
        assert_eq!(
            settle(&s, RunEnd::Stopped, None),
            Outcome::Unknown,
            "stopping never proves nothing was charged"
        );
        let opened = |outcome| {
            let s = slot();
            {
                let mut p = s.lock().expect("p");
                p.sent = true;
                p.opening = Some(outcome);
            }
            s
        };
        assert_eq!(
            settle(
                &opened(scm_sdk::RemoteOutcome::Rejected),
                RunEnd::Failed,
                Some(InferenceFailure::Blocked)
            ),
            Outcome::Rejected
        );
        assert_eq!(
            settle(
                &opened(scm_sdk::RemoteOutcome::Unknown),
                RunEnd::Failed,
                Some(InferenceFailure::Unavailable)
            ),
            Outcome::Unknown,
            "a 5xx is the SDK's unknown, whatever its label"
        );
        assert_eq!(
            settle(
                &opened(scm_sdk::RemoteOutcome::Unknown),
                RunEnd::Failed,
                Some(InferenceFailure::Rejected)
            ),
            Outcome::Unknown,
            "the label never decides the outcome"
        );
        let s = slot();
        s.lock().expect("p").sent = true;
        assert_eq!(
            settle(&s, RunEnd::Failed, Some(InferenceFailure::Network)),
            Outcome::Unknown
        );
        let s = slot();
        {
            let mut p = s.lock().expect("p");
            p.sent = true;
            p.answered = true;
        }
        assert_eq!(
            settle(&s, RunEnd::Failed, Some(InferenceFailure::InvalidResponse)),
            Outcome::Completed
        );
        assert_eq!(settle(&slot(), RunEnd::Truncated, None), Outcome::Completed);
        assert_eq!(
            settle(&slot(), RunEnd::Incomplete, None),
            Outcome::Completed,
            "the Gateway answered: cut short is not unsent"
        );
    }
}
