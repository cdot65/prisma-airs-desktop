# Execution location — owner correction, 2026-11-04

Automated development and native testing belong in the dedicated Worf macOS
VM, now reachable as `admin@worf-vm`. Kirk is the owner live-acceptance host.
Do not run native development drivers on Kirk. The old Kirk launcher and
supervisor have been retired; their originals and raw failures are preserved.

The isolated VM worktree is `/Users/cdot/development/redtail/p09-queue/refinement-20261104/core`,
on `refinement/20261104-vm`. Its transferred source was exactly published commit
`2e4439fa04a56c822a44286359b294587428e161`; the complete source fingerprint and
660 application inputs were compared before new VM work. Existing dirty and
historical VM worktrees remain untouched. Node 24.21.0, Rust 1.99.0, pnpm 10.6.5
and agent-desktop 1.0.2 are present. The exact previously built fixture bundle
was copied and independently hash-verified; no replacement app build was claimed.

The 439 passing source-test executions and 20 validator tests ran on Kirk before
this correction. Preserve those original outcomes and bindings, and use explicit
affected verification for subsequent app fixes; do not relabel them VM runs or
repeat unrelated suites for migration/source provenance alone. Native observations
now run in the VM and retain its actual host, process and operator bindings.

VM-only readiness probes verified owned-window activation, legacy Swift AX
visibility and actual System Events typing. Native attempt 05 then completed
initial OAuth failure/success setup and exposed Settings Add Tenant being nested
inside an inert/aria-hidden shell column. That actual application composition
bug requires an App-level regression and fix; a driver-label workaround is not
justified. Attempt 05 and the earlier failed host attempts remain unchanged.

The VM currently needs a temporary loopback-only SSH relay through Kirk for
Forgejo Git traffic because its DNS resolves the service to an unreachable public
address and it has no LAN subnet route. This relay performs no builds, tests or
UI actions. VM development/test execution does not depend on a Kirk GUI session.
