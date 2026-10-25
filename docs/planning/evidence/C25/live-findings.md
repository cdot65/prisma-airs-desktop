# C25 controlled live run: findings (2026-10-25)

Scope: the owner-approved private manifest (`~/airs-v2-live.json`, outside the
repository; its hash is in every receipt). Two test tenants: Redtail (TSG
1001464285, environment test-a) and Vulture (TSG 1852583913, test-b). Changes
were made only in Redtail, under the `airs-v2-c25-` prefix. Receipts:
`live/<family>/<check>.json` (redacted). Earlier runs' receipts:
`live/history/`. Consumption: `live/consumption.jsonl`. Reconciliation:
`live/reconciliation.jsonl`.

## Results by family

| Family | Live result | Open |
|---|---|---|
| L01 Runtime scan | 2/2 passed | — |
| L02 Management reads | 5/5 passed, including the read-only principal's refused write | — |
| L03 Runtime changes | 10/10 passed: topic, profile, key create and rotate, application update; every cleanup read back absent | — |
| L04 DLP | 7 passed: data pattern create, replace and delete; dictionary multipart create, replace and delete; deployment catalog | Data-profile write-back: blocked by the service (HTTP 400 to every data profile and data pattern read or write by ID, predefined included; the list works). Filtering-profile write-back: blocked by the SDK request contract, which allows direction BOTH, UPLOAD or DOWNLOAD and an upper-case `log_severity`, while the service stores `c2s`/`s2c` and `low` (SDK follow-up; the service accepted an unchanged write-back in a diagnostic). Target: the scratch `test123` profiles, designated by the implementer under the owner's delegation. Both withdrawn from C25 by the owner ([D053](../../decisions.md)) |
| L05 Sessions | 4/4 passed on the run's own synthetic session (app `airs-v2-c25`) | — |
| L06 Red Team, Model and skill reads | 3/3 passed on existing records; no scan was started | — |
| L07 Gateway reads | 4/4 passed, including the read-only principal's refused write, read back over the complete service key list | — |
| L08 Gateway changes | 6 passed: route create, update and delete; service key create, rotate and delete | Workspace provisioning: IAM refuses the binding (below); withdrawn from C25 by the owner ([D052](../../decisions.md)) |
| L09 Gateway inference | 3/3 passed: stream, operator Stop (charge reported unknown), refused unknown model with no fallback | — |
| L10 Operational observation | 2/2 passed | — |
| L18 Scope isolation | 3/3 passed: L01's key is refused for L18's profile ID (400 "AI Profile not found") | — |
| L11 Academy identity | device sign-in approved by the owner in the browser, the core showing the learner signed in; protected learner calls admitted; sign-out deleted the session, revoked the refresh token and refused the old learner | Account switch: no second test learner; withdrawn from C25 by the owner ([D053](../../decisions.md)) |
| L12 Academy learning | 16/16 passed, each naming the workflow it is evidence for: W50 course, path, handbook and unit reads; W51 labs, lab attempts, a hosted-graded inline check, the progress summary; W52 gamification, notifications, mark all read; W65 Daily Incident, board, Shift history and eligibility | — |
| L13 Academy assessment and account | 10/10 passed: W53 certifications, exam eligibility, certificates, identity-practical status; W54 settings, a hint marked seen, a synthetic avatar re-encoded, uploaded and removed; W66 the affected-course check, a reset of Application Architecture and the summary after it | — |
| L14 Academy local work | 2 passed: the public Application Architecture course downloaded under its lease and sealed for the account; a synthetic local draft sealed, read back and cleared (nothing submitted) | Access-change denial: withdrawn ([D053](../../decisions.md)); explicit lab submit: withdrawn ([D055](../../decisions.md)) |
| L15 API Client | 7/7 passed against the public echo service `https://httpbin.org` (chosen by the implementer under the owner's delegation; synthetic data and a synthetic bearer token only): headers and query, cookies, multipart upload, download saved exactly through the save sheet, stored secret; an approved AIRS read; a reviewed AIRS write (a disposable `airs-v2-c25-wb-topic`) held for review, sent once after the native confirmation, then deleted by the ID its response reported. An earlier run against the owner-approved `https://academy.cdot.io` passed the download and both AIRS checks; that site does not echo requests (`live/history/`) | — |
| L16 Harness | not run | Deferred to C26 by the owner ([D055](../../decisions.md)): it runs in the signed app with company sign-in |
| L17 Workspace MCP | 3 passed: discovery (the catalog lists `strata-cloud-manager`, 8a969a23-9dc6-40b0-9e15-1795d3711848, found by name); OAuth sign-in through the browser with PKCE and a loopback callback, the desktop holding the grant; sign-out revoked the tokens and deleted their item | Scope change: outside the `airs-v2-c25-*` change scope; withdrawn from C25 by the owner ([D053](../../decisions.md)). Approved call, denied call and refresh: deferred to C26 by the owner ([D055](../../decisions.md)) |

## Product defects found by the live run, fixed

1. **Gateway chat inference sent `max_tokens`.** Current OpenAI models refuse
   it (HTTP 400 `unsupported_parameter`). Chat now sends
   `max_completion_tokens` (`gateway_inference.rs`, with its unit and
   integration tests). Not verifiable live for other providers: the
   designated key reaches only the OpenAI integration.
2. **An application update never carried the application's deployment
   `auth_code`.** The service refuses it (HTTP 400 "auth_code is
   required"). The review now looks the code up from the deployment profile
   the application record names (`api_keys_dp_info[].dp_name`); the code is
   never taken from the draft and every listed entry must name the same
   profile (`changes::with_app_auth_code`, unit and integration tests).
3. **A deleted DLP item read back as present.** The DLP service archives a
   deleted item: it stays readable by its ID with `status: deleted` and an
   `_archived_<time>` name. The readback now counts such an item absent, and
   a review of it is refused as not found (`dlp_subset` test).
4. **A data filtering profile could not be edited as read.** The Edit
   action starts from the item as the service returns it: unset fields as
   null, plus the service's own fields (`id`, `name`, `tenant_id`, `type`,
   `version`, `audit_metadata`, `is_parent_managed`). The request contract
   refuses the nulls, so even an unchanged edit was refused. The review now
   drops the nulls and leaves out the service's own fields; the service
   keeps both when they are left out (verified live)
   (`changes::normalize`, unit test). The `direction` enum mismatch remains
   (SDK follow-up, above).
5. **No Academy sign-in could complete.** Academy's `desktop.me` names the
   account by its own user ID, a UUID; the core accepted only Clerk-style
   `user_` IDs, so it refused every real sign-in after the learner approved
   it (and revoked the token). The core now accepts the UUID, in the
   sign-in and in the saved learner record (`academy::is_account_id`, unit
   test; the runner's fake Academy now returns UUIDs).

6. **The live check itself was answered from the Gateway's cache.** Runs
   after the first repeated L09's prompt, and the Gateway replayed its
   cached answer as one chunk with no `finish_reason`; the SDK's stream
   contract requires it, so the run failed as an invalid response
   (diagnosed from the raw stream). L09 now sends a prompt unique to each
   run. The app's behaviour on a cached answer is pinned by a test
   (`a_cached_gateway_answer_without_finish_reason_is_an_invalid_response`)
   and is an SDK follow-up: a workspace with caching on, asked the same
   prompt again, gets an invalid response. A replay of a live uncached
   stream (`service_tier`, `obfuscation`, null `refusal`, usage in a last
   chunk without choices, delivered in small pieces) completes.

The app now builds against scm-sdk 0.2.4 (owner direction, D054); every
family that needs no operator was run again on it.

## Service behaviour recorded (no product change)

- The AIRS profile inventory omits inactive profiles. An inactive profile
  cannot be read back or listed afterwards, and no endpoint finds it by name.
  An inactive `airs-v2-c25-profile` from one run is left in Redtail and
  needs the owner to delete it from the console (`reconciliation.jsonl`,
  open). The owner could not find it in the console either, and the
  inventory still lists no `airs-v2-c25-*` profile; being inactive, it is
  not used by any scan. The runner now creates active profiles. The app's change review
  now says, when a profile is saved inactive, that the service will not list
  it or read it back (`changes::impact`, unit test).
- Rotating an API key replaces its `api_key_id`; deleting an application's
  last key deletes the application.
- IAM scope names allow only letters, digits and underscores. The SDK's
  name check accepts hyphens (SDK follow-up; the SDK is read-only here).
- IAM refuses `resource_type: workspace` bindings from the designated
  service account (on PUT and on POST), although the tenant's other scopes
  carry them. The app's provisioning reported the partial result (scope and
  workspace created, binding refused), kept both and asked for
  reconciliation, as designed; both were then deleted.
- IAM scope DELETE works live (the SDK marks it experimental).
- The read-only principal (`desktop-read-only@…`) can read everything, so
  least privilege is shown by refused writes (L02 topic, L07 service key),
  each read back over the complete inventory.

## Consumption (cumulative, including the implementer's diagnostics)

| Budget | Used | Cap |
|---|---|---|
| L01 scans | 14 | 20 |
| L18 scans | 16 | 16 (raised from 10 to rerun L18 on scm-sdk 0.2.4 and after the implementation binding, under the owner's delegation) |
| L09 tokens | 124 | 50,000 |
| L09 runs | 39 | 60 |
| L12 calls | 16 | 40 |
| L13 calls | 11 | 40 |

Diagnostics outside the runner (direct scans, model probes, profile, key and
application probes, IAM and DLP reads) are recorded as operator notes in the
ledger or the reconciliation log. Each diagnostic resource was deleted by ID.
