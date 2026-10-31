# C41 remediation after attempt-07 (9/8/9/8/8, accepted at 8 under D034; the push to 9)

| Finding | Remediation | Where |
|---|---|---|
| R1 a cURL import's reply took a fresh lifetime | The import is dispatched through the API Client, which begins the load (collection and generation) before the call and settles the reply against it; the sheet still says a failure inside itself and closes on a draft. Tests: an import's reply held while the core selects the draft, the sheet closes and another request is selected: the reply is ignored; an import held across a collection close and re-open: ignored too | `api-client.tsx`, `tools-sheet.tsx`, `api-client.test.tsx` |
| R2 a tree entry's failed load was shown only once selected | A failed load names the request it was for (id and path) and is shown above the current editor whatever the selection, with Try again reopening that request once; it clears when a load begins or a document shows. Test: an unselected tree entry refused (missing file), then a lost observation after Try again: the alert says each reason, the previous editor stays, one `openRequest` per attempt, no automatic retry | `api-client.tsx`, `api-client.test.tsx` |
