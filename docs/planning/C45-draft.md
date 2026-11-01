# C45 — One presentation, one identity (P10)

Owner decisions settled on 2026-11-01 through the grill-me rounds. This brief
supersedes the 2026-10-31 draft. Required predecessor: P09 phase review passing
at 9, after C44 and the L19-dependent C29–C32 reviews. C44 attempt-05 has now
passed at 9 in every dimension and is merged. L19 passed; C29 attempt-07 was
accepted at 8, its missing native coverage has been restored in the driver,
and fresh native execution plus C29–C32/P09 reviews remain pending in Worf.
This approved planning brief is retained before the VM move; C45 implementation
has not started. Proposed decision id D061 must be checked for availability then.

## Confirmed outcome

- Display name: **Prisma AIRS Desktop**, with the version in About.
- Release version: **1.0.0**, replacing the owner's pending 2.0.0 version bump.
- Production identity and protected-data prefix: **io.cdot.airs-desktop**.
- Fixture identity: **io.cdot.airs-desktop.fixture**; fixture bundle name
  **Prisma AIRS Desktop Fixture**.
- **Fresh setup.** No automatic migration or credential adoption from either the
  alpha or the current v2 development installation. Their data and Keychain items
  remain intact. There is no one-way v2-to-v1 migration in this chunk.
- **P10** contains C45 and closes before C26's signed release. C45 follows D034:
  accept at 8 with no blockers, record and remediate findings, then raise every
  score to 9. P10 requires its own cumulative phase review at 9, with no required
  fixes, blockers or evidence gaps and no owner acceptance of the phase.

The earlier decisions remain: remove the legacy presentation and Rust variant
machinery entirely; remove every alpha-import surface (D059, D060). The new
presentation preserves the working behavior of active Settings, Sessions,
Diagnostics and Academy helpers while replacing or moving their implementation.

## Repository facts that correct the earlier draft

A TypeScript import-graph walk from `apps/desktop/src/main.tsx` found 19 live
modules under `features/`. Eight files in the old deletion list are still live:

- `academy/desktop-link.ts`, `academy/host-actions.ts`
- `diagnostics/diagnostics-panel.tsx`
- `sessions/sessions-model.ts`, `sessions/sessions-view.tsx`
- `tenants/credential-entry.tsx`, `tenants/profile-import.tsx`,
  `tenants/tenants-settings.tsx`

Settings and Sessions are mounted through the current shell. Their behavior must
be ported before their old views are deleted. The profile-import surface is an
intentional D060 removal; the surrounding tenant and credential behavior remains.
The Academy truth-graph test is not a runtime import and must retain its coverage
under a new path rather than being deleted merely because it is unreachable
from the production entry point.

The installed alpha is `/Applications/Prisma AIRS Desktop.app`, identifier
`io.cdot.prisma-airs-desktop`, version 0.11.11. Its Academy Keychain namespace is
`io.cdot.prisma-airs-desktop.academy`. The old draft's unsuffixed identifier would
collide with it. The chosen `io.cdot.airs-desktop` namespace avoids that collision.
The display-name installation-path collision belongs to the later C26 install
plan and its signing/release grilling session; building C45 does not install over
that app.

Research inventory: `evidence/C45/import-graph-2026-11-01.json`. Recompute the
import graph on the merged P09 tree before deleting files.

## Implementation order

1. Create an isolated C45 worktree from the completed P09 integration. Inventory
   owner modifications and preserve them. Stage explicit paths only. Archive
   named legacy edits that the owner explicitly retired before removing them;
   preserve unrelated owner core, build, release and shared-helper work.
2. Port the active Settings, Sessions and Diagnostics views to the one
   presentation. Move the live Academy, administration, Runtime, tenant and
   workbench helpers out of `features/`, preserving their compiled behavior.
   Keep the Academy truth and hosted-authority guards under maintained test
   paths. Delete dead views and superseded tests after their behavior is covered
   by the one presentation. Remove the two untracked legacy twins only after
   confirming that no maintained module imports them. The `features/` directory
   ends empty and is removed.
3. Remove `UiVariant`, per-variant catalogues/default navigation, the stored flag,
   `SetUiVariant`, the snapshot field, generated types and variant-only fixtures
   and tests. Remove the renderer's legacy-layout context once its consumers are
   ported. There is one catalogue and one presentation.
4. Remove the alpha reader and adapters, `V1Import*` intents/views, Settings'
   tenant-profile file import and API Client profile adoption, their discovery
   paths, and import-only fixtures/tests. Ordinary user-selected API collection
   files keep their current format and editor behavior. This is not a file-format
   rewrite.
5. Apply the chosen production/fixture identities, version and namespaces across
   Tauri/Rust/package metadata, native menus, About, storage, credential services,
   fixture setup, drivers and current documentation. Production state uses
   `prisma-airs-desktop/state/1` in the new application's data root. Start fresh
   without reading or deleting alpha/v2 roots, credentials, sessions or approvals.
   Update current live-tool naming without making live calls or silently changing
   the owner's approved manifest. Historical verdicts, evidence and the frozen
   prototype retain their provenance; checkout/remote paths remain stable.
6. Register C45/P10 in the controller, checks, native inventory, review schema,
   plan and status. Record the owner-approved sequencing: P10 follows P09 and
   precedes the signed P08 release. If C26 still blocks controller transport,
   retain the explicit fresh-exec fallback limitation rather than inventing
   controller receipts or treating C26 as completed.

## Gate and evidence

During implementation: typecheck, affected Vitest suites and Rust tests. Resolve
contracts from the active checkout, including when dependencies are shared across
worktrees. Preserve the C44 scope and commit-phase editor regressions.

Before the heavy gate, verify the production import graph has no `features/`
consumer, old variant/import contracts are gone, protected namespaces are distinct,
and current names/version are consistent. Fixture tests seed old alpha/v2 canaries
in synthetic roots and prove that fresh v1 setup neither imports nor purges them.
They never inspect real old credentials to prove this.

Then rebuild the renamed fixture and run the **full C44 cumulative set**:
legacy C03, C04, C05, C06, C07, C10, C11, C24, C31 through the adapter and C35–C43,
plus the whole-app C44 checks and new C45 identity/fresh-setup checks. Bind every
required assertion to the exact executable. The assertion-level map remains the
source of truth: a replaced legacy aggregate such as C24-N06 is proved by the
current whole-app aggregate, not demanded from the obsolete legacy matrix.
Any new retirement needs an owner decision; the accepted retirement list is not
silently expanded.

Parameterize reused drivers so C45 records/captures land under `evidence/C45/`.
Keep P09/C44 evidence intact. Run the full source-bound check suite, freeze an
attempt folder that has no verdict, build a self-excluding review packet, and
obtain the fresh read-only Codex review. Record the same-model-family limitation
and actual implementer/orchestrator. Verify hashes, run logs and native bindings
again after the verdict. Report every score immediately and retain every failure.

Raise C45 to 9, integrate its tree while preserving owner work, rerun integrated
checks and obtain the P10 phase review at 9. The signed C26 build then qualifies
that integrated tree under the owner's live/signing scope.

## Outside this chunk

C29's L19 designation and live row, P09's phase pass, C25 Academy live approvals,
C26 signing/notarization/install decisions, and publishing. They retain their
existing owner gates. The installed alpha's app-name collision is resolved in
the C26 installation plan. No push without the owner's word.
