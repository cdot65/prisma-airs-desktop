# Administration inventory: every prototype product, tab and action

Source: `prototype/R2App.dc.html` `PRODUCTS` (tabs, `single`, `readOnly`),
the `T` tables (`rt:Security profiles`, `rt:Custom topics`, `rt:API keys`,
`gw:Routes`, `rtm:Scans`, `sc:Skill scans`, `ais:Tool policies`,
`aes:Discovered AI`, `aes:Policies`) and row actions (line 1099). Admitted
operations are the retained core's: `ManagementResource` reads and
`changes::allowed(kind, action)` (`changes.rs` 253–268), `GatewayRead` and
`gateway_changes::allowed` (`gateway_changes.rs`), and the C13 read-only
observation functions in `red_team.rs`. The `AdminInventory` scope (C31) is
generated from this table and tested against it (V2-T20); the core refuses
any (kind, action) outside the matrices (existing `allowed`, V2-R11).

Dispositions: **Read** (admitted read, rows shown), **Change** (admitted
reviewed change with the exact actions), **Unavailable** (no admitted
service: tab renders "Not available in this release" with the gate, no rows,
no actions), **Removed** (prototype tab not carried, with reason).

## AI Runtime Security (`rt`)

Reads: `ManagementResource::{Profiles, Topics, Applications, ApiKeys,
DeploymentProfiles, DlpCatalog, DataProfiles, DataPatterns,
FilteringProfiles, Dictionaries, Sessions}`.

| Tab | Disposition | Reads | Admitted actions (`ChangeKind` · `ChangeAction`) | Denied in the UI (never offered) | Fixture / contract |
|---|---|---|---|---|---|
| Overview | Read | counts from the reads above; `ScanRecordView` | — | — | C08/C12 fixtures |
| Playground | Read + scan | `scan.rs` (existing synchronous scan) | — | — | C03 scan fixtures |
| Security profiles | Change | Profiles | Profiles · Create ("New profile…"), Update ("Edit…"), Delete | — | C09 fixtures |
| Custom topics | Change | Topics | Topics · Create ("New topic…"), Update ("Edit…"), Delete | — | C09 fixtures |
| Applications | Change | Applications | Applications · Update ("Edit…"), Delete | **Create** (not admitted; no "New application…") | C09 |
| API keys | Change | ApiKeys | ApiKeys · Create ("New API key…"), Version ("Issue additional key…"), Rotate ("Regenerate…"), Delete ("Remove…") | **Update** (not admitted; no "Edit…") | C09 one-time secrets |
| DLP | Change | DlpCatalog, DataPatterns, DataProfiles, FilteringProfiles, Dictionaries | DataPatterns · Create/Update/Delete; Dictionaries · Create/Update/Delete; DataProfiles · Create/Update; FilteringProfiles · Update | **DataProfiles · Delete**, **FilteringProfiles · Create/Delete**, **DlpCatalog · any** | C10 fixtures |
| Deployment profiles | Read | DeploymentProfiles | — | any change | C08 |

## AI Gateway (`gw`)

Reads: `GatewayRead::{Inventory, Detail, IntegrationModels, Prerequisites,
Observe, McpServers}`.

| Tab | Disposition | Reads | Admitted actions | Denied in the UI | Fixture / contract |
|---|---|---|---|---|---|
| Overview | Read | Observe, Prerequisites | — | — | C16/C18 |
| Workspaces | Change | Inventory/Detail (workspaces) | GatewayWorkspaces · Create ("New workspace…"), Update, Delete | Rotate | C16/C17 |
| Routes | Change | Inventory/Detail (routes) | GatewayRoutes · Create ("New route…"), Update, Delete | Version | C17 |
| Providers | Change | Inventory/Detail (providers) | GatewayProviders · Create ("New provider…"), Update, Delete | Models | C17 |
| Guardrails | Change | Inventory/Detail (guardrails) | GatewayGuardrails · Create ("New guardrail…"), Update, Delete | — | C17 |
| Keys | Change | Inventory (service keys, user keys) | GatewayServiceKeys / GatewayUserKeys · Create ("New key…"), Update ("Edit…"), Rotate ("Rotate…", regenerates in place: `GatewayApiKeyRotateRequestSchema`, one-time reveal through the host, no staged sequence), Delete ("Remove…") | **Version** (not admitted: no "Issue additional key…", no "Finish rotation…", no rotation reminder) | C17 |
| Integrations (prototype folds into Providers) | Change | IntegrationModels | GatewayIntegrations · Create, Update, Delete, Models ("Replace model list…") | Rotate | C17 |
| Limits | **Removed** | — | — | — | the prototype's route budget limits have no admitted read or change; budgets appear in Routes detail only when the service returns them (C16) |
| Logs | Read | Observe (telemetry) | — | any creation (no "New…" on an activity tab) | C18 |
| MCP servers (in AIS in the prototype) | Read | McpServers | — | — | C22; used by Harness lifecycle 2 (P16) |

