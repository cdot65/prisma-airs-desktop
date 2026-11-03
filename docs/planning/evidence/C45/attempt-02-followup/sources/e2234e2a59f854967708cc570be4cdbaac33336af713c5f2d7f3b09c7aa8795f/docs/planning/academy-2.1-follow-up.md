# Academy qualification follow-up (2.1.0)

This addresses C25-R2-F1/G1 under D056. It does not change C25's historical
verdict, the release gates or the Academy presentation overhaul.

The owner designated `airs-desktop@cdot.io` on 2026-10-26 for the hosted
qualification. Only that disposable learner's designated coursework may be
changed. An authenticated operator must approve device sign-in as that learner;
the runner never reads a learner password or a browser session.

## Milestone 1 — result assertions and fixture qualification

Every Academy `call.*` entry now needs a result expectation. An entry without
one is blocked before its procedure is called. The grammar is:

```text
call.<ordered-label> = <workflow-ids> <procedure> [<SuperJSON-input>] ?? <expectation> [ && <expectation>]
```

Labels are 1-96 ASCII letters, digits, hyphens or underscores; they cannot
select receipt directories. Labels must also be unique ignoring case on macOS
filesystems. Labels execute in lexical order within a family. An expectation starts with
a JSON pointer (empty means the whole answer):

- `/correct=false` compares a hosted grading verdict to the designated answer's
  expected result. `progress.checkInline` requires an exact boolean verdict;
  `/correct:bool` alone cannot qualify grading.
- `/units=0` and `/hasProgress=false` verify reset results.
- `:nonempty-array` requires an actual baseline containing certificates;
  `:array` alone would permit an empty baseline.
- `=@01-certificates` compares the whole answer to an earlier validated answer.
  `/attemptId=@01-start/attemptId` compares selected fields.
- `/outcome in ["correct","incorrect"]` accepts only a graded Shift answer;
  `ungraded` and `breached` fail.
- `observe` in place of workflow IDs permits diagnostic queries without
  granting any workflow credit. Mutations cannot use `observe`.
- Shape predicates: `bool`, `array`, `number`, `object`, `string`,
  `nonempty-array`, `nonempty-string`, `positive-number`.

Inputs may refer to an earlier validated answer using an exact JSON object:

```json
{"json":{"attemptId":{"$call":"01-start","pointer":"/attemptId"}}}
```

References must point backward within the same family. Malformed pointers,
forward references and invalid reference objects block the program before
dispatch. A failed or missing dependency blocks the dependent call without
spending its budget or sending a request. Strings are never interpolated. Literal JSON strings containing the DSL
separators ` ?? `, ` && ` or ` in ` must escape those characters (for example with
JSON Unicode escapes); unescaped separators fail closed.

Receipts record each assertion, its pass/fail result and summaries of both
compared values. Boolean, number and null values are retained. Private strings,
objects and arrays record presence and size only; their values and unsalted
fingerprints are omitted to prevent guessing low-entropy private values.
The resolved outgoing input is not written. The designated template preserves
references but redacts `answer` and `values` fields so served answers are not
published as an answer key. Membership literals are summarized too. Authentication/admission receipts omit
all `call.*` entries; they cannot serialize the unredacted program a second
time. Full-receipt fixture scans cover authored answers, Shift values and
private expected literals, including failed assertions.
Missing data differs from an explicit null. Malformed/missing SuperJSON
answers fail rather than becoming a null that could satisfy an expectation.

Successful reads cannot close interactive rows. In addition to individual
receipts, the runner records an ordered checkpoint receipt for each claimed
W53/W65/W66 row:

| Row | Minimum ordered validated calls |
| --- | --- |
| W53 | `exam.start`, `exam.submissionState`, `certificate.mine` |
| W65 | `shift.start`, `shift.serve`, `shift.answer`, `shift.decide`, `shift.debrief` |
| W66 | `certificate.mine`, `progress.courseSummary`, `progress.resetCourse`, `progress.courseSummary`, `certificate.mine` |

The runner rejects weak programs before dispatch: exam identity/state, Shift
identity/order/graded outcome/settlement/debrief, and reset baseline/counts/
certificate preservation have mandatory assertion sets in
`crates/xtask/src/live_academy/contracts.rs`. Critical procedures must claim
their corresponding workflow regardless of family; relabeling a reset as
W51/W54 cannot bypass W66. Duplicate critical starts/resets/checkpoints are
rejected rather than allowing later weak occurrences. A reset cannot run after an earlier
failed checkpoint of the same workflow. A designated Shift `hand_off` may still
be attempted as cleanup, subject to identity, reference and budget checks.
Academy permits it only at a completed stage boundary. A failed serve or
mid-stage answer can leave a pending alert, so hand-off is refused and the
spent Daily run remains active until the server stale sweep contains it. The
runner records a reconciliation requirement and stops further work when
hand-off is not confirmed; it does not claim successful cleanup. Once a Daily
start succeeds or has an unknown remote outcome, an unsettled-run marker
survives failed assertions. Budget exhaustion, unavailable cleanup dependencies
and core/transport failures cannot drop its reconciliation requirement. No
extra request is sent to exceed the cap or use failed prerequisites. Only a
confirmed hand-off clears that marker. A failure after confirmed settlement
still fails the workflow, without inventing an active run.

