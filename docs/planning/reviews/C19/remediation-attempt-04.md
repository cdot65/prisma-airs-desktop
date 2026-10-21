# C19 remediation after attempt-04

Verdict attempt-04 scored architecture 9, behavior 8, authority/security 8,
validation 8 and overall 8, with three required fixes and no blockers. All
three are remediated now, so the chunk reaches the phase bar without a later
raise.

## R1: completed answers are held to the same bounds as their deltas

`item/completed` text now goes through `Conversation::complete`
(`crates/desktop-core/src/harness/session.rs`). It is checked against the
same two bounds as the deltas:
- the turn's answer bytes (256 KiB), counting every item of the turn; the
  item's own earlier text is replaced, not counted twice;
- the transcript's remaining room (1 MiB for the conversation).

Text past either bound is cut at a character boundary, the entry is marked
truncated, the turn is marked as truncating and the answer is interrupted.
The turn then ends as truncated, never as answered. A completion for an
item the engine never started is added the same way. `answer_bytes` now
tracks the kept text of completed items too.

Test (production interface, stand-in engine modes `big-complete`,
`two-items` and `fill`):
`completed_answers_and_token_counts_are_held_to_their_bounds`
- A short delta, then a completion of 256 KiB + 1 byte: the entry keeps
  exactly 256 KiB, is truncated, and the turn ends truncated.
- Two items in one turn, 200 KiB then 100 KiB: the first is complete, the
  second is cut so the turn keeps exactly 256 KiB, and the turn is
  truncated.
- Whole 256 KiB answers with no delta: the fourth turn is cut to the
  transcript's remaining room. The transcript totals exactly 1 MiB, and the
  fourth answer is truncated.

Ordinary streamed completions are unchanged (the existing conversation,
echo-key and approval tests).

## R2: the failed-answer statement matches what happens

The status for a failed answer no longer says it "was not sent again". It
now says that the engine may have sent the message to the Gateway up to five
times before the app stopped it, that each may have been processed, and that
nothing was undone. This matches D046 and
`a_failed_answer_is_named_and_only_server_failures_are_retried_by_the_engine`
(five requests for a 503). The presentation test `names every failure and
stop…` pushes a failed turn and checks the new text and the absence of the
old claim.

## R3: token counts are published only within JavaScript's safe integers

A `thread/tokenUsage/updated` counter above `MAX_SAFE_COUNT` (2^53 − 1)
leaves the usage unpublished; the previous value stays. The contract's
safe-integer rule is unchanged.

Tests:
- Integration (`huge-usage`: 2^53 and u64::MAX counters): the answer is
  shown, `usage` stays null, and every number in the full Core snapshot JSON
  is at most 2^53 − 1.
- Contract: usage at `Number.MAX_SAFE_INTEGER` is accepted. 2^53 and the
  u64 maximum are refused.

## Probes

Four probes were added (40 in all): a completion past the bound shown as
complete; completed answers not counted against the turn; an unsafe token
count published; the failed answer claiming no resend.

## A test race found while verifying

`drafts_and_uncertain_outcomes_survive_an_offline_restart_and_nothing_is_replayed`
(C17, `crates/desktop-core/tests/gateway_changes`) failed once under load:
it read the kept file before its first write. It waited for `unsaved` to be
false, but `unsaved` is also false before the first write starts. It now
waits for the file itself as well. The production code is unchanged, and the
test passes repeatedly.

# C19 remediation after attempt-05

Verdict attempt-05 accepted R1–R3. It scored architecture 9 and every other
score 8, with one required fix (R4) and no blockers.

## R4: item ids are bounded before they can name an entry

The R1 fallback created an entry for a completion of an item the engine
never started, using an unchecked item id. That fallback is kept: a
completion with no start or delta is a whole answer. But both
`item/agentMessage/delta` and `item/completed` now require the item id to
pass the same `bounded_id` rule as `item/started` (1–128 characters, no
control characters). An id that fails it is a sanitized protocol failure:
the conversation fails and is reaped, and no entry id longer than the
contract's 300 characters is ever published.

Tests (stand-in modes):
- `long-id` joins the malformed/oversized/duplicate/foreign table of
  `malformed_oversized_duplicate_or_foreign_frames_fail_the_conversation_and_reap_it`.
  Its completion has a 301-character id and no matching start, and it fails
  as `protocol`. For every mode in that table, every published entry id is
  now checked to be at most 300 characters.
- `complete-only`, in `completed_answers_and_token_counts_are_held_to_their_bounds`,
  checks the legitimate case: a completion with no start and no delta is
  kept as a complete entry `u-1/unstarted`.

Probe added: an unbounded completion item id names an entry (41 in all).
