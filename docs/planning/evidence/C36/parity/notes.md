# C36 parity notes — Home (R2App start=home)

Composites: prototype board left, fixture app right, both at the window's measured web area (1440×804 on this Mac), light and dark, captured with `tools/parity/capture_app.py --chunk C36 --screen home --read` against the bundle in `bundle.json`. The tenant was read first (Home's Refresh), so the cards carry the synthetic management plane's figures, never scripted ones.

## Deviations from the board, and why

- **Greeting.** The prototype greets a signed-in user ("Good morning, Dana"). The app greets the Harness sign-in account when there is one; without an account the heading is "Home". The fixture has no account.
- **Figures are the core's.** The Runtime card's figure is the read's detection events and its bars are the read's chart buckets (the synthetic plane answers one bucket, so one bar; the prototype draws twelve). The Gateway card shows the chosen workspace's day only when a workspace is selected; AI Red Teaming and Model and skill scans carry their read coverage (the core publishes no figures for them); the two gated products carry their gate with the New tag. Products the tenant has not chosen stay "Connecting · First read pending". Metric lines name the period the core has ("last read", "today") rather than the prototype's sample periods.
- **Recheck.** A product whose read is partial, failed or missing a role shows a Recheck control on its card (the C30 recheck); the prototype has no such control.
- **Needs attention** rows carry the entry's own action label (here "Inspect sessions") rather than the prototype's "Open evidence"; the acknowledge and snooze icon buttons are the same intents the Inbox sends; the second line starts with the severity word (Warning, Critical, Info) because colour and icon alone do not carry it (I9). The empty state names the read, as I4 requires.
- **Unknowns stay unknown.** A card's last line names what a read could not establish ("Sessions chart not read", "Usage not read", "Error count unknown", "No Gateway workspace chosen") rather than claiming an all-clear; the prototype's sample data has no such states.
- **Quick actions** are the prototype's four; "Test a prompt" opens AI Runtime Security in Administration (its Playground tab is the Runtime product's), "Keep learning" opens Academy.
- **Explain banner** shows only while the calls are shown (the fixture starts with them hidden).

## Owner's approval

Approved by the owner on 2026-10-29 ("approved.") on the composites captured after the attempt-01 remediation (bundle 39f44c84…, `bundle.json`); the gate runs on that approval.

After the approval, one change for the 200% layout check: the attention row's second line wraps instead of clipping with an ellipsis at narrow widths. At the board's width it is one line, as approved.
