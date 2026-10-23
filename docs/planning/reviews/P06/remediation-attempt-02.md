# P06 remediation after attempt-02

Phase attempt-02 scored 9/8/8/8/8 with two required fixes and one evidence
gap. Both fixes concern the stop-time handling added for attempt-01.

## R2-F1: draining an export while stopping releases nothing

**Before.** A next message sent while a finished turn's export was pending
was deferred. If the conversation was then stopped, and the export
answered with a refusal during the grace, the ordinary failure handler
released that deferred message. It sent `turn/start` while stopping.

**Now** (`harness/session.rs`):
- `finish_export` marks the conversation `stopping`, and the HistoryRead
  failure arm releases no deferred command while it is set. Only frames
  are handled during the drain.
- A message still deferred when the engine stops was never sent. It is
  added to the transcript as an operator entry `failed`, and it is never
  sent later.

## R2-F2: the unsaved obligation outlives an export error

**Before.** A refusal or a malformed result during the drain cleared the
pending-export flag without saving, so the stop-time save was skipped.

**Now:**
- `unsaved` is set when a turn ends and cleared only by a save that
  succeeds.
- The stop-time save keys on an open turn or `unsaved`. That covers an
  export that is pending, refused, malformed or never answered. The
  transcript as shown is saved without a restore point (`notRestorable`),
  never paired with an older export.
- The separate pending-export condition is subsumed and removed. Its
  probe is retired in favour of `an unsaved transcript is dropped at a
  stop`, which fails both tests below.

## G1: evidence

- `a_stop_while_an_export_fails_keeps_the_turn_and_sends_nothing_waiting`
  (`harness_identity_history`) uses stand-in modes
  `history-slow-{refuse,malformed}[-later]`. Each answers the export after
  2 s, inside the 3 s grace, on a first turn and on a saved conversation:
  - the turn ends, the next message is queued behind the export, and the
    Mac sleeps;
  - the queued message is kept as `failed`, with nothing after it, and no
    turn starts;
  - the record is written with the exact entries shown;
  - offline, it reads `notRestorable`;
  - reconnected, continuing is refused and no engine starts.
- `refused_or_unanswered_exports_and_refused_restores_are_named` now
  expects an ended conversation whose exports were refused to keep its
  turns, inspect-only.
- Probes (C20 set):
  - `a waiting message is sent while stopping`
  - `an unsaved transcript is dropped at a stop`
  - `a waiting message vanishes at a stop`
- D047 records the rules.

## Test timing after the longer stop

A stop now drains a pending export (up to 3 s) and saves the transcript,
so a retired conversation takes longer to finish ending after it leaves
the view. Tests that start a new conversation right after a retirement
now use the shared `start_when_free` helper (`harness_process/desk.rs`),
which retries a busy start for up to 10 s:
- `signing_out_again_or_off_or_a_new_revision_retires_the_conversation`
- `a_changed_key_workspace_or_context_retires_the_conversation_at_once`
- C21's project test

The test that requires a concurrent start to be refused still uses the
plain start.
