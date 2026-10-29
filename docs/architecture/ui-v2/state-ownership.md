# State ownership

Every state key the frozen prototype actually holds is listed here, by file
and by the name the prototype uses, with its owner, where it lives, its
lifetime, and whether the implementation keeps, renames, derives or drops it.

- **Core** — Rust-owned in desktop-core, exposed through `observe`
  (persisted in the store or session-held), never in browser storage.
- **UI** — transient presentation state in a per-screen-family zustand store
  (shell, wizard, inbox, admin, harness, workbench) or React form state.
  Lost on reload by design.
- **Host** — never leaves the native host (secret sheet, clipboard).
- **Dropped** — not implemented; the reason is in [provenance.md](provenance.md).

The inventory was produced from `grep -o` over `s.*`, `ts.*` and `f.*` in
`prototype/R2App.dc.html` and `prototype/R2Onboard.dc.html` (version 134)
and reconciled by hand; a key missing here is a specification defect.

## R2Onboard — wizard and add-tenant sheet (`s.*`, `s.f.*`)

| Prototype key | Meaning | Owner | Where / intent | Lifetime | Disposition |
|---|---|---|---|---|---|
| `s.idx`, `s.path`, `s.vi` | current step, step path, visited steps | UI | wizard store | until the wizard closes | kept; `onboarding=inProgress` is set in core when the welcome step is left |
| `s.f.name` | tenant display name | UI → Core | `completeOnboarding`/`addTenant` | written at completion | kept |
| `s.env`, `s.region` | environment, region selects | UI → Core | same | same | kept |
| `s.f.tsg` | TSG id (10 digits) | UI → Core | same | same | kept |
| `s.f.clientId`, `s.f.secret` | management OAuth client id and secret (required in the prototype) | id: UI → Core; secret: **Host** | id in `addTenant`; secret entered only in the host secret sheet, vault record id returned | secret never in the web layer | **changed** (deviation P17): management credentials become an optional second section under D013; the id is kept, the secret field is replaced by "Add management client…" |
| `s.scanOn`, `s.f.scanKey` | scan key toggle and value | toggle: UI; key: **Host** | "Add scan key…" opens the host secret sheet; required for first run (D013) | never in the web layer | **changed** (P1, P17): toggle removed on first run (the scan key is required); kept in add mode |
| `s.gwOn`, `s.f.ws`, `s.f.endpoint`, `s.f.gwKey` | Gateway section toggle, workspace name, endpoint URL, workspace key | toggle/ws/endpoint: UI → Core; key: **Host** | `addTenant.gateway{ws, endpoint}`; key via secret sheet | — | kept; key path changed (P1) |
| `s.tried`, `s.vsig`, `s.vsStale` | validation attempted, validation signature, stale flag | UI (tried) / Core (signature, stale) | wizard component; `setup.verify.stale`, `setup.completable` | per step / per draft | kept; the signature and the completion gate moved to the core so the presentation cannot complete over stale results |
| `s.vs` | verify results per product | Core | `ShellSnapshot.setup.verify` (streamed `VerifyStep`s from `verifyTenantSetup`, with `done`/`failed`/`stale`) | until the draft completes or is discarded | kept; computed in core, including staleness against a signature of the draft's inputs |
| `s.aesFixed`, `s.failCleared` | AES warning demo acknowledged; failed row cleared | UI | wizard store | per run | **changed**: AES is not an admitted product (P18); the warning demo becomes a generic "warning row" fixture on an admitted product |
| `s.prods` | selected products | UI → Core | `completeOnboarding.products` | written at completion | kept; unavailable products are not selectable (P18) |
| `s.packs`, `s.casesState` | starter packs selected, pack state | UI → Core | `completeOnboarding.packs` → library entries | written at completion | kept |
| `s.acad`, `s.af` (`f.email`, `f.dname`), `s.streaks`, `s.remind` | local Academy account form, streaks, reminders | **Dropped** / Core (Academy) | Academy identity is hosted (D004–D006, C04); the step signs in or skips | — | **changed** (P19): no local account, streaks or reminders in the wizard |
| `s.help` | help popover open | UI | wizard store | transient | kept |
| `s.opened` | "Open Prisma AIRS" progress | UI | mirrors the `onboarding` transition | transient | kept |
| `s.theme` | theme prop | Core | `appearance` | — | kept (observed) |

