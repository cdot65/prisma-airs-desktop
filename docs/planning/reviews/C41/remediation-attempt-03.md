# C41 remediation after attempt-03 (9/8/9/8/8, accepted at 8 under D034; the push to 9)

| Finding | Remediation | Where |
|---|---|---|
| R1 a run shown from History hid a transfer in flight and its Cancel request | The response pane's head always shows a transfer in flight (its progress line with Cancel request) and, under it, the head of whatever response is inspected; the body stays the inspected record. Test: an older run shown from History, Send held pending with a running transfer in the snapshot: the old run stays shown as recorded (17 ms), the progress and Cancel are in view, Send is disabled, Cancel dispatches `cancel` with the active run id exactly once, one send only | `response-pane.tsx`, `api-client.test.tsx` |
