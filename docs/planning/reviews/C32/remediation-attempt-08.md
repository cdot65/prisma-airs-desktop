# C32 remediation after attempt-08 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 an admitted retry settling the unconfirmed original | An admitted send whose text matches an unconfirmed send awaiting the same conversation claims its own transcript entry (`harnessClaims`): claims take matching entries first, and an unconfirmed record settles only from a matching entry, added after its baseline, that no claim takes. One entry for the retry leaves the original "Not confirmed" (across renders and remounts); a further entry settles it. Test: "an admitted retry never settles the unconfirmed original" (the two-unknown, older-entry, cross-conversation and observation-before-receipt tests retained) | `store.ts`, `harness.tsx` |
| Optional | screens/harness.md and state-ownership.md describe the workspace-scoped composer and its kept messages (provenance P50) | docs |

The native C32-N04 Reduce Motion pass re-selects the workspace after its relaunch (AI Gateway's workspaces are read again) and asserts each step.

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
