# Shell, Home, tenant switcher, palette, toasts

Source: `prototype/R2App.dc.html` (sidebar markup, `nav`, `switchPatch`,
`requestSwitch`, `palette*`, `toast*`, `top`, Home section). Chunk: C28 (shell,
Home skeleton), C30 (Home attention list backed by Inbox scope).

## Sidebar

- Top: tenant switcher button showing tenant name and environment pill;
  ⌘T opens the list. Entries: every tenant (name, environment, TSG), then
  "Add a tenant…". Selected tenant uses `sel/selInk`.
- Destinations in the order of DESIGN-V2 §5 with icon, label, shortcut hint.
  Unavailable destinations render disabled with their availability reason as
  a tooltip and accessible description, never hidden.
- Bottom: appearance toggle (light/dark/system) and "Show the calls" toggle,
  both dispatching their Rust preference.
- Collapses to icons below 1280px window width.

## Home

- Title "Home" with the tenant name and environment beneath; last-read line
  from `ReadStatus` ("Read finished at 07:45 · all sources complete" or
  "· 3 sources partial", warn-styled when partial; "First read pending" on a
  fresh tenant).
- "Needs attention": up to five Inbox items of kind Needs action, each with
  severity (colour + icon + word), title, product, action button that
  navigates with context, Acknowledge, and Snooze whose tooltip and
  accessible name are the item's `snoozeLabel`.
- Product cards for admitted, licensed products with their read state;
  unlicensed products are listed once as "Not licensed in this tenant" and
  unavailable products once as "Not available in this release" (DESIGN-V2
  §6), never hidden.
- Playground card (Runtime Security scan): prompt, direction, profile select,
  verdict panel (word + icon + reason + plain explanation). Verdicts are
  profile-aware: a detector not enabled, or a topic not assigned, reads "Not
  detected" with the reason; monitor-only turns Blocked into Flagged with the
  profile named.

## Tenant switch (I1, I2)

- `switchTenant{tenant_id, resolution?}`; the core refuses with
  `SwitchBlocked{pending: {conversation, decision}}` when a Harness decision is
  pending. The UI renders the guard dialog (Archive and switch / Discard and
  switch / Stay) from that payload and re-dispatches with `resolution`. Initial
  focus is on Stay, the safe default (Enter on arrival never ends the
  conversation); Archive and switch stays the visually primary action.
- Every entry point (switcher, palette, menu, wizard completion) uses the same
  intent. After a switch the new tenant's Home renders from cached scopes within
  100 ms with read status pending until the read lands.

## Command palette

- ⌘K. Entries: destinations from the Rust inventory, tenants, and three actions
  (Navigate, Switch tenant, Read now). Typing filters; Enter dispatches; Escape
  closes and restores focus. Native menu navigation while the palette is open
  closes it first.

## Toasts (I10)

- Kinds ok, info, warn with icon and colour; one visible at a time, newest
  replaces; 6 s expiry (persist under Reduce Motion); optional Undo action
  (snooze). A toast never carries information absent from an observed scope.

## Modals (I8)

- Stack order kept in the shell store; only the topmost is interactive;
  siblings get `aria-hidden`; Escape closes the topmost; focus returns to the
  opener. Detail sheets (transcripts, facts) are modals in this stack.

## Fixtures and tests

- Fixture boards: R2Palette, R2Tenants, R2Dark, R2App default.
- V2-T01, T02, T09, T10, T11; V2-N01, N03, N04, N05.
