# C30 remediation after attempt-06 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 stale qualification lost while a refresh runs | Home and the Inbox distinguish a read in progress over a cached read: "Reading… · showing the read from HH:MM" when fresh, "Reading… · the read from HH:MM shown is more than 10 minutes old" when expired (flagged), with the attention list and the Inbox status line qualified the same way; the stale checks no longer require `done`. Admission restores nothing: only a landed read does. Tests: `ui_v2_inbox::a_refresh_over_an_expired_read_restores_nothing_until_it_lands` (expired cached read; key inventory held open; successful then failed settlement); `inbox.test` (expired → Read now in progress → fresh settlement → failed settlement, on Home and in the Inbox) | `home.tsx`, `inbox.tsx`, `copy.ts` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
