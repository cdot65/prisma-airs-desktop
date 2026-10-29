# C35 remediation after attempt-02 (9/8/9/8/8, fail: one required fix)

| Finding | Remediation | Where |
|---|---|---|
| R1 the arrival effect re-applied on every fresh snapshot | Administration consumes each arrival once, by its sequence, when its target is in the inventory: a product (and tab) named by the rail or the tenant menu, or a Runtime inventory named by an Inbox or Home action (the C30 effect had the same dependency and now shares the guard). A fresh snapshot with new inventory arrays never reopens a collapsed product or reselects a tab the operator left; a new arrival selects and opens its target again. Test: the R3 test now selects Routes and collapses AI Gateway after the workspace handoff, proves both survive two fresh snapshots, and that the next handoff reopens Workspaces | `admin/administration.tsx`, `v2-shell.test.tsx` |
| Optional: N01 captures named by an assumed appearance | The N01 captures select the light appearance explicitly before capturing; the layout matrix was already selecting each theme | `acceptance_c35.py` |

Attempt-03's first check runs failed `core-fixtures` three times on the same inputs, each time on a different timing-sensitive test outside this chunk (`academy_identity` 4 s waits, `workbench_documents`, `ui_v2_reviews`; the flake logs are retained), while the owner's browser and a second session shared the Mac. The check runner now sets `RUST_TEST_THREADS=4` for the cargo test runs (recorded in the run record's environment): the same tests and assertions, less contention. No test was changed for this.

