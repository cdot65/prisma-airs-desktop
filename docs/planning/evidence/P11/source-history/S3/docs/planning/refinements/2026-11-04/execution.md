# Refinement qualification checkpoint — 2026-11-04

At this checkpoint, all four implementation streams are integrated and all
**12 required P11 native assertions have passing observations**. Independent
chunk/phase reviews and a new production prerelease remain next. No review score,
stable acceptance or new publication is claimed here. Later bound review and
standing receipts are authoritative for acceptance.

The owner-approved 17-item scope and decisions are in [plan.md](plan.md).
The integration base is Forgejo main
`f1e58da5cdeb1c67eed650b186081661cefc2fa2`.

| Stream | Implemented behavior | Native observations |
| --- | --- | --- |
| C46 | Required management OAuth before a new tenant is committed; separate optional Scan Key; metadata-only Edit; source-specific warning rechecks/history and Supply Chain routing | N01 in attempt06; N02–N03 in attempt12 |
| C47 | Header H1 and compact page introductions/actions; nonmodal Explain; removed header Ask Harness; improved sun icon | N02–N03 in attempt10; N01 derived in attempt11 from the unchanged attempt10 layout matrix and two new API keyboard traces |
| C48 | Colored JSON/URL editing; scoped footer; scrolling tabs; remembered collection choice; safe response filename suggestions | N01–N03 in attempt07 |
| C49 | Compact Settings cards; independent Appearance/Theme; isolated Add flow; Academy palette isolation; credential and metadata controls | N01 in attempt07, N02 in attempt08, N03 in attempt09 |

All interactive development, local checks/builds and native UI run inside the
dedicated `admin@worf-vm` VM, in branch `refinement/20261104-vm`.
Kirk's old native launcher is retired. The earlier Kirk source checks retain
their original host, source, command and outcome bindings; they are not reported
as VM executions.

## Candidate and focused source verification

The current fixture is the actual VM build
[Native build04](../../evidence/P11/native-build-04-vm), including the Modal
composition fix. Its executable SHA256 is
`099669efc96b8b54bc300dda33295be6271e81b59b9d11128434330e8073434d`;
the complete bundle SHA256 is
`eb9f93a1a4394574ca36e0c0affdd2c9206dfb5b3496649368cb24079394c7f5`.
The earlier `3239facd…` candidate and every original build/failed run remain
historical evidence; they are not the current artifact.

The VM's affected verification executed formatting/typechecking and all twelve
registered frontend test groups: **379 passing tests**. The original complete
source Test run had **439 passing executions** on Kirk. Its eight unchanged
Rust test commands (62 executions), other unaffected checks and their original
bindings are retained with explicit dependency/tool/impact proofs. They were
not rerun merely for the VM move, test/helper changes or a new source fingerprint.

The real Settings Add failure was fixed by moving desktop dialogs outside inert
page ancestors and preserving opener focus. Both themed App regressions passed.
The Administration eager-review test was corrected to await its dialog.
Application code and artifact bytes have stayed unchanged throughout the later
driver/provenance corrections.

The final current Python helper verification passed **75/75 tests**, with no
skips, source drift or application/Rust/Node-suite replay. Its raw record is
[helper-qa-final-02](../../evidence/P11/helper-qa-final-02/record.json).

## Native evidence and visibility

The [native selection](../../evidence/P11/native-final-01/native-selection.json)
preserves all seven actual origins, attempts06–12. Failed overall process exits
remain failed. Only completed assertions with all facts true are selected.
Attempt10's false keyboard fact remains explicitly excluded; attempt11 retains
its unchanged sixteen layout measurements and supplies two actual API keyboard
traces. Both traces reached the required controls after 56 tab stops, beyond the
old driver's arbitrary 24-stop limit.

Later corrections were confined to driver assumptions: AX whitespace/roles,
reselecting session storage after form remount, measuring actual native chrome,
the current Manage API keys label, and split latest-check paragraph text.
Passing assertions were carried with exact app/helper/condition proofs; no
unrelated native or Rust suite was replayed.

Attempt12 exited **0**, completed both remaining warning assertions and recorded
no errors. A denied recheck remained active; a matching successful recheck
resolved the finding while preserving its original reason/date. The original
history screenshot shows its Settings container. A separate
[read-only visibility supplement](../../evidence/P11/warning-history-visual-01/receipt.json)
shows the original dated warning, latest Passed result and resolution time.
That eight-second capture changed no persisted fixture state and replayed no
assertion. The derived native acceptance adds its bound evidence while retaining
the original acceptance and raw observations unchanged.

All selected captures were individually inspected. Academy theme/focus/portal
checks preserve its existing guest cosmetics; they do not claim a new responsive
qualification of its unchanged reader layout.

## Remaining work and separate lanes

Run the actual independent C46–C49 reviews, report every score/finding, then the
P11 phase review against the same qualified candidate. No scores have been
recorded at this dated checkpoint.

The Linux branch `refinement/20261104-linux` includes the later Modal and
Administration fixes through `e6c88db3`. Its focused typecheck and 77/77 DOM
tests passed inside the VM; original failures and raw results are retained.
These are tests of Linux integration source on macOS, not Linux native,
AppImage or installed acceptance. Linux remains independently unqualified.

After acceptance, pin the accepted source for the approved Jadzia Mac builder,
prepare a new immutable production candidate and preserve RC1. CI admission
needs its reviewed P11 source pin; signing readiness on Jadzia is unverified.
Stable installed/live C26 acceptance remains deferred by the owner.

No real tenant API calls, remote IAM changes, owner credential reads, historical
seal edits or RC1 asset changes were performed for these refinements.
