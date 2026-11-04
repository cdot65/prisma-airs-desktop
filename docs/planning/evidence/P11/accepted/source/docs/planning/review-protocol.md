# Independent review and evidence protocol

Applies to G00/G01 as planning/design reviews, every implementation chunk and
every phase. Scope the verdict honestly: a plan review evaluates the plan and
future gates, not application code or tests that do not exist.

G00/G01 were reviewed by Claude Code; those records are historical and cover
only their recorded inputs. Under D026, Claude Code implements and Codex is the
independent reviewer for C01–C26 and the phase reviews P01–P08.

## Gate

Actual independent Codex review must score at least 9/10 overall and in each
critical dimension: architecture, behavior, authority_security, validation.
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

Owner rule D034 (2026-10-18): a chunk may complete on a review scoring at least
8 in every dimension with no blockers; its remaining findings are recorded as
accepted failures. The reviewer still scores against the 9/10 gate above. At
the end of each phase every chunk accepted below 9 is remediated and
re-reviewed until it passes the full gate, before the phase review.

## Phase reviews

After the last chunk of a phase completes, request a cumulative Codex review of
that phase (P01: C01–C03, P02: C04–C07, P03: C08–C12, P04: C13–C15, P05: C16–C18,
P06: C19–C22, P07: C23–C24, P08: C25–C26). The phase packet binds the current
committed tree, every chunk receipt in the phase and earlier phases' receipts,
the phase's integrated test/native results rerun against that tree, and the
diff since the previous phase. The reviewer assesses integration across the
phase and the earlier features it affects. The same five-score threshold and
empty required_fixes/blockers/evidence_gaps apply, with no owner acceptance
(D034), and every member chunk must hold a passing 9/10 completion. The next phase does not
begin until its predecessor phase review passes. Historical chunk scores are
inputs to assess, never a substitute.

## Review transport

The controller writes an immutable request packet (request/attempt IDs, chunk or
phase, scope, predecessor receipts, source/dependency/build-flag/evidence
fingerprints, exact commands/results/evidence paths, diff, limitations, the
verdict schema and one absolute verdict path), then delivers it:

- Preferred: the owner-designated Codex thread, addressed by exact ID through
  `codex queue --thread <id> --message <text>`; never `--last`. A prior
  reviewer-authored handshake file proves routing. Queue success is submission
  only. The controller waits with bounded progress for the matching verdict.
- Fallback: a fresh `codex exec --sandbox read-only --ignore-user-config
  --ignore-rules --ephemeral` reviewer with web search disabled, approval
  policy never, `--output-schema` and captured JSONL stdout/stderr. It is
  recorded as a fresh reviewer under a new attempt; the prior attempt is
  expired so a late verdict cannot advance the gate. No bypass flags.

The verdict must validate against the strict schema, echo the request ID and
supplied fingerprints, and the frozen inputs must be unchanged afterwards.

## Receipt fields

Record review ID, chunk/gate/phase ID, attempt, request ID, transport (thread
ID or fresh exec), time, actual harness/version/model when reported, input prompt hash, source fingerprint, dependency manifest hash,
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

Use actual Codex review. The fresh fallback runs read-only, ephemeral, without
user configuration/rules, web search, hooks or connectors; the controller verifies
the effective settings it can observe rather than assuming the sandbox disables
every tool. The preferred thread's own sandbox is not controlled by this project;
its read-only obligation is instructed and enforced by unchanged pre/post
fingerprints. The reviewer may write only its designated review artifacts.
The reviewer reads project guidance and the exact scoped files/evidence deliberately. Evidence hashes detect
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
from a successful CLI exit code or queue acknowledgement alone. It also rejects
a verdict whose request ID or echoed fingerprints differ from the packet, a
verdict for an expired attempt, and a phase completion lacking a passing phase
review.

## C45/P10 verification scheduling (owner decision, 2026-11-02)

Use verification-policy.md and the candidate's small change-impact/verification-plan record. Current source is reviewed independently; a new score/attempt/source fingerprint alone does not schedule a full native or Rust rerun. One justified C45 identity/isolation/cleanup baseline can serve both chunk and P10 reviews. Follow-up fixes execute causal/affected checks. Required coverage is partitioned into current execution, exact proof reuse, honest unaffected carry, justified not-applicable or an actual explicit owner decision; missing/edited/failed proof is rejected. Original run/verdict/artifact bindings are immutable. Historical P09 and prior gate semantics stay unchanged. C26/P08 remains open with the approved fresh read-only review fallback; no controller completion is fabricated.
