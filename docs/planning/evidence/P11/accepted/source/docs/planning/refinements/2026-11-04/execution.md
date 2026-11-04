# Refinement qualification checkpoint — 2026-11-04

Checkpoint: 2026-11-04 22:45 UTC. The final build08 and affected recovery04 have passed;
all three current capture references were individually accepted. This frozen,
evidence-before-seal document supplies qualification04. The resulting bound
consumer record controls review admission. No unreturned C48/phase score,
final P11 standing, stable acceptance or production publication is claimed.

All development, checks/builds and native UI run inside `admin@worf-vm`, branch
`refinement/20261104-vm`. Kirk's old native launcher is retired. The owner-approved
17-item scope remains [plan.md](plan.md), integration base Forgejo main
`f1e58da5cdeb1c67eed650b186081661cefc2fa2`.

## Actual review standing

Scores below are architecture / behavior / authority_security / validation /
overall. Every original request, verdict, transport, audit and score remains
unchanged. No score was overridden.

| Subject | Actual reviewed standing | Current treatment |
| --- | --- | --- |
| C46 attempt-02 | 9 / 9 / 9 / 9 / 9, qualification03 | Retain the original full-nine review through explicit unchanged-impact carry; do not reopen it. |
| C47 attempt-02 | 9 / 9 / 9 / 9 / 9, qualification03 | Retain the original full-nine review through the same bounded carry. |
| C49 attempt-02 | 9 / 9 / 9 / 9 / 9, qualification03 | Retain the original full-nine review through the same bounded carry. |
| C48 attempt-02 | 9 / 8 / 9 / 8 / 8, qualification03 | Required P2: undelivered Load outcomes lost recovery/Retry, and initial delivery failure could remain at Opening. Accepted at eight under D034; fix and fresh attempt-03 remain the push to nine. |
| P11 phase | Not yet run | Fresh full-source/integration/carry review after current C48 passes its full-nine gate. |

The prior attempt-01 scores remain C46 9/9/9/8/8, C47 9/9/9/8/8,
C48 9/8/9/8/8 and C49 all 9. Their findings and D034 acceptance records are
preserved, not replaced by the later scores.

The dated member-carry proof binds exactly C46/C47/C49 attempt-02, original
qualification03, the exact [S4 source archive](../../evidence/P11/review-carry-baseline-01/source-preservation.json)
and [old f558 app](../../evidence/P11/review-carry-baseline-01/application-preservation.json),
current qualification04/source and the hash-pinned three-file C48 retry-only
delta. It includes per-subject rationale and unchanged dependency identities,
including all resources. C48 and P11 cannot be carried. The phase explicitly
consumes mixed original/current member qualifications and reviews all current
source, integration and the carry logic. Final archival context retains S4 and
the old app alongside the accepted current source/app; no review is retagged.

## Focused fixes and source evidence

C46's older Harness/Inbox fixtures completed unauthenticated drafts and were
omitted from affected coverage. Only their two test files changed: authenticate
synthetic management credentials, reacquire context and assert completable.
The nine named consumers passed (3 Harness + 6 Inbox), both integration targets'
scoped Clippy passed, and production authentication remains unchanged.
[Raw red/green records](../../evidence/P11/c46-consumer-remediation-01/manifest.json)
and [Clippy](../../evidence/P11/c46-consumer-remediation-01/clippy-supplement.json)
retain exact sources and outcomes.

C47's independent-scroll gap was closed on the exact original 099/eb9 app:
11 fixture calls overflowed, PageDown reached lower call index 4, drawer content
moved and the Home anchor stayed fixed. Both images were accepted; fixture state,
modes and roots stayed unchanged. The [14-second supplement](../../evidence/P11/c47-scroll-supplement-01/supplement.json)
is a separate actual execution, not an original capture rewritten.

C48's first fix preserved failed preferred-collection recovery across navigation.
Its original API 42, shell 29/Test Cases 16, Save/Send 20 and later focused 4/typecheck
checks keep their own exact source/timing bindings. A subsequent real
App/DesktopClient/native-adapter regression exposed initial/retry busy admission
and rejected observation. The minimal API/store/test fix keeps every nonpositive
Load unresolved, stores only sanitized delivery messages in the existing scoped
recovery state, offers Start/Retry while loaded=false, and prevents automatic
remount retries. Eight focused cases and typecheck passed. The malformed original
busy fixture produced a genuine generic failed-delivery result; its red 4 and
first green 2-fail/6-pass outputs stay unchanged with that interpretation.

