//! L09 Gateway inference against the synthetic cloud: a bounded streamed
//! answer, a stopped stream, an unknown model refused, the run and token
//! budgets.

use serde_json::{Value, json};

use crate::cloud::{Chat, Cloud, MODEL, RUNTIME, WS_A};
use crate::common::{
    Browser, CLIENT_SECRET, assert_no_secret, designate, keychain, management_family,
    management_secrets, manifest, outcomes, receipt,
};
use crate::reads::{Served, served};

const INFERENCE_KEY: &str = "synthetic-inference-key-c25";

fn scope(s: &Served, budget: Value) -> Value {
    let mut value = manifest();
    let mut family = management_family(json!({
        "nonsecret": {"workspace_id": WS_A, "model": MODEL,
            "inference_endpoint": format!("https://localhost:{}{RUNTIME}", s.server.port)},
        "credentials": {"inference_key": keychain("inference")}
    }));
    family["budget"] = budget;
    designate(&mut value, "L09", family);
    value
}

#[tokio::test(flavor = "multi_thread")]
async fn streamed_stopped_and_refused_runs_are_bounded_and_recorded() {
    let s = served(Cloud::new()).await;
    // The stopped run streams slowly; the others answer at once.
    let paced = std::sync::Arc::clone(&s.cloud);
    let (repo, record) = {
        let value = scope(&s, json!({"runs": 3, "tokens": 100}));
        let fixture = &s.fixture;
        let keys = management_secrets(&[("inference", INFERENCE_KEY)]);
        let run = fixture.run(value, &["L09"], &keys, Browser::idle());
        let switch = async {
            // After the first run completes, pace the second.
            for _ in 0..400 {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                let mut c = paced.lock().expect("cloud");
                if c.writes
                    .iter()
                    .filter(|w| w.contains("/chat/completions"))
                    .count()
                    >= 1
                {
                    c.chat = Chat::Paced;
                    break;
                }
            }
        };
        let (result, ()) = tokio::join!(run, switch);
        result
    };
    let record = record.expect("run");
    let checks = outcomes(&record, "L09");
    assert!(checks.iter().all(|c| c.ends_with(": passed")), "{checks:?}");
    let stream = receipt(&repo, "L09", "L09-stream");
    assert_eq!(stream.observed["usage"]["total"], json!(8));
    assert_eq!(stream.observed["served_model"], json!("gpt-4o"));
    assert!(stream.ids.contains_key("trace"));
    let cancel = receipt(&repo, "L09", "L09-cancel");
    assert_eq!(cancel.observed["state"], json!("stopped"));
    let error = receipt(&repo, "L09", "L09-error-recovery");
    assert_eq!(error.observed["state"], json!("failed"));
    assert_eq!(error.consumption["runs"], json!({"used": 3, "limit": 3}));
    // Every request went to the designated Gateway origin only.
    assert!(
        s.server
            .seen()
            .iter()
            .all(|r| !r.request_line.contains("api.openai.com"))
    );
    assert_no_secret(&repo, &[CLIENT_SECRET, INFERENCE_KEY]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_over_its_token_cap_stops_the_family() {
    let s = served(Cloud::new()).await;
    s.cloud.lock().expect("cloud").tokens = 50;
    let (repo, record) = s
        .fixture
        .run(
            scope(&s, json!({"runs": 3, "tokens": 10})),
            &["L09"],
            &management_secrets(&[("inference", INFERENCE_KEY)]),
            Browser::idle(),
        )
        .await;
    let record = record.expect("run");
    let stop = record.stopped.expect("stopped");
    assert!(stop.reason.contains("unexpected cost"), "{stop:?}");
    assert_eq!(stop.check, "L09-stream");
    assert_eq!(
        receipt(&repo, "L09", "L09-stream").consumption["tokens"],
        json!({"used": 50, "limit": 10})
    );
    let calls = s
        .server
        .seen()
        .iter()
        .filter(|r| r.request_line.contains("/chat/completions"))
        .count();
    assert_eq!(calls, 1, "no further run was started");
}
