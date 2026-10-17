# Superseded native acceptance run (retained; it passed)

Run of `tools/native-acceptance/acceptance_c01.py` on 2026-10-17 (03:09–03:21 CDT,
fixture executable 20a746a3…): 13/13 passed and it was the native evidence for
Codex review attempt-03.

Superseded because the app changed afterwards: a test run then failed
intermittently (`treats appearance as Rust-owned`, 2 of 40 isolated runs)
because the shell applied the snapshot's theme in a passive effect, so the
shell could be committed before `data-theme` was set — a one-frame flash of
the previous theme in the app. The theme is now applied in a layout effect in
the same commit (0 of 120 isolated runs failed afterwards). The new
executable needs its own native acceptance.
