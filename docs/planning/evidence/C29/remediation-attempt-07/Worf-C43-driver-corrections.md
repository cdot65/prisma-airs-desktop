# Worf C43 driver corrections — 2026-11-01

The first complete Worf generation failed honestly: C43-N01 fail, C43-N02 pass,
C43-N03 fail; 22/24 retained behavior assertions passed and all 48 matrix rows
were emitted. The independent coverage checker found ten errors. The complete
237-file generation is preserved in
`P09/native-history/C43-Worf-failed-20261101-start-03/` with the original driver
source and per-file hashes. Embedded paths in those raw records remain unchanged.

Two driver defects were confirmed against source and actual AX observations:

- The wizard entered TSG `1762204418`, but `acceptance_c08.MgmtEndpoint.start`
  passed `1234567890` to the synthetic server. Its profiles route matches that
  configured TSG exactly. Corrected/stale verification therefore reported that
  the service found nothing for the wizard TSG. C43 now uses `c8.TSG` both for
  typing and the expected tenant summary, matching the server's existing input.
- Signed-out Academy advances with the actual footer button `Skip for now`.
  The driver's primary selector omitted it, yielding null primary and a full
  60-Tab walk. Primary selection now takes the step; only Academy admits this
  action. Packs retains `Continue` as its advancing primary. Checks against all
  eight retained Academy/Packs theme/zoom AX trees confirm this distinction.

No app source, acceptance oracle, keyboard containment, geometry threshold,
assertion ID or fixture capture location changed. The exact prepared executable
remains `3420d66c8dc74e117646dea8d08a3c03f7d41a7e59ce19493fa56e8a63c1925d`.
C36-N01/N02 passed on these same bytes and remain valid. The full C43 rerun is
pending; retained trees used to diagnose selection are not new native passes.
No product endpoint or real credential is involved. Run03 logs remain unchanged;
new private progress/logs are in `worf-agent/2026-11-01-resume-04`.

The corrected full Worf rerun passed C43-N01/N02/N03. The independent
`wizard_coverage.py` check passed with no errors: 24/24 behaviors and 48 rows,
including actual contained keyboard traversal and hash-bound AX/captures.
The controller returned `required_pass: true`, no errors or carried failures.
Fresh evidence is `P09/native/cumulative/C43/`; the failed generation remains
immutable under the history path above. Independent C29 re-review remains pending.

The next serial native driver, C11, exposed a separate AX observation defect:
its zero-cache sentence is split across inline static-text nodes in this WebKit.
The complete failed generation is preserved under
`P09/native-history/C11-Worf-failed-20261101-resume-04/` (18 files and original
driver). The correction joins static text only inside the Investigation cache
region, preserves all three required zero counters, seal deletion and fresh-read
checks, and saves the clear-state tree before resealing. Actual retained trees
reject the nonempty state and accept the zero state; text outside the named
region is rejected. A full C11 rerun is pending. The check-audit exception names
this additional driver-only change explicitly; no application input is exempted.

C11 full rerun passed all 11 checks, including actual cache/seal deletion and
fresh-read/reseal assertions. Controller required_pass=true with no errors.
