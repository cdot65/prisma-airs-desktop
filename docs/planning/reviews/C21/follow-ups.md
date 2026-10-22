# C21 follow-ups for the P06 push to 9/10 (D034)

C21 completed at 8/10 under D034. Codex attempt-01 scored 9/8/8/8/8 with no
blockers, four required fixes (F1–F4), one evidence gap and two optional
suggestions. All are remediated in the P06 push. Each fix has a test and a
mutation probe in `docs/planning/evidence/C21/mutation-probes/probes.py`
(section "P06 follow-ups"). D048 records the resulting rules.

## F1: host tool calls are leased to their run and turn

- Code:
  - `harness/session.rs`:
    - Every tool request carries a `ToolLease { run, turn }`, with `run`
      random per engine.
    - A `ToolResult` is sent only when its request id and lease match a call
      still waiting.
    - `turn_cancel` is a token made per turn. It is cancelled at interrupt,
      at turn end and when the engine stops.
    - Once an interrupt is wanted, a new call is answered as unavailable and
      never run.
  - `authority/harness_tools.rs`:
    - The sink races each call against its cancel token and sends nothing
      once cancelled.
    - `run` checks `current()` (cancel and a live conversation) under the
      lock before management, scan, library and proposal admission, and in
      its polling loops.
- Tests:
  - Unit `a_stopped_turns_tool_calls_are_cancelled_refused_and_never_answered_late`:
    - interrupt refuses a new call, and the turn's end cancels the one
      under way;
    - its late result is dropped;
    - the next turn reuses request id 1, and the old lease and another
      run's lease are dropped;
    - only the current lease answers.
  - Integration `a_tool_call_under_way_when_its_turn_is_stopped_is_never_answered`
    (`harness_tools`):
    - a management read is held for 3 s and the turn interrupted while it
      runs;
    - the turn ends `interrupted`, and no tool output ever reaches the
      model;
    - the next turn answers normally.
- Probes:
  - `a stale lease's tool result is sent`
  - `a tool call is admitted while its turn is stopped`
  - `a stopped turn's tool calls are not cancelled`

## F2: JSON inside string fields is scrubbed before the model sees it

- Code (`harness/tools.rs`): `structured` parses JSON objects and arrays
  held in strings before `scrub` runs, so the scrub reaches every field. It
  opens text at most 4 levels deep and only up to 4 MiB.
- An oversized result becomes a valid JSON envelope:
  `{"cut": "<note>", "partial": "<the first part, as text>"}`. The model
  never receives malformed JSON, and the note asks for a smaller page. This
  answers the optional suggestion.
- Tests:
  - The unit test `results_are_scrubbed_redacted_and_bounded` carries
    canaries in JSON-in-string fields.
  - The `harness_tools` API keys fixture adds nested and mixed metadata
    canaries (`NESTED-C21-CANARY`, `MIXED-C21-CANARY`). The safe field
    `safe-c21-metadata` is kept.
- Probe: `JSON inside a string is not scrubbed`.

## F3: an approval whose scope cannot be shown whole is declined

- Code (`harness/session.rs`): an approval is asked only when it is
  reviewable; anything else is declined at once and never shown.
  - A command must be redacted, non-empty and at most 8 KiB, and is shown
    whole (no cut).
  - A file change must name its files.
  - An MCP call's arguments must be at most 8 KiB.
- The contract (`client.ts`) bounds command and arguments at 8 KiB.
- The engine's patch writer may ask before or after naming its file, and
  both orders are covered:
  - `harness_tools` accepts either order.
  - Native C21-N04: shown whole and failing in the sandbox, or declined
    unshown.
  - In both, nothing is written.
- Probes:
  - `a command too long to show whole is asked`
  - `a file change naming no files is asked`

## F4: replacing an entry keeps the transcript within its bound

- Code: `upsert` replaces an entry in place and then evicts the oldest
  entries until the transcript is within `MAX_TRANSCRIPT_BYTES`. It is used
  by both item update paths (command/file change, and MCP).
- Unit test `a_replaced_entry_keeps_the_transcript_within_its_bound`
  (64 KiB replacement).
- Probe: `a replaced entry bypasses the transcript bound`.

## Evidence gap: T23 offline and reconnect

- Integration `nothing_pending_survives_going_offline_and_nothing_is_resubmitted_on_reconnect`
  (`harness_tools`, real engine):
  - Setup: a staged proposal open in review and a command waiting for
    approval.
  - Offline relaunch:
    - the conversation, approval, proposal and review are gone;
    - the project and its access remain;
    - this Mac's history lists the conversation (`local`).
  - Reconnected:
    - no Gateway turn, no management write and no engine start;
    - the command never ran.
  - Continuing from history is the operator's explicit choice, and restores
    without running or proposing anything.
- Native C21-N09 does the same in the fixture app. A command waits for
  approval and a proposal is open in Administration's review when the app
  quits. The app relaunches with the synthetic services stopped (local
  history only), then reconnects:
  - no conversation, approval or review comes back;
  - no Gateway turn or management write is sent;
  - the folder is unchanged.
- C20's offline history (`listHarnessHistory` without a workspace) is what
  serves the offline read.

## Optional suggestions

- Structured truncation envelope: see F2.
- `confirmWrite` now reads in the future tense: "Your sign-in needs
  renewing, so the engine will restart with this conversation."