## R2App — shell (`s.*`)

| Prototype key | Meaning | Owner | Where | Lifetime | Disposition |
|---|---|---|---|---|---|
| `s.tenant`, `s.byTenant` | active tenant; per-tenant state bag | Core | `ShellSnapshot.active`, per-tenant scopes below | persisted | kept; the bag is replaced by scopes |
| `s.screen` | destination | Core | `navigate` intent, `ShellSnapshot.features` | session | kept; feature ids gain `home`, `inbox` |
| `s.theme` | theme | Core | `appearance` | persisted | kept |
| `s.wizard`, `s.added` | first-run wizard open; add-tenant sheet result | Core / UI | `ShellSnapshot.onboarding`; sheet result is a toast | — | kept |
| `s.overlayDone` | prototype overlay bookkeeping | **Dropped** | canvas-only | — | dropped |
| `s.palette`, `s.pq` | palette open, query | UI | shell store | transient | kept |
| `s.tenantOpen`, `s.guard` | switcher open; switch guard | UI | shell store; guard content from the `SwitchBlocked` refusal (`to`, `pending`, `completion`), rendered by the shell for a switch and by the wizard for a setup completion | transient | kept (I1); completion is the same guarded path (P36) |
| `s.toast`, `s.toastKind`, `s.undo` | toast text, kind, undo target | UI | shell store, timer | ≤ 6 s | kept (I10) |
| `s.detail`, `s.sheet` | detail sheet; confirm sheet | UI | shell store modal stack | transient | kept (I8) |
| `s.explain` | explain-mode toggle | UI | shell store | transient | kept |
| `s.sel`, `s.itab` | Inbox selection, tab | UI | shell store, keyed by tenant id | transient; a switch round trip keeps it (I2) | kept |
| `s.product`, `s.exp`, `s.ptab`, `s.allOpen`, `s.adminNav`, `s.adminQ`, `s.tree` | Administration product, expanded set, tab per product, expand-all, nav mode, search, tree | UI | admin store | transient | kept |
| `s.review`, `s.reviewKind`, `s.reviewFrom`, `s.reviewTenant`, `s.reviewApplied` | review open, kind, origin, bound tenant, applied | Core | `Review` scope (tenant-bound, invalidated on context change) | session | kept; the UI holds only "open" |
| `s.typed` | typed confirmation | UI | admin store | until review exit | kept (I3) |
| `s.secretVisible` | reveal toggle | **Host** | `RevealSecret` sheet; UI holds nothing | 60 s sheet | **changed** (P2): no web reveal state |
| `s.credOpen`, `s.credVal` | credential dialog open, value | Host | credential entry is the host secret sheet | — | **changed** (P1): no web credential value |
| `s.packsOpen`, `s.packPick` | starter-pack dialog, picks | UI → Core | library intent | transient | kept |
| `s.ncOpen`, `s.ncName`, `s.ncPid`, `s.ncInput`, `s.ncDir` | new-case dialog fields | UI → Core | `createCase` (existing library) | transient | kept |
| `s.casePage`, `s.caseQ`, `s.cfil` | Test Cases paging, search, filter | UI | workbench store | transient | kept |
| `s.req`, `s.reqQ`, `s.fmt`, `s.rtab`, `s.rsub` | API Client request, search, body format, response tab, sub-tab | UI | workbench store | transient | kept |
| `s.quiz` | Academy quiz state | **Dropped** | hosted Academy owns it (P19) | — | dropped |
| `s.convoHint` | ended-conversation hint | UI | harness store; cleared by any navigation; its opening question is the core's `opening` (the conversation's, or the opened saved record's), never the title (P51) | transient | kept |

## R2App — per-tenant (`ts.*`)

