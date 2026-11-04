**Prisma AIRS Desktop refinement plan — accepted**

Owner feedback of 2026-11-04, all 17 items and Q1–Q13 decisions. The owner confirmed the complete draft with “confirmed. proceed.” Implementation is authorized. This is the accepted scope; completed checks and remaining qualification are recorded separately in execution.md.

The source assessment is bound to accepted Forgejo main f1e58da5cdeb1c67eed650b186081661cefc2fa2 and Linux platform branch 0178b25a577f67a98152bbf3e3cfc0ab544d888b. The published Mac RC1 remains immutable and stable acceptance remains deferred. Common refinements start from accepted main and are integrated into the Linux workstream without bringing unfinished Linux enablement into the Mac release.

**Agreed behavior**

- Every new tenant, including first-run, Add and a replacement tenant, needs a TSG ID and successful management OAuth authentication before it is committed. Scan Key is separate and optional during setup; scans remain unavailable until a key is configured. Configuration does not initiate a validation scan; the scan service checks the credential when the user actually submits a scan.
- Existing tenants continue to load. Expired or session-only credentials do not restart onboarding. Product-specific authorization failures remain distinct from authentication failures.
- Explain is a right overlay drawer that does not shrink the page. The page remains interactive and navigation can continue. The drawer has its own scrolling, Close and scoped Escape behavior; existing dialogs retain priority. Tenant switching closes it and retires the previous tenant's selection.
- Appearance remains Match system / Light / Dark. Theme independently selects Prisma AIRS or Gruvbox, with light and dark variants. Prisma AIRS is the default. Existing appearance preferences are retained.
- Desktop themes cover desktop shell/product pages. Academy retains its current general light/dark mapping and Academy-controlled cosmetic rules. Desktop selection does not grant earned cosmetics.
- SCM is the initial collection when no selection preference exists. Later built-in/custom choices persist. Explicitly closing a collection persists a closed state. Missing or moved custom folders produce recovery choices instead of silent replacement or folder creation.

**Four implementation workstreams**

| Workstream | Feedback coverage | Result |
| --- | --- | --- |
| Tenant connection and setup diagnostics | F01, F02, authentication/editing parts of F16 | Reliable authentication gate, precise current setup checks, correct Supply Chain routing and safe tenant lifecycle. |
| Shell hierarchy and Explain | F03, F04, F05, F17 | One page title in the app header, compact responsive description/actions, overlay Explain, no redundant Ask Harness, improved sun icon. |
| API Client | F06–F11 | Colored JSON, one URL field, scoped footer metrics, sensible export names, scrolling tabs and remembered collections. |
| Settings and themes | F12–F16 | Compact cards, separate appearance/theme controls and clear tenant configuration. |

**1. Tenant connection and setup diagnostics**

Use the existing isolated setup draft/context for all new-tenant entry points. Settings currently has a credential-free CreateTenant path; it must not bypass the new rule or configure credentials against the previous active tenant. Rust completion must enforce authentication itself. Bind authentication evidence to the current draft, client, TSG and credential generation, invalidating it on changes. Cancel, retry, late replies and failed persistence must preserve the previous active tenant and clean up abandoned draft credentials.

Current ManagementBinding.verify obtains/uses OAuth and then reads one Security Profiles entry. This conflates authentication with Runtime Security permission. Provide a distinct authentication result and keep per-product capability checks separate. Neither a stored secret reference nor a generic HTTP 403 proves successful authentication.

Add Recheck setup for an existing tenant. Recheck the relevant finding and resolve its active Inbox warning only after a matching fresh check succeeds. Retain the original dated outcome for diagnostics. Show the exact failing source and reason instead of only “2 of 3.” Preserve source/result identity across tenant, workspace and credential changes. Genuine configuration and permission failures remain visible with a precise action.

The Gateway warning compares IAM resources with the selected workspace slug while displaying its UUID. The real remote cause is still unknown. Preserve the documented slug contract until evidence establishes a defect; do not automatically alter remote IAM configuration.

Route Model/Skill findings to the modelSkill administration section, carrying the product through the complete Inbox arrival path. Use AI Supply Chain Security as the product display name, retaining Model scans and Skill scans as subordinate capabilities. Home Refresh and Inbox Read now have the same readTenant scope; label both Refresh. Ordinary Refresh must not imply that the setup IAM check was rerun.

**2. Shell hierarchy and Explain**

Remove the header Ask Harness navigation button. Keep the sidebar destination. Move the page title into a real, focusable app-header H1 and retain navigation focus and main-region labeling. Academy course/unit headings and breadcrumbs remain intact.

Each content page starts with a concise description beside its relevant controls: Home refresh, Inbox refresh, Academy account badge, Test Cases count/actions, API collection/environment selectors and Administration expand/collapse. Give actions sufficient space and bound the description width/length. Allow wrapping at narrow effective widths and zoom; controls and complete accessible text remain reachable.

Reuse call history, selection and redaction logic inside the new nonmodal Explain drawer. Define Close/Escape ownership, return focus where appropriate, retain dialog priority and clear previous-tenant selection. Replace the gear-like sun artwork while preserving the existing switch-to-light action and accessible semantics.

Retain the Rust catalogue and existing authority checks, and preserve exactly-once Save behavior when changing the editors. The previously discussed platform-hint and Linux tenant-shortcut cleanup remains a separate follow-up outside these 17 items. Craft-keymap, remapping, profiles and shortcut imports remain deferred.

