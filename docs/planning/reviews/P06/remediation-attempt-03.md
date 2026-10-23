# P06 remediation after attempt-03

Phase attempt-03 scored 9/8/8/8/8 with two required fixes and one evidence
gap. Both fixes concern operator work still pending when a conversation
stops. Rather than patch each path, the stop accounting is restructured
around one rule: once a stop has begun, nothing the operator sent is
released, and nothing the operator sent is lost.

## R3-F2: a stop during an await releases nothing

**Before.** After a turn's export arrived, `drive` awaited its save and
then released any deferred message or renewal without looking at the
stop. A sleep, end or retirement during a slow history write let a
`turn/start` (or a renewal restart) through.

**Now** (`harness/session.rs`):
- After the save returns, `drive` re-checks cancellation and the deadline
  before anything deferred is released, and marks the conversation
  `stopping`.
- `launched` treats a renewal as abandoned once the conversation is
  stopping (cancelled or past its deadline): no new engine starts, and its
  message is accounted for below. So is a renewal that cannot continue
  because the engine failed, was not cleaned up, or has nothing to
  restore.
- A renewal's message waits in the new engine as deferred work until the
  engine is ready, so a stop or failure while the engine starts leaves it
  unsent rather than dropping it.

## R3-F1: pending operator work is accounted for at the stop

**Before.** The deferred message was added after `turn_open` was
computed. When the export arrived during the grace and its save
succeeded, the final save was skipped and the record lacked the message.
A deferred renewal and a message still queued in the command channel were
dropped.

**Now:**
- At the stop, `launched` collects every unsent message:
  - the deferred `Send` or `Renew`;
  - an abandoned renewal's message;
  - every `Send` or `Renew` still queued in the channel (when the
    conversation does not continue).
- Each is shown as an operator entry `failed` (redacted), and any unsent
  message forces the final save. The record then matches the screen
  exactly. The last export stays its restore point (or none), so the
  completed turns stay continuable and the unsent message is never
  replayed.
- In the authority (`authority/harness_domain.rs`):
  - a renewal overtaken by a stop while its sign-in is renewed gives its
    message back to the box (`unsent`), the established C20 path, rather
    than vanishing;
  - so does a renewal waiting for changes to be allowed again when the
    conversation is stopped or ended.

## R3-G1: evidence

New production-interface tests (`harness_identity_history`). Stand-in mode
`history-slow-ok[-later]` answers an export successfully after 2 s, inside
the grace; `Idp::refresh_delay_ms` holds a sign-in renewal.

- `a_message_waiting_for_an_export_that_comes_at_a_stop_is_kept_unsent`:
  - the next message waits for a valid export that arrives during the
    stop's grace and is saved;
  - the message is shown `failed`, the offline record equals the screen
    exactly, and the record stays continuable;
  - reconnected and continued, the entries are unchanged, nothing is
    answered and exactly one engine starts.
- `a_stop_during_a_held_save_sends_nothing_waiting_or_queued`:
  - the save is held with `hold_history_writes`, and the Mac sleeps
    before it is released;
  - this is run with the message deferred behind the export, and with it
    queued in the channel behind the held save, on the stand-in and on the
    real engine;
  - the message is `failed` and saved, and the offline record equals the
    screen;
  - with the real engine, the Gateway sees no further request.
- `a_renewal_waiting_for_an_export_at_a_stop_never_restarts_or_sends`: a
  due sign-in turns the next message into a renewal deferred behind an
  export. At the stop, no engine starts and the message is kept `failed`.
- `a_renewal_under_way_when_the_mac_sleeps_never_restarts_or_loses_the_message`
  covers three windows:
  - The Mac sleeps while the sign-in is renewed. The message returns to the
    box, and the record is unchanged.
  - It sleeps while the old engine is stopped for the restart (stand-in
    `hang-on-eof`). No engine starts, and the message is kept `failed` in
    the record.
  - It sleeps once the new engine is launched but still starting (stand-in
    `slow-start`). That engine is never sent the message, which is kept
    `failed` in the record, and no further engine starts.
- `a_renewal_never_restores_an_export_older_than_the_conversation` now
  expects the message whose renewal cannot restore to be kept `failed`.
- `a_renewal_restarts_with_changes_only_once_the_operator_allows_them_again`
  now ends with a sleep while the confirmation is asked. The message
  returns to the box, with no refresh, restart or request.

Probes (C20 set, "P06 phase review R3"):
- `a waiting message vanishes at a stop`
- `a waiting renewal vanishes at a stop`
- `a stop during a save releases what waits`
- `a queued message vanishes at a stop`
- `an unsent message is not saved`
- `a renewal restarts after a stop`
- `a renewal's message vanishes while its engine starts`
- `a renewal that cannot restore loses its message`
- `a renewal overtaken by a stop loses its message`
- `a confirmation pending at a stop loses its message`

One earlier probe is re-guarded. `an unsaved transcript is dropped at a
stop` (attempt-02) was guarded by the refusal/malformed test. That test
queues a message behind the export, and an unsent message now forces the
final save by itself, so the probe now names
`a_finished_turn_whose_export_is_pending_is_kept_when_stopped`. That test
queues nothing, and it fails under the mutation.

D047 records the rule.
