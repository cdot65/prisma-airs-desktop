//! C20 Harness company sign-in, renewal and protected history through the
//! production authority interface.
//!
//! A synthetic OpenID provider (`idp`) and the synthetic TLS Gateway share
//! one loopback server. The browser is played by the test: it completes
//! the authorization the app opened and calls the app's loopback callback.
//! Conversations run on the real pinned engine, so renewals restart it and
//! restore the conversation with the engine's own `desktop/history/*`; a
//! scripted stand-in covers what the engine cannot be made to do.

#[allow(dead_code)]
#[path = "../context_credentials/support.rs"]
mod support;

#[allow(dead_code, unused_imports)]
#[path = "../harness_process/desk.rs"]
mod desk;

mod idp;

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use desktop_core::fixtures::SaveScript;
use desktop_core::vault::RecordId;
use serde_json::{Value, json};

use crate::desk::*;
use crate::idp::{AUDIENCE, CLIENT, Idp, OTHER_SUBJECT, SUBJECT, Shared};

/// How one case breaks the provider.
type Break = Box<dyn Fn(&mut Idp)>;

const FIRST: &str = "Remember the C20 canary word: periwinkle.";
const SECOND: &str = "What word did I ask you to remember?";

/// A desk with the real engine and the identity provider on its server.
async fn desk() -> (Desk, Shared) {
    desk_on(Some(real_engine()), None).await
}

async fn desk_on(
    engine: Option<desktop_core::harness::engine::Engine>,
    pin: Option<desktop_core::harness::engine::Pin>,
) -> (Desk, Shared) {
    let desk = desk_with(engine, pin).await;
    let origin = format!("https://localhost:{}", desk.server.port);
    let idp = Arc::new(Mutex::new(Idp::new(&origin, "team-a")));
    let routed = Arc::clone(&idp);
    desk.gateway.lock().expect("gateway").hook = Some(Arc::new(move |seen| {
        routed.lock().expect("idp").route(seen)
    }));
    (desk, idp)
}

/// One HTTP/1.1 GET to the app's loopback callback; the status code.
fn callback(url: &str) -> u16 {
    callback_with_host(url, None)
}

