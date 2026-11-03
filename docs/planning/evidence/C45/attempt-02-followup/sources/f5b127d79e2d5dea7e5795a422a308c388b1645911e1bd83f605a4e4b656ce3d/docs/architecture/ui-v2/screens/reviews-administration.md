# Reviews, Administration and key rotation

Source: `R2App` (`REVIEWS` line 826, `rvKind`, `canApply`, `doApply`,
`appliedKey`, `appliedOther`, `TOPICS`, `PROFILES`, `verdictFor`, `T` admin
tables, API keys row actions). Chunk: C31. Every change goes through the
existing Configuration review (`changes.rs`, D014): the review binds tenant,
credentials, resource and draft, and changed context invalidates it.

## Administration

- Accordion of the products in DESIGN-V2 §6. Each admitted product expands to
  tabs from `AdminInventory`, generated from the complete table in
  [administration-inventory.md](administration-inventory.md): every tab is
  mapped to an admitted read and every creation or row action to an admitted
  `ChangeKind` + `ChangeAction` with its label, and every denied action,
  unavailable tab and removed tab is named there. A product or tab with no
  admitted service renders as **Not available in this release** with the
  dependency it waits on; it never shows invented inventory and never offers
  a mutation (V2-T20, V2-R11).
- Tables: columns per tab, row actions "Edit…", "Rotate…", "Finish
  rotation…", "View", "Results". Read-only products (Red Team, Model/skill)
  never offer a change.
- Security profiles and Custom topics derive "Used by" from one topic
  inventory; detector ids and topic keys are distinct types.

## Review (I3)

- Opened by `openReview{kind, action, resource}`; the scope carries tenant,
  resource, before/after diff lines, impact (from `changes::impact`), and the
  typed-name requirement.
- The UI shows the diff, impact, a text field "Type <name> to confirm" and
  Apply (disabled until match). `confirmReview{review_id, typed}` is
  validated in the core: wrong name, different tenant or changed context
  refuses with the reason, and the UI shows a warn toast naming the tenant.
- Applied panel (non-key): "Live. The item was read back." with name,
  revision, time, tenant. Done. Unknown outcome renders the existing
  "not confirmed" state with the reconciliation step, never a success.
- Any review exit (Done, Escape, tenant switch, context retirement) calls
  `ForgetSecret` for any held one-time key and resets `secretVisible` (I6).

## Key rotation (I6, I7) — supported sequence

The prototype promised a grace period with a later revocation and a secret
recoverable from the Keychain. The retained core supports neither as one
operation: `ChangeAction::Rotate` regenerates a key in place (no grace), and
`ChangeAction::Version` creates an additional key while the existing one is
unchanged (`changes.rs` 842–846, `management.rs` 980–988). Issued keys are
held per result for the active context only and never persisted
(DESIGN.md "Secrets"). The implemented flow therefore uses the two admitted
operations in sequence and never stores a secret. It applies to Runtime
Security API keys only: Gateway service and user keys admit `Rotate`
(regenerate in place) and not `Version`, so they get the plain "Rotate…"
review with the existing one-time reveal and no staged sequence
(administration-inventory.md):

1. **Issue an additional key** — review `ApiKeys · Version` for the
   application. On apply the core reads the key back, holds the one-time
   secret in the zeroizing buffer, and the panel reads **Additional key
   issued · old key still active**: Reveal (host `RevealSecret` sheet, 60 s,
   selectable label) and Copy (host clipboard with marker, cleared after
   60 s); note "Shown once. Not stored by this app; if lost, issue another
   key"; actions **Finish later** / **Application updated**. Side effects:
   Inbox item `rotation-<application>` (Needs action, "Update
   support-copilot with the new key, then remove the old key"), with a
   snooze that returns in 24 hours and no invented deadline; the key table
   shows both keys, the old one marked "Replace after deployment".
2. **Resumed** — panel reads **Rotation in progress · secret no longer
   available**: "The new key was shown once when it was issued and this app
   does not keep it. If support-copilot does not have it, issue another key."
   Actions: **Issue another key** (returns to step 1) / **Application
   updated**.
3. **Application updated** — records a *local* deployment acknowledgement
   with its time ("Deployment acknowledged 07:51 by you on this Mac"; this is
   the operator's statement, not remote evidence) and opens the review
   `ApiKeys · Delete` for the old key.
4. **Old key removed** — on apply the core performs the delete and reads
   back; absent is the expected readback and the panel reads **Rotation
   complete · old key removed 07:52** with the readback time stored once and
   never recomputed. The Inbox item clears and the completion toast fires.
   An unknown delete outcome shows the reconciliation state and keeps the
   Inbox item.

Two timestamps are kept and labelled distinctly: `deployment_acknowledged_at`
(local statement) and `old_key_removed_at` (remote readback). Neither is
derived from read counts. `ChangeAction::Rotate` (regenerate in place) stays
available from the key row as "Regenerate…" with the existing impact text
"Replaces this key; update every integration using it"; it is not called
rotation in the UI.

## Playground verdicts

- Detector results and topic matches are independent. A profile without the
  detector, or without the topic assigned, yields "Not detected" with the
  reason; monitor-only turns Blocked into Flagged; applying a topic never
  suppresses an enabled detector. The applied topic's example prompts
  classify under the topic and show Flagged when assigned.

## Fixtures and tests

- Boards R2Admin, R2Review. Tests V2-R03 (confirmReview refusals), V2-R06
  (held key dropped on every review exit, tenant switch and context
  retirement; Reveal after drop refuses), V2-R07 (rotation state machine:
  Version → acknowledge → Delete → readback, with these named failure
  cases: (a) failed issuance: refusal shown, no key held, no Inbox item;
  (b) unknown issuance (uncertain Version outcome): state "issuance not
  confirmed" with the existing reconciliation read of the application's key
  inventory, no automatic reissue, no Inbox item until a key is read back;
  (c) rejected or failed Delete: the reminder stays, the old key stays listed,
  `old_key_removed_at` stays unset and the panel stays at stage 3 with the
  refusal reason; (d) unknown Delete outcome: "removal not confirmed" with
  reconciliation by readback, reminder retained; (e) both timestamps stable
  across reads and tenant switches), V2-T03, T06 (resumed panel has no secret
  controls), T07 (Inbox item lifecycle), T08 (timestamps labelled and
  stable), V2-T19 (unavailable product renders no inventory and no action).
- Live (C31 only): one topic review apply and read-back against the
  change-allowed tenant under the `airs-v2-c25-` prefix, with cleanup. Key
  issuance and deletion are not run live in P09; they are covered by the
  C09 fixtures and the existing C25 L-rows.