**3. API Client**

Color JSON keys/values in the request JSON editor and Pretty response presentation. Keep request text, raw responses and saved/copied bytes authoritative. In-progress invalid JSON remains editable; invalid/truncated response previews degrade gracefully. Preserve current body/preview limits, read-only states and draft protection. Render numeric/string tokens accurately, including large integers and escapes. Reuse the existing editor foundations with only the required JSON support; no general new UI framework.

Replace the external base-variable badge plus full URL input with one editable URL template and inline variable coloring. Preserve the string-valued URL contract, selection/cursor behavior and complete-template copy/paste. This repairs presentation without rewriting stored collection URLs.

Move request timing/size/status information into the existing bottom status bar beside Ready and the collection request count. Scope it to the displayed response and its run/request/history identity. Retain a clear history identity when a historical response is selected, and prevent late/background responses from replacing another tab/collection's status. Preserve every active transfer's Cancel control and keep save-state messages distinct from response metrics.

For automatic Save Response suggestions, turn whitespace runs into single hyphens: “My request” becomes “My-request-response.json.” Preserve case, extension and the response suffix. Respect a filename and directory explicitly chosen in the native picker; exported response bytes remain exact.

Use one horizontally scrolling request-tab row, keeping tabs readable and revealing selected/focused tabs. Preserve selection, closing and keyboard behavior.

Persist a collection selection with distinct uninitialized, built-in/custom and explicitly closed states. Apply startup selection once through the core load path. Preserve SCM's edit-aware refresh, original collections and credentials. Missing/replaced custom folders keep their preference and offer Retry, Open collection and SCM recovery actions.

**4. Settings and themes**

Use compact responsive cards and appropriate controls instead of full-width form rows:

- Appearance: Appearance and Theme dropdowns.
- Academy: account/settings card, retaining the existing learner flow.
- Diagnostics: existing export/diagnostic actions, including dated warning results.
- Tenant section: separate Scan Key, Management Client, Scan Destination, Update Tenant and Add Tenant cards.

Credential cards retain secure-storage/session-only choices and Replace/Remove actions. Keep secret fields uncontrolled, clear them before asynchronous work, and preserve context-keyed remount/cleanup. Add Tenant includes label, TSG, management client ID/secret and scan-region selection. OAuth authentication is required; Scan Key setup is available separately. Reuse existing activation/switch guards.

Update Tenant selects a profile and offers Edit/Delete. Edit initially changes label/environment. TSG identity changes create a replacement tenant. Destination changes retain their own reviewed card, including advanced custom HTTPS support; Add Tenant has the region dropdown instead of region/custom-endpoint radio buttons. Delete retains existing confirmation and cleanup behavior.

Provide light/dark Prisma AIRS and Gruvbox palettes, defaulting to Prisma AIRS. Make theme persistence and existing-state defaults explicit. PersistedState rejects unknown fields, so a new preference must have a deliberate storage/migration and downgrade policy, rather than assuming old binaries can read it. Preserve the new app namespace and never read alpha/v2 data.

Isolate desktop palette styling from Academy, including portaled content. The existing LearnerHost theme prop alone does not isolate the global CSS variables. Check contrast, focus states and shared controls in all four palette/appearance combinations.

**Integration and verification**

Agree shared command/state, authentication and CSS contracts first. Assign ownership for overlapping Settings and shell files, then parallelize the bounded streams. Integrate common changes into the Linux branch while preserving its platform-specific adapters and Harness pin. Save all work on normal remote feature branches; preserve unrelated worktrees and owner changes.

Start each reported defect with a focused local reproducer/fixture at the actual affected interface. Distinguish reproduced behavior from source risks and remote facts. Authentication fixtures cover invalid OAuth versus authenticated-but-forbidden product reads, direct completion bypass attempts, changed credentials, cancellation, failed persistence, late replies and wrong-tenant isolation. Warning fixtures cover matching rechecks, history and the complete Supply Chain navigation path.

Run affected frontend tests/typechecking and targeted Rust checks. Exercise the changed native flows on Mac and Linux using owned fixture state: credential entry/cleanup, focus and nonmodal drawer behavior, 100%/200% layouts, JSON/URL editing/copy, tab scrolling, footer ownership, Save exactly once and Save Response picker behavior. Review themes on representative shell/product/Academy/portal surfaces.

Follow the owner's verification strategy. Preserve original raw outcomes and reuse unrelated evidence with exact-artifact proof or an honest unchanged-impact rationale. A commit, merge or review alone does not request a full Rust suite or 19 native gates. Follow standing independent chunk/phase review rules, report every actual score and verify only affected remediation. Complete focused checks and cross-consumer review before building the combined qualification candidate. Linux's first platform qualification remains a separate requirement.

No real credentials or product calls are needed for local fixtures. If the actual Gateway/Supply Chain findings still require a live diagnosis, prepare a bounded read-only scope for that tenant/workspace under the existing live-manifest rule. Keep authentication material out of logs and make no automatic remote configuration changes.

Package accepted changes as a new immutable candidate; preserve existing RC1 assets. Stable promotion still requires acceptance of the exact final artifact. Acceptance of this plan does not claim implementation, qualification or publication has completed.
