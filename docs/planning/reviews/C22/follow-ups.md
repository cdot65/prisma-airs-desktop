# C22 follow-ups for the P06 push to 9/10 (D034)

C22 completed at 8/10 under D034 (008dc4b). Codex attempt-01 scored
9/8/8/8/8 with no blockers, five required fixes (F1–F5), three evidence gaps
(G1–G3) and one optional suggestion. All are remediated in the P06 push.
Each fix has a production-interface test and a mutation probe in
`docs/planning/evidence/C22/mutation-probes/probes.py` (section "P06
follow-ups"). D049 records the resulting rules.

## F1: a token's own expiry is enforced when a call is decided

- Code (`authority/harness_mcp.rs`):
  - Each live grant keeps its token's expiry (`expires_at`), set wherever
    the token is loaded, renewed or signed in.
  - `usable_now` requires no failure flag and a clock before that expiry.
  - `mcp_server_usable` uses it, so the native check behind every decision
    declines a call whose token lapsed, whatever the operator chose.
- Test `an_approval_past_its_tokens_expiry_is_declined_and_inference_goes_on`
  (`workspace_mcp`, ten-second tokens):
  - An approval held 11 s past its token, with no message in between and no
    test-only flag, is declined when allowed.
  - A call first offered after a fresh token expired (the model answers
    after its lifetime) is declined too.
  - Neither reaches the server, and the next message is answered.
- Probe: `a token past its expiry runs a call`.

## F2: a catalog that is loading or failed authorizes nothing remote

- Code:
  - `mcp_catalog_checked` returns the catalog only after a successful read
    with none under way.
  - `mcp_scope_check` is three-way:
    - `Changed` (a server off, re-signed or with another revision or URL in
      the last read) retires the conversation.
    - `Unchecked` keeps it, but nothing remote runs.
    - `Current` requires a fresh matching read.
  - What needs `Current`:
    - new starts (`mcp_scope_now`);
    - continuation from history (otherwise `otherMcp`);
    - Send (refused, with the message kept);
    - the MCP approval decision.
- Test `a_catalog_that_cannot_be_read_blocks_remote_use_until_read_again`:
  - A failed refresh keeps the last catalog for display.
  - The pending call is declined natively.
  - The conversation stays, a message is refused with nothing sent, and no
    conversation starts or continues.
  - A successful read allows all of it again.
- Probes:
  - `a failed catalog read still authorizes`
  - `an unchecked scope sends`
- The existing `a new revision keeps the conversation` and
  `signing in again keeps the conversation` probes are re-anchored into
  `mcp_scope_check`.

## F3: signing out ends a sign-in to the same server under way

- Code: `sign_out_mcp_server` cancels and drops a flow for that tenant,
  workspace and server. A late callback finds no listener, and a late token
  reply is not current, so it installs nothing and its tokens are revoked.
- Test `signing_out_during_a_sign_in_again_never_signs_the_server_back_in`
  covers a sign-in again interrupted before its callback, and during a
  token exchange held for 2 s. Each time:
  - the server is signed out;
  - no token item or ledger entry remains;
  - the late refresh token is revoked at the server.
- Probe: `signing out leaves a sign-in under way`.

## F4: a written record stays reserved until it is settled

- Code (shared with company sign-in, D047):
  - `write` no longer ends the write's flight.
  - `settle_write` ends it under the same lock as the save that commits the
    record or leaves it in the ledger, for both MCP and Gateway sign-in and
    rotation.
  - Cleanup can no longer select a record between its put and its
    settlement.
  - A cleanup asked for during a pass now runs again when that pass ends
    (`clean_again`), so a record released meanwhile is not left until some
    later trigger.
- Fixture gate `Core::hold_write_settlement` holds a write after its put.
- Test `a_written_token_survives_cleanup_until_it_is_settled` covers the
  initial sign-in and a rotation:
  - cleanup run in that window leaves the new item;
  - after settlement exactly one item remains, it is the stored grant's,
    and the ledger is empty;
  - it survives a relaunch, and a conversation starts with it.
- Probe: `a write leaves flight before it is settled`.

## F5: MCP arguments are shown whole or the call is refused

- Remediated with C21 F3 (approval scope shown whole):
  - An MCP item keeps its full redacted arguments when they fit 8 KiB.
  - An elicitation is asked only for a reviewable item.
  - The approval carries the whole arguments.
  - Larger calls are declined at once and never shown.
  - The contract bounds arguments at 8 KiB.
- Covered by the unit test
  `only_an_mcp_approval_naming_this_turns_running_call_is_asked`, which
  includes oversized arguments.
- Probe: `an MCP call too large to show whole is asked`.

## Evidence gaps

- **G1: T23, an interrupted call and reconnecting.**
  - A turn cut short by a stop (sleep, quit or ending the conversation) is
    saved as it stood, its unfinished entries `interrupted`, with the last
    export (whose context does not hold the turn).
  - `a_call_cut_short_is_recorded_as_interrupted_and_never_replayed`:
    - An approved call is still running at the server when the Mac
      sleeps. The server does run it, and the record shows the call
      `interrupted`, an unknown outcome.
    - Offline, the record is read from this Mac (`workspaceUnavailable`).
    - Reconnected, nothing is replayed, and continuing is explicit and runs
      nothing.
    - Probe: `a turn cut short is not saved`.
  - `a_pending_mcp_call_never_survives_going_offline_or_runs_on_reconnect`:
    - A call is waiting for approval when the app relaunches offline and
      then reconnects.
    - No conversation or approval comes back, no call or Gateway turn is
      sent, and the server stays on and signed in.
- **G2: combined Gateway and MCP rotation.**
  `a_due_gateway_and_mcp_renewal_restart_once_after_one_confirmation`
  (`harness_identity_history`) uses the real engine, company sign-in and a
  workspace MCP server with changes allowed. With both tokens due:
  - one confirmation is asked, with no refresh or restart before it;
  - Not now gives the message back with nothing sent;
  - Allow does one restart that renews both (one refresh each) and
    restores, then exactly one turn;
  - no approvals or calls are replayed.
- **G3: trace correlation.**
  - The synthetic MCP server returns a per-call trace (`trace-<slug>-<n>`)
    with the arguments it ran, and records them.
  - `an_approved_tool_call_runs_once_with_the_memory_only_bearer` proves
    four things. The declined call leaves no trace, and the approved one
    leaves exactly one with the arguments shown for approval. The displayed
    outcome carries that call's trace. A second call shows its own trace,
    never the first's.

## Optional suggestion

- Native C22-N06 now reaches Approve once with Shift-Tab from Decline,
  activates it with Space, and requires that the call then ran.

## Attempt-02 remediation

Attempt-02 scored 9/8/9/8/8 with one required fix and its evidence gap. It
found F1–F5, G2 and G3 closed.

**R2-F1 and R2-G1: an interrupted call is kept even with no restore point.**
- Code (`harness/session.rs`): a turn a stop cuts short is saved whether or
  not an export exists. With none (the first turn, or after a refused
  export), the record is saved with no replay or context.
- `not_continuable` names such a record `notRestorable`. It can be read and
  exported, never continued, and no context is invented. The contract and
  the history panel know the reason.
- Tests:
  - `a_first_turns_call_cut_short_is_kept_without_a_restore_point`
    (`workspace_mcp`, real engine):
    - with no turn ever exported, the Mac sleeps while an approved call
      runs at the server, and the server does run it;
    - offline, this Mac's record shows the call `interrupted` and reads
      `notRestorable`;
    - reconnected, continuing is refused and nothing is replayed (no call,
      no Gateway turn).
  - `a_turn_cut_short_after_a_refused_export_is_kept_without_a_restore_point`
    (`harness_identity_history`, stand-in mode `history-refuse-stall`):
    every export is refused, and the second turn stalls until the Mac
    sleeps. The record is written with that turn interrupted and reads
    `notRestorable`.
- Probes:
  - `a cut turn with no export is not saved`
  - `a record without a restore point is continuable`

**Suggestion.** In
`signing_out_during_a_sign_in_again_never_signs_the_server_back_in`, the
sign-out now waits for the token request to be observed (a counter in the
delayed MCP plane), not a fixed 300 ms. The test asserts that the
late-issued refresh token itself is revoked.

**A gate race in a C21 test.** The first attempt-03 run stopped at the
test gate. `the_project_and_its_access_are_the_binding_and_unsafe_folders_are_refused`
started a new conversation as soon as the retired one left the view, while
it was still finishing ending. The start was refused as busy. The test now
retries the start until that conversation has ended, bounded at 10 s; no
product behavior changed. The full set passed three runs in a row.

## Attempt-03 remediation

Attempt-03 scored 9/8/9/8/8. It had one required fix and its evidence gap,
and found the no-export cases closed.

**R3-F1 and R3-G1: a turn is saved when stopped while open, whatever its
entries show.**
- Code (`harness/session.rs`): the stop-time save keys on the turn being
  open (asked and not ended), not only on an entry still streaming. An
  approved call that has settled while the turn waits for the model's next
  answer is therefore saved. Its known outcome is kept unchanged, and no
  replay is invented.
- Test `a_finished_call_waiting_for_the_model_is_kept_when_the_turn_is_cut_short`
  (`workspace_mcp`, real engine) runs once on a first turn and once after a
  saved turn:
  - the approved call runs and settles `complete`, and the model's next
    answer is held, so no entry is streaming and the turn is answering;
  - the Mac sleeps, and a record is saved;
  - offline, this Mac's record shows the call `complete` with its own trace
    (`notRestorable` on the first turn);
  - reconnected, nothing is replayed (one call, no further Gateway turn).
- Probe: `an open turn with nothing streaming is not saved`.
