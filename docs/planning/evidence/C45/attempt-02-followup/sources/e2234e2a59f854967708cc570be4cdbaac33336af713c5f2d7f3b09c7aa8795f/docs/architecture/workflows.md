# Workflow ledger and live requirement expansion

All v2 evidence is unrun. Source references name the pinned v1 baseline in
source-evidence.md; they establish behavior, not new native/live acceptance.
This expands parity-matrix.md into operation/workflow classes. Every literal
catalogue entry remains separately checked by C15 metadata fixtures, not promoted
to a dedicated GUI workflow or a mandatory real mutation.

`required` means a representative authorized live operation of that supported
class under the named L-family. Local input/race/unknown permutations use fixtures
and native tests (N03); final critical paths also map to F01–F09. Blocked access
never changes required to optional. The owner approves the concrete account/
operation/resource/budget/cleanup execution manifest before C25; this ledger
itself authorizes no live calls or production registration changes.

| ID | Workflow / exact retained subset | Chunk | Dependency/source contract | Live |
|---|---|---|---|---|
| W01 | Create/edit/select/remove scan-only tenant | C02 | v1 settings.rs/workspace.rs; independent v2 vault/context | N01 local; credential success L01 |
| W02 | Protected versus session-only credentials, verify/reset | C02 | settings.rs/local_state.rs; capability-specific adapters | L01/L02 for verification; storage N01 |
| W03 | Reviewed custom endpoints and nonsecret metadata import | C02 | C02 whitelist/destination contract; no secret/file-reference transfer | L18 destination/isolation; import N01 |
| W04 | Prompt/response/profile/application synchronous scan | C03 | SDK ec6230d2 Scanner::sync_scan + SyncScanOptions | required L01/F01 |
| W05 | Denied, cancel, timeout, unknown/partial verdict and explicit retry | C03 | SDK RequestOptions/Session/RemoteOutcome; native context | L01 representative denial; races N03/F01 |
| W06 | Save/load/edit/delete named scan cases and expected outcomes | C03,C12 | case_library.rs/settings.rs; only Runtime execution | N01; execution L01/F07 |
| W07 | Profile list/detail/create/update/delete, guided/JSON editor | C08,C09 | runtime.rs/bridge runtime.ts; accepted Phase4 exact operations/options at C08 | required L02/L03/F03 |
| W08 | Topic list/detail/create/update/delete | C08,C09 | runtime.rs can_create/edit/delete subset; Phase4 | required L02/L03 |
| W09 | Application list/detail/update/delete; no inferred dedicated create | C08,C09 | runtime.rs Kind::Apps; Phase4 | required L02/L03 |
| W10 | API key list/detail/create/additional-version/rotation/delete; no dedicated update | C08,C09 | protocol.ts key operations + runtime.rs key_version_body; Phase4 | required L02/L03/F03 |
| W11 | One-time secret reveal/copy/retirement after key operation | C09 | Rust reviewed operation + native protected store | L03 real receipt; native confinement/F03 |
| W12 | DLP catalogue/list/detail and deployment-profile browse | C08,C10 | runtime.rs DlpCatalog/Deployments; no deployment writes | required L02/L04 |
| W13 | Data-pattern create/update/delete | C10 | runtime.rs Patterns + actual DLP bridge contract | required L04 |
| W14 | Data-profile create/update; no inferred delete | C10 | runtime.rs DataProfiles | required L04 |
| W15 | Filter profile update; no inferred create/delete | C10 | runtime.rs Filters | required L04 |
| W16 | Dictionary create/replace/delete and multipart semantics | C10 | runtime.rs Dictionaries + actual bridge multipart contract | required L04 |
| W17 | Sessions inventory/filter/detail/transactions | C11 | workspace.rs/runtime.rs + accepted Phase4 reads | required L05 |
| W18 | Scan/session/detector searches with 1000-session/100-match bounds | C11 | existing investigation/search contract; visible completeness | required L05; bound permutations N03 |
| W19 | Explicit content reveal/hide and protected investigation cache | C11 | investigation_cache.rs; authorization independent of cache | required L05; storage N01 |
| W20 | FP/FN metadata edit/remove/undo/CSV export | C11 | reports stay metadata-only, no suppression/policy writes | N01/F07 |
| W21 | Home/Inbox Runtime source facts/charts/freshness/attention | C12 | cache.rs/observability worker source semantics | required L10 |
| W22 | Five-product case Library author/import/export/starters/catalog | C12 | case_library.rs; non-Runtime entries are specifications | N01/F07; Runtime execution L01 |
| W23 | Existing Red Team inventory/statistics/static reports | C13 | accepted read-only SDK family pin before C13 | required L06 |
| W24 | Existing Model/skill inventory/summaries/partial-source status | C13 | accepted read-only SDK family pin before C13 | required L06 |
| W25 | HTTP request/document edit/undo/validity/save/restart | C14 | RequestDocument fresh-snapshot/protection contract, ADR27 | N01/F06 |
| W26 | Conflict/Disk/Discard/Rename/Duplicate while newer input exists | C14 | ADR27 native production-handler ordering | N01/N03 |
| W27 | Collections/environments/variables/private secrets | C14 | rusty-core document/session contracts; explicit secret classes | N01; execution L15 |
| W28 | Collection/import/schema/Git/Rusty profile adoption | C14 | existing path/version/body/conflict guards; no imported grants | N01/F06 |
| W29 | HTTP headers/query/body/multipart/cookies/redirects | C15 | scoped native HTTP adapter + existing workbench transport subset | required L15/F06 |
| W30 | History/download/cancel/large body/error recovery | C15 | native owned files/history; explicit uncertainty | required L15 representative transfer; extremes N03 |
| W31 | Built-in SCM 193 request specifications and options | C15 | pinned catalog.json literal metadata; 81 mutation-method specs | N04; representative execution L15 |
| W32 | Managed workbench read/mutation admission, edited/imported IDs/URLs | C15 | ADR0004; exact request/review/destination/credential binding | required L15/L18/F03/F06 |
| W33 | Workbench 401/lost-response/reconnect no automatic resend | C15 | v2 stricter no-replay; preserve history/outcome | L15 representative 401; lost-response timing N03 |
| W34 | Gateway workspace list/detail/select | C16 | gateway/mod.rs/bridge gateway.ts + accepted Gateway SDK pin | required L07 |
| W35 | Provider/integration/config/config-version/guardrail/key inventories/details | C16 | exact shipped read subset; no universal resource CRUD inference | required L07 |
| W36 | Routing/model/catalog/limit prerequisites | C16 | selected Gateway workspace and capability contract | required L07 |
| W37 | Reviewed workspace/config/provider/integration/guardrail changes; integration models/workspace bindings | C17 | exact v1 GatewayOperation subset + accepted SDK options | required L08/F03 |
| W38 | Service/user key create/update/rotate/delete/limits where supported | C17 | exact key/IAM subset; rotation returns one-time secret | required L08 |
| W39 | Multi-step workspace/IAM provisioning and reconciliation | C17 | partial/unknown steps recorded; no remote rollback claim | required L08 |
| W40 | Gateway Playground streaming/model/route/cancel | C18 | intended Gateway only, accepted streaming SDK input | required L09/F04 |
| W41 | Gateway logs/telemetry/usage/cost/budget/limits/charts | C18 | operational administration retained; freshness/partial states | required L10/F04 |
| W42 | Harness start/stream/model routing/stop/crash/restart | C19 | native engine pin/protocol + new resource seal | required L16/F05; process faults N03 |
| W43 | Harness scoped key setup/company SSO/refresh-restart/history restore/sign-out | C20 | Rust native identity, stricter JWT/JWKS validation | required L16/F05 |
| W44 | Protected Harness history inspect/export/delete/resume | C20 | fresh context/protocol/grants; no pending approval restoration | N01/F05; resumed inference L16 |
| W45 | Typed investigation/read and configuration proposal | C21 | principal-restricted native authority; no engine Apply | required L16/F05 |
| W46 | Selected-project read/default and approved single-process tool | C21 | existing sandbox restrictions; no descendants/pipelines | N01 native sandbox/F05 |
| W47 | MCP sign-in/discovery/server selection/fixed conversation scope | C22 | separate native grant; at most 8 servers/64KiB bootstrap | required L17 |
| W48 | MCP call-item approve-once/deny, token-only restart/expiry/grant retirement | C22 | call item + server revision/grant + arguments/context | required L17/F05 |
| W49 | Academy guest/device sign-in/account/organization selection | C04 | hosted desktop config/allowlist; no manager role | required L11/F02 |
| W50 | Academy catalog/path/course/unit/handbook client-safe reads | C05 | Academy-owned shared learner presentation + hosted desktop projection | required L12/F02 |
| W51 | Knowledge check/lab/sandbox/progress/hosted grading | C05 | hosted attempt/progress authority; no tenant credentials in sandbox | required L12 |
| W52 | Academy feature-local gamification/cosmetics/awards/notifications | C05,C06 | actual desktop learner procedure allowlist; no global effects | required L12/L13 as hosted behavior; isolation native |
| W53 | Program/exams/practical/certificates with hosted timing/access | C06 | same hosted learner contracts; synthetic coursework only | required L13 |
| W54 | Academy preferences/avatar/export/account workflows | C06 | allowed learner procedures and scoped upload/download destinations | required L13 |
| W55 | Client-safe download/lease/encrypted read/draft save/restart | C07 | hosted desktop bundle/draft/access contracts | required L14/F02; local encryption N01 |
| W56 | Academy explicit submit/receipt/conflict/access/account retirement | C07 | no offline grading/replay; account/revision/lease ownership | required L14 |
| W57 | Reviewed user work/settings import, fresh credentials | C23 | versioned dry run/re-encryption/new v2 generation | N01/F08 |
| W58 | Interrupted import/restart/duplicate/conflict/rollback/source preservation | C23 | atomic manifest + receipt and retained v2 generation | N01/N03/F08 |
| W59 | Native menus/shortcuts/focus/VoiceOver/themes/resizing | C01–C24 | per-UI-chunk T09 plus cumulative native quality | N01/F09 |
| W60 | Local redacted diagnostics/explicit export/no telemetry | C01–C24 | canary secret/customer content tests; protected work separate | N01/F01/F09 |
| W61 | Cross-feature offline local work and no business replay | C03–C24 | T23; valid cached content versus authoritative remote state | N01/N03/F07; actual remote paths owning L-family |
| W62 | Named performance and installed-package footprint | C03–C26 | pre-registered budgets; measured native process tree | N01/F09 |
| W63 | Signed/notarized/stapled DMG, quarantine install and tamper denial | C26 | exact source/dependency/build/resource/installed-byte inventory | required final-artifact F01–F09 |
| W64 | v1 coexistence/reset/uninstall/manual update/rollback | C23–C26 | distinct namespaces, source preservation and schema compatibility | N01/F08/F09 |
| W65 | Academy Shift/Daily Incident start/serve/answer/decide/debrief/history/boards/context | C06 | bridge academy.ts + src/ui/academy.rs; slow optional briefing/ghost cannot block timed answer/serve | required L12/L13; scheduling/account races N03 |
| W66 | Academy course reset/removal with affected-course confirmation; earned certificates remain valid | C06 | src/ui/academy/workflow.rs + hosted learner allowlist | required L13 on synthetic coursework/account |

## Explicitly excluded execution

Exception Patterns remains synthetic/inert; new Red Team execution and dedicated
Model execution remain previews/stubs. Academy administration/assessment-authoring
remains web-only; legitimate learner feedback is classified by actual authority,
not an admin namespace string. Non-Runtime Library cases are specifications.
These use N02/N04 rather than invented live passes. Billing/media/server-stack
and an in-app updater remain excluded.

## Dependency closure at each chunk

C01 creates machine-readable rows/evidence fields from this ledger. Before a
consuming chunk starts, map every supported operation/option to the exact accepted
SDK/shared contract and literal fixture; unavailable methods block that chunk.
G01 selects module interfaces, ownership and these closure rules. Later accepted
SDK family pins and account credentials are not prerequisites to designing or
building the independently gated first scan. No later row is complete merely
because its family name or future SDK method appears here.
