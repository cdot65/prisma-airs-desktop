//! C04 Academy identity and learner transport through the production
//! authority interface (dispatch/observe) and the app-owned HTTP client,
//! against a synthetic Academy + Clerk endpoint on this Mac (real rustls and
//! the platform verifier). Request expectations are literal.

include!("harness.rs");

#[tokio::test(flavor = "multi_thread")]
async fn device_sign_in_is_a_public_client_flow_bound_to_the_academy_account() {
    let mut script = Script::new();
    script.polls = vec![
        (400, json!({"error": "authorization_pending"})),
        (200, bearer("access-1", "refresh-1", 3600)),
    ];
    let mut desk = desk_with(script, None).await;
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("start");
    assert!(eventually(|| matches!(desk.academy(), AcademyView::AwaitingApproval { .. })).await);
    let AcademyView::AwaitingApproval {
        user_code,
        verification_uri,
        expires_in,
    } = desk.academy()
    else {
        unreachable!()
    };
    assert_eq!(user_code, "WDJB-MJHT");
    assert!(verification_uri.ends_with("/activate?code=WDJB-MJHT"));
    assert!(expires_in > 590 && expires_in <= 600);
    desk.sign_in_wait().await;
    let AcademyView::SignedIn { display_name, .. } = desk.academy() else {
        unreachable!()
    };
    assert_eq!(display_name, "Ada Lovelace");

    let device = &desk.requests("/oauth/device")[0];
    assert_eq!(device.request_line.split(' ').next(), Some("POST"));
    assert_eq!(
        device.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    assert_eq!(
        form(device),
        vec![
            ("client_id".into(), CLIENT.into()),
            ("scope".into(), SCOPE.into())
        ]
    );
    let polls = desk.requests("/oauth/token");
    assert_eq!(polls.len(), 2, "one pending poll, then the grant");
    assert_eq!(
        form(&polls[0]),
        vec![
            ("client_id".into(), CLIENT.into()),
            (
                "grant_type".into(),
                "urn:ietf:params:oauth:grant-type:device_code".into()
            ),
            ("device_code".into(), "device-secret-1".into()),
        ]
    );
    for seen in desk.server.seen() {
        let trpc = seen.request_line.contains("/api/trpc/");
        assert_eq!(
            seen.header("authorization").is_some(),
            trpc,
            "{}",
            seen.request_line
        );
    }
    let me = &desk.requests("/api/trpc/desktop.me")[0];
    assert_eq!(me.header("authorization"), Some("Bearer access-1"));

    // The refresh token is only in the learner namespace; nothing
    // credential-bearing is in snapshots or the saved document.
    let saved = desk
        .adapters
        .store
        .saved()
        .expect("saved")
        .learner
        .expect("learner");
    assert_eq!(
        (saved.account_id.as_str(), saved.client_id.as_str()),
        ("user_2fixture", CLIENT)
    );
    let secret = desk
        .adapters
        .learner_vault
        .get(&saved.record)
        .expect("secret");
    assert_eq!(
        serde_json::from_slice::<Value>(&secret).expect("json"),
        json!({"refreshToken": "refresh-1"})
    );
    let shown = serde_json::to_string(&desk.snap()).expect("json")
        + &serde_json::to_string(&desk.adapters.store.saved()).expect("json");
    for secret in ["access-1", "refresh-1", "device-secret-1"] {
        assert!(!shown.contains(secret), "{secret} leaked");
    }
    assert!(
        desk.adapters.vault.calls().is_empty(),
        "the scan namespace is untouched"
    );
}

impl Desk {
    async fn sign_in_wait(&self) {
        assert!(
            eventually(|| matches!(self.academy(), AcademyView::SignedIn { .. })).await,
            "{:?}",
            self.academy()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_down_widens_the_poll_interval_by_five_seconds() {
    let mut script = Script::new();
    script.polls = vec![
        (400, json!({"error": "slow_down"})),
        (200, bearer("access-1", "refresh-1", 3600)),
    ];
    let mut desk = desk_with(script, None).await;
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("start");
    let started = std::time::Instant::now();
    assert!(
        eventually_for(Duration::from_secs(15), || matches!(
            desk.academy(),
            AcademyView::SignedIn { .. }
        ))
        .await
    );
    assert!(
        started.elapsed() >= Duration::from_secs(7),
        "1 s, then 6 s: {:?}",
        started.elapsed()
    );
    assert_eq!(desk.requests("/oauth/token").len(), 2);
}

async fn eventually_for(limit: Duration, check: impl Fn() -> bool) -> bool {
    let deadline = std::time::Instant::now() + limit;
    while std::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    check()
}

#[tokio::test(flavor = "multi_thread")]
async fn declined_expired_and_misconfigured_sign_ins_fail_explicitly() {
    use desktop_core::academy::AcademyFailure as F;
    type Setup = Box<dyn Fn(&mut Script)>;
    let cases: Vec<(Setup, F)> = vec![
        (
            Box::new(|s| s.polls = vec![(400, json!({"error": "access_denied"}))]),
            F::Denied,
        ),
        (
            Box::new(|s| s.polls = vec![(400, json!({"error": "expired_token"}))]),
            F::Expired,
        ),
        (
            Box::new(|s| {
                s.config_status = 503;
                s.config = json!({"error": "Desktop sign-in is not configured."});
            }),
            F::NotConfigured,
        ),
        (
            Box::new(|s| {
                s.config_status = 502;
                s.config = json!({});
            }),
            F::Network,
        ),
        (
            Box::new(|s| {
                s.config =
                    json!({"issuer": "http://localhost:1", "clientId": CLIENT, "scope": SCOPE})
            }),
            F::InvalidConfig,
        ),
        (
            Box::new(|s| {
                s.config = json!({"issuer": s.origin, "clientId": "bad id!", "scope": SCOPE})
            }),
            F::InvalidConfig,
        ),
        (
            Box::new(|s| {
                s.config = json!({"issuer": s.origin, "clientId": CLIENT, "scope": "openid offline_access"})
            }),
            F::InvalidConfig,
        ),
        (
            Box::new(|s| s.metadata_issuer = Some("https://evil.example".into())),
            F::InvalidConfig,
        ),
        (
            Box::new(|s| s.token_endpoint = Some("https://evil.example/oauth/token".into())),
            F::InvalidConfig,
        ),
        (
            Box::new(|s| {
                s.polls = vec![(
                    200,
                    json!({"token_type": "Bearer", "access_token": "a", "refresh_token": "r", "expires_in": 60, "scope": "openid"}),
                )]
            }),
            F::ScopeMissing,
        ),
        (
            Box::new(|s| {
                s.trpc = vec![(
                    "desktop.me".into(),
                    200,
                    json!({"result": {"data": {"json": {"accountId": "org_1", "displayName": "X"}}}}),
                )]
            }),
            F::InvalidResponse,
        ),
    ];
    for (setup, expected) in cases {
        let mut script = Script::new();
        let mut desk = {
            let ca = TestCa::new("airs-v2 c04 ca");
            let shared = Arc::new(Mutex::new(Script::new()));
            let routed = Arc::clone(&shared);
            let server = TlsServer::start(
                ca.leaf(&["localhost"]),
                Reply::Route(Arc::new(move |seen| {
                    routed.lock().expect("script").route(seen)
                })),
            )
            .await;
            script.origin = format!("https://localhost:{}", server.port);
            setup(&mut script);
            *shared.lock().expect("script") = script;
            start(ca, server, shared, None).await
        };
        desk.send(json!({"kind": "startAcademySignIn"}))
            .await
            .expect("start");
        assert!(
            eventually(|| matches!(desk.academy(), AcademyView::Failed { failure, .. } if failure == expected)).await,
            "expected {expected:?}, got {:?}",
            desk.academy()
        );
        assert!(
            desk.adapters
                .store
                .saved()
                .and_then(|s| s.learner)
                .is_none(),
            "{expected:?}: nothing saved"
        );
        if matches!(expected, F::ScopeMissing | F::InvalidResponse) {
            assert!(
                desk.requests("/oauth/revoke").is_empty() == matches!(expected, F::ScopeMissing),
                "a grant that cannot be bound is revoked"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_sign_in_revokes_a_late_grant_and_the_browser_opens_only_the_issued_page() {
    let mut script = Script::new();
    script.polls = vec![(400, json!({"error": "authorization_pending"}))];
    let mut desk = desk_with(script, None).await;
    assert_eq!(
        desk.send(json!({"kind": "openAcademyVerification"})).await,
        Err(Refusal::StaleContext)
    );
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("start");
    assert!(eventually(|| matches!(desk.academy(), AcademyView::AwaitingApproval { .. })).await);
    assert_eq!(
        desk.send(json!({"kind": "startAcademySignIn"})).await,
        Err(Refusal::Busy)
    );
    desk.send(json!({"kind": "openAcademyVerification"}))
        .await
        .expect("open");
    assert!(eventually(|| desk.adapters.opener.opened().len() == 1).await);
    assert!(desk.adapters.opener.opened()[0].ends_with("/activate?code=WDJB-MJHT"));
    // The grant arrives while the operator cancels: the poll is in flight.
    let polled = desk.requests("/oauth/token").len();
    {
        let mut script = desk.script.lock().expect("script");
        script.polls = vec![(200, bearer("late-access", "late-refresh", 3600))];
        script.poll_delay_ms = 1200;
    }
    assert!(
        eventually_for(Duration::from_secs(3), || desk
            .requests("/oauth/token")
            .len()
            > polled)
        .await
    );
    desk.send(json!({"kind": "cancelAcademySignIn"}))
        .await
        .expect("cancel");
    assert!(
        eventually_for(Duration::from_secs(4), || desk
            .requests("/oauth/revoke")
            .iter()
            .any(|s| form(s).contains(&("token".into(), "late-refresh".into()))))
        .await,
        "the late grant is revoked"
    );
    assert_eq!(desk.academy(), AcademyView::SignedOut);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(
        desk.academy(),
        AcademyView::SignedOut,
        "a late grant never signs in"
    );
    assert!(
        desk.adapters
            .store
            .saved()
            .and_then(|s| s.learner)
            .is_none()
    );
    assert!(
        desk.adapters
            .learner_vault
            .calls()
            .iter()
            .all(|c| !matches!(c, fixtures::VaultCall::Put(_)))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn learner_calls_pass_only_allowlisted_procedures_in_wire_form() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let data = json!({"json": {"id": "u1", "title": "Prompt injection", "updatedAt": "2026-10-01T00:00:00.000Z"},
                      "meta": {"values": {"updatedAt": ["Date"]}}});
    desk.script.lock().expect("script").trpc = vec![
        ("units.get".into(), 200, json!({"result": {"data": data}})),
        (
            "units.complete".into(),
            200,
            json!({"result": {"data": {"json": {"ok": true}}}}),
        ),
    ];
    let input = r#"{"json":{"unitId":"u1"}}"#;
    let result = desk
        .call("units.get", "query", Some(input))
        .await
        .expect("admitted");
    let AcademyCallResult::Data { data: returned } = result else {
        panic!("{result:?}")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&returned).expect("json"),
        data,
        "SuperJSON meta preserved"
    );
    let get = &desk.requests("/api/trpc/units.get")[0];
    let target = get.request_line.split(' ').nth(1).expect("target");
    let query: Vec<(String, String)> =
        url::form_urlencoded::parse(target.split_once('?').expect("query").1.as_bytes())
            .into_owned()
            .collect();
    assert_eq!(query, vec![("input".into(), input.into())]);
    assert_eq!(get.request_line.split(' ').next(), Some("GET"));
    assert_eq!(get.header("authorization"), Some("Bearer access-1"));

    let result = desk
        .call(
            "units.complete",
            "mutation",
            Some(r#"{"json":{"unitId":"u1"}}"#),
        )
        .await
        .expect("admitted");
    assert!(matches!(result, AcademyCallResult::Data { .. }));
    let post = &desk.requests("/api/trpc/units.complete")[0];
    assert_eq!(post.request_line.split(' ').next(), Some("POST"));
    assert_eq!(post.body, br#"{"json":{"unitId":"u1"}}"#);

    let learner = desk.learner();
    for (procedure, kind) in [
        ("admin.users.list", "query"),
        ("admin.feedback.list", "query"),
        ("units.get", "mutation"),
        ("units.complete", "query"),
        ("exam.grade", "mutation"),
    ] {
        assert_eq!(
            desk.call_as(&learner, procedure, kind, None).await.err(),
            Some(Refusal::CapabilityMissing),
            "{procedure} as {kind}"
        );
    }
    assert_eq!(
        desk.call_as(&learner, "units.get", "query", Some("{not json"))
            .await
            .err(),
        Some(Refusal::InvalidInput)
    );
    assert_eq!(
        desk.call_as("lx-000000000000000000000000", "units.get", "query", None)
            .await
            .err(),
        Some(Refusal::StaleContext)
    );
    let requests = desk.server.seen().len();
    assert_eq!(requests, desk.server.seen().len(), "refusals send nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn errors_and_private_fields_are_projected_without_leaking_truth() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let error = json!({"json": {"message": "Not enough credits", "code": -32600,
        "data": {"code": "BAD_REQUEST", "httpStatus": 400, "path": "shop.buy"}}});
    desk.script.lock().expect("script").trpc = vec![
        ("shop.buy".into(), 400, json!({"error": error})),
        (
            "units.get".into(),
            200,
            json!({"result": {"data": {"json": {"steps": [{"answerKey": 1}]}}}}),
        ),
        (
            "labs.get".into(),
            200,
            json!({"result": {"data": {"json": {"lab": {"rubric": {"x": 1}}}}}}),
        ),
        (
            "orientation.get".into(),
            200,
            json!({"result": {"data": {"json": {"script": {"alerts": [{"options": ["a", "b"], "answerKey": 1}]}}}}}),
        ),
        (
            "progress.get".into(),
            500,
            json!({"error": {"json": {"message": "boom", "code": -32603, "data": {"code": "INTERNAL_SERVER_ERROR", "httpStatus": 500}}}}),
        ),
    ];
    let result = desk
        .call("shop.buy", "mutation", Some(r#"{"json":{"itemId":"x"}}"#))
        .await
        .expect("admitted");
    let AcademyCallResult::Error {
        error: returned,
        http_status,
        outcome,
    } = result
    else {
        panic!("{result:?}")
    };
    assert_eq!(
        (
            serde_json::from_str::<Value>(&returned).expect("json"),
            http_status
        ),
        (error, 400)
    );
    assert_eq!(outcome, Outcome::Rejected);
    for procedure in ["units.get", "labs.get"] {
        let result = desk.call(procedure, "query", None).await.expect("admitted");
        assert_eq!(
            result,
            AcademyCallResult::Failed {
                failure: desktop_core::academy::CallFailure::InvalidResponse,
                outcome: Outcome::Unknown
            },
            "{procedure}: a server-private field never reaches the renderer"
        );
    }
    assert!(matches!(
        desk.call("orientation.get", "query", None)
            .await
            .expect("admitted"),
        AcademyCallResult::Data { .. }
    ));
    assert!(matches!(
        desk.call("progress.get", "query", None)
            .await
            .expect("admitted"),
        AcademyCallResult::Error {
            http_status: 500,
            ..
        }
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_token_is_renewed_once_and_only_queries_are_asked_again() {
    let mut desk = desk().await;
    let unauthorized = json!({"error": {"json": {"message": "UNAUTHORIZED", "code": -32001,
        "data": {"code": "UNAUTHORIZED", "httpStatus": 401}}}});
    desk.sign_in().await;
    desk.script.lock().expect("script").trpc = vec![
        ("progress.get".into(), 401, unauthorized.clone()),
        (
            "progress.get".into(),
            200,
            json!({"result": {"data": {"json": {"done": 3}}}}),
        ),
        ("units.complete".into(), 401, unauthorized.clone()),
    ];
    assert!(matches!(
        desk.call("progress.get", "query", None)
            .await
            .expect("admitted"),
        AcademyCallResult::Data { .. }
    ));
    let calls = desk.requests("/api/trpc/progress.get");
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[1].header("authorization"),
        Some("Bearer access-r1"),
        "renewed token"
    );
    let result = desk
        .call(
            "units.complete",
            "mutation",
            Some(r#"{"json":{"unitId":"u1"}}"#),
        )
        .await
        .expect("admitted");
    assert!(
        matches!(
            result,
            AcademyCallResult::Error {
                http_status: 401,
                ..
            }
        ),
        "{result:?}"
    );
    assert_eq!(
        desk.requests("/api/trpc/units.complete").len(),
        1,
        "a mutation is never resent"
    );
    assert_eq!(
        desk.requests("/oauth/token")
            .iter()
            .filter(|s| form(s).iter().any(|(k, _)| k == "refresh_token"))
            .count(),
        2
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_during_a_renewal_revokes_the_new_token_and_discards_the_reply() {
    let mut desk = desk().await;
    desk.sign_in().await;
    {
        let mut script = desk.script.lock().expect("script");
        script.refresh = vec![(200, bearer("access-new", "refresh-rotated", 3600))];
        script.refresh_delay_ms = 800;
        script.trpc = vec![(
            "progress.get".into(),
            401,
            json!({"error": {"json": {"message": "x", "code": -32001, "data": {"code": "UNAUTHORIZED", "httpStatus": 401}}}}),
        )];
    }
    let learner = desk.learner();
    let receipt = desk
        .send(json!({"kind": "academyCall", "learner": learner, "procedure": "progress.get", "procedureKind": "query", "input": null}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!()
    };
    assert!(
        eventually(|| desk
            .requests("/oauth/token")
            .iter()
            .any(|s| form(s).iter().any(|(k, _)| k == "refresh_token")))
        .await
    );
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("sign out");
    assert_eq!(desk.academy(), AcademyView::SignedOut);
    let result = desk.result(&operation).await;
    assert!(
        matches!(
            result,
            AcademyCallResult::Failed {
                failure: desktop_core::academy::CallFailure::Cancelled,
                ..
            }
        ),
        "{result:?}"
    );
    assert!(
        eventually(|| desk
            .requests("/oauth/revoke")
            .iter()
            .any(|s| form(s).contains(&("token".into(), "refresh-rotated".into()))))
        .await,
        "the rotated token is revoked"
    );
    assert!(
        desk.adapters
            .store
            .saved()
            .and_then(|s| s.learner)
            .is_none()
    );
    assert!(
        eventually(|| desk.adapters.learner_vault.records().is_empty()).await,
        "learner namespace purged"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_vault_never_signs_in_and_revokes_the_grant() {
    let mut desk = desk().await;
    desk.adapters
        .learner_vault
        .fail_with(Some(VaultError::Locked));
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("start");
    assert!(
        eventually(|| matches!(
            desk.academy(),
            AcademyView::Failed {
                vault: Some(VaultError::Locked),
                ..
            }
        ))
        .await,
        "{:?}",
        desk.academy()
    );
    assert!(eventually(|| !desk.requests("/oauth/revoke").is_empty()).await);
    assert!(
        desk.adapters
            .store
            .saved()
            .and_then(|s| s.learner)
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restart_resumes_the_learner_from_the_keychain_and_a_missing_record_needs_sign_in() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    drop(core);
    let mut desk = start(
        ca,
        server,
        Arc::clone(&script),
        Some((adapters, state.clone())),
    )
    .await;
    assert!(
        matches!(desk.academy(), AcademyView::SignedIn { .. }),
        "{:?}",
        desk.academy()
    );
    script.lock().expect("script").trpc = vec![(
        "progress.get".into(),
        200,
        json!({"result": {"data": {"json": {"done": 1}}}}),
    )];
    assert!(matches!(
        desk.call("progress.get", "query", None)
            .await
            .expect("admitted"),
        AcademyCallResult::Data { .. }
    ));
    assert_eq!(
        desk.requests("/api/trpc/progress.get")[0].header("authorization"),
        Some("Bearer access-r1"),
        "renewed from the saved refresh token"
    );

    // The saved record disappeared (deleted outside the app).
    let record = state.learner.clone().expect("learner").record;
    let _ = desk.adapters.learner_vault.delete(&record);
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    drop(core);
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    let result = desk
        .call("progress.get", "query", None)
        .await
        .expect("admitted");
    assert!(
        matches!(
            result,
            AcademyCallResult::Failed {
                outcome: Outcome::NotSent,
                ..
            }
        ),
        "{result:?}"
    );
    assert!(
        matches!(
            desk.academy(),
            AcademyView::Failed {
                failure: desktop_core::academy::AcademyFailure::SessionExpired,
                vault: Some(VaultError::NotFound),
                display_name: Some(_)
            }
        ),
        "{:?}",
        desk.academy()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tenant_switches_keep_the_learner_and_account_changes_reject_old_replies() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let learner = desk.learner();
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc = vec![(
            "progress.get".into(),
            200,
            json!({"result": {"data": {"json": {"done": 2}}}}),
        )];
        script.trpc_delay_ms = 700;
    }
    let receipt = desk
        .send(json!({"kind": "academyCall", "learner": learner, "procedure": "progress.get", "procedureKind": "query", "input": null}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(in_flight),
        ..
    } = receipt
    else {
        panic!()
    };
    for label in ["Tenant A", "Tenant B"] {
        desk.send(json!({"kind": "createTenant", "profile": {"label": label, "destination": {"kind": "region", "region": "europe"}}}))
            .await
            .expect("tenant");
    }
    let b = desk
        .snap()
        .tenants
        .iter()
        .find(|t| t.label == "Tenant B")
        .expect("B")
        .id
        .clone();
    desk.send(json!({"kind": "selectTenant", "tenant": b}))
        .await
        .expect("switch");
    assert_eq!(
        desk.learner(),
        learner,
        "tenant switches never change the learner"
    );
    assert!(
        matches!(
            desk.result(&in_flight).await,
            AcademyCallResult::Data { .. }
        ),
        "learner work survives a tenant switch"
    );

    // An account change withdraws a settled, unobserved reply of the old
    // account and retires work still in flight for it.
    {
        let mut script = desk.script.lock().expect("script");
        script.trpc_delay_ms = 0;
        script.trpc = vec![
            (
                "units.listByPath".into(),
                200,
                json!({"result": {"data": {"json": []}}}),
            ),
            (
                "progress.get".into(),
                200,
                json!({"result": {"data": {"json": {"done": 9}}}}),
            ),
        ];
    }
    let receipt = desk
        .send(json!({"kind": "academyCall", "learner": learner, "procedure": "units.listByPath", "procedureKind": "query", "input": null}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(unobserved),
        ..
    } = receipt
    else {
        panic!()
    };
    assert!(eventually(|| !desk.requests("/api/trpc/units.listByPath").is_empty()).await);
    tokio::time::sleep(Duration::from_millis(300)).await;
    desk.script.lock().expect("script").trpc_delay_ms = 1500;
    let receipt = desk
        .send(json!({"kind": "academyCall", "learner": learner, "procedure": "progress.get", "procedureKind": "query", "input": null}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(old),
        ..
    } = receipt
    else {
        panic!()
    };
    // The old account's link is already waiting for this result.
    let Ok(Observation::AcademyCall(waiting)) = desk.handle.observe(ViewScope::AcademyCall(old))
    else {
        panic!("observation")
    };
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("out");
    desk.sign_in().await;
    assert_ne!(desk.learner(), learner);
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(5), waiting.result())
            .await
            .expect("in time"),
        AcademyCallResult::Failed {
            failure: desktop_core::academy::CallFailure::Cancelled,
            ..
        }
    ));
    assert!(
        matches!(
            desk.handle.observe(ViewScope::AcademyCall(unobserved)),
            Err(Refusal::InvalidInput)
        ),
        "a settled reply of the previous account is withdrawn"
    );
    assert_eq!(
        desk.call_as(&learner, "progress.get", "query", None)
            .await
            .err(),
        Some(Refusal::StaleContext),
        "the old reference grants nothing"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reset_signs_the_learner_out_and_purges_its_namespace() {
    let mut desk = desk().await;
    desk.sign_in().await;
    assert_eq!(desk.adapters.learner_vault.records().len(), 1);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    assert!(eventually(|| desk.snap().storage == desktop_core::StorageHealth::Ready).await);
    assert_eq!(desk.academy(), AcademyView::SignedOut);
    assert!(desk.adapters.learner_vault.records().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_result_is_observed_once() {
    let mut desk = desk().await;
    desk.sign_in().await;
    desk.script.lock().expect("script").trpc = vec![(
        "progress.get".into(),
        200,
        json!({"result": {"data": {"json": {}}}}),
    )];
    let learner = desk.learner();
    let receipt = desk
        .send(json!({"kind": "academyCall", "learner": learner, "procedure": "progress.get", "procedureKind": "query", "input": null}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!()
    };
    assert!(matches!(
        desk.result(&operation).await,
        AcademyCallResult::Data { .. }
    ));
    assert!(matches!(
        desk.handle.observe(ViewScope::AcademyCall(operation)),
        Err(Refusal::InvalidInput)
    ));
}

fn shared_fixture(name: &str) -> String {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/desktop-contracts/fixtures/academy")
        .join(name);
    std::fs::read_to_string(path)
        .expect("fixture")
        .trim()
        .to_owned()
}

/// The same literal SuperJSON/envelope fixtures drive the TypeScript
/// desktop-link suite (apps/desktop/src/features/academy).
#[tokio::test(flavor = "multi_thread")]
async fn literal_superjson_envelopes_pass_through_unchanged() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let result: Value =
        serde_json::from_str(&shared_fixture("units-get.result.json")).expect("json");
    let error: Value = serde_json::from_str(&shared_fixture("shop-buy.error.json")).expect("json");
    desk.script.lock().expect("script").trpc = vec![
        ("units.get".into(), 200, result.clone()),
        ("shop.buy".into(), 400, error.clone()),
    ];
    let input = shared_fixture("units-get.input.json");
    let reply = desk
        .call("units.get", "query", Some(&input))
        .await
        .expect("admitted");
    let AcademyCallResult::Data { data } = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(
        serde_json::from_str::<Value>(&data).expect("json"),
        result["result"]["data"]
    );
    let get = &desk.requests("/api/trpc/units.get")[0];
    let target = get.request_line.split(' ').nth(1).expect("target");
    let query: Vec<(String, String)> =
        url::form_urlencoded::parse(target.split_once('?').expect("query").1.as_bytes())
            .into_owned()
            .collect();
    assert_eq!(
        query,
        vec![("input".into(), input)],
        "the serialized input is forwarded byte for byte"
    );
    let reply = desk
        .call("shop.buy", "mutation", Some(r#"{"json":{"itemId":"x"}}"#))
        .await
        .expect("admitted");
    let AcademyCallResult::Error {
        error: returned,
        http_status,
        outcome,
    } = reply
    else {
        panic!("{reply:?}")
    };
    assert_eq!(
        (
            serde_json::from_str::<Value>(&returned).expect("json"),
            http_status
        ),
        (error["error"].clone(), 400)
    );
    assert_eq!(outcome, Outcome::Rejected);
}

#[tokio::test(flavor = "multi_thread")]
async fn signing_out_after_a_restart_revokes_the_saved_refresh_token() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let state = desk.adapters.store.saved().expect("saved");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    drop(core);
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(matches!(desk.academy(), AcademyView::SignedIn { .. }));
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("out");
    assert!(
        eventually(|| desk
            .requests("/oauth/revoke")
            .iter()
            .any(|s| form(s).contains(&("token".into(), "refresh-1".into()))))
        .await,
        "the saved token is revoked although this launch never loaded it"
    );
    assert!(eventually(|| desk.adapters.learner_vault.records().is_empty()).await);
}

/// Releases held vault writes even if an assertion fails first, so a failing
/// test cannot hang on a blocked write.
struct Release(Arc<fixtures::ScriptedVault>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn saved_learner(desk: &Desk) -> Option<desktop_core::store::StoredLearner> {
    desk.adapters.store.saved().and_then(|s| s.learner)
}

fn secret_of(desk: &Desk, record: &desktop_core::vault::RecordId) -> Option<Value> {
    desk.adapters
        .learner_vault
        .get(record)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

/// RF01: a slow revocation of account A never removes B's credential.
#[tokio::test(flavor = "multi_thread")]
async fn a_delayed_revocation_never_removes_a_newer_sign_in() {
    let mut script = Script::new();
    script.polls = vec![
        (200, bearer("access-A", "refresh-A", 3600)),
        (200, bearer("access-B", "refresh-B", 3600)),
    ];
    let mut desk = desk_with(script, None).await;
    desk.sign_in().await;
    desk.script.lock().expect("script").revoke_delay_ms = 1500;
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("out");
    desk.sign_in().await;
    let b = saved_learner(&desk).expect("B saved").record;
    assert!(
        eventually_for(Duration::from_secs(4), || desk
            .requests("/oauth/revoke")
            .iter()
            .any(|s| form(s).contains(&("token".into(), "refresh-A".into()))))
        .await
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        eventually(|| desk.adapters.learner_vault.records() == vec![b.clone()]).await,
        "only B's record remains"
    );
    assert_eq!(
        secret_of(&desk, &b),
        Some(json!({"refreshToken": "refresh-B"}))
    );
    // B survives a restart and still works.
    let state = desk.adapters.store.durable().expect("durable");
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    drop(core);
    let mut desk = start(ca, server, script, Some((adapters, state))).await;
    desk.script.lock().expect("script").trpc = vec![(
        "progress.get".into(),
        200,
        json!({"result": {"data": {"json": {}}}}),
    )];
    assert!(matches!(
        desk.call("progress.get", "query", None)
            .await
            .expect("admitted"),
        AcademyCallResult::Data { .. }
    ));
    assert!(
        desk.requests("/oauth/token")
            .iter()
            .any(|s| form(s).contains(&("refresh_token".into(), "refresh-B".into())))
    );
}

/// RF01: a cancelled flow whose identity reply arrives late writes nothing
/// and deletes nothing; its grant is revoked.
#[tokio::test(flavor = "multi_thread")]
async fn a_superseded_flow_never_writes_or_removes_credentials() {
    let mut script = Script::new();
    script.polls = vec![
        (200, bearer("access-A", "refresh-A", 3600)),
        (200, bearer("access-B", "refresh-B", 3600)),
    ];
    script.me_delays = vec![1500];
    let mut desk = desk_with(script, None).await;
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("A");
    assert!(
        eventually_for(Duration::from_secs(4), || matches!(
            desk.academy(),
            AcademyView::Completing
        ))
        .await
    );
    desk.send(json!({"kind": "cancelAcademySignIn"}))
        .await
        .expect("cancel");
    desk.sign_in().await;
    let b = saved_learner(&desk).expect("B").record;
    tokio::time::sleep(Duration::from_millis(1800)).await;
    assert!(
        matches!(desk.academy(), AcademyView::SignedIn { .. }),
        "B stays signed in"
    );
    assert_eq!(saved_learner(&desk).map(|l| l.record), Some(b.clone()));
    assert!(eventually(|| desk.adapters.learner_vault.records() == vec![b.clone()]).await);
    assert!(
        desk.requests("/oauth/revoke")
            .iter()
            .any(|s| form(s).contains(&("token".into(), "refresh-A".into())))
    );
    assert!(
        !desk
            .adapters
            .learner_vault
            .calls()
            .iter()
            .any(|c| matches!(c, fixtures::VaultCall::Purge)),
        "no namespace purge"
    );
}

/// RF02: a crash after the Keychain write but before the account is saved
/// leaves a ledgered record that the next launch deletes.
#[tokio::test(flavor = "multi_thread")]
async fn a_crash_between_write_and_commit_leaves_no_untracked_credential() {
    let mut desk = desk().await;
    desk.adapters.learner_vault.hold_writes();
    let vault = Arc::clone(&desk.adapters.learner_vault);
    let _release = Release(Arc::clone(&vault));
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("start");
    assert!(
        eventually_for(Duration::from_secs(4), || vault.waiting() == 1).await,
        "write held"
    );
    let durable = desk.adapters.store.durable().expect("durable");
    assert!(durable.learner.is_none());
    assert_eq!(
        durable.learner_cleanup.len(),
        1,
        "write-ahead entry is durable before the write"
    );
    vault.release();
    assert!(
        eventually(|| vault.records().len() == 1).await,
        "the write landed"
    );
    // Crash: the next launch sees only the durable document.
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    drop(core);
    let desk = start(ca, server, script, Some((adapters, durable))).await;
    assert_eq!(desk.academy(), AcademyView::SignedOut);
    assert!(
        eventually(|| desk.adapters.learner_vault.records().is_empty()).await,
        "ledgered orphan deleted"
    );
}

/// RF02: an unconfirmed rotation never deletes the record a recovered
/// document may still name; a later confirmed save lets cleanup finish.
#[tokio::test(flavor = "multi_thread")]
async fn an_unconfirmed_rotation_keeps_the_previous_record_until_confirmed() {
    let mut script = Script::new();
    script.polls = vec![(200, bearer("access-1", "refresh-1", 1))];
    script.refresh = vec![(200, bearer("access-2", "refresh-2", 3600))];
    let mut desk = desk_with(script, None).await;
    desk.sign_in().await;
    let old = saved_learner(&desk).expect("saved").record;
    desk.script.lock().expect("script").trpc = vec![(
        "progress.get".into(),
        200,
        json!({"result": {"data": {"json": {}}}}),
    )];
    // Hold the new record's write so only the ownership-transfer save (after
    // it) is unconfirmed; the write-ahead save before it is confirmed.
    let vault = Arc::clone(&desk.adapters.learner_vault);
    let _release = Release(Arc::clone(&vault));
    vault.hold_writes();
    let learner = desk.learner();
    let receipt = desk
        .send(json!({"kind": "academyCall", "learner": learner, "procedure": "progress.get", "procedureKind": "query", "input": null}))
        .await
        .expect("admitted");
    let Receipt::Admitted {
        operation: Some(operation),
        ..
    } = receipt
    else {
        panic!()
    };
    assert!(eventually_for(Duration::from_secs(4), || vault.waiting() == 1).await);
    desk.adapters.store.unconfirm_next(1);
    vault.release();
    assert!(matches!(
        desk.result(&operation).await,
        AcademyCallResult::Data { .. }
    ));
    let new = saved_learner(&desk).expect("saved").record;
    assert_ne!(new, old, "rotated");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        desk.adapters.learner_vault.records().contains(&old),
        "kept while the transfer is unconfirmed"
    );
    assert_eq!(
        desk.adapters
            .store
            .durable()
            .and_then(|d| d.learner)
            .map(|l| l.record),
        Some(old.clone())
    );
    // A later confirmed save (any change) lets cleanup delete the old record.
    desk.send(json!({"kind": "setAppearance", "theme": "dark"}))
        .await
        .expect("save");
    assert!(eventually(|| !desk.adapters.learner_vault.records().contains(&old)).await);
    assert_eq!(
        secret_of(&desk, &new),
        Some(json!({"refreshToken": "refresh-2"}))
    );
}

/// RF02: a failed cleanup keeps the ledger entry and the next launch
/// finishes it.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_cleanup_is_finished_after_restart() {
    let mut desk = desk().await;
    desk.sign_in().await;
    let record = saved_learner(&desk).expect("saved").record;
    desk.adapters
        .learner_vault
        .fail_with(Some(VaultError::Unavailable));
    desk.send(json!({"kind": "signOutAcademy"}))
        .await
        .expect("out");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let state = desk.adapters.store.saved().expect("saved");
    assert!(
        state.learner.is_none() && state.learner_cleanup.contains(&record),
        "ledgered for later"
    );
    desk.adapters.learner_vault.fail_with(None);
    assert!(desk.adapters.learner_vault.records().contains(&record));
    let Desk {
        adapters,
        script,
        server,
        ca,
        core,
        ..
    } = desk;
    drop(core);
    let desk = start(ca, server, script, Some((adapters, state))).await;
    assert!(eventually(|| desk.adapters.learner_vault.records().is_empty()).await);
    assert!(
        eventually(|| desk
            .adapters
            .store
            .saved()
            .is_some_and(|s| s.learner_cleanup.is_empty()))
        .await
    );
}

/// RF02: reset waits for an admitted learner write and leaves no orphan.
#[tokio::test(flavor = "multi_thread")]
async fn reset_waits_for_a_held_learner_write() {
    let mut desk = desk().await;
    desk.adapters.learner_vault.hold_writes();
    let vault = Arc::clone(&desk.adapters.learner_vault);
    let _release = Release(Arc::clone(&vault));
    desk.send(json!({"kind": "startAcademySignIn"}))
        .await
        .expect("start");
    assert!(eventually_for(Duration::from_secs(4), || vault.waiting() == 1).await);
    desk.send(json!({"kind": "resetLocalData"}))
        .await
        .expect("reset");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !vault
            .calls()
            .iter()
            .any(|c| matches!(c, fixtures::VaultCall::Purge)),
        "no purge while a write is held"
    );
    vault.release();
    assert!(
        eventually_for(Duration::from_secs(4), || desk.snap().storage
            == desktop_core::StorageHealth::Ready)
        .await
    );
    assert!(vault.records().is_empty(), "no post-reset orphan");
    assert_eq!(desk.academy(), AcademyView::SignedOut);
}

/// RF03: error envelopes cross the same private-field boundary.
#[tokio::test(flavor = "multi_thread")]
async fn error_envelopes_with_private_fields_never_reach_the_renderer() {
    let mut desk = desk().await;
    desk.sign_in().await;
    desk.script.lock().expect("script").trpc = vec![
        (
            "exam.answer".into(),
            400,
            json!({"error": {"json": {"message": "Wrong", "code": -32600, "data": {"code": "BAD_REQUEST", "httpStatus": 400, "answerKey": 2}}}}),
        ),
        (
            "exam.submit".into(),
            500,
            json!({"error": {"json": {"message": "x", "code": -32603, "data": {"code": "INTERNAL_SERVER_ERROR", "httpStatus": 500, "detail": {"rubric": {}}}}}}),
        ),
        (
            "shop.buy".into(),
            500,
            json!({"error": {"json": {"message": "busy", "code": -32603, "data": {"code": "INTERNAL_SERVER_ERROR", "httpStatus": 500}}}}),
        ),
    ];
    for procedure in ["exam.answer", "exam.submit"] {
        let result = desk
            .call(procedure, "mutation", Some(r#"{"json":{}}"#))
            .await
            .expect("admitted");
        assert!(
            matches!(
                result,
                AcademyCallResult::Failed {
                    failure: desktop_core::academy::CallFailure::InvalidResponse,
                    ..
                }
            ),
            "{procedure}: {result:?}"
        );
    }
    // A server error says the mutation may have had an effect.
    let result = desk
        .call("shop.buy", "mutation", Some(r#"{"json":{}}"#))
        .await
        .expect("admitted");
    assert!(
        matches!(
            result,
            AcademyCallResult::Error {
                http_status: 500,
                outcome: Outcome::Unknown,
                ..
            }
        ),
        "{result:?}"
    );
}
