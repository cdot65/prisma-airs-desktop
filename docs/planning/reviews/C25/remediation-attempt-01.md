# C25 remediation of attempt-01

Codex attempt-01 scored 9/8/7/6/7, with four required fixes, two blockers
and three evidence gaps.

## C25-F1: Academy calls are charged before they are sent

- Code (`crates/xtask/src/live_academy.rs`):
  - Every L12, L13 and L14 `call.*` entry is charged one `calls` unit through
    `Guard::spend` before `Desk::academy` sends it.
  - L13's avatar upload and removal are charged two units before either is
    sent.
  - A refused charge records the check blocked, sends nothing and stops the
    run (unexpected cost).
- The designated scope gives L12 and L13 `calls: 40` and L14 `calls: 10`.
- Test: `an_exhausted_call_budget_sends_nothing_and_an_untagged_call_is_refused`.
  L13's second call is refused unsent, and the fake Academy sees only the
  first. The main Academy test also asserts the receipts' `calls.used`.

## C25-F2 and C25-G2: a call counts only for the workflows it names

- Code: a `call.*` entry is now `<workflows> <procedure> [<input>]`.
  - The workflow IDs must be among those the family may claim:
    - L12: W50, W51, W52, W65
    - L13: W52, W53, W54, W65, W66
    - L14: W55, W56
  - The receipt names only those workflows. An entry that names none is
    refused unsent.
  - L14's local draft now claims only W55 (no submit). The avatar claims
    only W54.
- Test: the main Academy test asserts each receipt's workflows; the new
  test asserts the untagged entry is refused.
- Template: `docs/architecture/live-manifest.template.json` documents the
  format.
- Live, on the final tree, with one owner-approved device sign-in, each row
  now has its own representative operations:

| Row | Operations |
|---|---|
| W50 | course, path and handbook reads; one unit read |
| W51 | labs and lab-attempt reads; one hosted-graded inline check; the course progress summary |
| W52 | gamification summary; notification reads; mark all read |
| W53 | certifications; exam eligibility; certificates; identity-practical status |
| W54 | settings read; a hint marked seen; a synthetic avatar uploaded and removed |
| W65 | Daily Incident, its board, Shift history and eligibility |
| W66 | the affected-course check, a course reset of Application Architecture (clearing L12's inline check) and the summary after it |

- W56's live explicit submit is withdrawn by the owner (D055).
- `xtask ledger --record` credits a row only from receipts that name it.

## C25-F3: the recorded test plan runs the affected suites

`docs/architecture/checks.json` C25 now carries `host_commands`:
- xtask `live_run` (37 tests) and xtask `--lib`;
- desktop-core `runtime_changes`, `dlp_subset`, `gateway_inference`,
  `academy_identity`, `academy_learner`, `academy_offline` and
  `context_credentials`.

`xtask test --chunk C25` runs each, and each must execute tests.

## C25-F4 and C25-G3: receipts name what produced them

- Code (`crates/xtask/src/live_run.rs`): every receipt and the run record
  carry `implementation`:
  - the repository's source fingerprint (`fingerprint::source`, evidence and
    reviews excluded);
  - `Cargo.lock`'s SHA-256;
  - the runner binary's SHA-256.
- A live run refuses to start without them.
- Test: the L15 test asserts the binding in a receipt and the run.
- Every family was run again on the final tree, so no receipt is carried
  forward:
  - L11–L14 and L17 with the owner in the browser;
  - L01–L10, L15 and L18 without the operator.
- L18's scan cap was raised from 13 to 16 under the owner's delegation to
  allow that rerun.

## C25-B1 and C25-G1: Harness-run checks deferred to C26 by the owner

D055: L16 (W42–W45) and L17's MCP tool call, approval, denial and refresh
(W48) run inside the Harness engine, which runs only in the signed app.
C25 does not claim them; C26's signed-app acceptance must pass them before
a stable release.

## C25-B2: the inactive profile's cleanup is waived by the owner

D055: the inactive `airs-v2-c25-profile` cannot be listed, read or found
by name through the API or in the console. The owner waived its cleanup;
`live/reconciliation.jsonl` records the waiver (status `waived`).

## Found after these changes, before attempt-02

- The C25 test gate's new `dlp_subset` target found a stale expectation:
  `filtering_profiles_are_replaced_only` still expected the profile's `name`
  in the replace body. The replace leaves the service's own fields out (the
  service keeps them, shown live), so the expectation was updated. The
  comment in `changes.rs` that said the contract refuses those fields was
  corrected: the SDK contract accepts unknown fields; only nulls are
  refused.
- These two edits came after the final live runs. `live/carry-forward.md`
  binds the receipts to the final tree: restoring the two files reproduces
  the receipts' source fingerprint exactly, and the diff is recorded.
- The full desktop-core suite, every integration target included, passed
  (`cargo test -p desktop-core --features test-fixtures --no-fail-fast`,
  26 binaries). One run before that saw `harness_tools`'
  `a_tool_call_under_way_when_its_turn_is_stopped_is_never_answered` fail
  once under parallel load; it passed three times alone and in the full
  run. It is a C21 timing test that C25 does not touch.
