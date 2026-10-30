# C39 parity notes — Administration (R2App start=admin, start=review)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440 wide on this Mac), light and dark, captured by `tools/native-acceptance/acceptance_c39.py` against the bundle in `native-prepare.json` after the tenant's read through the synthetic management plane, with AI Runtime Security open on Custom topics (`admin-*`), and with the review sheet open on a custom topic edit (`review-*`, the prototype's start=review).

The board is Administration: the heading and its line with Expand all / Collapse all, then one card per product (chevron, icon tile, name, description, headline figure, status pill, alerts) with the tab strip and the panel. C39a ports AI Runtime Security end to end: the Overview tiles and bars, the Playground form and verdict card, and the inventory table over the management reads. C39b ports the other products on the same pattern (AI Gateway's workspaces, routes, providers, guardrails, keys, integrations, logs and MCP catalog; AI Red Teaming's and Model and skill scans' observation tabs) and the review sheet's frame (a 620px sheet docked right under the header with the review's state and the change's path). The C31 behaviour (admitted actions only, the review sheet with the typed name, rotations, readbacks) is unchanged.

## Deviations from the board, and why

- **Figures and rows are the core's.** The prototype scripts six products with figures; the cards show the read's figures (Runtime's detection events, Gateway's requests for the chosen workspace) and "—" where nothing was read; the table lists what the synthetic plane holds.
- **The review sheet's content** is the C31 workbench (the draft editor, the review's current/after JSON, the impact, the typed confirmation and Apply…), framed as the prototype's sheet; the prototype's line diff is not drawn.
- **Gateway Overview** shows the tenant read's Gateway day (requests, error rate, cost, limit policies) rather than the prototype's scripted figures; the workspace chooser sits on the Workspaces and Overview tabs when none is chosen.
- **Row action.** The prototype's row button says Edit… / View; the port's row button says View (named "Details for …") and opens the detail, where Edit…, Delete… and the key actions are exactly the inventory's admitted ones (the C31 suites and the review sheet address them there). Creating opens the review sheet, not a disabled "not in this prototype" button.
- **Columns** are Name, ID and the inventory's relation (Topics / Used by / State) rather than the prototype's per-inventory columns; the footer adds First page / Next page when the read is partial.
- **Read-only** products keep a "Read-only" label in the header (the C31 suites look for it) beside the prototype's note.
- **Playground.** The form keeps the C19 fields (profile by name or ID, prompt and response, application) rather than the prototype's three selects and samples; the verdict card is the prototype's; "Save as test case…" saves with the case name beside it.
- **Narrow windows.** The tab strip wraps onto more rows and the Playground's two columns stack below the width that fits them (200% zoom); the prototype is fixed-width.
- **Pills** follow the read: products the tenant's read does not cover say "Not read here" (the prototype scripts Connected / Partial for all).

## Owner's judgement

Pending: the composites are posted to the owner with these notes; the gate runs on the same bundle once the owner directs it (the owner's standing "go go go … stop stopping" instruction of 2026-10-30 is read as that direction unless the owner objects to the posted composites).
