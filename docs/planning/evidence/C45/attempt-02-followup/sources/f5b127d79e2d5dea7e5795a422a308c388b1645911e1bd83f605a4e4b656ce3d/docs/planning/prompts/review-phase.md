# Canonical cumulative phase reviewer prompt

Under D026 Codex performs a cumulative review after the last chunk of every
phase. `xtask review --phase PHASE_ID` writes the packet (phase chunks, all
predecessor chunk/phase receipts, the committed tree fingerprint, integrated
check/test/native results rerun against that tree, the diff since the previous
phase, limitations, schema and `VERDICT_PATH`) and sends the same transport
message as the chunk prompt. The packet's `review_instructions`:

```text
Independently review phase PHASE_ID (chunks CHUNK_LIST) of Prisma AIRS Desktop
v2 as an integrated whole. Read AGENTS.md, GLOSSARY.md, decisions, ADRs, the
implementation plan, each phase chunk instruction, review-protocol.md, and the
packet's receipts and evidence. Assess how the phase's features compose with
each other and with earlier phases they affect: authority boundaries across
features, shared state, regressions, outcome truthfulness, and whether the
integrated tests and native evidence exercise the combined behavior.

Historical chunk scores are evidence to assess, not a substitute for this
review. A defect visible only across chunks is a required fix even if every
chunk passed alone. Do not edit application files, call endpoints, read
secrets, or implement fixes.

Score architecture, behavior, authority_security, validation and overall 1-10.
Return the same verdict schema as chunk reviews with the phase as subject. A
pass requires every score >=9 and empty required_fixes/blockers/evidence_gaps.
Do not inflate scores to meet the target.
```