Native03's PNG inspection then found a newly surfaced, misleading generic
core-refused folder detail. A final two-line message classification now displays
shared text only for undelivered outcomes, preserves prior text on cancellation,
and uses the accurate generic restore warning for core refusal. Failed/generation
behavior did not change. Four affected cases plus typecheck passed; the eight
prior cases retain their actual earlier inputs instead of being rerun or called
same-source. The [final source selector chain](../../evidence/P11/retained-qa/c48-undelivered-02/supplement.json)
retains all raw failures, exact diffs, snapshots and old broader source outcomes.

The source change is pinned to API Client, its tests and scoped store; App,
Rust, contracts, Save/Send controllers, JSON/URL/footer/tabs and styling remain
unchanged by this last retry correction. The current evidence consumers and
standing/carry guards passed [20 affected pure Python tests](../../evidence/P11/review-carry-qa-01/record.json).
Earlier 81 and the one parser follow-up retain original bindings; no broad
application/Rust/native/helper suite was relabelled current.

## Final current fixture and native slice

[Build08](../../evidence/P11/native-build-08-vm/receipt.json) passed on the final
three product files from commit `a4708d29fee5af327509e78895d741c668b45d1e`.
All 660 application inputs and the sealed old canonical prepare stayed unchanged.
The executable SHA256 is
`db9a1d0af38d677c4d7c9ffa76c0e5fa4a1bd3d2abb06706e9000e71f11eb304`;
the complete bundle SHA256 is
`dd4657c2bc71631568f8783aad51a6700e4c05f2248ac0452b1d06d1cf3dd04f`.
Its [versioned preparation](../../evidence/P11/native-build-08-vm/native-prepare.json)
uses the same fixed ARM64 fixture recipe. Original builds/qualified apps remain
bound to their original sources; old099 and qualification03's ba72/f558 app are
preserved independently of the current target tree.

Build07 failed with ENOSPC while archiving desktop-host, before changing app
inputs or canonical preparation. Its raw output and reserved empty prepare remain.
Only this P11 target's debug/incremental/rcgu scratch was removed; source,
evidence, preserved applications, rlibs and test binaries were retained. Build08
uses the same final product source, not another code change.

[Recovery04](../../evidence/P11/recovery-native-04/supplement.json) actually ran
22:40:54–22:41:39 UTC and exited 0 with all ten facts true. Missing/replaced
warnings survive Home/back, Retry stays explicit and recovers restored roots or
identity, intentional close survives restart, the original owned collection is
reopened, and no request replays. Root individually accepted all three captures,
including accurate generic warnings without the misleading folder detail.
The full native helper closure, application inputs and core fixture state stayed
unchanged; no endpoint requests were sent. Exact picker input bytes were restored
separately, preserving the raw helper's false newline-comparison flag.

Native03's all-ten AX result remains an actual passing process but its two
warning PNGs were visually rejected and are not selected for qualification04.
Recovery01's fixture-Keychain denial, old helper postcheck failures, and all
original raw results remain untouched. The dedicated fixture ACL only appends
the exact new code requirement while preserving prior trusted apps, item value
and other metadata. Current access06 completed with actual exit 0 and semantic
before/after verification. No owner/default Keychain or credential value was
read or modified; no owner password was used.

## Qualification and next actions

Qualification04 retains the original 35 unaffected requirements through explicit
impact proofs and six actual affected selector goals. Original native/source
results keep their own application, source, host, command and process bindings.
No changed app is called byte-identical to its predecessor. The eight-test then
four-test source chain and current native slice are distinct actual executions.

Original qualification02 accounts for 41 registered requirements on 099/eb9 and
all 12 positive native assertions from attempts06–12. Failed process exits stay
failed; attempt10's false keyboard fact remains excluded in favor of the genuine
attempt11 traces plus retained layout matrix. Qualification03 and its three
full-nine member reviews stay at their original source/app bindings. D2/S2/S3/S4
archives and every original source/evidence seal remain immutable. The earlier
Kirk 439 source executions, unchanged Rust 8 commands/62 executions, VM 379 frontend
executions and old captures are not new build08 executions.

After the bound qualification04 consumer passes, run only fresh C48 attempt-03;
then run fresh P11 phase with the explicit member-review carry. Preserve every
actual score/finding. `controller_complete` remains false; no final P11 acceptance
or production publication is claimed at this evidence-before-seal checkpoint.
Linux qualification and stable installed/live C26 acceptance remain separate.
No real tenant API calls, remote IAM changes, historical seal edits or RC1 asset
changes are claimed for these refinements.
