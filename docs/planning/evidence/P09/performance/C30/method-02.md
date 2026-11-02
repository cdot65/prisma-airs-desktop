# C30 cached-switch observation method 2

This version updates the observation endpoint for the current one presentation
before any new timing run. The original C30 budget table and registration remain
unchanged and historical results are not overwritten. Limit: nearest-rank p95
at most 100 ms, at least 10 alternating measured switches. Two fixed warmup switches
precede 10 measured switches; all 12 raw results are retained. No failed sample is
omitted or retried, and no adaptive warmup, outlier removal or changed limit is
permitted. Calibration cannot qualify acceptance.

The current presentation has an active-tenant AXPopUpButton rather than the old
static tenant-name line. The new Swift `switch-home` probe starts its monotonic
clock immediately before AXPress on the unique unselected tenant checkbox in
the already-open native-menu-triggered switcher. It polls every 2 ms until the
active selector names the destination AND the main-content subtree exposes the
Home heading AND its own status region exposes the destination's cached read
state. It traverses the real AX main subtree in document order, stopping when
those predicates hold; sidebar/card status text cannot satisfy the Home status
predicate. Cached AX references name the actual persistent selector/main nodes;
no backend timer or fabricated DOM result supplies a sample. Foreground process
checks run before and during timing. Setup, opening the menu and evidence reads
after a sample lie outside the registered switch interval, as previously.

Setup uses the current first-run wizard and available tenant Settings form via
v2_setup.tenant_on_home, without requiring the removed interface checkbox. Its
credential-free setup placeholder remains on a separate plane; the measured
workload uses only Tenant A/Tenant B on synthetic loopback. A is read once. B is
never read. A's Home must expose “Read finished at”; B's must expose “First read
pending”. Both selectors identify the exact tenant. The synthetic management
request count must remain unchanged during the 12 switches, proving no read was
awaited or silently sent. Actual post-switch AX trees and raw probe stdout are
hash bound. The current prepared app/probe/driver hashes and equal before/after
input seals are mandatory.

Execution is inside the owner-authorized Worf macOS guest, with guest hardware,
macOS, load and window bounds recorded at execution. The earlier registered
Mac17,3/macOS 27 host description is historical; this run does not claim those
physical conditions, installed/signing qualification, or a host-native test.
The owner requires VM-only development and native execution. The numeric budget
is unchanged; current source/evidence and this presentation/guest rationale
require acknowledgement in the fresh independent C30 review. No successful
measurement or reviewer acknowledgement is claimed by this document.
