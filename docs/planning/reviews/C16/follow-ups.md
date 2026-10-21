# C16 follow-ups for the P05 push to 9/10 (D034)

C16 completed at 8/10 under D034. Codex attempt-01 scored 9/8/9/8/8 with no
blockers and two required fixes. Both are remediated in C17, which edits the
same Gateway view, and are re-reviewed with it and at the P05 push.

- R1, integration models bound to the integration shown: DetailPanel keeps
  its models across a change of row and commits a model reply without
  checking which integration is current, so A's models (or a late A reply)
  can appear under B. The fix keys model state to the integration and drops
  replies for any other. A regression test drives both transitions (cached
  and pending A, then B) through App/DesktopClient.
- R2, the chosen workspace's detail readable from the UI: no control reads
  `Detail { resource: workspaces }`, so a workspace's defaults and embedded
  usage/rate limits are unreachable. The fix adds a read-only, keyboard-reachable
  "Workspace details" view of the selected workspace with
  loading/denied/error states, retired on selection and context changes, with
  App/DesktopClient tests and native keyboard/focus evidence.

## Remediation (P05 push)

**R1: models are bound to the integration shown.**
- Code (`apps/desktop/src/features/gateway/gateway-view.tsx`, landed with C17):
  - `InventoryPanel` keys `DetailPanel` by the item's ID. Opening another row
    mounts a fresh panel, so no models, loading state or failure carry over.
  - `readModels` commits a reply only while its integration is still the
    one shown (`shownId`), and only when the reply names that integration.
- App test `never shows one integration’s models under another`: A's models
  read, then B opened; A's read still out when B is opened, then released;
  B read, showing only B's.
- Native C16-N10 (new) does both transitions in the fixture app with two
  integrations. A's model read is delayed by the loopback service; B never
  shows A's models, count or "Reading models…", and reading B lists only
  B's model.
- Probe: `a detail keeps another integration's models`.

**R2: the chosen workspace's detail is readable.**
- Code: a Workspace tab (the first tab) reads `Detail { resource: workspaces }`
  for the chosen workspace as an unscoped read (`readWorkspace`). It shows:
  - every projected field, including `defaults` and the embedded
    `usage_limits` and `rate_limits`;
  - "Reading the workspace…" while a read is out;
  - the failure, named, in an alert;
  - a "not returned" note.
- Retirement: the read is tied to the selection generation and its own
  token. A reply for the workspace before, or after a context change, is
  dropped. Another workspace starts with nothing held.
- App test `shows the chosen workspace’s own settings and limits, and its failures`
  now also holds a refresh of A, chooses B, releases A, and checks that only
  B's detail is shown, with no failure carried over.
- Native C16-N11 (new):
  1. Shows Team B's own settings, defaults and limits.
  2. Reaches "Refresh workspace" by Tab and presses it with Space; the read
     is sent and focus stays on the button.
  3. Chooses Team A and shows only A's settings.
- C17-N08 also reads the workspace detail natively.
- Probes:
  - `a late workspace detail is shown under another workspace`
  - `the workspace detail is never read`
