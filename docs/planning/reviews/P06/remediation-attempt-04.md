# P06 remediation after attempt-04

Phase attempt-04 scored 9/8/9/8/8 with one required fix and one evidence
gap. Both concern a renewal whose replacement engine never runs.

## R4-F1: a renewal's message is saved however the new engine fails

**Before.** A renewal's message was shown as `failed` when the new
engine's bootstrap, launch or output pipe failed, but only in memory: those
exits returned before the final save, so the record lacked it. A failed
runtime home before the launch, a reader or writer thread that could not
be spawned, or an old home that could not be removed dropped the message
entirely.

**Now** (`harness/session.rs`):
- The renewal's message is the new conversation's deferred work from the
  moment the conversation is built. The thread-spawn failure, a stop
  during start and a failed start all reach the ordinary stop accounting
  (R3), which saves it.
- Every exit before a conversation exists goes through one finalizer,
  `keep_unsent`. That covers the key read, bootstrap, launch, missing
  pipes, home creation in the renewal loop, and an old home that could not
  be removed. `keep_unsent`:
  - adds the message as a redacted `failed` operator entry;
  - saves the transcript with the last export as its restore point (or
    none);
  - shows the save's outcome (`savedAt` or `saveFailure`).
- Fixture-only fault injection: `ProcessLauncher::fault` (a refused launch,
  or a process without its output pipe, from the nth launch) and
  `fixtures::sidecar::refuse_homes` (no home can be made under one runtime
  root).

## R4-G1: evidence

`a_renewal_whose_engine_cannot_start_keeps_and_saves_its_message`
(`harness_identity_history`) completes and saves a turn, then makes the
next message need a renewed sign-in. Only the replacement then fails, in
one of four ways:
- its launch is refused;
- it runs without an output pipe;
- its home cannot be made;
- the old engine's home cannot be removed (a read-only directory in it).

Each time:
- the phase is `failed`;
- the message is the last entry, `failed`, exactly once;
- the save is shown as succeeded;
- there is one refresh, and only the attempted replacement launch;
- offline, the record equals the screen exactly;
- reconnected and continued from the first turn's export, the entries are
  unchanged, nothing answers the failed message, and exactly one engine
  starts.

The bootstrap and key-read exits share the same finalizer call, but the
production interface cannot make them fail after a verified renewal.

Probes (C20 set, "P06 phase review R4"):
- `a renewal whose engine never runs is not saved`
- `a refused launch loses the renewal's message`
- `a pipeless engine loses the renewal's message`
- `a home not made loses the renewal's message`
- `an old home kept loses the renewal's message`

D047 records the rule.
