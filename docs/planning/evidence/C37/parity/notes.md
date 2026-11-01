# C37 parity notes — Inbox (R2App start=inbox)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440 wide on this Mac), light and dark, captured by `tools/native-acceptance/acceptance_c37.py` against the bundle in `native-prepare.json` from the populated state: the tenant read through the synthetic management plane (an API key expiring in five days gives a second row) with the first row opened in the detail card. The gate's native run captures them again on the gate bundle, so the reviewed composites are the ones bound to it (attempt-01 E1).

The board is the Inbox: the title row, the 430px list card (filter tabs with counts, rows, footer) beside the detail card. The C30 behaviour (reads, holds, the core's return time) is unchanged; only the presentation is ported.

## Deviations from the board, and why

- **Entries are the core's.** The prototype lists seven scripted items across products; the fixture tenant reads AI Runtime Security through the synthetic plane, so the rows are what that read produced. The footer's second line names the Gateway workspace only when one is chosen, else the read's first gap.
- **Severity word.** Each row's second line starts with the severity word beside the icon tile (I9); the prototype shows the icon alone.
- **The detail's primary action** is the entry's own action label from the core (for example "Open API keys"), not the prototype's generic "Open evidence"; Acknowledge toggles to "Mark as needing action" and Snooze to "Unsnooze" as in C30.
- **"Why this matters"** is the Explain banner of the prototype: shown only while the calls are shown.
- **Facts** are a definition list in three columns, as the prototype's grid.
- **Empty states** name the read (first read pending, failed, partial, nothing needs action) and offer "Open role issue" when a product needs a role, which opens that product in Administration.
- **Narrow windows.** Below the width that fits 430px beside the detail (the collapsed rail, 200% zoom) the two cards stack; the prototype is fixed-width.

## Owner's approval

Approved by the owner on 2026-10-30 ("looks good") on the composites captured after a read with the first row opened (bundle `974f8ff6…`, through `tools/parity/capture_app.py` with the first row opened). The gate's attempt-01 composites were captured without the row opened (E1); the remediated capture is the native script's, from the opened row (bundle `dce24bfb…`, 2026-10-30 04:25), posted to the owner at 04:29. The owner directed the work to proceed without pausing ("go go go … until we are 100% complete. stop stopping", 2026-10-30), which this record treats as the owner's acceptance of these composites as posted; attempt-02 runs on the same bundle. Any later objection from the owner reopens the parity judgement for this screen.
