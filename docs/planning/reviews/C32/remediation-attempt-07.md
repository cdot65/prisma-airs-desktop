# C32 remediation after attempt-07 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 one entry settling two identical unconfirmed sends | Each transcript entry (`conversation|entry id`) settles at most one record, once: the store keeps the entries used (`harnessSettled`) and settles a record and uses its entry in one update (`settleHarnessUnsent`), so later renders and remounts find nothing more to settle. The box is cleared only when no unresolved record of the same text remains. Test: "one transcript entry settles one of two identical unconfirmed sends, across renders and a remount" (cross-conversation, older-entry and observation-before-receipt tests retained) | `store.ts`, `harness.tsx` |
| R2 the UI v2 accessibility additions not checked natively | C32-N02 now asserts the guard as the one topmost modal (rail and page hidden, initial focus Stay) and Escape returning focus to the switcher; C32-N03 the ended conversation in a status region; C32-N04 the focus ring on a changed control (pixel diff, reusing C31's helper) and, after a Reduce Motion relaunch, the ended conversation's toast persisting past 6 s until dismissed with the setting restored. The Harness now confirms an ended conversation with a toast bound to its tenant and context (I10). Test: "confirms an ended conversation with a toast bound to its tenant and context" | `acceptance_c32.py`, `native-checks.json`, `harness.tsx` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
