# C06 remediation after attempt-01

Verdict attempt-01: architecture 8, behavior 7, authority/security 9,
validation 7, overall 7; RF01–RF03, EG01.

## RF01 — the assessment timer was not rendered

Academy's learner header export now carries the page's session-critical
status (`useLearnerPageHeader().status`, from the topbar `right` slot), and
the Academy destination renders it in a sticky slot under the trail
(`LearnerStatus`, `academy-view.tsx`). The timer counts down to Academy's
deadline and submits once at it; it unmounts with the attempt and with the
learner generation.

- Desktop composed tests (`learner-interactions.test.tsx`, `assessment
  timer`): countdown from Academy's deadline with the proctored pill in the
  status slot; one submission at the deadline and Academy's grade; an
  unknown outcome at the deadline reported once and never resent; leaving
  the attempt or another generation submits nothing.
- Native: C06-N02 (countdown 59:59 'Time remaining' and PROCTORED in view),
  C06-N12 (signed in again, deadline 8 s away: one submission 8.02 s after
  the attempt was read, unknown outcome reported, nothing resent).
- Probe: removing the status slot fails the composed test.

## RF02 — overlapping lab draft saves

Academy's desktop draft hook (`use-desktop-draft.ts`) runs saves one at a
time through a queue; each saves the work current when it starts, a queued
save of confirmed work sends nothing, and the confirmed version only moves
forward. Academy test `use-desktop-draft.test.tsx` (production interface:
LearnerHost and a link with test-released replies): overlapping saves with
changed work, a duplicate queued save, and an unknown outcome retried with
the same work and mutation id. The first two fail on the previous hook.
Probe: removing the queue fails the hermetic Academy test.

## RF03 — retained learner workflows

- Course reset: `LearnerHost courseReset` (off on Academy web) mounts
  Academy's `ResetCourseProgressAction` (counts, certificates kept).
  Composed test and C06-N14 (dialog counts '3 units, 1 quizzes, 0 labs',
  Cancel sends nothing, confirm sends one `progress.resetCourse`).
- Completed-lesson feedback: `lessonFeedback` mounts Academy's
  `FeedbackWidget` on completed content units (`admin.feedback.submit`).
  Composed tests and C06-N15.
- Avatar: native upload/readback/removal (DESIGN "Avatar outside the
  renderer", D030). Core: `UploadAvatar` (native picker, signature and cap
  check, presign with the account credential, credential-free PUT only to a
  reviewed destination, confirm never resent) and `LoadAcademyImage`
  (Academy's public avatar address only, `data:` URL only when type and
  bytes agree); removal is Academy's `settings.removeAvatar`. Rust tests
  (`academy_program`): success with literal requests; cancel, unsupported
  type, SVG and oversize send nothing; unreviewed destination, denied
  presign, failed store (no confirm) and unknown confirmation (sent once);
  guest and stale generations refused; readback from Academy's origin only,
  type/bytes mismatch rejected, other addresses refused before a request.
  Composed tests (upload, five outcomes, removal) and C06-N13 (presign,
  PUT without credential, confirm, public readback with no load warning,
  cancel, removal). Probes cover the destination policy, the address
  policy, the image cap and the upload/readback props.

## EG01 — interrupted export

`PrintPage` is now an operation observed like a learner call: whether the
sheet opened. A sheet that cannot open reports `hostUnavailable`; the
certificate page says "The print sheet could not open. Your certificate is
unchanged; try Download PDF again." in an alert region. Executed: Rust
`a_print_sheet_that_cannot_open_says_so_and_changes_nothing` (scripted
printer, unchanged state, stale generation refused), composed test (no
mutation sent), C06-N04 (fixture flag: alert shown, no sheet opened, no
learner call). A sheet the operator dismisses after it opened is the
system's and changes nothing in the desktop or Academy; Tauri does not
report that dismissal, so it is not distinguished.
