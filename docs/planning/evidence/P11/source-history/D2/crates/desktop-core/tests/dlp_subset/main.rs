//! C10 DLP authoring through the production authority interface against a
//! stateful synthetic DLP service: v1's exact subset (patterns create/
//! replace/delete, data profiles create/replace, filtering profiles
//! replace, dictionaries create/replace/delete), unsupported variants that
//! never reach the transport, literal multipart dictionary uploads from
//! typed text and from a file chosen in the native sheet, and success,
//! denial, conflict and unknown outcomes.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

use std::sync::{Arc, Mutex};

use desktop_core::ImportError;
use desktop_core::changes::ChangeProblem;
use desktop_core::fixtures::{self, Handles};
use desktop_core::management::{ManagementEndpoints, ManagementFailure};
use desktop_core::runtime::Outcome;
use desktop_core::transport::{ProxyMode, TransportPolicy};
use desktop_core::{
    AuthorityHandle, ChangeFailure, ChangeResultView, ChangeReviewState, ChangeReviewView, Core,
    CoreConfig, Delivery, Intent, Observation, Readback, Receipt, Refusal, ScanState,
    ShellSnapshot, StagedFileFailure, StagedFileState, ViewScope,
};
use serde_json::{Value, json};

use crate::support::{Reply, Seen, TestCa, TlsServer, eventually};

const CLIENT_ID: &str = "synthetic-client@1234567890.iam.panserviceaccount.com";
const CLIENT_SECRET: &str = "synthetic-client-secret-c10";
const TSG: &str = "1234567890";
const KEYWORDS: &str = "internal\nconfidential\n";

/// A stateful synthetic DLP service.
struct Service {
    issued: u32,
    serial: u32,
    patterns: Vec<Value>,
    profiles: Vec<Value>,
    filters: Vec<Value>,
    dictionaries: Vec<Value>,
    overrides: Vec<(String, String, Reply)>,
}

impl Service {
    fn new() -> Self {
        Self {
            issued: 0,
            serial: 100,
            patterns: vec![
                json!({"id": "pattern-1", "name": "Card numbers", "type": "custom",
                                  "detection_config": {"technique": "regex"}, "status": "active"}),
            ],
            profiles: vec![json!({"id": "dp-1", "name": "PCI", "detection_rules": []})],
            filters: vec![
                json!({"id": "filter-1", "name": "Default", "file_based": true, "non_file_based": false}),
            ],
            dictionaries: vec![
                json!({"id": "dict-1", "name": "Codes", "original_file_name": "codes.txt",
                                      "category": "Confidential", "region_name": "United States"}),
            ],
            overrides: Vec::new(),
        }
    }

    fn store(&mut self, path: &str) -> Option<&mut Vec<Value>> {
        match path {
            "data-patterns" => Some(&mut self.patterns),
            "data-profiles" => Some(&mut self.profiles),
            "data-filtering-profiles" => Some(&mut self.filters),
            "dictionaries" => Some(&mut self.dictionaries),
            _ => None,
        }
    }

    fn route(&mut self, seen: &Seen) -> Reply {
        let mut parts = seen.request_line.split(' ');
        let method = parts.next().unwrap_or("").to_owned();
        let target = parts.next().unwrap_or("").to_owned();
        let path = target.split('?').next().unwrap_or("").to_owned();
        if let Some(i) = self
            .overrides
            .iter()
            .position(|(m, p, _)| *m == method && path.starts_with(p.as_str()))
        {
            return self.overrides.remove(i).2;
        }
        if method == "POST" && path == "/oauth2/access_token" {
            self.issued += 1;
            return raw(
                200,
                &json!({"access_token": format!("c10-access-{}", self.issued),
                                    "token_type": "Bearer", "expires_in": 3600}),
            );
        }
        let Some(rest) = path.strip_prefix("/dlp/v2/api/") else {
            return raw(404, &json!({"error": "not found"}));
        };
        let (collection, id) = match rest.split_once('/') {
            Some((c, id)) => (c.to_owned(), Some(id.to_owned())),
            None => (rest.to_owned(), None),
        };
        let is_dictionary = collection == "dictionaries";
        let body = if is_dictionary && method != "GET" && method != "DELETE" {
            multipart_metadata(seen)
        } else {
            serde_json::from_slice(&seen.body).unwrap_or(Value::Null)
        };
        self.serial += 1;
        let serial = self.serial;
        let Some(store) = self.store(&collection) else {
            return raw(404, &json!({"error": "not found"}));
        };
        match (method.as_str(), id) {
            ("GET", None) => raw(
                200,
                &json!({"content": store.clone(), "last": true, "totalElements": store.len()}),
            ),
            ("GET", Some(id)) => match store.iter().find(|i| i["id"] == id.as_str()) {
                Some(item) => raw(200, item),
                None => raw(404, &json!({"error": "not found"})),
            },
            ("POST", None) => {
                let mut item = body;
                item["id"] = json!(format!("{collection}-{serial}"));
                store.push(item.clone());
                raw(200, &item)
            }
            ("PUT", Some(id)) => {
                match store.iter_mut().find(|i| i["id"] == id.as_str()) {
                    Some(item) => {
                        *item = body;
                        item["id"] = json!(id);
                        if is_dictionary {
                            // The dictionary replace replies 204.
                            return Reply::Raw {
                            head: "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close".into(),
                            body: Vec::new(),
                        };
                        }
                        raw(200, &item.clone())
                    }
                    None => raw(404, &json!({"error": "not found"})),
                }
            }
            ("DELETE", Some(id)) => {
                if is_dictionary {
                    store.retain(|i| i["id"] != id.as_str());
                } else if let Some(item) = store.iter_mut().find(|i| i["id"] == id.as_str()) {
                    // As the live service (C25): a deleted item is archived,
                    // still readable by its ID with status `deleted`.
                    let name = item["name"].as_str().unwrap_or_default().to_owned();
                    item["name"] = json!(format!("{name}_archived_20261025145341"));
                    item["status"] = json!("deleted");
                }
                Reply::Raw {
                    head: "HTTP/1.1 204 No Content\r\ncontent-length: 0\r\nconnection: close"
                        .into(),
                    body: Vec::new(),
                }
            }
            _ => raw(405, &json!({"error": "method"})),
        }
    }
}

