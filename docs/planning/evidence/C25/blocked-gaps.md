# C25 blocked-gap report (2026-10-25)

Every required workflow row the live run could not pass, from
`docs/architecture/workflow-ledger.json` (`xtask ledger --record`, derived
from the receipts under `live/` and `live-scope.json`). A blocked row is not
passed and not "not applicable": per the [C25 contract](../../chunks/C25.md)
it blocks stable completion until it passes or the owner records a reduction.
The owner reduced every blocked check (D052, D053, D055) and deferred the Harness-run checks to C26 (D055).

Totals over the latest receipt of every check: 73 passed, 7 blocked,
3 native app. Rows: 42 of 54 required rows passed live; 12 rows need no
live evidence.

## Reduced by the owner

Nothing required remains blocked: each check below was withdrawn from C25
by the owner.

| Row | Check | Cause | Decision |
|---|---|---|---|
| W14 | L04-data-profile-replace | The DLP service answers HTTP 400 to every data profile and data pattern read or write by ID, predefined profiles included, while its list works (`reconciliation.jsonl`, L04, 17:56Z) | [D053](../../decisions.md) |
| W15 | L04-filtering-profile-replace | The SDK request contract allows `direction` BOTH, UPLOAD or DOWNLOAD and an upper-case `log_severity`; the service stores `c2s`/`s2c` and `low`, and accepted an unchanged write-back with those values (`reconciliation.jsonl`, L04 diagnostics). SDK follow-up | [D053](../../decisions.md) |
| W37, W39 | L08-provision-workspace, L08-cleanup-workspace | IAM refuses the workspace binding with HTTP 400 `invalid resource_type: workspace`, although the request matches every existing binding and the service account is superuser. The rest of L08 passed live | [D052](../../decisions.md) |
| W48 | L17-scope-change | Changing the shared `strata-cloud-manager` server's scope or grant is outside the `airs-v2-c25-*` change scope | [D053](../../decisions.md) |
| W49 | L11-account-switch | No second Academy test learner | [D053](../../decisions.md) |
| W56 | L14-access-change-denied | The designated course is public; denial needs an entitled course whose access is withdrawn during the run | [D053](../../decisions.md) |
| W56 | explicit lab submit | It would leave a graded submission on the owner's Academy account | [D055](../../decisions.md) |

## Deferred to C26 by the owner

| Row | Check | Why | Decision |
|---|---|---|---|
| W48 | L17-approved-read-call, L17-denied-call, L17-refresh | MCP tool calls, their native approval or denial, and refresh run inside the Harness engine, only in the signed app | [D055](../../decisions.md) |
| W42–W45 | L16 (not run) | The Harness with Gateway inference and company sign-in runs in the signed app | [D055](../../decisions.md) |
| W63 | final artifact | Owned by C26 | — |

## Reconciliation

`airs-v2-c25-profile`, an inactive Redtail profile from one run, cannot be
listed or read by the API or found in the console. Its cleanup was waived
by the owner ([D055](../../decisions.md); `reconciliation.jsonl`, status
`waived`). Every other resource the runs created was deleted and read back
absent.
