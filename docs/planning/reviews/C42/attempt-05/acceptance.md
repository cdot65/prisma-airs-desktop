# C42 attempt-05 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C41's pre-gate commit 70b4a920 at 1ad32fe2): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, two required fixes, one evidence gap.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance; the ledger entry is written when the P09 chunks are run through `xtask review`/`xtask complete` (the controller's predecessor gate refuses P09 subjects until C26 completes; see docs/planning/status.md).

Accepted failures, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: the active Academy navigation link (`aria-current='page'` on an accent-soft surface) takes the shell's ink-on-accent focus ring meant for the accent-filled sidebar destination: #ffffff on #fbf8f1 (1.06:1) in light, #1d2021 on #282828 (about 1.11:1) in dark. Active and inactive Academy links must keep a focus indicator of at least 3:1 against the adjacent surfaces in both themes without regressing the sidebar's indicator (DESIGN-V2 section 8).
- R2: N02's ring assertion counts changed pixels between a capture with the adjacent Home link focused and one with Learn focused, so the neighbour's ring disappearing passes a low-contrast ring on the target (262 of the 310 counted pixels lie left of Learn's box; the ring's own change is 25 RGB levels, under the >60 threshold). Isolate the target's indicator from other focus changes and verify its contrast; the retained failing sample must be rejected.
- E1: no retained native evidence establishes an accessible Academy focus indicator; supply rebuilt, source-bound selected and unselected Academy navigation focus evidence in both themes after R1.

Optional suggestions (the stacked frame at 900×600/200%, a mounted appearance-change regression, `tagline` in the Rust paths.list fixture) are noted for the same generation.
