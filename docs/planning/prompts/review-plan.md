# Independent plan/design reviewer prompt

Use for G00 planning; for G01 additionally supply selected candidate records,
ADRs, concrete interface/command/provenance decisions, and architecture evidence.

```text
Independently review the Prisma AIRS Desktop v2 planning package in this checkout.
Read all non-review Markdown files, AGENTS.md/CLAUDE.md and GLOSSARY.md, including
every chunk instruction and canonical harness prompt. Review the user's accepted
D001-D025 decisions against plan coverage and sequencing. Read-only; do not edit,
run shell commands, call endpoints, or open secrets.

For G00, evaluate the plan, dependency/test/acceptance design, and executable future
handoff. Application implementation and G01 selected interfaces are intentionally
not started; do not pretend to review them or require them to be implemented now.
Do flag a missing concrete future gate, contradictory decision, unsupported claim,
overbroad chunk without observable acceptance, unsafe authority, or parity gap.

Check fresh Tauri/Rust + React foundation, selective framework reuse, shared Academy
source/hosted learning authority and feature-local gamification, direct Rust SDK
with staged availability/macOS qualification, native Harness/no-Node host migration,
API Client parity and mutation admission, credential/context authority, explicit
offline/no replay, v1 coexistence/import rollback, native/accessibility/privacy/
performance budgets, controlled live scope, signed installed DMG and manual updates.

Score 1-10 each for architecture, behavior, authority/security, validation, overall.
Give required fixes with locations, impact, and acceptance conditions; identify
blockers, evidence gaps, optional suggestions and limitations separately. Missing
application evidence is expected for G00; missing plan requirements is not.
Pass only if every score >=9 and no required fixes/blockers/evidence gaps remain.
Be candid; the target is not permission to round up or ignore problems.
```