fn callback_with_host(url: &str, host: Option<&str>) -> u16 {
    let url = url::Url::parse(url).expect("callback url");
    let port = url.port().expect("port");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("timeout");
    let target = match url.query() {
        Some(q) => format!("{}?{q}", url.path()),
        None => url.path().to_owned(),
    };
    let host = host.map_or_else(|| format!("127.0.0.1:{port}"), str::to_owned);
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .expect("write");
    let mut reply = String::new();
    let _ = stream.read_to_string(&mut reply);
    reply
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn sign_in_view(desk: &Desk) -> Value {
    desk.json()["harness"]["signIn"].clone()
}

async fn configure(desk: &mut Desk, reference: &str, idp: &Shared) {
    let context = desk.context();
    let issuer = idp.lock().expect("idp").issuer.clone();
    let endpoint = desk.endpoint(None);
    desk.send(
        json!({"kind": "configureHarnessSignIn", "context": context, "workspace": reference,
        "endpoint": endpoint, "issuer": issuer, "clientId": CLIENT, "audience": AUDIENCE}),
    )
    .await
    .expect("configured");
}

/// Starts a sign-in and plays the browser; returns the sign-in view once it
/// settles (signed in, or failed).
async fn sign_in(desk: &mut Desk, idp: &Shared) -> Value {
    let opened = desk.adapters.opener.opened().len();
    let context = desk.context();
    desk.send(json!({"kind": "signInHarness", "context": context}))
        .await
        .expect("sign-in admitted");
    let settled = settle(|| {
        let v = sign_in_view(desk);
        desk.adapters.opener.opened().len() > opened
            || (v["step"].is_null() && !v["failure"].is_null())
    })
    .await;
    assert!(settled, "{}", sign_in_view(desk));
    if let Some(url) = desk.adapters.opener.opened().get(opened) {
        let url = url::Url::parse(url).expect("authorize url");
        let (_, back) = idp.lock().expect("idp").authorize(&url);
        // Another local page cannot complete it: a wrong state or host is
        // answered and ignored.
        let forged = back.replace("state=", "state=x");
        assert_eq!(callback(&forged), 400);
        let port = url::Url::parse(&back)
            .expect("callback")
            .port()
            .expect("port");
        assert_eq!(
            callback_with_host(&back, Some(&format!("localhost:{port}"))),
            400
        );
        assert_eq!(callback(&back), 200);
    }
    assert!(
        settle_for(15, || {
            let v = sign_in_view(desk);
            v["step"].is_null() && (!v["account"].is_null() || !v["failure"].is_null())
        })
        .await,
        "{}",
        sign_in_view(desk)
    );
    sign_in_view(desk)
}

/// A ready conversation with company sign-in in WS_A.
async fn signed_in_conversation(desk: &mut Desk, idp: &Shared) -> String {
    let reference = desk.choose(WS_A).await;
    configure(desk, &reference, idp).await;
    let view = sign_in(desk, idp).await;
    assert_eq!(view["account"], format!("{SUBJECT}@example.com"), "{view}");
    let id = desk.start(&reference).await.expect("started");
    assert!(
        settle_for(30, || desk.conversation()["phase"] == "ready").await,
        "{}",
        desk.conversation()
    );
    id
}

/// Waits for the conversation to hold `entries` entries with its last turn
/// ended (a renewal may come first).
async fn answered_with(desk: &mut Desk, entries: usize) -> Value {
    assert!(
        settle_for(40, || {
            let c = desk.conversation();
            c["phase"] == "failed"
                || (c["phase"] == "ready" && c["entries"].as_array().map(Vec::len) == Some(entries))
        })
        .await,
        "{}",
        desk.conversation()
    );
    desk.conversation()
}

fn bodies(desk: &Desk) -> Vec<String> {
    desk.requests(&format!("{RUNTIME}/responses"))
        .into_iter()
        .map(|s| String::from_utf8_lossy(&s.body).into_owned())
        .collect()
}

fn gateway_keys(desk: &Desk) -> Vec<String> {
    desk.requests(&format!("{RUNTIME}/responses"))
        .into_iter()
        .filter_map(|s| s.header("x-portkey-api-key").map(str::to_owned))
        .collect()
}

fn history_files(root: &Path) -> Vec<std::path::PathBuf> {
    files_under(root)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "sealed"))
        .collect()
}

