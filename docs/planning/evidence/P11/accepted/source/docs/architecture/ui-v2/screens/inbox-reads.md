# Inbox and reads

Source: `R2App` (`ITEMS`, `visibleItems`, `ack`, `snoozed`, `snooze`,
`inboxRows`, `sel`, `startRead`, `recheck`, `toGo`). Chunk: C30.

## Inbox

- Tabs All / Needs action / Updates / Snoozed with counts. Rows: severity
  (colour + icon + word), title, product, source line ("API keys · cached",
  "rotated 07:46"), when, acknowledged mark. Selecting opens the detail pane:
  body, facts table, "Why this matters" explanation, action button (navigates
  with context), Acknowledge / Mark as needing action, Snooze or Unsnooze.
- Empty states distinguish "first read pending" from "nothing needs action"
  and name partial sources ("3 of 6 sources were read at 07:45").
- Snooze: per item, core-computed return time, 24 hours for every item in
  P09 (the prototype's deadline-bound rotation snooze is not carried: no
  admitted operation has a grace deadline, P20). The label, toast and
  Snoozed-tab "Returns …" text all state the same time (I11). The
  `deadline` field stays optional for a future item that has a real one.
- Contextual navigation: an item's action carries a target (screen, product,
  tab, conversation). Into an ended conversation it sets a transient hint
  offering "Open the saved transcript" and "Start a new investigation".

## Reads (I4)

- `readTenant` runs a finite read of every selected product; `ReadStatus`
  carries per-product state (pending, ok, partial, needsRole), last-read
  time and counts. First read on a fresh tenant shows pending everywhere.
- Completion toast: ok "Read finished at 07:45 · all sources complete" or
  warn "· 3 sources partial"; for fresh tenants "· 1 product still needs a
  role" or "· no activity yet in <tenant>". `recheckProduct` after a role is
  granted shows an info toast and re-reads.

## Core contract

- Scope `InboxItem{id, kind, severity, title, product, source, when, body,
  facts, action_target, explanation, deadline?, snooze_return?, acknowledged}`
  per tenant; intents `acknowledgeItem`, `snoozeItem`, `unsnoozeItem`,
  `readTenant`, `recheckProduct`.
- The rotation reminder (`rotation-<application>`) is a distinct item id from
  the expiry alert and clears only when the old key's deletion is read back
  (I7). Items exist only for admitted products (DESIGN-V2 §6).

## Fixtures and tests

- Board R2Inbox. Tests V2-R02, R04, R08, R09; V2-T04, T12, T13; V2-N02.
