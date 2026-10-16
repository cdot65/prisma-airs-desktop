# Independent review and evidence protocol

Applies to G00/G01 as planning/design reviews and every implementation chunk.
Scope the verdict honestly: a plan review evaluates the plan and future gates,
not application code or tests that do not exist.

## Gate

Actual independent Claude Code review must score at least 9/10 overall and in
each critical dimension: architecture, behavior, authority/security, validation.
No blocking or required fixes may remain. Missing mandatory evidence, stale
inputs, or a mismatched receipt prevents completion independently of scores.

The implementer cannot substitute its own score or edit the reviewer's verdict.
Do not ask a reviewer to raise a score; fix findings and request a new review.
Retain all attempts and explain remediation. Optional suggestions may remain
only when explicitly classified as nonrequired and consistent with the plan.

## Dimensions

- **Architecture:** depth at domain interfaces; locality; justified seams/real
  adapters; authoritative ownership; no needless pass-through layers or hidden
  cross-module coupling. Use the glossary and accepted ADRs.
- **Behavior:** complete chunk requirements, exact working-v1 subset, correct
  success/denied/empty/stale/partial/unknown outcomes, and no invented support.
- **Authority/security:** context/credential isolation, review/approval binding,
  trusted hosted assessment, protected storage, safe external destinations,
  bounded execution, provenance, Node-free shipped backend, and secret hygiene.
- **Validation:** meaningful production-interface tests; independent external
  expectations; required native/live/artifact evidence; reproducibility, hashes,
  explicit limitations, and causal failure cases. Test counts alone earn no score.

Scores below 9 require concrete findings with locations, impact, and acceptance
conditions. Even a 9/10 verdict with a required fix cannot pass.

## Receipt fields

Record review ID, chunk/gate ID, attempt, time, actual harness/version/model when
reported, input prompt hash, source fingerprint, dependency manifest hash,
check/evidence manifest hash, raw output hash, reviewer scores, findings,
required fixes, blocking findings, limitations, and whether source stayed frozen.
For final packages also bind pre-sign/post-sign/stapled artifact hashes and the
installed-byte inventory. A review of a different tree or package cannot pass.

The input fingerprint includes tracked/nonignored source, instruction files,
configuration, file modes/symlink targets, and exact compiled dependency inputs.
Document narrow exclusions for review outputs/mutable status to avoid circular
hashes. Exclusions cannot hide application sources, lockfiles, generated bindings,
build flags, fixture rules, tests, notices, or package inventories. Evidence and
status hashes remain independently recorded even when excluded from source hash.

Use actual read-only CLI execution with only reading/search tools and no write,
shell, browser, external connector, or application-network tools. Disable hooks,
custom plugins/MCP, and persistence for the review. The reviewer reads project
guidance and the exact scoped files/evidence deliberately. Evidence hashes detect
stale/tampered records; they are not cryptographic proof of provider authorship.

## Completion and invalidation

All predecessors must have passing receipts. Checks must be executed against
the same inputs and exact intended feature/build flags. Record each result as
passed, failed, blocked, or not applicable with a reason. An unavailable required
check is blocked, never silently not applicable.

On a failed gate: keep chunk active, fix findings, rerun affected checks plus
any regression surface the fix reaches, freeze new inputs, and obtain a fresh
independent review. A dependency update reopens affected consumers. Final release
revalidates cumulative behavior and source-bound prior evidence; it cannot merely
sum historical 9s from incompatible versions.

Completion is followed by a focused commit whose tree is verified against the
reviewed source. Changes to source after review require a new relevant review.
Git publication uses normal pushes to the specified v2 remote; no force-push.

## Evidence handling

Repository summaries contain reproducible commands, environments, hashes,
nonsecret fixture IDs, pass/fail/blocked status, and limitations. Keep real
credentials, prompts/responses, tokens, Keychain exports, unredacted traces,
customer records, signing secrets, and signed-in browser captures private.
Redact output before publication; retain original sensitive evidence in private
operator storage with a hash reference when appropriate.

The C01 controller must reject a score8 in any dimension, required fixes despite
score9, missing check results, changed dependency/source hashes, edited raw output,
duplicate/invalid receipt IDs, changed build flags, malformed schemas, and missing
native/artifact proof. Completion state is not writable by a reviewer or inferred
from a successful CLI exit code alone.
