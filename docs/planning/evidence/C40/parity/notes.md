# C40 parity notes — Test Cases (R2App start=cases)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440 wide on this Mac), light and dark, captured by `tools/native-acceptance/acceptance_c40.py` against the bundle in `parity/bundle.json` after the bundled starter packs were loaded through the Load starter packs… dialog.

The board is Test Cases: the title row with Load starter packs… and New case, the Explain banner while the calls are shown, one table card (search, product pills with counts, the rows with the result pill and the action with its ⋯ menu, the footer with the range and pages), the packs and new-case dialogs (560px) and the detail sheet (520px). The C33 behaviour (results recorded only by a scan run in the Playground, packs, Runtime-only Open in Playground, Inspect, import, export and copy) is unchanged.

## Deviations from the board, and why

- **Rows are the core's.** The prototype scripts nine cases across six products with results; the fixture lists the bundled starter packs (Runtime Security cases and specifications for the other products), their results recorded only once a scan ran in the Playground.
- **Starter packs** are the app's two sources (the bundled packs, checked by default, and the public example catalog) rather than the prototype's per-product packs with counts; the operator's own cases move by Import… / Export… from the same dialog.
- **The ⋯ menu** holds Remove and Copy to <tenant> (the prototype's menu is a placeholder).
- **Result pills** say Blocked / Flagged / Allowed / Not determined as the core's verdict words; the prototype's Detected / Denied belong to products whose results the desktop does not record.
- **The new-case dialog** keeps the C12 fields (security profile and application for a Runtime case; the product payload for a specification; the goal) rather than the prototype's direction select.
- **Narrow windows.** The table's five columns keep their proportions and the action column its 190px until 200% zoom, where the card scrolls horizontally inside itself rather than the page; the dialogs are bounded to the viewport.

## Owner's judgement

Pending: the composites are posted to the owner with these notes; the gate runs on the same bundle once the owner directs it (the owner's standing "go go go … stop stopping" instruction of 2026-10-30 is read as that direction unless the owner objects to the posted composites).
