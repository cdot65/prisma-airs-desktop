Recorded UTC 2026-11-02T04:14:01.805746+00:00

# Supplementary read-only audit — pending independent verification

This is supplementary orchestration evidence, not an independent numeric verdict, runtime reproduction or completed fix. It records static observations before the next C30 review. Original attempt10 verdicts/receipts and generation05 native outputs are unchanged. Do not interrupt a native/performance driver. This documentation path is outside the sealed build-input inventory; it changes no application bytes, native driver/helper or fixture state.

## Captured-null initial selection remains unbound

A remaining renderer navigation/retry edge appears production-reachable:

1. Settings confirms deletion through features/tenants/tenants-settings.tsx271-274.
2. Native authority.rs3986-3989 removes the active tenant but retains other tenants; reissue_context2182-2186 leaves active_ref null.
3. shown_onboarding1882-1887 returns Done when any tenant remains. Snapshot1926-1940 contains remaining tenants plus active:null; client.ts2874,2906-2910 accepts it.
4. App mounts V2Shell; its TenantSwitcher remains selectable (v2-shell.tsx223, tenant-switcher.tsx216-228).
5. An initial SelectTenant B with uncertain IPC receipt captures null (app.tsx66). Store insertion/retirement and render treat null as globally unbound (store.ts165/205/212; v2-shell.tsx87). After B then C snapshots, the old Retry can remain and replay B (v2-shell.tsx481-483). DesktopClient clears unknown-delivery queue/session at3046-3049/3162-3164, so later C actions can proceed.

The native per-refusal guard still protects the original B1 history-deletion path. This observation concerns explicit Retry of stale navigation after uncertain delivery, not restored destructive authority. Independently verify against current code before disposition.

Narrow proposed treatment: distinguish captured SelectTenant source context (null included) from genuinely unbound actions; compare captured source exactly during insertion, retirement and render, preserving existing native refusal-token matching. No native protocol change appears necessary. Regression should start onboarding Done with active:null and B/C present; cover unknown-before-B-snapshot and B-snapshot-before-unknown; after B/C snapshots assert old Retry absent and no old B dispatch.

## Timing/artifact observations

Method02 retains fixed2warmups+10measured switches and100ms budget, with raw trials, genuine target selector+Home status completion and exact executable checks. No retry/outlier hiding found. Remaining proof observations: Swift optionalBool!=true permits missing/unrecognized target AXValue instead of positive unselected evidence; pre-switch source A/B is assigned by Python rather than observed. Consider those before final measurement/review.

Whole app-bundle hash is currently metadata agreement in measure_c30/phase_inputs. A read-only canonical tree comparison at midG5 actually matched prepared bundle cab28f3d41c276d03edf2ad0ae90324b0b3da1b228516413884ba5a18bc1c610, executable b2b78863682a597e6a79ad99616d616afbbda6337149b33c7faf7f32f0717cad and io.cdot.prisma-airs-desktop.v2.fixture, before=after with no errors. It is instantaneous proof, not final acceptance. Script/output are retained outside checkout at /Users/admin/.agent-workflows/airs/2026-11-01-worf-start/root-inspect-live-bundle.py and root-live-bundle-mid-G5-20261102.json. Repeat final canonical complete-bundle/identifier comparison as needed. No extra UI/Rust run is warranted solely for documenting or rereading unchanged artifacts.

Phase audit follow-up: native structural/catalog and archive-map redundancy are defense in depth, not demonstrated current blockers. Frozen current drivers emit valid structures; execution/raw/review inventories constrain later edits; no archive corruption or missing manifest-listed file was observed. Do not inflate these into speculative hardening gates. C41's exact historical scores are covered by its explicit owner decision; P09 phase still requires actual full9.

## Continued scope

Root also saved a C45 readiness map at /Users/admin/.agent-workflows/airs/2026-11-01-worf-start/root-C45-advance-audit-20261102.md. C45 starts only after actual P09 phase9. The game Epic1/thirteenissues remain future backlog, not current implementation. L19 is exhausted. No new live calls, credential access, push, signing or release authority is asserted.