fn contains(path: &Path, needle: &str) -> bool {
    std::fs::read(path)
        .map(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
        .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn company_sign_in_verifies_both_tokens_and_keeps_only_the_refresh_token() {
    let (mut desk, idp) = desk().await;
    let id = signed_in_conversation(&mut desk, &idp).await;
    let c = desk.conversation();
    assert_eq!(c["source"], "companySignIn");
    desk.say(&id, FIRST).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    // The engine sent the verified access token, and nothing else.
    let keys = gateway_keys(&desk);
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].split('.').count(), 3);
    // Only the refresh token is kept, in the Keychain, as its own item.
    let state = format!("{:?}", desk.adapters.store.saved());
    assert!(!state.contains(&keys[0]) && !state.contains("refresh-"));
    let grant = &desk.adapters.store.saved().expect("doc").harness_sign_ins[0];
    let record = grant.grant.as_ref().expect("grant").record.clone();
    let secret =
        String::from_utf8(desk.adapters.vault.secret(&record).expect("item")).expect("utf8");
    assert!(secret.contains("refresh-1-"));
    assert!(!secret.contains(&keys[0]));
    // PKCE, nonce and state were checked by the provider and the callback;
    // the JWKS was read once.
    assert_eq!(idp.lock().expect("idp").jwks_reads, 1);
    assert!(!desk.json().to_string().contains(&keys[0]));
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tokens_or_providers_that_do_not_verify_are_refused_and_nothing_is_kept() {
    let (mut desk, idp) = desk().await;
    let reference = desk.choose(WS_A).await;
    configure(&mut desk, &reference, &idp).await;
    let cases: Vec<(&str, Break, &str)> = vec![
        (
            "forged signature",
            Box::new(|i| i.issue.forged = true),
            "invalidToken",
        ),
        (
            "other issuer",
            Box::new(|i| i.issue.access = json!({"iss": "https://evil.example/idp"})),
            "invalidToken",
        ),
        (
            "other audience",
            Box::new(|i| i.issue.access = json!({"aud": ["https://other.example"]})),
            "invalidToken",
        ),
        (
            "other client",
            Box::new(|i| i.issue.access = json!({"azp": "another-client"})),
            "invalidToken",
        ),
        (
            "expired",
            Box::new(|i| {
                i.issue.access = json!({"exp": idp::now() - 600, "iat": idp::now() - 1200})
            }),
            "invalidToken",
        ),
        (
            "not yet valid",
            Box::new(|i| i.issue.access = json!({"nbf": idp::now() + 600})),
            "invalidToken",
        ),
        (
            "issued in the future",
            Box::new(|i| i.issue.access = json!({"iat": idp::now() + 600})),
            "invalidToken",
        ),
        (
            "ID token nonce",
            Box::new(|i| i.issue.id = json!({"nonce": "replayed"})),
            "invalidToken",
        ),
        (
            "ID token for another client",
            Box::new(|i| i.issue.id = json!({"aud": "another-client"})),
            "invalidToken",
        ),
        (
            "opaque access token",
            Box::new(|i| i.issue.raw_access = Some("opaque-token-value".into())),
            "opaqueToken",
        ),
        (
            "unsigned",
            Box::new(|i| i.issue.raw_access = Some(unsigned())),
            "invalidToken",
        ),
        (
            "symmetric",
            Box::new(|i| i.issue.raw_access = Some(symmetric())),
            "invalidToken",
        ),
        (
            "another workspace",
            Box::new(|i| i.issue.access = json!({"portkey_workspace": "team-b"})),
            "wrongWorkspace",
        ),
        (
            "discovery names another issuer",
            Box::new(|i| i.metadata = json!({"issuer": "https://evil.example"})),
            "invalidConfig",
        ),
        (
            "an endpoint off the issuer",
            Box::new(|i| i.metadata = json!({"token_endpoint": "https://evil.example/token"})),
            "invalidConfig",
        ),
        (
            "no S256",
            Box::new(|i| i.metadata = json!({"code_challenge_methods_supported": ["plain"]})),
            "invalidConfig",
        ),
    ];
    let mut revoked = 0;
    for (name, break_it, failure) in cases {
        {
            let mut i = idp.lock().expect("idp");
            revoked += i.revoked.len();
            *i = Idp::new(i.issuer.trim_end_matches("/idp"), "team-a");
            break_it(&mut i);
        }
        let view = sign_in(&mut desk, &idp).await;
        assert_eq!(view["failure"], failure, "{name}: {view}");
        assert!(view["account"].is_null(), "{name}");
        let doc = desk.adapters.store.saved().expect("doc");
        assert!(doc.harness_sign_ins[0].grant.is_none(), "{name}");
    }
    // Every refresh token issued for a token refused after the exchange
    // (thirteen cases) was revoked.
    revoked += idp.lock().expect("idp").revoked.len();
    assert_eq!(revoked, 13);
    // A key the provider has not published yet: one more JWKS read, and
    // never a third.
    {
        let mut i = idp.lock().expect("idp");
        *i = Idp::new(i.issuer.trim_end_matches("/idp"), "team-a");
        i.rotated = Some(idp::SigningKey::new("c20-key-2"));
        i.rotate_after = u32::MAX;
        i.sign_rotated = true;
    }
    let view = sign_in(&mut desk, &idp).await;
    assert_eq!(view["failure"], "invalidToken", "{view}");
    assert_eq!(idp.lock().expect("idp").jwks_reads, 2);
    // A rotation: the new key appears on the second read and is accepted.
    {
        let mut i = idp.lock().expect("idp");
        i.jwks_reads = 0;
        i.rotate_after = 1;
    }
    let view = sign_in(&mut desk, &idp).await;
    assert!(!view["account"].is_null(), "{view}");
    assert_eq!(idp.lock().expect("idp").jwks_reads, 2);
}

fn unsigned() -> String {
    use base64::Engine as _;
    let e = |v: &Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
    format!(
        "{}.{}.",
        e(&json!({"alg": "none", "typ": "JWT"})),
        e(&json!({"iss": "x", "sub": SUBJECT}))
    )
}

fn symmetric() -> String {
    let claims = json!({"iss": "x", "aud": [AUDIENCE], "sub": SUBJECT, "exp": idp::now() + 600, "iat": idp::now()});
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    header.kid = Some("c20-key-1".into());
    jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(b"guessable"),
    )
    .expect("hs256")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_renewal_restarts_the_engine_and_restores_the_conversation() {
    let (mut desk, idp) = desk().await;
    // A twelve-second token is renewed six seconds in (room for a loaded
    // host to answer the first message before its margin).
    idp.lock().expect("idp").issue.lifetime = 12;
    let id = signed_in_conversation(&mut desk, &idp).await;
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    let first_pid = desk.last_pid().expect("pid");
    let history = desk.conversation()["historyId"].clone();
    tokio::time::sleep(Duration::from_secs(7)).await;
    desk.say(&id, SECOND).await.expect("sent");
    let c = answered_with(&mut desk, 4).await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert_eq!(c["id"], id);
    assert_eq!(c["historyId"], history);
    assert_eq!(c["entries"].as_array().map(Vec::len), Some(4), "{c}");
    // A new engine with the renewed token, and the old one is gone.
    let second_pid = desk.last_pid().expect("pid");
    assert_ne!(first_pid, second_pid);
    assert!(gone(first_pid).await);
    let keys = gateway_keys(&desk);
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
    assert_eq!(idp.lock().expect("idp").refreshes, 1);
    // The restored engine sent the first turn with the second.
    let bodies = bodies(&desk);
    assert!(bodies[1].contains("periwinkle"), "{}", bodies[1]);
    assert!(bodies[1].contains(SECOND));
    // Only one runtime home at a time, and none left.
    desk.end(&id).await;
    assert!(settle(|| desk.run_dirs().is_empty()).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_renewal_restarts_with_changes_only_once_the_operator_allows_them_again() {
    let (mut desk, idp) = desk().await;
    idp.lock().expect("idp").issue.lifetime = 12;
    // A project the Harness may change (C21).
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .canonicalize()
        .expect("fixture")
        .join(format!("airs-c21-renew-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("fixture");
    desk.adapters.folders.reply(Some(dir.clone()));
    let context = desk.context();
    desk.send(json!({"kind": "chooseHarnessProject", "context": context}))
        .await
        .expect("choose");
    assert!(settle(|| !desk.json()["harness"]["project"].is_null()).await);
    let context = desk.context();
    desk.send(json!({"kind": "setHarnessWrite", "context": context, "write": true}))
        .await
        .expect("write");
    let id = signed_in_conversation(&mut desk, &idp).await;
    assert_eq!(desk.conversation()["write"], true);
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    let first_pid = desk.last_pid().expect("pid");
    tokio::time::sleep(Duration::from_secs(7)).await;
    // The renewal waits for the operator: nothing restarts or is sent.
    desk.say(&id, SECOND).await.expect("sent");
    assert!(settle(|| desk.conversation()["phase"] == "confirmWrite").await);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(desk.last_pid(), Some(first_pid));
    assert_eq!(gateway_keys(&desk).len(), 1);
    assert_eq!(idp.lock().expect("idp").refreshes, 0);
    // Not now: the message comes back and the conversation stays.
    desk.send(json!({"kind": "confirmHarnessWrite", "conversation": id, "allow": false}))
        .await
        .expect("declined");
    assert!(settle(|| desk.conversation()["phase"] == "ready").await);
    let c = desk.conversation();
    assert_eq!(c["unsent"], SECOND, "{c}");
    assert_eq!(c["entries"].as_array().map(Vec::len), Some(2));
    // A confirmation that is not asked for is refused.
    assert!(
        desk.send(json!({"kind": "confirmHarnessWrite", "conversation": id, "allow": true}))
            .await
            .is_err()
    );
    // Allowed: the engine restarts with changes and the conversation.
    desk.say(&id, SECOND).await.expect("sent");
    assert!(settle(|| desk.conversation()["phase"] == "confirmWrite").await);
    desk.send(json!({"kind": "confirmHarnessWrite", "conversation": id, "allow": true}))
        .await
        .expect("allowed");
    let c = answered_with(&mut desk, 4).await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert_eq!(c["write"], true);
    assert_eq!(c["project"], dir.to_str().expect("utf8"));
    assert_ne!(desk.last_pid(), Some(first_pid));
    assert!(gone(first_pid).await);
    assert_eq!(idp.lock().expect("idp").refreshes, 1);
    assert!(bodies(&desk)[1].contains("periwinkle"));
    desk.end(&id).await;
    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_refused_renewal_gives_the_message_back_and_a_new_sign_in_continues() {
    let (mut desk, idp) = desk().await;
    idp.lock().expect("idp").issue.lifetime = 12;
    let id = signed_in_conversation(&mut desk, &idp).await;
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    idp.lock().expect("idp").refuse_refresh = true;
    tokio::time::sleep(Duration::from_secs(7)).await;
    desk.say(&id, SECOND).await.expect("admitted");
    assert!(
        settle(|| desk.conversation()["signInFailure"] == "sessionExpired").await,
        "{}",
        desk.conversation()
    );
    let c = desk.conversation();
    assert_eq!(c["unsent"], SECOND);
    assert_eq!(c["phase"], "ready");
    assert_eq!(c["entries"].as_array().map(Vec::len), Some(2));
    // Nothing was sent with the lapsed token.
    assert_eq!(gateway_keys(&desk).len(), 1);
    assert_eq!(sign_in_view(&desk)["failure"], "sessionExpired");
    // Signing in again as the same account keeps the grant: the same
    // conversation continues in a new engine with its history.
    idp.lock().expect("idp").refuse_refresh = false;
    let view = sign_in(&mut desk, &idp).await;
    assert!(view["failure"].is_null(), "{view}");
    assert_eq!(desk.conversation()["id"], id);
    desk.say(&id, SECOND).await.expect("sent");
    let c = answered_with(&mut desk, 4).await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert!(c["unsent"].is_null());
    let bodies = bodies(&desk);
    assert_eq!(bodies.len(), 2);
    assert!(bodies[1].contains("periwinkle"));
    desk.end(&id).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn another_account_or_signing_out_ends_the_grant_and_its_conversation() {
    let (mut desk, idp) = desk().await;
    let id = signed_in_conversation(&mut desk, &idp).await;
    let _ = list(&mut desk).await;
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let pid = desk.last_pid().expect("pid");
    let grant = desk.adapters.store.saved().expect("doc").harness_sign_ins[0]
        .grant
        .clone()
        .expect("grant");
    // Signing in again as someone else is refused and keeps the grant.
    idp.lock().expect("idp").issue.subject = OTHER_SUBJECT.into();
    let view = sign_in(&mut desk, &idp).await;
    assert_eq!(view["failure"], "differentAccount", "{view}");
    assert_eq!(desk.conversation()["id"], id);
    // Signing out ends the grant: the conversation is withdrawn and its
    // engine reaped, the refresh token revoked and its item deleted.
    let context = desk.context();
    desk.send(json!({"kind": "signOutHarness", "context": context}))
        .await
        .expect("signed out");
    assert!(settle(|| desk.conversation().is_null()).await);
    assert!(gone(pid).await);
    assert!(settle(|| !idp.lock().expect("idp").revoked.is_empty()).await);
    assert!(settle(|| desk.adapters.vault.secret(&grant.record).is_none()).await);
    // A new sign-in, even as the same account, is a new grant: the saved
    // conversation can be read but not continued.
    idp.lock().expect("idp").issue.subject = SUBJECT.into();
    let view = sign_in(&mut desk, &idp).await;
    assert!(!view["account"].is_null(), "{view}");
    let again = desk.adapters.store.saved().expect("doc").harness_sign_ins[0]
        .grant
        .clone()
        .expect("grant");
    assert_ne!(again.id, grant.id);
    let history = desk.json()["harness"]["history"]["conversations"][0]["id"]
        .as_str()
        .expect("saved")
        .to_owned();
    open(&mut desk, &history).await;
    assert_eq!(
        desk.json()["harness"]["history"]["opened"]["notContinuable"],
        "otherCredential"
    );
}

async fn open(desk: &mut Desk, id: &str) {
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(
        json!({"kind": "openHarnessHistory", "context": context, "workspace": workspace, "id": id}),
    )
    .await
    .expect("open");
    assert!(
        settle(|| desk.json()["harness"]["history"]["opened"]["id"] == id).await,
        "{}",
        desk.json()["harness"]["history"]
    );
}

async fn list(desk: &mut Desk) -> Value {
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(json!({"kind": "listHarnessHistory", "context": context, "workspace": workspace}))
        .await
        .expect("list");
    assert!(settle(|| desk.json()["harness"]["history"]["state"] != "loading").await);
    desk.json()["harness"]["history"].clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn history_is_sealed_after_every_turn_and_read_exported_and_deleted() {
    let (mut desk, _idp) = desk().await;
    let id = desk.ready(None).await;
    let _ = list(&mut desk).await;
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    desk.say(&id, SECOND).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("id")
        .to_owned();
    assert!(settle(|| desk.json()["harness"]["history"]["conversations"][0]["entries"] == 4).await);
    let view = desk.json()["harness"]["history"].clone();
    assert_eq!(view["conversations"][0]["id"], history.as_str(), "{view}");
    assert_eq!(view["conversations"][0]["entries"], 4);
    assert_eq!(view["conversations"][0]["title"], FIRST);
    // On disk: one sealed record, owner-only, with no plaintext of the
    // conversation or the key; its key is in the Keychain.
    let files = history_files(&desk.history_root);
    assert_eq!(files.len(), 1);
    for needle in ["periwinkle", FIRST, ANSWER, KEY, "airs-desktop"] {
        assert!(!contains(&files[0], needle), "{needle}");
    }
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&files[0])
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir = std::fs::metadata(files[0].parent().expect("dir"))
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(dir & 0o777, 0o700);
    }
    // While it runs it cannot be deleted.
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    assert!(desk
        .send(json!({"kind": "deleteHarnessHistory", "context": context, "workspace": workspace, "id": history}))
        .await
        .is_err());
    desk.end(&id).await;
    // Read, export (the transcript only) and delete.
    open(&mut desk, &history).await;
    let opened = desk.json()["harness"]["history"]["opened"].clone();
    assert_eq!(opened["entries"].as_array().map(Vec::len), Some(4));
    assert!(opened["notContinuable"].is_null(), "{opened}");
    desk.adapters.saver.script(SaveScript::Save);
    for format in ["markdown", "json"] {
        desk.send(
            json!({"kind": "exportHarnessHistory", "context": context, "workspace": workspace,
            "id": history, "format": format}),
        )
        .await
        .expect("export");
        assert!(settle(|| desk.json()["harness"]["history"]["export"]["state"] == "saved").await);
        desk.adapters.saver.script(SaveScript::Save);
    }
    let saved = desk.adapters.saver.saved();
    let markdown = String::from_utf8(saved[0].1.clone()).expect("utf8");
    assert!(
        saved[0].0.ends_with(".md")
            && markdown.contains("periwinkle")
            && markdown.contains("## Harness")
    );
    let exported: Value = serde_json::from_slice(&saved[1].1).expect("json");
    assert_eq!(exported["entries"].as_array().map(Vec::len), Some(4));
    assert!(exported.get("replay").is_none() && exported.get("context").is_none());
    desk.send(json!({"kind": "deleteHarnessHistory", "context": context, "workspace": workspace, "id": history}))
        .await
        .expect("delete");
    assert!(settle(|| desk.json()["harness"]["history"]["conversations"] == json!([])).await);
    assert!(desk.json()["harness"]["history"]["opened"].is_null());
    assert!(history_files(&desk.history_root).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_saved_conversation_continues_only_with_its_binding() {
    let (mut desk, _idp) = desk().await;
    let id = desk.ready(None).await;
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    let history = desk.conversation()["historyId"]
        .as_str()
        .expect("id")
        .to_owned();
    desk.end(&id).await;
    desk.send(json!({"kind": "clearHarness", "conversation": id}))
        .await
        .expect("cleared");
    let _ = list(&mut desk).await;
    open(&mut desk, &history).await;
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(
        json!({"kind": "resumeHarness", "context": context, "workspace": workspace, "id": history}),
    )
    .await
    .expect("resumed");
    assert!(
        settle_for(30, || desk.conversation()["phase"] == "ready").await,
        "{}",
        desk.conversation()
    );
    let c = desk.conversation();
    assert_eq!(c["historyId"], history.as_str());
    assert_eq!(c["entries"].as_array().map(Vec::len), Some(2));
    let resumed = c["id"].as_str().expect("id").to_owned();
    desk.say(&resumed, SECOND).await.expect("sent");
    let c = desk.answered().await;
    assert_eq!(c["lastTurn"], "completed", "{c}");
    assert_eq!(c["entries"][2]["id"], "u-2");
    let bodies = bodies(&desk);
    assert!(bodies[1].contains("periwinkle"), "{}", bodies[1]);
    desk.end(&resumed).await;
    // The same record was updated, not a new one made.
    assert_eq!(history_files(&desk.history_root).len(), 1);
    // Another key: it can be read, never continued with the new one.
    let reference = workspace.as_str().expect("reference").to_owned();
    desk.store_key(&reference, OTHER_KEY, None).await;
    let _ = list(&mut desk).await;
    open(&mut desk, &history).await;
    assert_eq!(
        desk.json()["harness"]["history"]["opened"]["notContinuable"],
        "otherCredential"
    );
    assert!(desk
        .send(json!({"kind": "resumeHarness", "context": context, "workspace": workspace, "id": history}))
        .await
        .is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn without_its_key_history_is_never_written_in_plaintext_and_reads_unreadable() {
    let (engine, pin) = fixture_engine();
    let (mut desk, _idp) = desk_on(Some(engine), Some(pin)).await;
    let id = desk.ready(None).await;
    let _ = list(&mut desk).await;
    // A locked Keychain: the turn is answered, nothing is written at all.
    desk.adapters
        .vault
        .fail_with(Some(desktop_core::vault::VaultError::Locked));
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    assert!(
        settle(|| desk.conversation()["saveFailure"] == "locked").await,
        "{}",
        desk.conversation()
    );
    assert!(history_files(&desk.history_root).is_empty());
    desk.adapters.vault.fail_with(None);
    desk.say(&id, SECOND).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    assert!(settle(|| desk.conversation()["saveFailure"].is_null()).await);
    assert_eq!(history_files(&desk.history_root).len(), 1);
    desk.end(&id).await;
    // Its key gone: the record is named unreadable, never rewritten, and
    // can be deleted.
    let key = desk
        .adapters
        .vault
        .records()
        .into_iter()
        .find(|r| desk.adapters.vault.secret(r).is_some_and(|s| s.len() == 32))
        .expect("history key");
    desk.adapters.vault.plant(&key, &[7u8; 32]);
    let view = list(&mut desk).await;
    assert_eq!(view["conversations"], json!([]), "{view}");
    assert_eq!(view["unreadable"].as_array().map(Vec::len), Some(1));
    let unreadable = view["unreadable"][0].clone();
    let context = desk.context();
    let workspace = desk.json()["gateway"]["selected"]["reference"].clone();
    desk.send(json!({"kind": "deleteHarnessHistory", "context": context, "workspace": workspace, "id": unreadable}))
        .await
        .expect("delete");
    assert!(settle(|| desk.json()["harness"]["history"]["unreadable"] == json!([])).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_workspace_refuses_new_history_and_never_evicts() {
    let (engine, pin) = fixture_engine();
    let (mut desk, _idp) = desk_on(Some(engine), Some(pin)).await;
    let reference = desk.choose(WS_A).await;
    desk.store_key(&reference, KEY, None).await;
    let _ = list(&mut desk).await;
    for n in 0..17 {
        let id = desk.start(&reference).await.expect("started");
        assert!(settle_for(30, || desk.conversation()["phase"] == "ready").await);
        desk.say(&id, &format!("conversation {n}"))
            .await
            .expect("sent");
        assert_eq!(desk.answered().await["lastTurn"], "completed");
        if n < 16 {
            assert!(
                settle(|| !desk.conversation()["savedAt"].is_null()).await,
                "{n}"
            );
        } else {
            assert!(settle(|| desk.conversation()["saveFailure"] == "full").await);
        }
        desk.end(&id).await;
        desk.send(json!({"kind": "clearHarness", "conversation": id}))
            .await
            .expect("cleared");
    }
    let view = list(&mut desk).await;
    assert_eq!(view["conversations"].as_array().map(Vec::len), Some(16));
    assert_eq!(view["full"], true);
    assert_eq!(history_files(&desk.history_root).len(), 16);
    assert!(
        view["conversations"]
            .as_array()
            .expect("list")
            .iter()
            .all(|c| c["title"] != "conversation 16")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_the_tenant_removes_its_history_its_key_and_its_sign_in() {
    let (mut desk, idp) = desk().await;
    let id = signed_in_conversation(&mut desk, &idp).await;
    desk.say(&id, FIRST).await.expect("sent");
    assert_eq!(desk.answered().await["lastTurn"], "completed");
    assert!(settle(|| !desk.conversation()["savedAt"].is_null()).await);
    desk.end(&id).await;
    assert_eq!(history_files(&desk.history_root).len(), 1);
    let before: Vec<RecordId> = desk.adapters.vault.records();
    let tenant = desk.json()["tenants"][0]["id"].clone();
    desk.send(json!({"kind": "deleteTenant", "tenant": tenant}))
        .await
        .expect("deleted");
    assert!(settle(|| history_files(&desk.history_root).is_empty()).await);
    // The history key and the refresh token item are both deleted.
    assert!(settle(|| desk.adapters.vault.records().len() + 2 <= before.len()).await);
    let doc = desk.adapters.store.saved().expect("doc");
    assert!(doc.harness_sign_ins.is_empty() && doc.harness_history.is_empty());
}
