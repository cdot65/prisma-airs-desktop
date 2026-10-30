# C37 parity notes — Inbox (R2App start=inbox)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440 wide on this Mac), light and dark, captured with `tools/parity/capture_app.py --chunk C37 --screen inbox --read` against the bundle in `bundle.json`. The tenant was read first, so the list carries the synthetic management plane's entries.

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

Approved by the owner on 2026-10-30 ("looks good") on the composites captured after a read with the first row opened (bundle in `bundle.json`); the gate runs on that approval.
