# First-run wizard and add-tenant sheet

Source: `prototype/R2Onboard.dc.html` (steps, `ERR`, `touch`, `vsig`,
`verify`, `opened`, `railCtx`, `mode="add"`), `R2App` `addtenant` overlay.
Chunk: C29. Realises D013 (scan-key-first onboarding).

## Steps (first run)

1. **Welcome** — explains that the app is built entirely on Palo Alto
   Networks' public APIs and exists to show application builders what those
   interfaces make possible; three cards: Built on the public APIs / See the
   calls as you go / Learn by doing. Continue.
2. **Tenant** — name, environment (select), region (select), TSG id, then
   three credential sections in this order (D013, deviation P17):
   **Scan key** (required on first run): "Add scan key…" opens the host
   secret sheet and returns a vault id shown as a masked indicator.
   **Management client** (optional): client id field plus "Add client
   secret…" (host secret sheet); without it Administration is read-only and
   says so. **Gateway** (optional toggle): workspace name, endpoint URL,
   "Add workspace key…". Secondary action "Import from Prisma AIRS Desktop
   v1" fills name/environment/TSG from the import intent; secrets are always
   re-entered. Validation per field on blur with
   `aria-invalid` and `aria-describedby`; the Continue button names what is
   missing. After this step the rail shows "Name · Environment · TSG …" under
   its title.
3. **Products** — cards for the products in DESIGN-V2 §6. Admitted products
   show licence state read from the tenant and are selectable; Red Team and
   Model/skill are marked read-only; Agentic Identity Security and Agentic
   Endpoint Security are shown as "Not available in this release" and cannot
   be selected (P18).
4. **Verify** — dispatch `verifyTenantSetup`; per-product rows fill as
   `VerifyStep` results stream (pass / warning / fail with reason) using the
   existing reads for each admitted product (scan capability check, a
   management read when a client is present, Gateway prerequisites).
   Unavailable products show "Not available", never a result. The warning
   and failed rows are fixtures on admitted products. Fail offers "Fix and verify
   again" (returns to Tenant) or "Continue with warnings"; warnings become
   Inbox items on completion.
5. **Starter packs** (optional) — test-case packs per selected product with
   counts.
6. **Academy account** (optional) — sign in or skip; the trust copy states
   exactly what syncs (Academy progress only).
7. **Done** — summary; "Open Prisma AIRS" is finite (`opening` → done) and
   dispatches `completeOnboarding`.

## Add-tenant sheet (`mode="add"`)

- 1160×780 sheet over the shell, rail 244px, no Welcome or Academy steps,
  Cancel at every step; completion dispatches `addTenant` then `switchTenant`.
  Cancel discards everything except a credential already written by the
  secret sheet, which is shown in Administration as unattached and can be
  removed there.

## Core contract

- `verifyTenantSetup{draft}` → stream of `VerifyStep{product, state, reason}`
  then `VerifyDone{warnings}`.
- `completeOnboarding{name, env, region, tsg, scan_credential_id,
  management: {client_id, credential_id}?, gateway: {ws, endpoint,
  credential_id}?, products, packs, academy_opt_in, warnings}` writes the tenant profile, product
  selection, packs and Inbox warning items; sets `onboarding=done`.
- `addTenant{...same minus academy}`.
- The core never receives a secret through these intents; `credential_ids`
  reference vault records created by the host secret sheet.

## Fixtures and tests

- Boards: see [boards.md](boards.md) (`fx.on.*`, `fx.addTenant`).
- Tests: step validation and invalidation signature; streaming verify order;
  cancel semantics; `onboarding` transitions; V2-N02.
- Live (C29 only): verify against Vulture, reads only, under the designated
  scope.
