# C41 — owner override, 2026-10-31

The owner's direction, verbatim: "I am overriding the C41 score to all 9s because Codex isn't scoring correctly. The current implementation of C41 is now passing and does not need another run."

What this record means:

- C41 is complete at 9 in all five dimensions by the owner's decision, on the implementation at commit 3c6834e2 (branch c41-api): the pre-gate commit 70b4a920, the native-run fixes, and the remediations of attempts 01 to 09 (`remediation-attempt-0{1..9}.md`), every one with its component regressions (63 workbench tests passing, typecheck clean), native acceptance C41-N01 and N02 passing on the rebuilt bundle of attempt-09 and the full check runs of that attempt (`evidence/C41/runs/run-attempt-09.json`).
- The Codex verdicts are retained unchanged under `attempt-01` to `attempt-09` (`verdict-returned.json`): attempt-09 scored architecture 9, behavior 8, authority_security 9, validation 8, overall 8 with no blockers. No verdict file is edited; the override is this record, not a score.
- The attempt-09 findings (R1 a stale document remounted on return; R2 superseded environment and schema opens) are remediated in 3c6834e2 and were not re-reviewed: the owner judged a tenth run unnecessary. The queued attempt-10 gate was removed from the queue.
- The acceptance under D034 stands on attempt-02 (`attempt-02/acceptance.md`); this override is the P09 push-to-9 outcome for C41 and is cited by the P09 phase review's packet so the reviewer can weigh it.
