# Normative invariants

These twelve rules are the behaviours Codex forced into the prototype over
sixteen review rounds. They are normative: an implementation chunk that
violates one fails its behaviour or authority dimension regardless of how the
screen looks. Code shape is free; the rule and its test are not.

Each rule names its acceptance test id. `V2-Txx` tests are Vitest component
tests in the desktop app (fixture-driven); `V2-Rxx` are Rust integration tests
in desktop-core; `V2-Nxx` are native acceptance checks added to
`native-checks.json` by the chunk that implements the rule.

| Id | Rule | Tests | Chunk |
|---|---|---|---|
| I1 | There is exactly one tenant-switch path. The core refuses a switch while a Harness decision is pending; the UI renders the archive-or-discard guard from the refusal and re-dispatches with the chosen resolution. | V2-R01 refusal on pending decision; V2-T01 guard rendered from refusal payload; V2-N01 switch via switcher, palette and menu all pass through the guard | C30 |
| I2 | Every observed scope is per tenant. Switching tenants never shows another tenant's Inbox items, reads, reviews, conversations, drafts or rotation state. | V2-R02 scope isolation across two tenants; V2-T02 switcher round-trip leaves per-tenant transient state intact | C30 |
| I3 | A review is bound to the tenant and resource it was opened for and applies only when the typed name matches, validated in the core. A tenant change invalidates the review. | V2-R03 confirmReview refuses on mismatch and on changed context; V2-T03 Apply disabled until names match | C31 |
| I4 | Reads are finite. The UI distinguishes "first read pending" from "no activity", reports per-product partial coverage, and the completion toast is warn-kind whenever any source is partial. | V2-R04 ReadStatus transitions; V2-T04 toast kind and text per coverage; V2-N02 pending state visible on first run | C30 |
| I5 | API responses are immutable snapshots. A later configuration change never alters an earlier response in history; a newly sent request reflects the change. | V2-R05 history entries hash-stable; V2-T05 topic applied → old response unchanged, new response includes it | C33 |
| I6 | An issued secret is disclosed only through the host (`RevealSecret` sheet, host clipboard), is held per result for the active context and never persisted; any review exit, including a tenant switch or context retirement, drops it (`ForgetSecret`) and resets the reveal state. A dropped secret is not recoverable; the UI offers to issue another key. | V2-R06 held key dropped on review exit, switch and retirement; Reveal after drop refuses; V2-T06 resumed rotation shows no secret controls | C31 |
| I7 | Key rotation is the admitted sequence additional key (`Version`) → local deployment acknowledgement → old key `Delete` with readback. Until the old key is removed, the reminder is a distinct Inbox item with its own identity and stated return time and the key table shows both keys. Two timestamps are stored once and labelled distinctly: deployment acknowledged (local statement) and old key removed (remote readback). A failed or unknown issuance never reissues automatically; a rejected or unknown Delete keeps the reminder and leaves the completion timestamp unset; unknown outcomes are shown as unconfirmed with reconciliation, never as complete. | V2-R07 state machine incl. failed/unknown issuance and rejected/unknown delete; V2-T07 Inbox item lifecycle; V2-T08 timestamps labelled and stable after refresh | C31 |
| I8 | One topmost modal. Siblings of the topmost dialog are `aria-hidden`; Escape closes only the topmost; focus returns to the opener. | V2-T09 stacking; V2-N03 keyboard-only dialog traversal | C28 |
| I9 | A verdict or severity is colour plus icon plus word, never colour alone. | V2-T10 every verdict renderer emits all three; V2-N04 accessibility tree carries the word | C28 |
| I10 | Toasts have a kind (ok, info, warn), expire on a timer, and are presentation only: everything a toast says is derivable from an observed scope. With Reduce Motion on, toasts persist until dismissed. | V2-T11 kinds and expiry; V2-N05 Reduce Motion persistence | C28 |
| I11 | Inbox acknowledge and snooze are per item, core-owned, with the return time stated wherever the action is offered (Home, Inbox row, Inbox detail) and identical across them. | V2-R08 snooze return computed in core; V2-T12 Home and Inbox labels equal | C30 |
| I12 | A fresh tenant never shows seeded or sample data. Its first Harness send creates a user conversation; its Inbox is empty until the first read lands. | V2-R09 fresh tenant scopes empty; V2-T13 first send creates conversation | C30, C32 |

Rules I1, I3, I6 and I7 are authority rules under D014: the core owns the
decision, the UI renders it. No invariant may be satisfied by invented data: a
product, tab or operation without an admitted service renders as unavailable
(DESIGN-V2 §6), and a test that passes only on fixtures for such a capability
is a specification defect.