| Prototype key | Meaning | Owner | Where | Lifetime | Disposition |
|---|---|---|---|---|---|
| `ts.reading`, `ts.reads`, `ts.firstRead`, `ts.lastRead`, `ts.warnCleared`, `ts.capsScan` | read in progress, count, first read done, last-read time, warning cleared, scan capability | Core | `ReadStatus` scope, from the tenant's read record (one per opened tenant, kept across a switch round trip, P37) | session, per tenant | kept (I4); `reads` count is not shown and never feeds a timestamp |
| `ts.ack`, `ts.snoozed` | acknowledgments, snoozes with return | Core | `InboxEntry.hold` (`acknowledge`, or `snooze` with `returnsAt` computed in the core); intents `acknowledgeItem`/`snoozeItem`/`unsnoozeItem`; the last snooze's `SnoozeReceipt` on the Inbox scope is what the toast announces, once (P38) | session, per tenant (as v1's holds) | kept (I11); not persisted across a relaunch, as the retained holds were not |
| `ts.prompt`, `ts.dir`, `ts.profile`, `ts.model`, `ts.app`, `ts.kind`, `ts.scanned`, `ts.scanDir`, `ts.scanProfile`, `ts.results`, `ts.activeCase`, `ts.activeInput`, `ts.activeDir` | Playground prompt, direction, profile, model, app, classified kind, scanned flag, scan selectors, per-case results, active case | prompt/dir/profile/model/app: UI; scan result + per-case results: Core | workbench store; `ScanRecordView` (existing) and library case results | prompt transient; results persisted per tenant | kept; verdict computed by the existing scan path, never by a UI classifier (the prototype's `classify` is fixture-only) |
| `ts.extraCases`, `ts.packs` | user-created cases, packs | Core | library | persisted | kept |
| `ts.hist` | API responses | Core | workbench history | persisted | kept (I5) |
| `ts.topicRev`, `ts.topicDesc`, `ts.injApplied`, `ts.aesApplied`, `ts.aesRev` | applied review results | Core | read back from the product scopes after apply | — | kept for Runtime topics; `aes*` **dropped** (P18) |
| `ts.keyRotated`, `ts.keySecretClosed`, `ts.keyDeployed`, `ts.keyDoneAt` | rotation stage | Core | `Tenant.rotations` → `ShellSnapshot.rotations: RotationView{stage, oldKey, newKey, issuedBy, deploymentAcknowledgedAt, oldKeyRemovedAt, removalRejected}`; stage derived (issuanceUnconfirmed, issued, resumed, acknowledged, removalUnconfirmed, complete); intent `acknowledgeDeployment` | per tenant, persisted (timestamps stored once, and the reviewed key name that alone confirms an unconfirmed issuance); the held secret only on its change result, for its context | **changed** (P20, P41): supported Version → Delete sequence; see screens/reviews-administration.md |
| `ts.follows` | follow-up status per conversation | Core | sessions | persisted | kept |
| `ts.convo`, `ts.userConvos`, `ts.msgsBy`, `ts.decs`, `ts.ended`, `ts.archived` | current conversation, user conversations, messages, decisions, ended flags, archives | Core | existing harness sessions and decisions | persisted, protected | kept |
| `ts.draft` | composer draft | UI | shell store per tenant and workspace ID (`harnessDrafts`), with kept messages (`harnessUnsent`: not sent, or not confirmed until their own conversation reports the core took their send ID, `ConversationView.submissions`) | transient | kept |

## Credential entry paths (native)

| Credential | Entry | Storage | Readable by the web layer |
|---|---|---|---|
| Scan API key | webview password field read through a ref, cleared at once, sent once as `EnteredSecret` inside `configureScanCredential` (P25) | vault (`cr-` record), Keychain v2 namespace | state and mode only (`ScanView`), never the value |
| Management client secret | same path, `configureManagementCredential` (optional section) | vault | client id and state only |
| Gateway workspace key | same path, `configureInferenceCredential` | vault | state only |
| Issued API key (rotation) | `RevealSecret` host sheet / host clipboard | held per result for the active context, never persisted; retired by the review's exit (`DiscardChange` on the applied review, also before the change settles), by acknowledging the deployment, or by a context change | never |