/// The `json` part of a dictionary upload, as the service would store it.
fn multipart_metadata(seen: &Seen) -> Value {
    multipart(seen)
        .into_iter()
        .find(|p| p.name == "json")
        .and_then(|p| serde_json::from_slice(&p.body).ok())
        .unwrap_or(Value::Null)
}

#[derive(Debug, Clone, PartialEq)]
struct Part {
    name: String,
    filename: Option<String>,
    content_type: Option<String>,
    body: Vec<u8>,
}

/// An independent multipart/form-data reader (RFC 7578) for assertions.
fn multipart(seen: &Seen) -> Vec<Part> {
    let Some(boundary) = seen
        .header("content-type")
        .and_then(|c| c.split("boundary=").nth(1))
        .map(|b| b.trim_matches('"').to_owned())
    else {
        return Vec::new();
    };
    let delimiter = format!("--{boundary}");
    let body = &seen.body;
    let mut parts = Vec::new();
    let text = body.clone();
    let positions: Vec<usize> = (0..text.len().saturating_sub(delimiter.len()))
        .filter(|i| text[*i..].starts_with(delimiter.as_bytes()))
        .collect();
    for window in positions.windows(2) {
        let chunk = &text[window[0] + delimiter.len()..window[1]];
        let chunk = chunk.strip_prefix(b"\r\n").unwrap_or(chunk);
        let chunk = chunk.strip_suffix(b"\r\n").unwrap_or(chunk);
        let Some(split) = (0..chunk.len()).find(|i| chunk[*i..].starts_with(b"\r\n\r\n")) else {
            continue;
        };
        let head = String::from_utf8_lossy(&chunk[..split]).to_string();
        let content = chunk[split + 4..].to_vec();
        let mut part = Part {
            name: String::new(),
            filename: None,
            content_type: None,
            body: content,
        };
        for line in head.split("\r\n") {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("content-disposition:") {
                for field in line.split(';').map(str::trim) {
                    if let Some(v) = field.strip_prefix("name=") {
                        part.name = v.trim_matches('"').to_owned();
                    }
                    if let Some(v) = field.strip_prefix("filename=") {
                        part.filename = Some(v.trim_matches('"').to_owned());
                    }
                }
            } else if lower.starts_with("content-type:") {
                part.content_type = Some(line[13..].trim().to_owned());
            }
        }
        parts.push(part);
    }
    parts
}

fn raw(status: u16, body: &Value) -> Reply {
    let body = serde_json::to_vec(body).expect("json");
    Reply::Raw {
        head: format!(
            "HTTP/1.1 {status} Status\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close",
            body.len()
        ),
        body,
    }
}

struct Desk {
    _core: Core,
    handle: AuthorityHandle,
    session: String,
    sequence: u64,
    adapters: Handles,
    service: Arc<Mutex<Service>>,
    server: TlsServer,
    _ca: TestCa,
    results_seen: usize,
}

