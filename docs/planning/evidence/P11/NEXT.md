# P11 continuation after source validation

Implementation and source validation are complete on the refinement branch.
`source-validation-01/receipt.json` binds the current Check selection, actual
P11 Test, current fingerprint and test-only build carry. All 20 selected test
commands passed (439 passing test executions, zero failures). The separate
`proof-tests-02` run passed all 20 verifier tests. Original failures are retained.

Native attempt 04 has not started. The owner has been asked to launch locally:

`/Users/cdot/development/redtail/p09-queue/refinement-20261104/core/.evidence-private/native-job-04/START-P11.command`

The computer-use tool rejected opening Terminal because that app is blocked.
Do not bypass this restriction, start a competing UI writer, or count pending
native assertions as passing. The launcher uses disposable fixture data and
owned local endpoints. No real product credentials or live calls are required.
Read `.evidence-private/native-job-04/state.json` and its output log after the
owner starts it. Failed attempts 01–03 contain no passing product assertions.

On a complete observation run, inspect every capture and create the explicit
capture-bound visual review. Finalize with the actual Test RunRecord, then use
`tools/p11/proof.py make` with the paths in the validation receipt, build 03,
current prepare, finalized native record, original observations and
`build-input-carry-03.json`. Follow `tools/p11/README.md` for the four actual
chunk reviews, the actual P11 phase review and accepted standing. Report every
real score. No formal scored review, accepted P11 archive, main merge or new
release has been completed.

The rebuilt executable and full bundle are byte-identical to candidate 02.
Only test failure messages changed after build 03; its bound current Check/Test
evidence justifies the explicit build carry. Do not rerun unrelated checks or
the legacy native matrix merely for a commit, source fingerprint or review.

Linux app/test refinements are separately pushed through `f2e5a2ba` on
`refinement/20261104-linux`; Linux platform qualification remains independent.
