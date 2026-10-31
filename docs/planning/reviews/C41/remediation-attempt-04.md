# C41 remediation after attempt-04 (9/8/9/8/8, accepted at 8 under D034; the push to 9)

| Finding | Remediation | Where |
|---|---|---|
| R1 only the first running transfer was shown and cancellable | The pane's head lists every transfer in flight for the shown request, each with its own progress line and Cancel request (named "Cancel request N of M" when there are several), above whatever response is inspected. Test: Send held pending, the core selects the health tab and the list tab again, Send again; two progress regions with "Cancel request 1 of 2" and "2 of 2"; the second cancels its own id only, then the first its own; two sends, two cancels | `response-pane.tsx`, `api-client.test.tsx` |