async fn desk() -> Desk {
    let ca = TestCa::new("airs-v2 c10 ca");
    let service = Arc::new(Mutex::new(Service::new()));
    let routed = Arc::clone(&service);
    let server = TlsServer::start(
        ca.leaf(&["localhost"]),
        Reply::Route(Arc::new(move |seen| {
            routed.lock().expect("service").route(seen)
        })),
    )
    .await;
    let origin = format!("https://localhost:{}", server.port);
    let (adapters, handles) = fixtures::adapters();
    let core = Core::new(
        CoreConfig {
            management: ManagementEndpoints::at_origin(&origin),
            ..CoreConfig::default()
        },
        adapters,
    );
    core.start();
    core.set_transport_policy(TransportPolicy {
        revision: 2,
        proxy: ProxyMode::Direct,
        extra_roots: vec![ca.pem.as_bytes().to_vec()],
    })
    .expect("policy");
    let handle = core.operator();
    let session = handle.register_delivery_session().expect("session").nonce;
    let mut desk = Desk {
        _core: core,
        handle,
        session,
        sequence: 0,
        adapters: handles,
        service,
        server,
        _ca: ca,
        results_seen: 0,
    };
    desk.send(
        json!({"kind": "createTenant", "profile": {"label": "Tenant A", "tsgId": TSG,
        "destination": {"kind": "custom", "origin": origin}}}),
    )
    .await
    .expect("tenant");
    let context = desk.context();
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "session",
        "clientId": CLIENT_ID, "secret": CLIENT_SECRET}),
    )
    .await
    .expect("management key");
    assert!(eventually(|| desk.snap().tenants[0].management.state == ScanState::Ready).await);
    desk.adapters.confirmer.auto(Some(true));
    desk
}

