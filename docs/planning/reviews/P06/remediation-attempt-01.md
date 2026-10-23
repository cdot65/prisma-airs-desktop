# P06 remediation after attempt-01

Phase attempt-01 scored 9/8/9/8/8 with one required fix and its evidence
gap, both at the C19 lifecycle and C20 persistence boundary.

## F1 and G1: a finished turn whose export is pending survives a stop

**Before.** After a turn ended, the desktop asked the engine for its export
and saved the record only when that answer came. A stop (sleep, End or
Quit) in that window returned at once. The stop-time save saw neither an
open turn nor a streaming entry, so nothing was saved:
- on a first turn, the conversation was lost;
- otherwise, its latest turn and any tool outcome were lost.

**Now** (`crates/desktop-core/src/harness/session.rs`):
- On a stop, the deadline or the end of commands, a pending export is
  waited for for up to 3 s (`finish_export`, `EXPORT_GRACE`). Only frames
  are handled meanwhile. When it comes, the turn is saved with it and stays
  continuable.
- If it does not come, the stop-time save treats the turn as unsaved
  (`turn_open` also covers a pending export). The transcript as shown is
  saved without a restore point, so the record reads `notRestorable`. It is
  never paired with an older export.
- An export that never answers within its 15 s bound fails the
  conversation as before (`timeout`). Its finished turn is now kept the
  same way, not lost.

**Evidence.**
- `a_finished_turn_whose_export_is_pending_is_kept_when_stopped`
  (`harness_identity_history`, stand-in modes `history-silent` and
  `history-silent-later`) runs on a first turn and on a conversation
  already saved:
  - the turn ends and is shown, its export never comes, and the Mac
    sleeps;
  - the record is written (a new save time);
  - offline, the record holds exactly the entries shown and reads
    `notRestorable`;
  - reconnected, continuing is refused, and no engine starts.
- `a_finished_turn_stopped_before_its_export_is_saved_with_it` (real
  engine): the Mac sleeps right after a turn ends. The export arrives
  within the grace, the record is saved, and it is continuable.
- `refused_or_unanswered_exports_and_refused_restores_are_named` now
  expects the unanswered-export conversation's turn to be kept,
  `notRestorable`.
- Probe (C20 set): `a finished turn whose export is pending is not saved
  at a stop`.
- D047 records the rule.

All C19–C22 probes were re-run on this tree, and the phase's native and
workload evidence was recorded again on a bundle prepared from it.
