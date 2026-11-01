# L19 attempt-01 — interrupted by the driver's early-settlement check

The driver treated the presence of Verify again as completion. The retained
native tree instead shows Checking…, Management client / Reading… / Pending.
It then navigated back and removed both temporary credentials, potentially
retiring the in-flight read. This attempt does not prove a failed credential
or a successful verification and cannot close the live row. The raw receipt
and captured tree are preserved unchanged.

One read remains conservatively charged. The next attempt uses the terminal
verification banner, keeps the pending state pending, and shares an append-only
private reservation ledger with this attempt. The approved maximum remains two
reads; no reservation is refunded and a third is refused before admission.
The actual pending tree is the offline regression fixture.
