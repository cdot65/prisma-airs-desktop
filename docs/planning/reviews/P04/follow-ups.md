# P04 phase review follow-ups

## Attempt-01 (9/8/8/8/8)

- R1, reset fences API Client transfers: sends run outside the workbench
  lane, so reset's lane turn did not cover them. A transfer cancelled by
  reset could still store its history and a dirty persistent cookie jar,
  create its response body, or load the profile key after the purge.
  - `Sending` now has a reset epoch, advanced when reset is admitted
    (`Workbench::clear`), and a fence (`tokio::sync::RwLock`).
  - A send is planned in an epoch (`Planned.epoch`). Each private-storage
    step runs through `fenced`, which holds the fence shared and runs only
    while that epoch is current: history load and store, cookie load and
    store, `BodyWriter::create`, and pruning bodies.
  - In-memory history updates check the epoch under the state lock, so a
    retired send never republishes its record. It ends `noCollection`
    (the collection it belonged to is closed).
  - Reset's purge (`Effect::Reset`) takes the fence exclusively after the
    lane and holds it to the end. A step already past its check therefore
    finishes before the purge, and any later step finds its epoch retired.
  - A body file opened before the purge only receives writes to an unlinked
    inode; nothing is created at finish.
  - Test: the production-interface
    `reset_retires_running_sends_and_nothing_they_held_is_kept`.
    1. A run is held inside its history write (the fixture Keychain holds
       reads). Reset is shown to wait (`resetting`, the private folder still
       present) until the step is released. The run then ends
       `noCollection`, and its next step (the body file) never runs: the
       private folder and the profile key stay removed.
    2. A run holding a dirty persistent cookie (a stalled response whose
       headers set `keep=1; Max-Age=3600`) is reset mid-transfer. Nothing
       is recreated.
    3. After a restart: the history is empty, the next request sends no
       cookie, nothing is replayed, and the operator collection is byte for
       byte unchanged.
  - Four probes (C15 probes.py): reset not waiting for the fence, a fenced
    step ignoring its epoch, a retired run republishing its record, and
    reset not advancing the epoch.
- Suggestion, packet diff base: `xtask review` now measures a phase's diff
  from the previous phase's verified completion commit (a chunk's from the
  latest completion). The packet therefore carries the whole cumulative
  change. Test: `a_phase_diff_starts_at_the_previous_phase`.

## Attempt-02 (9/8/8/8/8)

- R1 (remaining), reset during planning: a Send sampled its epoch only
  after its planning awaits (document capture, rendering), so a reset
  admitted meanwhile stamped the pre-reset request with the new epoch,
  which it then passed through every fence.
  - `begin` now samples the epoch before its first await (`admitted_in`)
    and passes it to `plan`, which keeps it in `Planned.epoch`; `apply`
    re-plans with the same epoch. A retired plan cannot load the jar (its
    fenced load refuses the retired epoch), so an ordinary Send ends
    `noCollection`; a tenant Send is refused `noTenant`, since reset retires
    every context.
  - The commit points check the epoch under the state lock as well:
    publishing a review, consuming one in Apply, and registering a run in
    `dispatch` (atomically with the run's cancellation token, so a reset
    either cancels it or has retired it).
  - Fixture hook (test-fixtures builds only): `Core::hold_send_planning`
    holds each Send after its document capture and before rendering;
    `sends_held_in_planning` counts those waiting.
  - Test: the production-interface
    `reset_during_planning_retires_the_send_before_anything_goes`. A first
    run leaves a history, a persistent cookie and the profile key. A
    context-null Send is held after document capture, reset is admitted
    (the purge waits for the lane: `resetting`), and planning resumes. The
    Send ends `noCollection`, no request reaches the target, no review or
    run appears, and the private folder and key stay removed. After a
    restart the history is empty, the next request sends no cookie,
    nothing is replayed and the operator files are unchanged. The two
    existing reset scenarios are kept.
  - Two probes (C15 probes.py): sampling the epoch after planning's awaits
    (caught by the new test), and admitting a Send in a different epoch
    (caught by `sends_the_fresh_snapshot_with_v1_auth_bodies_and_cookies`).
    The commit-point checks are second lines behind the jar fence and
    context retirement and are not separately observable through the
    production interface.
