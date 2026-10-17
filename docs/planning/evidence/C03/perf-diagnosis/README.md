# C03 scan-memory diagnosis (exploratory; not acceptance measurements)

The first complete acceptance measurement under budgets version 2
(`../perf-v2-over-budget-20261017.json`) failed `scan-peak-process-tree-resident`:
median 1494 MiB, p95 1916 MiB against 500 MiB. Every other budget passed. That
method typed each 100 KiB prompt as 20-character key events (about 5,000 edits
per scan) while polling the accessibility tree for the field's value.

These scripts ran on the same prepared fixture executable, against the synthetic
endpoint, to find where the memory went. Resident MiB per process (ps rss):

| Step | app (UI) | WebContent | GPU | Net | Total |
|---|---|---|---|---|---|
| Ready, prompt focused | 123 | 63–64 | 36 | 12 | 234–235 |
| 100 KiB typed as 20-unit events, with AX polling | 396 | 224 | 48 | 12 | 680 |
| + 5 scans of the same text, no retyping | 427 | 228 | 43 | 12 | 710 (flat after the first scan) |
| + 3 retypes with AX polling | 706 | 251 | 48 | 12 | 1017 (+100–160 per retype; kept after 30 s idle) |
| 100 KiB typed as 20-unit events, no AX reads (`typing-without-ax-reads.py`) | 393 | 224 | 42 | 12 | 671; retypes +20 each |
| 11 AX tree reads, then a scan | 432 → 461 | 238 | 43 | 12 | 725 → 755 |
| 100 KiB inserted as four ≤30,000-unit events, then a scan, ×5 (`large-insertions.py`) | 157–161 | 67–83 | 37–40 | 11 | 272–295 (flat) |

Findings:

- **Scans are bounded.** A scan adds about 30 MiB once (first TLS client and
  response handling) and nothing per later scan. No scan text is retained (the
  core keeps no prompt, and records hold only result fields).
- **The growth comes from editing.** Thousands of tiny edits to a 100 KiB
  textarea raise the UI process by about 270 MiB. Accessibility reads during
  typing compound it to about +150 MiB per 100 KiB retype. This is WebKit-side
  UI-process memory (undo and editing state, plus accessibility while an AX
  client is active), not application data.
- **Paste-like input is flat.** With the text inserted as a few large edits,
  as a paste does, the whole tree stays below 300 MiB across scans.

Action: budgets version 3 changes only the scan workload's input method to
paste-like insertion. Limits are unchanged, and the change needs reviewer
acknowledgement. The per-keystroke editing finding is carried as an open
observation: owner C24 (native performance and VoiceOver matrix), which must
measure long typed input under an active assistive client and register a
budget for it.

WebKit truncates one key event's text at 36,864 UTF-16 units (observed), so
30,000 is the per-event size used.
