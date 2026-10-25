# C25 controlled live scope: what the owner must designate

C25 cannot start without an owner-approved execution manifest
(`release-contract.md`, "Controlled live manifest before C25";
`live-checks.md`). None exists. This file lists what the manifest must name.
It is not the manifest and authorizes no calls.

Secrets never go in this repository or in a prompt. The private manifest lives
outside the repository. It names each secret by where it is stored (a Keychain
service and account under a v2 test service, or a file path readable only by the
operator), never by value. Public evidence records names and references only
(`checks.json`, `signing_and_live`).

The workflow ledger (`docs/architecture/workflow-ledger.json`) marks 54 of 66
rows `required`. Each row maps to these families. A family without designated
access is recorded **blocked**, never not-applicable. A reduction needs a
recorded owner decision and a revised review.

| Family | Rows | What to designate |
|---|---|---|
| L01 Runtime scan | W01 W02 W04 W05 W06 W22 | A test tenant's scan API key, region/endpoint, and an AI profile or application to scan against. A second key that is revoked or denied. The maximum number of scans. |
| L02 Management reads | W02 W07–W10 W12 | A confidential management OAuth client (client ID, secret reference, TSG) for that tenant, least-privileged. A second, more restricted principal for the denial case. |
| L03 Runtime changes | W07–W11 | Permission to create, update and delete disposable profiles, topics, applications and API keys under a name prefix you choose (for example `airs-v2-c25-`), including key versions and rotation. |
| L04 DLP | W12–W16 | Which DLP operations are allowed on that tenant: data patterns, data profiles, filter profiles, dictionaries (multipart upload, replace, delete). Disposable names. |
| L05 Sessions | W17–W19 | Synthetic scan sessions only. They can be the ones L01 creates, or a seeded set you name. No customer session content. |
| L06 Red Team, Model and skill reads | W23 W24 | Existing seeded test records to read. No new adversarial or model scans. |
| L07 Gateway reads | W34–W36 | A Gateway test workspace and the credential to read it. A restricted principal for the denial case. |
| L08 Gateway changes | W37–W39 | Permission to make reversible configuration, provider, guardrail, key and limit changes, plus multi-step workspace/IAM provisioning, on disposable resources. Cleanup expectations. |
| L09 Gateway inference | W40 | A routed model in that workspace, an inference credential and a token or cost budget for streaming, cancel and error cases. |
| L10 Operational data | W21 W41 | The Runtime and Gateway Home, Inbox, log, telemetry, usage and cost sources to observe (designated data). |
| L11 Academy identity | W49 | An Academy staging environment and two learner test accounts (for switch and revoke), with the device sign-in client. No manager authority. |
| L12 Academy learning | W50–W52 W65 | Synthetic coursework for knowledge checks, labs and progress with hosted grading. |
| L13 Academy assessment and account | W52–W54 W65 W66 | Staging exam/practical/certificate data and a test account whose preferences, avatar and export may be changed. No real certificates. |
| L14 Academy local work | W55 W56 | Downloadable lease content, and a way to change one account's access during the test (to prove denial). |
| L15 API Client | W27 W29–W33 | One HTTPS endpoint you control for headers, cookies, uploads and downloads (an echo service is enough), plus the approved AIRS read and reversible-write requests. |
| L16 Harness | W42–W45 | The Gateway inference path for the native engine, a scoped key, and a company SSO identity provider (issuer, client, Gateway audience) with a test user. |
| L17 Workspace MCP | W47 W48 | A Gateway MCP server with OAuth sign-in, a read-only tool to approve, a call to deny, and a way to refresh, revoke and change scope or grant. |
| L18 Scope isolation | W03 W32 | A second, distinct tenant, workspace and credential set, to prove calls go only to the intended destination. Not production, and not the same scope relabelled. |

Also needed for every family:
- named environments (staging or test, never production);
- explicit irreversible exclusions;
- consumption limits (scans, inference tokens or cost, assessments);
- expected effects;
- the cleanup and readback each mutation needs;
- who to contact if a resource is left in an uncertain state.

C26 (signed package) needs separately:
- **Signing identity.** A Developer ID Application identity for team G5QLZ5A8TA
  is installed. Using it for this release needs the owner's go-ahead.
- **Notarization.** Credentials stored as a `notarytool` Keychain profile. Give
  the profile name only.
- **A clean account.** A fresh non-developer macOS user account, or a second
  Mac, for the quarantined install tests.
- **The F01–F09 smoke.** These tests reuse the C25 scope.
