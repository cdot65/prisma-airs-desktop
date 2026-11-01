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

C04's first Worf run passed 11 checks but failed N05's legacy active-tenant
text check. Its restored AX tree names `Switch tenant: Tenant B · Production`.
The complete 10-file failed generation and original driver are retained under
`P09/native-history/C04-Worf-failed-20261101-resume-04/`. C04 now uses the existing
`make_active` result, which verifies the exact tenant label as the switcher's
first name, and retains a dedicated tenant-switch AX tree in N05 evidence.
It still requires Academy to remain signed in as Ada Lovelace. The full rerun
is pending; the check audit names this driver-only correction explicitly.

C04 full retry passed all 12 checks; N05 confirms Tenant B active while Academy
remains signed in. Controller required_pass=true with no errors.

C05's first Worf run passed 14 checks but failed N05's count-line recognition.
The actual grade heading was 100%; its exact result paragraph was split into
`1`, ` OF `, `1`, and ` CORRECT · KNOWLEDGE CHECK COMPLETE` static-text nodes.
The complete 22-file generation and original driver are preserved under
`P09/native-history/C05-Worf-failed-20261101-resume-04/`. The corrected reader
joins a paragraph only inside the named knowledge-check region. Actual graded
and ungraded trees, a wrong-count mutation and outside-region text verify that
it recognizes the result without accepting another grade. Hosted grading,
exact request bodies, Authorization and answer-key confinement checks remain.
Full C05 retry is pending; the input audit names this driver-only correction.

C05 full retry passed all 15 checks, including the exact count line, hosted
grade, literal request bodies and answer-key confinement. Controller has no errors.

C06's first run failed N01/N07 text recognition, with the other 13 checks
passing. Actual AX trees split Home's exact LVL 4 and 431/640 XP into static
text tokens and the enrollment certification sentence across inline nodes.
The complete 29-file generation/source is archived under
`P09/native-history/C06-Worf-failed-20261101-resume-04/`. Corrected predicates
read the exact Home tokens within Academy pages and the certification sentence
within the named removal dialog. Actual-tree positives and wrong-XP/outside-
region negatives pass; all course-list, certificate-validity, Cancel and exact
mutation assertions remain. Full retry pending; audit exception names C06 only.

C06 full retry passed all 15 checks, including exact Home facts and enrollment
confirmation/Cancel/mutation assertions. Controller reports no errors.

C07's first run failed N01/N06 because the Settings signed-in sentence splits
around its bold name; eight other checks passed. Same-bundle C04 AX demonstrates
the split, while C07's later trees show the correct Ada/Grace identities. The
complete 17-file generation and original source are preserved under
`P09/native-history/C07-Worf-failed-20261101-resume-04/`. The reader now requires
the exact account sentence within Academy account and saves every sign-in AX
tree. Actual-tree, wrong-account and outside-region checks pass. Offline account
isolation, encrypted bytes, explicit sync, lease/revocation and reset assertions
remain. Full retry pending; C07 is explicitly named as a driver-only exception.

C07 retry-02 fixed sign-in/N06, but N01 still failed its ready-offline unit count:
`Ready offline` and ` · 28 units · until…` are separate AX nodes. This second N01
false predicate was overlooked in the first diagnosis. The complete 20-file
retry/source is preserved as `C07-Worf-failed-02-20261101-resume-04`. The reader
now locates the specific course's Open link and reads only its containing card,
requiring the same ready state and 28-unit count. Actual downloaded/empty trees
and a wrong-count mutation pass focused checks. Exact request count, credential
binding and offline key assertions remain. Full retry-03 pending.

C07 full retry-03 passed all 10 checks; controller required_pass=true, no errors.
Both earlier failed generations remain immutable; all same-bundle native evidence
is retained. Independent review and remaining cumulative gates are still pending.
