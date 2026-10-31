# C41 remediation after attempt-09 (9/8/9/8/8, accepted at 8 under D034; the push to 9)

| Finding | Remediation | Where |
|---|---|---|
| R1 a return remounted an obsolete DocumentView over the editor's newer, protected fields | The API Client tracks which document's editor is up; once it has unmounted, a return to that request reloads it from the core instead of reusing the retained snapshot (the stale-snapshot draft protection is untouched). Test: A edited and protected, B selected with its load pending, back to A (second load, the protected URL), B selected and refused, back to A (third load), Save sends the current fields, no send happened | `api-client.tsx`, `api-client.test.tsx` |
| R2 a superseded environment or schema open replaced the current editor | Each open is a generation; New, Close and another open retire it, and a reply from a retired generation is dropped. Test: a held schema open, a new draft typed into, the old reply released: the draft and its typing stay; Close then a late reply: no editor; a held environment open superseded by a new private environment: the new editor and its typing stay | `tools-sheet.tsx`, `api-client.test.tsx` |