## Actual G5 timing follow-up and narrowed disposition

Actual acceptance attempt01 under evidence/P09/performance/C30 completed exit0 with10samples p95=max25.714625ms, unchanged100ms budget. Independent read-only inspection checked all12 retained posttrees and their recorded hashes. Each has exactly one observed intended alternating tenant selector, Home in the main landmark and Home's own status (TenantB First read pending; TenantA Read finished at). For every measured trial, its preceding retained posttree observes the opposite tenant, including warmup01 -> measured02. Between trials, driver only sleeps, opens Go->Switch Tenant, reads/waits AX and prepares focus; no intervening tenant-selection action exists. This provides observed source through a continuous retained sequence, beyond the assigned Python from field.

Limit preserved: posttrees do not directly retain the open-list target checkbox AXValue. Target-unselected is inferred from distinct observed active selector and single-active UI contract. Nil AXValue hardening/direct immediate pre-source capture are defense in depth for this actual proof, not a demonstrated no-op or current qualification failure. All12 raw exits0, hashes match, measured samples exactly raw[2:], management request count3->3. No need to rerun solely for these hypothetical guard improvements. Fresh independent C30 reviewer still decides qualification and acknowledges method02/presentation/VM conditions. This supplementary inspection is not a numeric review verdict.

## Unapplied patch proposal

A scratch proposal is available outside the active checkout at /Users/admin/.agent-workflows/airs/2026-11-01-worf-start/c30-null-context-proposal/. proposal.patch SHA256 be95e376a6e6a19e032f65e50e14b3cb6f190a19cdabc77a90bec627cf1ad6eb; manifest/base-sha256.json retain exact current base. It changes only store.ts, v2-shell.tsx and inbox/inbox.test.tsx. App already captures null; proposal distinguishes exact null source for SelectTenant from globally unbound intents, preserving review-token checks. Draft tests cover unknown-before-snapshot, snapshot-before-unknown and preserved globally unbound navigation. Independent advisory review is pending. No tests/typechecks have run on the proposal and it is NOT APPLIED or part of the qualified application. Root will validate only in an isolated preserved VM worktree after G5 native/performance UI activity ends. The main implementer retains adoption/disposition responsibility; do not count proposed code or tests as passed/current.

## Main-implementer disposition observed 2026-11-02 04:40Z

The main VM implementer acknowledged this advisory after G5's19natives/performance/provenance audit passed. It archived all950G5files and confirmed canonical bundle/executable/fixture identity (208e472c). It independently implemented the captured-null renderer correction as e1bae833, with two production App/DesktopClient regression orderings. Main reports both final regressions fail against the original G5 renderer at the stale assertion; corrected source passes550/550 and typecheck. Raw initial failing logs and the overbroad-toast replacement correction are retained. No numeric review or G6 native acceptance is asserted here; current candidate changes application bytes and has fresh qualification pending.

The external scratch proposal was never applied or tested and is now obsolete; main implemented its own change. Preserve it as a proposal only, do not count its three draft cases as executed/current tests. The speculative null->B->null epoch concern raised during scratch review was withdrawn as an established production blocker: DesktopClient serializes intents, subsequent same-client deletion cannot normally complete while original delivery remains pending, and no ordinary independent active-clear path was found. It is hypothetical hardening, not an added qualification requirement. Independent advisory review of the actual e1bae833 change is pending; official C30 review remains required.

## Applied-fix advisory review complete

A fresh read-only audit of the actual applied e1bae833 fix found no concrete issue: shared matching covers insertion, snapshot-driven retirement, effect retirement and render filtering; globally unbound actions retain prior behavior; destructive resolution context/review-nonce conditions are unchanged. Both production regressions exercise real dispatch/settlement and relevant stale assertions. No additional tests/UI/source changes were performed by this reviewer, and no numeric verdict assigned. Planned current-source native/check/official-review sequence remains required. External draft disposition.json records its obsolete/unapplied status.
