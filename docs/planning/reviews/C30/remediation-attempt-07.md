# C30 remediation after attempt-07 (9/9/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 guard initial focus undocumented and unasserted | The intended contract is now explicit: initial focus is Stay, the safe default (Enter on arrival never ends the conversation), while Archive and switch stays the visually primary action. `SwitchGuard` declares Stay as the initial control; the Modal honours a declared initial control before the first focusable (its selector previously let document order win). Native C30-N04 asserts the focused control is Stay from the AX tree (fails otherwise); `inbox.test` asserts Stay has focus when the guard opens; shell.md, provenance P35 and the check text state the contract | `switch-guard.tsx`, `modal.tsx`, `acceptance_c30.py`, `native-checks.json`, `screens/shell.md`, `provenance.md` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
