# Native acceptance procedure (T09 and chunk native assertions)

`xtask native --chunk Cxx` builds the fixture-identity app with
`native-prepare` and records its executable SHA-256. Acceptance is a separate
record, `docs/planning/evidence/Cxx/native-acceptance.json`
(schema `airs-v2/native-acceptance/1`, parsed strictly by xtask), produced by
actually driving that exact executable:

- `method: "cua"` — automation through macOS accessibility/automation APIs
  against the real native window (for example REA native UI capture or System
  Events), with the captured accessibility trees/screenshots stored under
  `evidence/Cxx/native/`; or
- `method: "operator"` — a person performs the checklist on the real app and
  records observations per check.

Every check must report `pass` with an observation and evidence paths. A
`fail` or `blocked` check, a record naming different executable bytes, or a
record for the production identity fails the gate. Build success, mocked IPC
tests or reviewer scores never substitute for this record.

Prerequisites on the acceptance host: macOS grants the automation client
**Accessibility** (to read the AX tree and send key events) and, for
screenshots, **Screen Recording**. Without these grants automated acceptance is
blocked; the operator method remains available.

## Required checks

The authoritative per-chunk check list is
[native-checks.json](native-checks.json) (`airs-v2/native-checks/1`). xtask
requires the record to contain exactly those IDs, once each, with result
`pass`, a nonempty observation and at least one evidence file that the review
packet hash-binds; the record must name the fixture identity and the prepared
executable, whose current bytes are re-hashed at completion. Changing the list
is a reviewed source change (it is part of the build-flags fingerprint).

C01 covers launch/identity, native menus, keyboard-only navigation and focus,
the palette (including native navigation while it is open), unavailable
destinations, Rust-owned appearance, the accessibility tree, minimum window
size, zoom to 150%/200%, Reduce Motion, absence of Academy gamification, and
startup network behavior.

VoiceOver: C01-N08 inspects the accessibility attributes VoiceOver consumes
(AXRole, AXTitle/AXDescription, AXValue/selected state and element order) and
records exactly which were read. Spoken VoiceOver output is not claimed by an
attribute inspection; it is recorded only when an operator actually runs
VoiceOver, and otherwise remains a C24 cumulative item.
