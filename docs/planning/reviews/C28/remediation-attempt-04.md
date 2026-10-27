# C28 remediation after attempt-04 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 closing a stacked dialog restored the lower dialog's first control instead of the real opener | A dialog moves focus into itself only when it opens; regaining the top after a higher dialog closes does not refocus, so the closing dialog's opener restore lands on the exact control that opened it. Regression test "restores the exact non-first opener of a stacked dialog"; the ⌘T hand-off tests still pass | `v2/modal.tsx`, `v2/modal.test.tsx` |
| R2 toast controls stayed interactive behind a modal | Toasts render inside a wrapper that is `inert` and `aria-hidden` while any modal is topmost and usable again after; test "makes toast controls inert and hidden while a modal is topmost"; native N05 checks Dismiss leaves the AX tree under the palette and returns after Escape, under Reduce Motion too | `v2/v2-shell.tsx`, tests, `acceptance_c28.py` |
| R3 tenant popup unbounded for long tenant lists | The popup is bounded to the viewport (`max-height: calc(100vh − top − 16px)`) and scrolls; focused rows scroll into view; test with twenty tenants reaches the last by keyboard; native N07 configures twenty tenants, Tabs to Tenant 20 and Add a tenant inside the scrolling popup at 900×600 and switches with Enter | `v2/tenant-switcher.tsx`, tests, `acceptance_c28.py` |
| E1 native evidence for the states above | Driver extended (toast+palette, Reduce Motion variant, twenty-tenant popup); fresh end-to-end record on the rebuilt bundle | `evidence/C28/native*` |
| Optional | Native record limitation corrected (twenty tenants configured); generated contract types included in the scoped inventory | `acceptance_c28.py`, packet builder |
