# C41 remediation after attempt-08 (9/8/9/8/8, accepted at 8 under D034; the push to 9)

| Finding | Remediation | Where |
|---|---|---|
| R1 a pending save's reply overwrote newer typing in the environment and schema editors | The reply is bound to the resource it saved and the text it was sent: it brings the new revision and replaces the text only when the editor still holds what was sent. Test: Save held pending, more typed, the reply released: the editor keeps the newer text, the next Save sends exactly it against the new revision; the same for a schema | `tools-sheet.tsx`, `api-client.test.tsx` |
| R2 an import's conversion report was lost when its draft's selection came first | A superseded import reply whose document is the selected request keeps its notes for that draft (shown at once if its document is already up, else when its own load lands); any other superseded reply is still ignored. Tests: snapshot first and result first, both show the headerReplaced or authRemoved note; a later selection shows no report | `api-client.tsx`, `api-client.test.tsx` |
