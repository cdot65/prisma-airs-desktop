# C48 attempt-01 — accepted at 8 (D034)

Actual fresh read-only Codex review: architecture 9, behavior 8, authority_security 9, validation 8, overall 8. The audited verdict has no blockers, one P2 required fix and no evidence gaps. Accepted at eight under standing owner D034 while immediately completing the focused push to nine. Original scores, findings and limitations remain unchanged; `post-review-audit.json` retains `full_gate_pass: false`. This is chunk acceptance, not full-nine or phase acceptance.

The open fix is preferred-collection recovery across navigation: failed custom-collection restoration must retain its warning and explicit Try again action after Home and back, while distinguishing an intentionally closed collection. Preserve recovery independently of the API Client page mount, add a production-path navigation regression, and verify the affected recovery flow on the new candidate. Retain unrelated valid evidence with explicit unchanged-impact rationale; no unrelated native or Rust suite replay is implied.

Stable installed/live acceptance and Linux platform qualification remain separate.