impl Desk {
    async fn send(&mut self, intent: Value) -> Result<Receipt, Refusal> {
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

    async fn review(&mut self, draft: &Value) -> ChangeReviewView {
        let context = self.context();
        self.send(json!({"kind": "reviewChange", "context": context, "draft": draft}))
            .await
            .expect("review admitted");
        assert!(
            eventually(|| self
                .snap()
                .changes
                .review
                .as_ref()
                .is_some_and(|r| matches!(
                    r.state,
                    ChangeReviewState::Ready | ChangeReviewState::Failed
                )))
            .await
        );
        self.snap().changes.review.expect("review")
    }

    /// Review, confirm and wait for the applied result and its write.
    async fn apply(&mut self, draft: Value, typed: &str) -> (ChangeResultView, Option<Seen>) {
        let review = self.review(&draft).await;
        assert_eq!(review.state, ChangeReviewState::Ready, "{review:?}");
        let before = self.writes().len();
        let known = self.known();
        self.send(json!({"kind": "requestApply", "review": review.review, "draft": draft, "confirmation": typed}))
            .await
            .expect("requested");
        let result = self.next_result(&known).await;
        let writes = self.writes();
        (result, writes.get(before).cloned())
    }

    /// Operations already listed (results are retained up to a bound).
    fn known(&self) -> Vec<desktop_core::runtime::OperationId> {
        self.snap()
            .changes
            .results
            .iter()
            .map(|r| r.operation.clone())
            .collect()
    }

    /// The first settled result not in `known`.
    async fn next_result(
        &mut self,
        known: &[desktop_core::runtime::OperationId],
    ) -> ChangeResultView {
        assert!(
            eventually(|| self
                .snap()
                .changes
                .results
                .first()
                .is_some_and(|r| r.settled && !known.contains(&r.operation)))
            .await
        );
        let results = self.snap().changes.results;
        self.results_seen = results.len();
        results[0].clone()
    }

    fn writes(&self) -> Vec<Seen> {
        self.server
            .seen()
            .into_iter()
            .filter(|s| !s.request_line.starts_with("GET ") && !s.request_line.contains("/oauth2/"))
            .collect()
    }
}

fn body(seen: &Seen) -> Value {
    serde_json::from_slice(&seen.body).unwrap_or(Value::Null)
}

fn draft(kind: &str, action: &str, target: &str, body: Value) -> Value {
    json!({"kind": kind, "action": action, "target": target, "body": body.to_string()})
}

fn pattern(name: &str) -> Value {
    json!({"name": name, "type": "custom", "description": "Synthetic",
           "detection_config": {"technique": "regex"}})
}

fn data_profile(name: &str) -> Value {
    json!({"name": name, "detection_rules": [{"rule_type": "expression_tree", "expression_tree": {
        "rule_item": {"detection_technique": "regex", "id": "pattern-1", "name": "Card numbers", "version": 1,
                      "match_type": "include", "confidence_level": "high", "occurrence_operator_type": "any",
                      "supported_confidence_levels": ["low", "high"]}}}]})
}

fn metadata(file: &str) -> Value {
    json!({"name": "Keywords", "original_file_name": file, "category": "Confidential", "region_name": "United States"})
}

#[tokio::test(flavor = "multi_thread")]
async fn data_patterns_are_created_replaced_and_deleted_exactly() {
    let mut desk = desk().await;
    let (created, write) = desk
        .apply(
            draft("dataPatterns", "create", "", pattern("Badges")),
            "CREATE",
        )
        .await;
    let write = write.expect("write");
    assert!(
        write
            .request_line
            .starts_with("POST /dlp/v2/api/data-patterns HTTP/1.1"),
        "{}",
        write.request_line
    );
    assert_eq!(
        body(&write),
        pattern("Badges"),
        "no authorship is added to DLP writes (v1)"
    );
    assert_eq!(write.header("content-type"), Some("application/json"));
    assert_eq!(
        (created.outcome, created.readback),
        (Some(Outcome::Completed), Some(Readback::Found))
    );
    let (replaced, write) = desk
        .apply(
            draft(
                "dataPatterns",
                "update",
                "pattern-1",
                pattern("Card numbers v2"),
            ),
            "Card numbers",
        )
        .await;
    let write = write.expect("write");
    assert!(
        write
            .request_line
            .starts_with("PUT /dlp/v2/api/data-patterns/pattern-1 ")
    );
    assert_eq!(body(&write)["name"], "Card numbers v2");
    assert!(
        replaced
            .current
            .as_deref()
            .expect("readback")
            .contains("Card numbers v2")
    );
    let (deleted, write) = desk
        .apply(
            draft("dataPatterns", "delete", "pattern-1", json!({})),
            "Card numbers v2",
        )
        .await;
    let write = write.expect("write");
    assert!(
        write
            .request_line
            .starts_with("DELETE /dlp/v2/api/data-patterns/pattern-1 ")
    );
    assert!(write.body.is_empty(), "the SDK delete sends no body");
    // The service archives the pattern (status `deleted`); it reads back
    // absent and cannot be reviewed for another change.
    assert_eq!(deleted.readback, Some(Readback::Absent));
    let again = desk
        .review(&draft(
            "dataPatterns",
            "update",
            "pattern-1",
            pattern("Card numbers v3"),
        ))
        .await;
    assert_eq!(again.state, ChangeReviewState::Failed);
    assert_eq!(
        again.failure,
        Some(ChangeFailure::Problem {
            problem: ChangeProblem::NotFound
        })
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn data_profiles_follow_v1s_structure_rules_and_have_no_delete() {
    let mut desk = desk().await;
    let before = desk.server.seen().len();
    // A pattern leaf without its confidence level fails v1's check: nothing is read or sent.
    let mut broken = data_profile("Broken");
    broken["detection_rules"][0]["expression_tree"]["rule_item"]
        .as_object_mut()
        .expect("leaf")
        .remove("confidence_level");
    let review = desk
        .review(&draft("dataProfiles", "create", "", broken))
        .await;
    assert_eq!(
        review.failure,
        Some(ChangeFailure::Problem {
            problem: ChangeProblem::DataProfile
        })
    );
    let long = desk
        .review(&draft(
            "dataProfiles",
            "create",
            "",
            data_profile(&"x".repeat(33)),
        ))
        .await;
    assert_eq!(
        long.failure,
        Some(ChangeFailure::Problem {
            problem: ChangeProblem::DataProfile
        })
    );
    for unsupported in [
        draft("dataProfiles", "delete", "dp-1", json!({})),
        draft(
            "filteringProfiles",
            "create",
            "",
            json!({"file_based": true, "non_file_based": true}),
        ),
        draft("filteringProfiles", "delete", "filter-1", json!({})),
    ] {
        let review = desk.review(&unsupported).await;
        assert_eq!(
            review.failure,
            Some(ChangeFailure::Problem {
                problem: ChangeProblem::Unsupported
            }),
            "{unsupported}"
        );
    }
    assert_eq!(
        desk.server.seen().len(),
        before,
        "unsupported or invalid drafts never reach the transport"
    );
    let (created, write) = desk
        .apply(
            draft("dataProfiles", "create", "", data_profile("Payments")),
            "CREATE",
        )
        .await;
    assert!(
        write
            .expect("write")
            .request_line
            .starts_with("POST /dlp/v2/api/data-profiles ")
    );
    assert_eq!(created.readback, Some(Readback::Found));
    // Replace drops the response-only legacy rendering and null fields (v1).
    let mut edited = data_profile("PCI");
    edited["advance_data_patterns_rule_request"] = json!({"legacy": true});
    edited["description"] = Value::Null;
    let (_, write) = desk
        .apply(draft("dataProfiles", "update", "dp-1", edited), "PCI")
        .await;
    let sent = body(&write.expect("write"));
    assert!(
        sent.get("advance_data_patterns_rule_request").is_none()
            && sent.get("description").is_none()
    );
    assert_eq!(
        sent["detection_rules"],
        data_profile("PCI")["detection_rules"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn filtering_profiles_are_replaced_only() {
    let mut desk = desk().await;
    let (replaced, write) = desk
        .apply(
            draft(
                "filteringProfiles",
                "update",
                "filter-1",
                json!({"name": "Default", "file_based": false, "non_file_based": true}),
            ),
            "Default",
        )
        .await;
    let write = write.expect("write");
    assert!(
        write
            .request_line
            .starts_with("PUT /dlp/v2/api/data-filtering-profiles/filter-1 ")
    );
    // The profile's name is the service's own field: a replace leaves it
    // out, and the service keeps it (C25).
    assert_eq!(
        body(&write),
        json!({"file_based": false, "non_file_based": true})
    );
    assert_eq!(replaced.readback, Some(Readback::Found));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_typed_dictionary_is_uploaded_as_literal_multipart() {
    let mut desk = desk().await;
    let (created, write) = desk
        .apply(
            draft(
                "dictionaries",
                "create",
                "",
                json!({"metadata": metadata("keywords.txt"), "file": KEYWORDS}),
            ),
            "CREATE",
        )
        .await;
    let write = write.expect("write");
    assert!(
        write
            .request_line
            .starts_with("POST /dlp/v2/api/dictionaries HTTP/1.1"),
        "{}",
        write.request_line
    );
    let content_type = write.header("content-type").expect("content type");
    assert!(
        content_type.starts_with("multipart/form-data; boundary=scm-sdk-"),
        "{content_type}"
    );
    let parts = multipart(&write);
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert_eq!(
        (
            parts[0].name.as_str(),
            parts[0].filename.as_deref(),
            parts[0].content_type.as_deref()
        ),
        ("json", Some("metadata.json"), Some("application/json"))
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&parts[0].body).expect("json"),
        metadata("keywords.txt")
    );
    assert_eq!(
        (
            parts[1].name.as_str(),
            parts[1].filename.as_deref(),
            parts[1].content_type.as_deref()
        ),
        ("file", Some("keywords.txt"), Some("text/plain"))
    );
    assert_eq!(parts[1].body, KEYWORDS.as_bytes(), "exact file bytes");
    assert_eq!(created.readback, Some(Readback::Found));
    // Replace replies 204: the readback reads the item by its identifier.
    let (replaced, write) = desk
        .apply(
            draft(
                "dictionaries",
                "update",
                "dict-1",
                json!({"metadata": metadata("codes.txt"), "file": "alpha\n"}),
            ),
            "Codes",
        )
        .await;
    let write = write.expect("write");
    assert!(
        write
            .request_line
            .starts_with("PUT /dlp/v2/api/dictionaries/dict-1 ")
    );
    assert_eq!(multipart(&write)[1].body, b"alpha\n");
    assert_eq!(
        (replaced.outcome, replaced.readback),
        (Some(Outcome::Completed), Some(Readback::Found))
    );
    let (deleted, _) = desk
        .apply(
            draft("dictionaries", "delete", "dict-1", json!({})),
            "Keywords",
        )
        .await;
    assert_eq!(deleted.readback, Some(Readback::Absent));
}

#[tokio::test(flavor = "multi_thread")]
async fn dictionary_files_and_names_are_bounded_before_any_request() {
    let mut desk = desk().await;
    let before = desk.server.seen().len();
    for (body, problem) in [
        (
            json!({"metadata": metadata("keywords.txt"), "file": ""}),
            ChangeProblem::DictionaryFile,
        ),
        (
            json!({"metadata": metadata("keywords.txt"), "file": "x".repeat(64 * 1024 + 1)}),
            ChangeProblem::DictionaryFile,
        ),
        (
            json!({"metadata": metadata("../keywords.txt"), "file": KEYWORDS}),
            ChangeProblem::FileName,
        ),
        (
            json!({"metadata": metadata("keywords.txt"), "file": KEYWORDS, "extra": 1}),
            ChangeProblem::Body,
        ),
        (
            json!({"metadata": {"name": "No category", "original_file_name": "a.txt", "region_name": "United States"}, "file": KEYWORDS}),
            ChangeProblem::Contract,
        ),
    ] {
        let review = desk
            .review(&draft("dictionaries", "create", "", body))
            .await;
        assert_eq!(review.failure, Some(ChangeFailure::Problem { problem }));
    }
    assert_eq!(desk.server.seen().len(), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_chosen_in_the_native_sheet_is_staged_validated_and_uploaded() {
    let mut desk = desk().await;
    let context = desk.context();
    // Oversized, non-text and empty files are reported, never staged.
    for (bytes, failure) in [
        (vec![b'a'; 64 * 1024 + 1], StagedFileFailure::TooLarge),
        (vec![0xff, 0xfe, 0x00], StagedFileFailure::NotText),
        (b"  \n".to_vec(), StagedFileFailure::Empty),
    ] {
        desk.adapters
            .text_files
            .reply(Some(Ok(("bad.txt".into(), bytes))));
        desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
            .await
            .expect("chosen");
        assert!(
            eventually(|| desk
                .snap()
                .changes
                .staged
                .as_ref()
                .is_some_and(|s| s.state == StagedFileState::Failed))
            .await
        );
        assert_eq!(
            desk.snap().changes.staged.expect("staged").failure,
            Some(failure)
        );
    }
    desk.adapters
        .text_files
        .reply(Some(Err(ImportError::TooLarge)));
    desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
        .await
        .expect("chosen");
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .staged
            .as_ref()
            .is_some_and(|s| s.failure == Some(StagedFileFailure::TooLarge)))
        .await
    );
    // A UTF-8 file (with a byte-order mark) is staged; the renderer sees only
    // its token, name and size.
    let file = "\u{feff}alpha\nbeta\ngamma\n".as_bytes().to_vec();
    desk.adapters
        .text_files
        .reply(Some(Ok(("keywords.txt".into(), file.clone()))));
    desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
        .await
        .expect("chosen");
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .staged
            .as_ref()
            .is_some_and(|s| s.state == StagedFileState::Ready))
        .await
    );
    let staged = desk.snap().changes.staged.expect("staged");
    assert_eq!(
        (staged.name.as_deref(), staged.lines),
        (Some("keywords.txt"), Some(3))
    );
    let token = staged.token.expect("token");
    assert!(
        !serde_json::to_string(&desk.snap())
            .expect("json")
            .contains("gamma"),
        "content is not in the snapshot"
    );
    let by_file = draft(
        "dictionaries",
        "create",
        "",
        json!({"metadata": metadata("keywords.txt"), "fileRef": token}),
    );
    let (created, write) = desk.apply(by_file.clone(), "CREATE").await;
    let parts = multipart(&write.expect("write"));
    assert_eq!(
        parts[1].body, b"alpha\nbeta\ngamma\n",
        "the staged text, without the byte-order mark"
    );
    assert_eq!(created.outcome, Some(Outcome::Completed));
    // An unknown reference fails review without a request.
    let before = desk.server.seen().len();
    let stale = desk
        .review(&draft(
            "dictionaries",
            "create",
            "",
            json!({"metadata": metadata("k.txt"), "fileRef": "sf-unknown"}),
        ))
        .await;
    assert_eq!(
        stale.failure,
        Some(ChangeFailure::Problem {
            problem: ChangeProblem::StagedFile
        })
    );
    assert_eq!(desk.server.seen().len(), before);
    // Choosing another file after review changes what Apply would send:
    // the reviewed draft no longer matches and nothing is sent.
    let review = desk.review(&by_file).await;
    desk.adapters
        .text_files
        .reply(Some(Ok(("other.txt".into(), b"delta\n".to_vec()))));
    desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
        .await
        .expect("chosen");
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .staged
            .as_ref()
            .is_some_and(|s| s.name.as_deref() == Some("other.txt")))
        .await
    );
    assert_eq!(
        desk.send(json!({"kind": "requestApply", "review": review.review, "draft": by_file, "confirmation": "CREATE"})).await.err(),
        Some(Refusal::ReviewInvalid)
    );
    assert_eq!(desk.writes().len(), 1);
    // A cancelled sheet stages nothing.
    desk.adapters.text_files.reply(None);
    desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
        .await
        .expect("chosen");
    assert!(eventually(|| desk.snap().changes.staged.is_none()).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn denial_conflict_and_lost_settlement_keep_faithful_outcomes() {
    let mut desk = desk().await;
    // Denied upload.
    desk.service.lock().expect("service").overrides.push((
        "POST".into(),
        "/dlp/v2/api/dictionaries".into(),
        raw(403, &json!({"message": "SERVER-TEXT"})),
    ));
    let (denied, _) = desk
        .apply(
            draft(
                "dictionaries",
                "create",
                "",
                json!({"metadata": metadata("keywords.txt"), "file": KEYWORDS}),
            ),
            "CREATE",
        )
        .await;
    assert_eq!(
        (denied.outcome, denied.failure),
        (
            Some(Outcome::Rejected),
            Some(ChangeFailure::Service {
                failure: ManagementFailure::Denied
            })
        )
    );
    // Concurrent edit between review and apply: conflict, not written.
    let replace = draft("dataPatterns", "update", "pattern-1", pattern("Mine"));
    let review = desk.review(&replace).await;
    desk.service.lock().expect("service").patterns[0]["name"] = json!("Theirs");
    let writes = desk.writes().len();
    desk.send(json!({"kind": "requestApply", "review": review.review, "draft": replace, "confirmation": "Card numbers"}))
        .await
        .expect("requested");
    assert!(
        eventually(
            || desk.snap().changes.results.len() == 2 && desk.snap().changes.results[0].settled
        )
        .await
    );
    desk.results_seen = 2;
    assert_eq!(
        desk.snap().changes.results[0].failure,
        Some(ChangeFailure::Conflict)
    );
    assert_eq!(desk.writes().len(), writes);
    // An interrupted upload (the connection drops): unknown, never resent.
    desk.service.lock().expect("service").overrides.push((
        "POST".into(),
        "/dlp/v2/api/dictionaries".into(),
        Reply::Close,
    ));
    let (lost, _) = desk
        .apply(
            draft(
                "dictionaries",
                "create",
                "",
                json!({"metadata": metadata("keywords.txt"), "file": KEYWORDS}),
            ),
            "CREATE",
        )
        .await;
    assert_eq!(lost.outcome, Some(Outcome::Unknown));
    let uploads = desk
        .writes()
        .iter()
        .filter(|w| w.request_line.starts_with("POST /dlp/v2/api/dictionaries"))
        .count();
    assert_eq!(uploads, 2, "denied once, lost once, never retried");
    assert!(!format!("{:?}", desk.snap()).contains("SERVER-TEXT"));
}

async fn stage_file(desk: &mut Desk, name: &str, bytes: &[u8]) -> String {
    let context = desk.context();
    desk.adapters
        .text_files
        .reply(Some(Ok((name.into(), bytes.to_vec()))));
    desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
        .await
        .expect("chosen");
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .staged
            .as_ref()
            .is_some_and(|s| s.name.as_deref() == Some(name)))
        .await
    );
    desk.snap()
        .changes
        .staged
        .and_then(|s| s.token)
        .expect("token")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_staged_files_content_never_reaches_a_projection_and_the_pending_hold_keeps_it() {
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(None);
    let Observation::Shell { mut updates, .. } =
        desk.handle.observe(ViewScope::Shell).expect("observe")
    else {
        unreachable!("shell scope")
    };
    let canary = "canary-keyword-c10\nsecond-canary\n";
    let token = stage_file(&mut desk, "canary.txt", canary.as_bytes()).await;
    let by_file = draft(
        "dictionaries",
        "create",
        "",
        json!({"metadata": metadata("canary.txt"), "fileRef": token}),
    );
    let review = desk.review(&by_file).await;
    let after: Value = serde_json::from_str(review.after.as_deref().expect("after")).expect("json");
    assert!(
        after["file"]
            .as_str()
            .expect("summary")
            .starts_with("2 keywords, "),
        "{after}"
    );
    desk.send(json!({"kind": "requestApply", "review": review.review, "draft": by_file, "confirmation": "CREATE"}))
        .await
        .expect("requested");
    assert!(eventually(|| desk.adapters.confirmer.held() == 1).await);
    // While the confirmation holds the editor, the file cannot be replaced.
    let context = desk.context();
    assert_eq!(
        desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
            .await
            .err(),
        Some(Refusal::Busy)
    );
    let mut projections = vec![serde_json::to_string(&desk.snap()).expect("json")];
    while let Some(update) = updates.try_next() {
        if let desktop_core::Update::Shell(snapshot) = update {
            projections.push(serde_json::to_string(&snapshot).expect("json"));
        }
    }
    for projection in &projections {
        assert!(
            !projection.contains("canary-keyword-c10"),
            "selected-file content in a projection"
        );
    }
    // The old confirmation applies exactly the reviewed file.
    assert!(desk.adapters.confirmer.complete(true));
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .results
            .first()
            .is_some_and(|r| r.settled))
        .await
    );
    let upload = desk.writes().pop().expect("upload");
    assert_eq!(multipart(&upload)[1].body, canary.as_bytes());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sheet_answering_during_a_confirmation_ends_it_and_late_or_retired_choices_stage_nothing()
{
    let mut desk = desk().await;
    desk.adapters.confirmer.auto(None);
    let token = stage_file(&mut desk, "first.txt", b"first\n").await;
    let by_file = draft(
        "dictionaries",
        "create",
        "",
        json!({"metadata": metadata("first.txt"), "fileRef": token}),
    );
    let review = desk.review(&by_file).await;
    // A sheet opened before the confirmation answers while it is pending.
    let context = desk.context();
    desk.adapters.text_files.hold();
    desk.adapters
        .text_files
        .reply(Some(Ok(("second.txt".into(), b"second\n".to_vec()))));
    desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
        .await
        .expect("chosen");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    // RequestApply cannot start while a sheet is open (the staging is busy);
    // so the sheet answers first here, then confirm with the old review.
    desk.adapters.text_files.release();
    assert!(
        eventually(|| desk
            .snap()
            .changes
            .staged
            .as_ref()
            .is_some_and(|s| s.name.as_deref() == Some("second.txt")))
        .await
    );
    assert_eq!(
        desk.send(json!({"kind": "requestApply", "review": review.review, "draft": by_file, "confirmation": "CREATE"})).await.err(),
        Some(Refusal::ReviewInvalid),
        "the reviewed file is no longer the staged one"
    );
    // A late answer after the context retired stages nothing.
    desk.adapters.text_files.hold();
    desk.adapters
        .text_files
        .reply(Some(Ok(("late.txt".into(), b"late\n".to_vec()))));
    desk.send(json!({"kind": "chooseDictionaryFile", "context": context}))
        .await
        .expect("chosen");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    desk.send(
        json!({"kind": "configureManagementCredential", "context": context, "mode": "session",
        "clientId": CLIENT_ID, "secret": "replacement-secret-c10"}),
    )
    .await
    .expect("replaced");
    desk.adapters.text_files.release();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        desk.snap().changes.staged.is_none(),
        "nothing staged for the retired context"
    );
    // A token staged for the old context does not resolve in the new one.
    let stale = desk
        .review(&draft(
            "dictionaries",
            "create",
            "",
            json!({"metadata": metadata("x.txt"), "fileRef": token}),
        ))
        .await;
    assert_eq!(
        stale.failure,
        Some(ChangeFailure::Problem {
            problem: ChangeProblem::StagedFile
        })
    );
    assert!(desk.writes().is_empty());
}

/// Each supported DLP mutation: denial, unknown outcome and (for an
/// existing item) a changed baseline, each without retry.
#[tokio::test(flavor = "multi_thread")]
async fn every_supported_dlp_mutation_reports_denial_unknown_and_conflict_truthfully() {
    let mut desk = desk().await;
    // (draft, typed confirmation, method, path, existing item (collection, id))
    type Op<'a> = (Value, &'a str, &'a str, &'a str, Option<(&'a str, &'a str)>);
    let ops: Vec<Op> = vec![
        (
            draft("dataPatterns", "create", "", pattern("P")),
            "CREATE",
            "POST",
            "/dlp/v2/api/data-patterns",
            None,
        ),
        (
            draft("dataPatterns", "update", "pattern-1", pattern("P2")),
            "Card numbers",
            "PUT",
            "/dlp/v2/api/data-patterns/pattern-1",
            Some(("data-patterns", "pattern-1")),
        ),
        (
            draft("dataPatterns", "delete", "pattern-1", json!({})),
            "Card numbers",
            "DELETE",
            "/dlp/v2/api/data-patterns/pattern-1",
            Some(("data-patterns", "pattern-1")),
        ),
        (
            draft("dataProfiles", "create", "", data_profile("New")),
            "CREATE",
            "POST",
            "/dlp/v2/api/data-profiles",
            None,
        ),
        (
            draft("dataProfiles", "update", "dp-1", data_profile("PCI")),
            "PCI",
            "PUT",
            "/dlp/v2/api/data-profiles/dp-1",
            Some(("data-profiles", "dp-1")),
        ),
        (
            draft(
                "filteringProfiles",
                "update",
                "filter-1",
                json!({"name": "Default", "file_based": false, "non_file_based": true}),
            ),
            "Default",
            "PUT",
            "/dlp/v2/api/data-filtering-profiles/filter-1",
            Some(("data-filtering-profiles", "filter-1")),
        ),
        (
            draft(
                "dictionaries",
                "create",
                "",
                json!({"metadata": metadata("k.txt"), "file": KEYWORDS}),
            ),
            "CREATE",
            "POST",
            "/dlp/v2/api/dictionaries",
            None,
        ),
        (
            draft(
                "dictionaries",
                "update",
                "dict-1",
                json!({"metadata": metadata("codes.txt"), "file": KEYWORDS}),
            ),
            "Codes",
            "PUT",
            "/dlp/v2/api/dictionaries/dict-1",
            Some(("dictionaries", "dict-1")),
        ),
        (
            draft("dictionaries", "delete", "dict-1", json!({})),
            "Codes",
            "DELETE",
            "/dlp/v2/api/dictionaries/dict-1",
            Some(("dictionaries", "dict-1")),
        ),
    ];
    for (op, typed, method, path, existing) in ops {
        // Denied.
        desk.service.lock().expect("service").overrides.push((
            method.into(),
            path.into(),
            raw(403, &json!({"message": "SERVER-TEXT"})),
        ));
        let sent = desk.writes().len();
        let (denied, _) = desk.apply(op.clone(), typed).await;
        assert_eq!(
            (denied.outcome, denied.failure),
            (
                Some(Outcome::Rejected),
                Some(ChangeFailure::Service {
                    failure: ManagementFailure::Denied
                })
            ),
            "{op}"
        );
        // Unknown (the connection drops after the request).
        desk.service.lock().expect("service").overrides.push((
            method.into(),
            path.into(),
            Reply::Close,
        ));
        let (lost, _) = desk.apply(op.clone(), typed).await;
        assert_eq!(lost.outcome, Some(Outcome::Unknown), "{op}");
        assert_eq!(
            desk.writes().len(),
            sent + 2,
            "one attempt each, never retried: {op}"
        );
        // A changed baseline: conflict, not sent.
        if let Some((collection, id)) = existing {
            let review = desk.review(&op).await;
            {
                let mut service = desk.service.lock().expect("service");
                let store = service.store(collection).expect("store");
                if let Some(item) = store.iter_mut().find(|i| i["id"] == id) {
                    item["changed_elsewhere"] = json!(true);
                }
            }
            let before = desk.writes().len();
            let known = desk.known();
            desk.send(json!({"kind": "requestApply", "review": review.review, "draft": op, "confirmation": typed}))
                .await
                .expect("requested");
            let conflicted = desk.next_result(&known).await;
            assert_eq!(conflicted.failure, Some(ChangeFailure::Conflict), "{op}");
            assert_eq!(desk.writes().len(), before, "conflict sends nothing: {op}");
            let mut service = desk.service.lock().expect("service");
            if let Some(item) = service
                .store(collection)
                .and_then(|s| s.iter_mut().find(|i| i["id"] == id))
            {
                item.as_object_mut()
                    .expect("object")
                    .remove("changed_elsewhere");
            }
        }
    }
    assert!(!format!("{:?}", desk.snap()).contains("SERVER-TEXT"));
}
