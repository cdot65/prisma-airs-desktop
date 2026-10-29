# C35 parity notes — the shell (R2App)

Composites: prototype board left, fixture app right, both at the window's web area (1440 wide; the app's height is the window minus its title bar), light and dark, captured with `tools/parity/capture_app.py --chunk C35` against the bundle in `bundle.json`. The baseline gap before the port is kept under `../parity-baseline/`.

The board is the shell: the sidebar, the header and the page frame. The page bodies (Home's greeting and cards, the Inbox list, Administration's accordion, the Harness) are later chunks (C36–C39) and are not judged here.

## Deviations from the board, and why

- **No drawn traffic lights.** The prototype paints three dots at the top of the sidebar because its window has no title bar. The fixture window has a title bar with the real controls (`tauri.conf.json` is owner-held), so the rail starts under it with 12px of padding instead of the prototype's 16px plus the 30px dots row.
- **Tenant subtitle.** The prototype shows `Production · harness-ws` (the Gateway workspace). The app shows the workspace when one is chosen, else `Production · TSG <id>`, on one line, truncated.
- **Status block.** The prototype's `Connected · 3 partial sources` is a scripted string; the app shows the core's read status (`First read pending`, `Read finished at …`, `Read failed …`) in the same place, flagged in the warning colour when the read is partial or failed. Harness shows `Signed in · SSO`, `Workspace key` or `Not signed in` from the sign-in view; Keychain follows storage health; Jobs counts a running read.
- **User row.** Shown only when an account is signed in (Harness sign-in, else the Academy account). The fixture has neither, so the row is absent.
- **Palette footer.** The prototype's third hint, `Type rt: gw: ais: aes: to search one product`, names a product-scoped search this palette does not have; it is left out.
- **Inbox badge and product counts** come from the Inbox's needs-action items; with no read yet there are none.
- **Accessibility.** Destinations are buttons with `aria-current`; the Administration children are a list under `aria-controls` from the chevron; the Explain control is a `switch`; the tenant list keeps its `group` of `aria-pressed` buttons; the destination heading takes focus after navigation (no ring, it is not a control). `FIXTURE BUILD` is the fixture identity line, not part of the design.

## Owner's approval

Approved by the owner on 2026-10-29 ("looks good, proceed") on the composites committed in f0a1a61a; the gate runs on that approval.