All learner mutations use the same uncertainty policy, not only Daily calls.
Both failed transport replies and server-error replies with `outcome=unknown`
stop with reconciliation; an admitted mutation that exceeds the core settle
bound also gets an explicit receipt. No later mutation or avatar action is
sent. Avatar upload/removal apply this policy too, and an unconfirmed removal
after a confirmed upload requires reconciliation. Query failures do not claim
a remote mutation occurred. Fixtures cover dropped exam/reset/avatar replies
and actual unsettled-call timeouts, including a timeout during a Daily run.
A query timeout without an active Daily stops without claiming mutation
uncertainty. Refused or locally blocked avatar uploads send neither a PUT nor
confirmation/removal; the existing avatar is preserved. Both policies have
fixture regressions.
These are minimum checkpoints, not proof of every subfeature in the broader
workflow row. Native
presentation, practical-assessment authority, history/boards and the other
row requirements retain their separate acceptance evidence.

Fixture regressions cover both grading verdicts, a reset acknowledged but
not applied, lost certificates, empty progress/certificate baselines, failed
dependencies, read-only workflow credit, typed server-issued IDs, wrong attempt
and settlement IDs, unknown-shift debriefs, ungraded/breached/refused Daily
runs, weak and mislabeled programs, duplicate critical calls, account binding,
and redacted receipts. A fixture tests rejected mid-stage hand-off and the
resulting reconciliation record. They run through the production Rust core against local TLS services.
They are fixture evidence, never hosted/live evidence.

## Milestone 2 — hosted qualification

Prepare a new private manifest outside the repository, mode 0600, for the
designated test learner. Keep unrelated families blocked. Name mutations in
`permitted_operations`, set bounded call budgets and document cleanup. Do not
reuse the owner's personal learning record. Preserve C25's recorded evidence;
run this follow-up from an isolated checkout and archive its new receipts
separately with source/dependency/runner bindings. These archived receipts do
not automatically update the C25 workflow ledger; formal gate admission remains
a separate reviewed action. Every family issuing mutations must specify
`expected_account_sha256`, pinned from the core identity observed after the
operator confirms the designated learner. A mismatch signs out and stops before
any learner program or avatar mutation. A read-only `observe` preflight may be
used to establish that identity; it grants no workflow completion.

1. Read the learner's accessible catalog and assessment eligibility. Record
   which course, unit, inline question and certification are designated.
   If access/readiness is denied, record the denial; do not bypass it.
2. Grade a designated deterministic inline answer using its actual
   `lessonId`, `questionId` and answer, asserting `/correct=true` or `false`.
3. W53: start the eligible exam with consent. Assert the returned attempt ID,
   deadline and time limit; use that attempt ID in `exam.submissionState` and
   verify its identity and `in-progress` state. Do not submit an exam or award
   a real credential merely to manufacture qualification data. Record how the
   disposable attempt is retired or expires.
4. W65: start today's Daily Incident. Its current contract returns `shiftId`,
   not `id`. `shift.serve` returns `shift.alertOrder`; `shift.answer` returns
   `outcome` (`correct`, `incorrect`, `ungraded` or `breached`), not `correct`.
   Use the started ID and served order. Require a graded answer with
   `/outcome in ["correct","incorrect"]`; `ungraded` and `breached` fail.
   The missed-objective debrief check needs a designated deterministic miss,
   and `/themes:nonempty-array`; an empty unknown-shift response cannot pass.
   A hand-off must return the same `shiftId` as the start, plus `handed_off`. Repeat serve/answer until the real stage
   boundary, then explicitly hand off and read the debrief. Respect the daily
   attempt limit and server timing. Do not turn a judge failure into a pass.
5. W66: first record a nonempty certificate baseline and positive progress on
   the disposable course. Assert reset acknowledgement against the previous
   counts (all four), then assert `hasProgress=false` and all four counts (`units`,
   `quizzes`, `labs`, `objectives`) equal zero. Read certificates again and
   compare against the baseline. A learner without an earned test certificate
   cannot establish preservation of an existing certificate; report that gap.
6. Retain redacted receipts and ledger charges, any denied/blocked steps and
   cleanup/reconciliation. Obtain a fresh independent review; do not relabel
   the old verdict or fixture outcomes as passing live evidence.

Browser automation was unavailable on 2026-10-26. After the owner switched
the default browser to the test learner, a fresh manual device sign-in and a
50-course catalog read passed through the production core from an isolated
checkout. The signed-in account differed from the earlier C25 account. A served
question was also graded `correct=false` for an intentionally out-of-range
index. Read-only discovery found no existing certificates, an ineligible exam
and a locked Shift pool (0 of 50 questions). These are documented hosted
test-data prerequisites, not passing assessment/reset evidence. See
[evidence and limitations](evidence/academy-2.1/hosted/summary.json). Historical
live receipts retain their own producer/source bindings; the safety changes
are separately bound to the final fixture/static validation records.

## Milestone 3 — presentation handoff

The incoming UI overhaul can consume the existing React Academy presentation
and Rust authority contracts. Keep hosted grading, assessment timing/access,
Daily Incident state and certificate issuance authoritative on Academy's
service. Gamification remains inside the Academy feature. This follow-up
changes build-time qualification tooling; it introduces no shipped backend or
UI authority changes.

Owner direction on 2026-10-26: document the hosted test-data prerequisites;
do not seed or bypass eligibility in this task. The final device code expired
without a completed sign-in, so that repeat sent no learner program. Earlier
successful hosted evidence remains retained with its original producer binding.
