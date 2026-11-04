# Final installed-package reviewer prompt

```text
Independently review C26 stable-release readiness, read-only. Read the release
contract, cumulative parity/receipt ledger, final source/dependency/build manifests,
signing/notarization/stapling evidence, exact DMG and installed-byte hashes, controlled
live readback/cleanup evidence, native/accessibility/performance/privacy matrix,
fresh-user no-Node runtime inventory, coexistence/import/update/rollback results.

Distinguish source/fixture/pre-sign evidence from actual shipping signed-artifact
acceptance. Reject missing required tests, different feature flags/source, stale
dependency pins, unknown cleanup outcomes, hidden runtime downloads or plaintext
credential/content logs. No skipped mandatory evidence may count as passed.

Return the canonical scores/findings. Require >=9 architecture, behavior,
authority/security, validation, overall, and no required fixes/blockers/gaps.
State exact supported scope and artifact hashes. Do not publish a release,
run live operations, touch secrets, or promote over v1 as part of review.
```