## AI Red Teaming (`rtm`, read-only, `readOnly: true`)

Reads: `red_team.rs` — `statistics`, `red_team_scans`, `attacks` (paged).

| Tab | Disposition | Reads | Actions | Fixture |
|---|---|---|---|---|
| Overview | Read | statistics | none | C13 |
| Targets | Read (identifiers and types only) | `target_id` and `target_type` as carried by `red_team_scans` (`red_team.rs` 56–66); target names are not admitted by the C13 privacy boundary and are never shown | none; "Results" opens the scan | C13; deviation P22; V2-T21 asserts no target name appears in the rendered tab or in any scope the tab reads |
| Scans | Read | red_team_scans | none | C13 |
| Attack results | Read | attacks | none | C13 |
| Reports | Read | static reports (existing C13 metadata-only) | none; no execution | C13 |

New scans, re-runs and "hand the spec to a scan" never dispatch anything;
Test Cases "Inspect" applies (P21).

## Model/skill observability (`sc`, "AI Supply Chain" in the prototype)

Reads: `red_team.rs` — `model_scans`, `skill_scans` (pseudonymised),
`skill_statistics`.

| Tab | Disposition | Reads | Actions | Fixture |
|---|---|---|---|---|
| Overview | Read | skill_statistics, model_scans counts | none | C13 |
| Model scans | Read | model_scans | none | C13 |
| Skill scans | Read | skill_scans | none | C13 |
| Codebase scans | **Unavailable** | none admitted | none | gate: an admitted codebase-scan read |
| Policies | **Unavailable** (`single: 'policy'` not carried) | none | none; no "New policy…" | gate: an admitted supply-chain policy service |

The product is shown under its C13 name ("Model and skill scans"), not as
a scanning product.

## Agentic Identity Security (`ais`) — Unavailable

| Tab | Disposition |
|---|---|
| Overview, Tool policies, Identities (Idira), Invocation log | **Unavailable**: "Not available in this release · needs an admitted AIS service (D058)". No rows, no "New policy…", no review. |
| MCP servers | carried as a Gateway tab (above) because the read is `GatewayRead::McpServers` |

## Agentic Endpoint Security (`aes`) — Unavailable

| Tab | Disposition |
|---|---|
| Overview, Endpoints, Discovered AI, Extensions & packages, Policies | **Unavailable** with the same notice. The prototype's `REVIEWS.aes` policy review, the `aes:Discovered AI` and `aes:Policies` tables and the wizard AES warning demo are fixtures only and are not implemented. |

## Row actions

| Prototype action | Carried as | Condition |
|---|---|---|
| Edit… | review `Update` | admitted Update for the tab's kind |
| Rotate… (Runtime API keys) | review `ApiKeys · Version` labelled "Issue additional key…" (the staged rotation sequence of screens/reviews-administration.md) and, separately, `ApiKeys · Rotate` labelled "Regenerate…" | Runtime Security API keys only |
| Rotate… (Gateway keys) | review `GatewayServiceKeys`/`GatewayUserKeys · Rotate` labelled "Rotate…": regenerate in place with the existing one-time reveal; no additional-key step, no acknowledgement, no Inbox reminder | Gateway service and user keys only; `Version` is not admitted for Gateway keys |
| Finish rotation… | opens the Runtime rotation panel (local acknowledgement, then review `ApiKeys · Delete` of the old key) | Runtime rotation in progress only |
| View | detail sheet (read) | always for Read tabs |
| Results | detail of a read-only scan | Red Team / Model-skill |

## Tests

- V2-T20: for every (product, tab) above, the rendered tab matches its
  disposition: Read/Change tabs show rows and exactly the listed actions;
  Unavailable tabs show the notice and no rows or actions; Removed tabs do
  not exist.
- V2-R11: the core refuses every (kind, action) pair in the "Denied" column
  (already enforced by `allowed`; the test pins the matrix so a UI label
  cannot outrun it), including `GatewayServiceKeys`/`GatewayUserKeys ·
  Version`.
- V2-T20 additionally asserts that Gateway key rows render "Rotate…" and
  never "Issue additional key…" or "Finish rotation…", and that no Gateway
  rotation creates an Inbox reminder.
- V2-T21: the Red Team Targets tab renders identifiers and types only; a
  fixture scan reply carrying a `target_name` field is rendered without it
  and the field never reaches the `AdminInventory` scope.
