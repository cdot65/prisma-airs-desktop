# Prototype boards → test fixtures

Every wrapper board on the canvas page "Refined Native v2" (version 134) is
a reviewed state. Each becomes a named Vitest fixture with the same initial
props, so reviewed-state coverage is auditable from this table without the
canvas. Props are the `data-props` of `prototype/R2App.dc.html`
(`start`, `convo`, `theme`, `overlay`) and `prototype/R2Onboard.dc.html`
(`step`, `filled`, `verify`, `theme`, `mode`).

| Board | Component | Initial props | Fixture id | Primary tests | Chunk |
|---|---|---|---|---|---|
| R2App (default) | R2App | start=home, theme=light | `fx.home` | V2-T09, T10, T11 | C28 |
| R2Dark | R2App | start=home, theme=dark | `fx.home.dark` | token contrast snapshot | C28 |
| R2Palette | R2App | start=home, overlay=palette | `fx.palette` | palette filter/dispatch, N03 | C28 |
| R2Tenants | R2App | start=home, overlay=tenants | `fx.switcher` | V2-T01, T02 | C28/C30 |
| R2AddTenant | R2Onboard | mode=add, step=tenant | `fx.addTenant` | cancel semantics, no Back on the first step | C29 |
| R2AddTenant drafted | R2Onboard | mode=add, step=tenant, drafted | `fx.addTenant.drafted` | optional scan key, Continue open | C29 |
| R2On01 Welcome | R2Onboard | step=welcome | `fx.on.welcome` | copy constants | C29 |
| R2On02 Tenant | R2Onboard | step=tenant, filled=false | `fx.on.tenant.empty` | validation, aria-invalid | C29 |
| R2On02b Tenant filled | R2Onboard | step=tenant, filled=true | `fx.on.tenant.filled` | rail context line, continue enabled | C29 |
| R2On03 Products | R2Onboard | step=products | `fx.on.products` | unavailable products not selectable (P18) | C29 |
| R2On04 Verify | R2Onboard | step=verify, verify=running | `fx.on.verify.running` | streaming order | C29 |
| R2On04b Verify warning | R2Onboard | step=verify, verify=warn | `fx.on.verify.warn` | warning row → Inbox item at completion | C29 |
| R2On04c Verify fail | R2Onboard | step=verify, verify=fail | `fx.on.verify.fail` | fix-and-verify-again path | C29 |
| R2On04d Verify stale | R2Onboard | step=verify, verify=done, inputs changed | `fx.on.verify.stale` | results no longer count; later steps unreachable | C29 |
| R2On05 Packs | R2Onboard | step=packs | `fx.on.packs` | pack counts, optional | C29 |
| R2On06 Academy | R2Onboard | step=academy | `fx.on.academy` | hosted sign-in or skip (P19) | C29 |
| R2On07 Done | R2Onboard | step=done | `fx.on.done` | finite open, `onboarding=done` | C29 |
| R2Inbox | R2App | start=inbox | `fx.inbox` | V2-T04, T12, T13 | C30 |
| R2Fresh | R2App | start=home, tenant never read | `fx.fresh` | first read pending, empty Inbox (I12) | C30 |
| R2Admin | R2App | start=admin | `fxAdmin` (v2/admin/administration.test.tsx) | V2-T19, T20, per-tab labels | C31 |
| R2Review | R2App | start=admin, overlay=review | `fxAdmin` + `review()`/`rotation()` (same file) | V2-T03, T06, T08; T07 in `ui_v2_reviews` | C31 |
| R2Harness | R2App | start=harness, convo=blocks | `fxHarness` + `harness-lifecycle-blocks.json` (v2/harness/harness.test.tsx) | V2-T17 | C32 |
| R2Harness2 | R2App | start=harness, convo=mcp | `fxHarness` + `harness-lifecycle-mcp.json` (v2/harness/harness.test.tsx) | V2-T17 (Gateway MCP read, P16) | C32 |
| R2Harness3 | R2App | start=harness, convo=keys | `fxHarness` + `harness-lifecycle-keys.json` (v2/harness/harness.test.tsx) | V2-T17, T18 | C32 |
| R2Api | R2App | start=api | `fx.api` | V2-T05, T14 | C33 |
| R2Cases | R2App | start=cases | `fx.cases` | V2-T15, T16 | C33 |
| R2Academy | R2App | start=academy | `fx.academy` | embed mounts, theme | C34 |

Test ids: `V2-Txx` Vitest (desktop app), `V2-Rxx` Rust integration
(desktop-core), `V2-Nxx` native checks (`native-checks.json`). Registry of
ids: [../invariants.md](../invariants.md) plus the per-screen contracts.
