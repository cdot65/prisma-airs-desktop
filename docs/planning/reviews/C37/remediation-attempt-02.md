# C37 remediation after attempt-02 (9/8/9/8/8, fail: one required fix, one evidence gap)

Attempt-02 passes the chunk bar (8 in every dimension, no blockers); its findings are remediated before acceptance rather than carried as accepted failures.

| Finding | Remediation | Where |
|---|---|---|
| R1 [P2] a settled partial read with nothing listed said "Nothing needs action." in both empty panes, also when the sessions source was denied and its activity unknown | The empty pane of All and Needs action qualifies a partial read: "Nothing needs action in what was read (n of m sources). Activity for <the unread sources> is unknown until it can be read." The bare "Nothing needs action." is said only when every source was read; the stale wording stays for an old complete read. The component test for the role-issue state expects the qualified text and rejects the bare all-clear; the native role-issue step asserts the qualified pane and that no bare all-clear text is on screen. | `apps/desktop/src/v2/copy.ts` (`emptyPartial`), `apps/desktop/src/v2/inbox/inbox.tsx`, `inbox.test.tsx`, `tools/native-acceptance/acceptance_c37.py`, `native-checks.json` C37-N01 |
| E1 `parity/bundle.json` named the attempt-01 gate bundle while the composites came from the rebuilt one | The native script rewrites `parity/bundle.json` with every capture (bundle path, executable hash, the script and the time), so the parity record is bound to the run that produced the composites; the registered check names it. | `acceptance_c37.py` (`composite`), `native-checks.json` C37-N01 |

## Commands and results

Filled in from the attempt-03 run (rebuild, native, the check run and the verdict under `reviews/C37/attempt-03/`).
