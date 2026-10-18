#!/usr/bin/env python3
"""C06 native acceptance: Academy's graded workflows, Program and learner
account in the prepared fixture app, driven through macOS Accessibility
against the synthetic Academy endpoint (academy_server.py serving
academy_catalog.py plus academy_program.py), evaluated against the C06
inventory (docs/architecture/native-checks.json).

The fixture build trusts the disposable test CA, talks only to the synthetic
endpoint named in its data root, records (does not open) print sheets and
browser pages, and keeps credentials in its dedicated fixture keychain
(inspected by attributes only). The fixture root is removed at the end.

One launch: guest navigation; sign-in; Home and Program; the final
assessment by Accessibility press (countdown, unknown outcome, Academy's
status) and by keyboard; the certificate's verify link, print and a print
sheet that cannot open; a desktop lab attempt and a lab without a desktop
adapter; Shift with a slow optional briefing; a completed lesson's rating;
course reset and removal; isolation; zoom and minimum size; the account
page (avatar upload, readback, cancel, removal) and its sign-out; signed in
again, the timer submitting at Academy's deadline.
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import academy_catalog  # noqa: E402
import academy_program as prog  # noqa: E402
import fixture_app as fx  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C06")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C06" else "native-C06")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C06" else "native-acceptance-C06.json")
ACADEMY_SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.academy"
CATALOG = "What will you take up next?"
COURSE = "Runtime Security Foundations"
PROGRAM = "Your program"
INTRO = "Final assessment"
UNKNOWN = "We could not confirm submission"
PENDING = "Grading your written answers"
LAB_PASSED = "Lab passed. Academy recorded it on your account."
NAV_SIGNED_IN = ["Home", "Learn", "Program", "Notifications", "Settings"]
GENERIC_FAILURE = "Something went wrong"
PROCEDURES = json.load(open(os.path.join(ROOT, "packages/desktop-contracts/fixtures/academy/learner-procedures.json")))
PUBLIC = set(PROCEDURES["public"])
ACCOUNT = {p["procedure"] for p in PROCEDURES["learner"]}
PRIVILEGED = ("exam.grade", "exam.override", "identityPractical.queue", "identityPractical.review", "admin.users",
              "certificate.issue", "gamification.award", "gamification.claim", "labs.submitLab", "labs.start")
# Hosted decisions the learner never sends.
FORBIDDEN_INPUT_KEYS = {"deadline", "deadlineAt", "serverNow", "score", "scorePercent", "passed", "award",
                        "xpAwarded", "correct", "startedAt", "slaTotalSeconds", "grade", "verdict"}
LEARNER_MARKERS = ["Run learner call probe", "Your program", "Final assessment", "Hands-on lab", "LVL ",
                   "Lab passed", "STAGE 1/5", "Remove enrollment"]
LOG = []


def sha256(path):
    import hashlib
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def texts(tree):
    return [n.get("label") or "" for n in tree] + [str(n.get("value")) for n in tree if n.get("value") is not None]


def shows(tree, fragment):
    return any(fragment in t for t in texts(tree))


def heading(tree, label, level=None):
    return next((n for n in tree if n.get("role") == "AXHeading" and (n.get("label") or "") == label
                 and (level is None or n.get("value") == level)), None)


def learner_calls(requests):
    return [r for r in requests if r.get("procedure")]


def body(call):
    try:
        return json.loads(call.get("input") or "{}").get("json") or {}
    except ValueError:
        return {}


def keys_deep(value):
    if isinstance(value, dict):
        for k, v in value.items():
            yield k
            yield from keys_deep(v)
    elif isinstance(value, list):
        for v in value:
            yield from keys_deep(v)


def in_alert(tree, fragment):
    """Whether text containing `fragment` lies inside an ARIA alert region
    (AXApplicationAlert subrole), which assistive technology announces."""
    alert_depth = None
    for n in tree:
        depth = n.get("depth", 0)
        if alert_depth is not None and depth <= alert_depth:
            alert_depth = None
        if n.get("subrole") == "AXApplicationAlert":
            alert_depth = depth
            continue
        if alert_depth is not None and fragment in (n.get("label") or ""):
            return True
    return False


def nav_links(tree):
    """Links of the 'Academy sections' navigation, in order (the nodes after
    the group up to the trail)."""
    out, inside = [], False
    for n in tree:
        if n.get("role") == "AXGroup" and n.get("label") == "Academy sections":
            inside = True
            continue
        if inside and n.get("role") == "AXGroup" and n.get("label") in ("Academy location", "Academy"):
            break
        if inside and n.get("role") == "AXLink":
            out.append(n.get("label"))
    return out


def png(width=8, height=8, noise=False):
    """A valid PNG: solid colour, or random pixels (incompressible, like a
    camera photo's detail)."""
    import struct
    import zlib

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    rows = b"".join(b"\x00" + (os.urandom(3 * width) if noise else b"\x2b\x6c\xb0" * width) for _ in range(height))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows)) + chunk(b"IEND", b""))


def timer_text(tree):
    """The text inside the assessment timer ('Time remaining'), e.g. '59:58';
    None when no timer is shown."""
    for i, n in enumerate(tree):
        if (n.get("label") or "") == "Time remaining":
            text = ""
            for m in tree[i + 1:]:
                if m.get("depth", 0) <= n.get("depth", 0):
                    break
                text += m.get("label") or ""
            return text
    return None


def prints():
    path = os.path.join(fx.DATA, "fixture-prints.log")
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [line.strip() for line in f if line.strip()]


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    if sha256(os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c06-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.AcademyEndpoint(os.path.join(work, "academy"))
    catalog = academy_catalog.build(ROOT)
    catalog.update(prog.build(ROOT, endpoint.origin))
    with open(os.path.join(endpoint.dir, "catalog.json"), "w") as f:
        json.dump(catalog, f)
    endpoint.install()
    endpoint.start()
    obs = {"origin": endpoint.origin}

    def wait(predicate, timeout=12):
        return app.wait_for(predicate, timeout)

    def go_academy():
        app.ax("press", "AXButton", "Academy")
        return wait(lambda t: heading(t, CATALOG) is not None or nav_links(t))

    def nav(label, ready, timeout=10):
        app.ax("press", "AXLink", label)
        return wait(ready, timeout)

    def calls_since(mark):
        return learner_calls(endpoint.requests()[mark:])

    def focus_to(role, prefix, steps=60):
        for _ in range(steps):
            app.keycode(48)
            f = app.focused(app.tree()) or {}
            if f.get("role") == role and (f.get("label") or "").startswith(prefix):
                return True
        return False

    app.launch()

    # ---- guest ----
    tree, _ = go_academy()
    time.sleep(1)
    tree = app.tree()
    obs["guest_nav"] = nav_links(tree)
    obs["tree_guest"] = save_tree("guest-learn", tree)

    # ---- sign in; Home and Program ----
    endpoint.mode(approval="granted", displayName="Ada Lovelace")
    app.open_settings()
    app.ax("press", "AXButton", "Sign in to Academy")
    _, obs["signed_in"] = wait(lambda t: app.has_text(t, "Signed in to Academy as"), 15)
    go_academy()
    time.sleep(1)
    tree = app.tree()
    obs["account_nav"] = nav_links(tree)
    mark = len(endpoint.requests())
    tree, home = nav("Home", lambda t: shows(t, "LVL 4") and shows(t, "431/640 XP"))
    time.sleep(2)
    tree = app.tree()
    obs["home"] = {"briefing": home, "failure_toast": shows(tree, GENERIC_FAILURE),
                   "calls": [[c["procedure"], c["status"]] for c in calls_since(mark)]}
    obs["tree_home"] = save_tree("home", tree)
    tree, ok = nav("Program", lambda t: heading(t, PROGRAM, 1) is not None)
    obs["program"] = {"hub": ok, "cards": [n.get("label") for n in tree if n.get("role") == "AXLink"
                                           and (n.get("label") or "").split(" ")[0] in ("CAPSTONE", "CREDENTIALS", "RECORD", "SUPPLY", "STANDINGS")]}
    obs["tree_program"] = save_tree("program", tree)

    # ---- final assessment by Accessibility press ----
    mark = len(endpoint.requests())
    tree, intro = nav("CAPSTONE", lambda t: heading(t, INTRO, 1) is not None and shows(t, "RETAKES LEFT"))
    obs["tree_exam_intro"] = save_tree("exam-intro", tree)
    app.ax("press", "AXCheckBox", "I agree")
    time.sleep(0.5)
    app.ax("press", "AXButton", "Start exam")
    tree, runner = wait(lambda t: heading(t, prog.EXAM_PROMPT, 1) is not None)
    time.sleep(1.5)
    tree = app.tree()
    obs["tree_exam_runner"] = save_tree("exam-runner", tree)
    countdown = timer_text(tree)
    proctored = any("proctored" in t.lower() for t in texts(tree))
    app.ax("press", "AXCheckBox", "A Before the model")
    time.sleep(1)
    app.ax("press", "AXButton", "See results")
    tree, unknown = wait(lambda t: shows(t, UNKNOWN), 10)
    time.sleep(2)
    tree = app.tree()
    obs["tree_exam_unknown"] = save_tree("exam-unknown-outcome", tree)
    answer_kept = (app.node(tree, "AXCheckBox", "A Before the model") or {}).get("value") == 1
    submits_before_check = len([c for c in calls_since(mark) if c["procedure"] == "exam.submit"])
    app.ax("press", "AXButton", "Check submission status")
    tree, pending = wait(lambda t: heading(t, PENDING, 1) is not None, 10)
    obs["tree_exam_pending"] = save_tree("exam-pending-review", tree)
    exam_calls = calls_since(mark)
    obs["exam"] = {"intro": intro, "runner": runner, "countdown": countdown, "proctored": proctored,
                   "unknown": unknown, "answer_kept": answer_kept,
                   "submits_before_check": submits_before_check, "pending_review": pending,
                   "server_text_hidden": not shows(tree, "upstream unavailable"),
                   "calls": [[c["procedure"], c["status"], c["authorization"]] for c in exam_calls],
                   "bodies": {c["procedure"]: body(c) for c in exam_calls if c["procedure"] in ("exam.start", "exam.answer", "exam.submit")}}

    # ---- final assessment by keyboard (resume the attempt) ----
    nav("Program", lambda t: heading(t, PROGRAM, 1) is not None)
    nav("CAPSTONE", lambda t: heading(t, INTRO, 1) is not None)
    mark = len(endpoint.requests())
    app.ax("press", "AXLink", "Resume an attempt")
    wait(lambda t: heading(t, prog.EXAM_PROMPT, 1) is not None)
    time.sleep(0.8)
    kb = {"option_focused": focus_to("AXCheckBox", "A Before the model")}
    if kb["option_focused"]:
        app.keycode(49)
        time.sleep(0.6)
    kb["option_selected"] = (app.node(app.tree(), "AXCheckBox", "A Before the model") or {}).get("value") == 1
    kb["submit_focused"] = focus_to("AXButton", "See results", 20)
    if kb["submit_focused"]:
        app.keycode(36)
    _, kb["unknown_announced"] = wait(lambda t: shows(t, UNKNOWN), 10)
    kb["unknown_in_alert"] = in_alert(app.tree(), UNKNOWN)
    kb["check_focused"] = focus_to("AXButton", "Check submission status", 40)
    if kb["check_focused"]:
        app.keycode(36)
    tree, kb["pending_review"] = wait(lambda t: heading(t, PENDING, 1) is not None, 10)
    kb["submits"] = len([c for c in calls_since(mark) if c["procedure"] == "exam.submit"])
    obs["exam_keyboard"] = kb
    obs["tree_exam_keyboard"] = save_tree("exam-keyboard", tree)


    # ---- certificate: verify link and print ----
    nav("Program", lambda t: heading(t, PROGRAM, 1) is not None)
    mark = len(endpoint.requests())
    tree, cert = nav("CREDENTIALS", lambda t: app.node(t, "AXButton", "Download PDF") is not None)
    share = next((n.get("url") for n in tree if n.get("role") == "AXLink" and n.get("label") == "Add to LinkedIn"), None)
    from urllib.parse import parse_qs, urlsplit
    cert_url = (parse_qs(urlsplit(share).query).get("certUrl") or [None])[0] if share else None
    before = prints()
    app.ax("press", "AXButton", "Download PDF")
    time.sleep(1.5)
    tree = app.tree()
    after_print = prints()
    obs["tree_certificate"] = save_tree("certificate", tree)
    # The print sheet cannot open: the page says so and nothing changes.
    flag = os.path.join(fx.DATA, "fixture-print-unavailable")
    open(flag, "w").close()
    app.ax("press", "AXButton", "Download PDF")
    tree, print_failed = wait(lambda t: shows(t, "The print sheet could not open"), 6)
    os.remove(flag)
    obs["tree_certificate_print_failed"] = save_tree("certificate-print-failed", tree)
    obs["certificate"] = {"shown": cert, "cert_url": cert_url, "prints_before": before, "prints_after": after_print,
                          "failure_shown": print_failed, "in_alert": in_alert(tree, "The print sheet could not open"),
                          "prints_after_failure": prints(),
                          "still_on_certificate": app.node(tree, "AXButton", "Download PDF") is not None,
                          "calls": [[c["procedure"], c["status"]] for c in calls_since(mark)]}

    # ---- desktop lab attempt ----
    nav("Learn", lambda t: heading(t, CATALOG) is not None)
    app.ax("press", "AXCheckBox", "Courses")
    wait(lambda t: app.node(t, "AXLink", f"COURSE {COURSE}") is not None, 5)
    app.ax("press", "AXLink", f"COURSE {COURSE}")
    wait(lambda t: heading(t, COURSE, 1) is not None)
    mark = len(endpoint.requests())
    tree, intro_ok = nav("Lab: Scanning", lambda t: app.node(t, "AXButton", "Start lab") is not None)
    obs["tree_lab_intro"] = save_tree("lab-intro", tree)
    app.ax("press", "AXButton", "Start lab")
    tree, started = wait(lambda t: heading(t, "Triage a red-team campaign", 2) is not None)
    app.ax("press", "AXCheckBox", "True positive", 0)
    time.sleep(0.6)
    app.ax("press", "AXCheckBox", "False positive", 1)
    time.sleep(0.6)
    app.ax("press", "AXButton", "Block the release")
    tree, passed = wait(lambda t: shows(t, LAB_PASSED), 15)
    obs["tree_lab_passed"] = save_tree("lab-passed", tree)
    lab_calls = calls_since(mark)
    obs["lab"] = {"intro": intro_ok, "started": started, "passed": passed,
                  "calls": [[c["procedure"], c["status"]] for c in lab_calls],
                  "bodies": [[c["procedure"], body(c)] for c in lab_calls if c["procedure"].startswith("desktop.")]}
    # A lab without a desktop adapter: Academy refuses the start; previewed.
    app.ax("press", "AXLink", COURSE)
    wait(lambda t: heading(t, COURSE, 1) is not None)
    app.ax("press", "AXButton", "2\n")
    wait(lambda t: app.node(t, "AXLink", "Lab: Responding") is not None, 5)
    mark = len(endpoint.requests())
    nav("Lab: Responding", lambda t: app.node(t, "AXButton", "Start lab") is not None)
    app.ax("press", "AXButton", "Start lab")
    tree, preview = wait(lambda t: shows(t, "no lab sandbox was started"), 10)
    obs["lab_unsupported"] = {"previewed": preview, "calls": [[c["procedure"], c["status"]] for c in calls_since(mark)]}
    obs["tree_lab_unsupported"] = save_tree("lab-unsupported", tree)

    # ---- Shift: a slow optional briefing never holds the timed answer ----
    nav("Home", lambda t: shows(t, "LVL 4"))
    mark = len(endpoint.requests())
    t0 = time.time()
    tree, shift = nav("The SOC is open", lambda t: shows(t, prog.SHIFT_PROMPT))
    wait(lambda t: app.node(t, "AXCheckBox", "Block the request") is not None, 5)
    app.ax("press", "AXCheckBox", "Block the request")
    time.sleep(0.4)
    app.ax("press", "AXButton", "Submit")
    tree, explained = wait(lambda t: shows(t, prog.SHIFT_EXPLANATION), 5)
    answered_at = time.time()
    obs["tree_shift"] = save_tree("shift-answered", tree)
    time.sleep(prog.BRIEFING_DELAY_MS / 1000 + 1)
    shift_calls = calls_since(mark)
    by = {}
    for c in shift_calls:
        by.setdefault(c["procedure"], []).append(c)
    briefing_done = min((c["t"] for c in by.get("shift.briefing", [])), default=None)
    obs["shift"] = {"shown": shift, "explained": explained, "answered_after_s": round(answered_at - t0, 2),
                    "sla_from_academy": shows(tree, "SLA remaining") or any(n.get("label") == "SLA remaining" for n in tree),
                    "serve": [round(c["t"] - t0, 2) for c in by.get("shift.serve", [])],
                    "answer": [round(c["t"] - t0, 2) for c in by.get("shift.answer", [])],
                    "briefing_replied": None if briefing_done is None else round(briefing_done - t0, 2),
                    "answer_body": body(by["shift.answer"][0]) if by.get("shift.answer") else None}

    # ---- lesson feedback on a completed lesson ----
    nav("Learn", lambda t: heading(t, CATALOG) is not None)
    app.ax("press", "AXCheckBox", "Courses")
    wait(lambda t: app.node(t, "AXLink", f"COURSE {COURSE}") is not None, 5)
    app.ax("press", "AXLink", f"COURSE {COURSE}")
    wait(lambda t: heading(t, COURSE, 1) is not None)
    tree, _ = nav("Scanning at runtime: part 1 ", lambda t: heading(t, "Scanning at runtime: part 1", 1) is not None)
    time.sleep(1)
    not_done_asks = shows(app.tree(), "How was this module?")
    app.ax("press", "AXLink", COURSE)
    wait(lambda t: heading(t, COURSE, 1) is not None)
    mark = len(endpoint.requests())
    tree, widget = nav("Scanning at runtime: part 3 ", lambda t: shows(t, "How was this module?"))
    star = next((n for n in tree if (n.get("label") or "") == "Rate 4 stars"), {})
    app.ax("press", star.get("role") or "AXCheckBox", "Rate 4 stars")
    time.sleep(0.5)
    app.ax("press", "AXButton", "Submit feedback")
    tree, thanks = wait(lambda t: shows(t, "Thanks for your feedback!"), 8)
    obs["tree_feedback"] = save_tree("lesson-feedback", tree)
    obs["feedback"] = {"not_done_asks": not_done_asks, "widget": widget, "thanks": thanks,
                       "sent": [body(c) for c in calls_since(mark) if c["procedure"] == "admin.feedback.submit"]}

    # ---- course reset, then removal ----
    app.ax("press", "AXLink", COURSE)
    wait(lambda t: app.node(t, "AXButton", "Reset course progress") is not None)
    mark = len(endpoint.requests())
    app.ax("press", "AXButton", "Reset course progress")
    tree, reset_dialog = wait(lambda t: heading(t, "Reset your progress?", 2) is not None, 5)
    obs["tree_reset_dialog"] = save_tree("reset-dialog", tree)
    reset = {"dialog": reset_dialog, "counts": shows(tree, "3 units, 1 quizzes, 0 labs"),
             "certificates_stay": shows(tree, "Certificates you’ve already earned stay valid.")}
    app.ax("press", "AXButton", "Cancel")
    time.sleep(1)
    reset["after_cancel"] = [c["procedure"] for c in calls_since(mark) if c["procedure"] == "progress.resetCourse"]
    app.ax("press", "AXButton", "Reset course progress")
    wait(lambda t: heading(t, "Reset your progress?", 2) is not None, 5)
    confirms = [n for n in app.tree() if n.get("role") == "AXButton" and n.get("label") == "Reset course progress"]
    app.ax("press", "AXButton", "Reset course progress", len(confirms) - 1)
    time.sleep(2)
    reset["resets"] = [body(c) for c in calls_since(mark) if c["procedure"] == "progress.resetCourse"]
    obs["reset"] = reset
    wait(lambda t: app.node(t, "AXButton", "Remove enrollment") is not None)
    mark = len(endpoint.requests())
    app.ax("press", "AXButton", "Remove enrollment")
    tree, dialog = wait(lambda t: heading(t, "Remove this course?", 2) is not None, 5)
    obs["tree_removal_dialog"] = save_tree("removal-dialog", tree)
    removal = {"dialog": dialog,
               "names_certification": shows(tree, "enrollment in the AI Application Security certification"),
               "names_courses": shows(tree, "Runtime Security Foundations, Model Security"),
               "certificates_stay": shows(tree, "Certificates you’ve already earned stay valid.")}
    app.ax("press", "AXButton", "Cancel")
    time.sleep(1)
    removal["after_cancel"] = [c["procedure"] for c in calls_since(mark) if c["procedure"] == "enrollment.removeByCourse"]
    app.ax("press", "AXButton", "Remove enrollment")
    wait(lambda t: heading(t, "Remove this course?", 2) is not None, 5)
    # The modal hides the page behind it, so its confirm is the last match.
    confirms = [n for n in app.tree() if n.get("role") == "AXButton" and n.get("label") == "Remove enrollment"]
    app.ax("press", "AXButton", "Remove enrollment", len(confirms) - 1)
    time.sleep(2)
    removal["removals"] = [body(c) for c in calls_since(mark) if c["procedure"] == "enrollment.removeByCourse"]
    obs["removal"] = removal

    # ---- isolation ----
    mark = len(endpoint.requests())
    away = {}
    for label, title in (("Runtime Security", "Scan a prompt or response"), ("Harness", "Harness")):
        app.ax("press", "AXButton", label)
        wait(lambda t, title=title: heading(t, title, 1) is not None, 6)
        time.sleep(2)
        tree = app.tree()
        away[label] = {"tree": save_tree(f"away-{label.split()[0].lower()}", tree),
                       "markers": [m for m in LEARNER_MARKERS if shows(tree, m)]}
    obs["away"] = away
    obs["away_requests"] = [c["procedure"] for c in calls_since(mark)]

    # ---- zoom and minimum size on the assessment ----
    go_academy()
    nav("Program", lambda t: heading(t, PROGRAM, 1) is not None)
    nav("CAPSTONE", lambda t: heading(t, INTRO, 1) is not None)
    app.ax("press", "AXLink", "Resume an attempt")
    tree, obs["zoom_runner"] = wait(lambda t: heading(t, prog.EXAM_PROMPT, 1) is not None)
    obs["tree_zoom_start"] = save_tree("zoom-start", tree)
    # 'See results' takes focus once an answer is chosen.
    app.ax("press", "AXCheckBox", "A Before the model")
    time.sleep(0.6)
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = reach(app, 2, "See results")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = reach(app, 1, "See results")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    time.sleep(0.5)

    # ---- account page and its sign-out ----
    mark = len(endpoint.requests())
    tree, account = nav("Settings", lambda t: heading(t, "Ada Lovelace", 1) is not None)
    obs["tree_account"] = save_tree("account", tree)
    acct = {"page": account, "photo_here": app.node(tree, "AXButton", "Change photo") is not None
            and not shows(tree, "Change your photo in Academy on the web."),
            "host_appearance": shows(tree, "This app sets the theme and accent"),
            "no_theme_choice": not any(n.get("role") == "AXRadioGroup" and n.get("label") == "Interface theme" for n in tree),
            "items_before": fx.keychain_accounts(ACADEMY_SERVICE)}
    # Avatar: chosen natively, checked, stored and confirmed by the core,
    # read back as an image this WebView can show; cancelled; removed.
    avatar = {}
    # A photo larger than Academy's 2 MiB upload cap, as cameras produce.
    image = png(1400, 1400, noise=True)
    mark = len(endpoint.requests())
    with open(os.path.join(fx.DATA, "fixture-avatar.img"), "wb") as f:
        f.write(image)
    app.ax("press", "AXButton", "Change photo")
    tree, avatar["uploaded"] = wait(lambda t: app.node(t, "AXButton", "Remove photo") is not None, 15)
    time.sleep(2)
    tree = app.tree()
    obs["tree_avatar"] = save_tree("avatar-uploaded", tree)
    upload = endpoint.requests()[mark:]
    avatar["load_warning"] = shows(tree, "can't be loaded") or shows(tree, "can’t be loaded")
    avatar["presign"] = [[body(c), c["authorization"]] for c in upload if c.get("procedure") == "settings.avatarUploadUrl"]
    avatar["put"] = [[r["contentType"], r["bytes"], r["authorization"], r["status"]] for r in upload if r.get("put")]
    avatar["confirm"] = [[body(c), c["authorization"]] for c in upload if c.get("procedure") == "settings.confirmAvatar"]
    avatar["read_back"] = [[r["avatarRead"], r["authorization"], r["status"]] for r in upload if r.get("avatarRead")]
    avatar["image_bytes"] = len(image)
    # An extreme portrait (1x8192) is prepared from its centre and uploaded.
    mark = len(endpoint.requests())
    with open(os.path.join(fx.DATA, "fixture-avatar.img"), "wb") as f:
        f.write(png(1, 8192))
    app.ax("press", "AXButton", "Change photo")
    wait(lambda t: any(r.get("procedure") == "settings.confirmAvatar" for r in endpoint.requests()[mark:]), 15)
    time.sleep(2)
    strip = endpoint.requests()[mark:]
    avatar["strip_put"] = [[r["contentType"], r["bytes"], r["status"]] for r in strip if r.get("put")]
    avatar["strip_confirmed"] = len([c for c in strip if c.get("procedure") == "settings.confirmAvatar" and c["status"] == 200])
    avatar["strip_warning"] = shows(app.tree(), "can't be loaded") or shows(app.tree(), "can’t be loaded")
    # The store keeps a second photo under the same address but its reply
    # is lost: the page must not claim the photo is unchanged, and reads
    # the photo Academy serves at the unchanged address again.
    endpoint.mode(approval="granted", displayName="Ada Lovelace", lose_put_reply=True)
    mark = len(endpoint.requests())
    with open(os.path.join(fx.DATA, "fixture-avatar.img"), "wb") as f:
        f.write(png(64, 64))
    app.ax("press", "AXButton", "Change photo")
    tree, avatar["lost_message"] = wait(lambda t: shows(t, "Your photo may have changed, but Academy did not confirm it"), 20)
    time.sleep(2)
    lost = endpoint.requests()[mark:]
    avatar["lost_put"] = [r["status"] for r in lost if r.get("put")]
    avatar["lost_confirms"] = len([c for c in lost if c.get("procedure") == "settings.confirmAvatar"])
    avatar["lost_read_again"] = [r["avatarRead"] for r in lost if r.get("avatarRead")]
    obs["tree_avatar_lost_reply"] = save_tree("avatar-lost-reply", app.tree())
    endpoint.mode(approval="granted", displayName="Ada Lovelace")
    # A file with a PNG signature that does not decode: nothing is sent.
    mark = len(endpoint.requests())
    with open(os.path.join(fx.DATA, "fixture-avatar.img"), "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n" + b"synthetic text, not pixels" * 40)
    app.ax("press", "AXButton", "Change photo")
    tree, avatar["malformed_message"] = wait(lambda t: shows(t, "Upload failed. Your photo is unchanged"), 8)
    avatar["malformed_in_alert"] = in_alert(tree, "Upload failed. Your photo is unchanged")
    avatar["malformed_requests"] = [c.get("procedure") or c["path"] for c in endpoint.requests()[mark:]
                                    if c.get("put") or (c.get("procedure") or "").startswith("settings.avatar")
                                    or c.get("procedure") == "settings.confirmAvatar"]
    # Within 8192 pixels a side but beyond the decoder's memory budget once
    # decoded (147 MB of pixels): refused before decoding, nothing sent.
    mark = len(endpoint.requests())
    with open(os.path.join(fx.DATA, "fixture-avatar.img"), "wb") as f:
        f.write(png(7000, 7000))
    app.ax("press", "AXButton", "Change photo")
    time.sleep(0.5)
    tree, avatar["over_budget_message"] = wait(lambda t: shows(t, "Upload failed. Your photo is unchanged"), 15)
    avatar["over_budget_requests"] = [c.get("procedure") or c["path"] for c in endpoint.requests()[mark:]
                                      if c.get("put") or (c.get("procedure") or "").startswith("settings.avatar")
                                      or c.get("procedure") == "settings.confirmAvatar"]
    mark = len(endpoint.requests())
    app.ax("press", "AXButton", "Change photo")
    time.sleep(2.5)
    tree = app.tree()
    avatar["cancel_requests"] = [c.get("procedure") or c["path"] for c in endpoint.requests()[mark:]
                                 if c.get("put") or (c.get("procedure") or "").startswith("settings.")
                                 and c["procedure"] != "settings.get"]
    avatar["cancel_alert"] = shows(tree, "Upload failed") or shows(tree, "may have saved")
    mark = len(endpoint.requests())
    app.ax("press", "AXButton", "Remove photo")
    tree, avatar["removed"] = wait(lambda t: app.node(t, "AXButton", "Remove photo") is None, 10)
    avatar["removals"] = len([c for c in calls_since(mark) if c["procedure"] == "settings.removeAvatar"])
    obs["avatar"] = avatar
    app.ax("press", "AXButton", "Sign out")
    tree, acct["guest_nav"] = wait(lambda t: nav_links(t) == ["Learn"], 10)
    acct["guest_nav_links"] = nav_links(tree)
    app.open_settings()
    _, acct["shell_signed_out"] = wait(lambda t: app.has_text(t, "Not signed in to Academy."), 10)
    acct["items_after"] = fx.keychain_accounts(ACADEMY_SERVICE)
    acct["revoked"] = any(r["path"] == "/oauth/revoke" for r in endpoint.requests()[mark:])
    obs["account"] = acct
    obs["tree_after_sign_out"] = save_tree("after-sign-out", tree)

    # ---- the timer submits at Academy's deadline ----
    # Signed in again (a fresh learner generation and cache), the learner
    # resumes the attempt; Academy's deadline is now 8 s away.
    endpoint.mode(approval="granted", displayName="Ada Lovelace", timed={"exam.getActive": 8})
    app.open_settings()
    app.ax("press", "AXButton", "Sign in to Academy")
    wait(lambda t: app.has_text(t, "Signed in to Academy as"), 15)
    go_academy()
    nav("Program", lambda t: heading(t, PROGRAM, 1) is not None)
    nav("CAPSTONE", lambda t: heading(t, INTRO, 1) is not None)
    mark = len(endpoint.requests())
    app.ax("press", "AXLink", "Resume an attempt")
    tree, _ = wait(lambda t: heading(t, prog.EXAM_PROMPT, 1) is not None)
    time.sleep(1)
    near = timer_text(app.tree())
    tree, expired_unknown = wait(lambda t: shows(t, UNKNOWN), 15)
    submit_at = [c["t"] for c in calls_since(mark) if c["procedure"] == "exam.submit"]
    time.sleep(3)
    obs["exam_deadline"] = {"countdown": near, "unknown": expired_unknown,
                            "submits": len([c for c in calls_since(mark) if c["procedure"] == "exam.submit"]),
                            "submitted_after_s": None if not submit_at else round(submit_at[0] - endpoint.requests()[mark]["t"], 2),
                            "timer_after": timer_text(app.tree())}
    obs["tree_exam_deadline"] = save_tree("exam-deadline", app.tree())
    endpoint.mode(approval="granted", displayName="Ada Lovelace")

    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["truth_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in ("answerKey", "correctValue", "AIRS-C05-TRUTH-CANARY"))]
    app.quit()
    endpoint.stop()
    obs["requests"] = endpoint.requests()
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def reach(app, zoom, label):
    """Tabs until `label` has focus; it must lie inside the window and end
    above the 60 CSS px bottom fade."""
    window = next(n for n in app.tree() if n.get("role") == "AXWindow")
    bottom = window["pos"][1] + window["size"][1]
    found = None
    for _ in range(90):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("role") == "AXButton" and f.get("label") == label:
            found = {"label": f["label"], "pos": f["pos"], "size": f["size"]}
            break
    clear = bool(found) and found["pos"][1] >= window["pos"][1] and found["pos"][1] + found["size"][1] <= bottom - 60 * zoom
    return {"zoom": zoom, "window": {"pos": window["pos"], "size": window["size"]}, "found": found, "clear": clear}


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C06"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    h, p = o["home"], o["program"]
    add("C06-N01", o["guest_nav"] == ["Learn"] and o["signed_in"] and o["account_nav"] == NAV_SIGNED_IN
        and h["briefing"] and not h["failure_toast"] and p["hub"] and len(p["cards"]) == 5,
        f"guest Academy navigation {o['guest_nav']}; signed in: {o['signed_in']}; account navigation {o['account_nav']}; "
        f"Home shows Academy's level and XP: {h['briefing']}, failure toast: {h['failure_toast']}; Program hub: {p['hub']} with cards {p['cards']}",
        [o["tree_guest"], o["tree_home"], o["tree_program"]])

    e = o["exam"]
    submits = [c for c in e["calls"] if c[0] == "exam.submit"]
    exam_ok = (e["intro"] and e["runner"] and bool(e["countdown"]) and e["countdown"] >= "59:00" and e["proctored"] and e["unknown"] and e["answer_kept"] and e["submits_before_check"] == 1
               and submits == [["exam.submit", 503, True]] and e["pending_review"] and e["server_text_hidden"]
               and ["exam.submissionState", 200, True] in e["calls"] and not o["truth_in_trees"])
    add("C06-N02", exam_ok,
        f"intro with Academy's eligibility facts: {e['intro']}; runner with Academy's question: {e['runner']}; Academy's countdown in view: {e['countdown']} (Time remaining, Proctored: {e['proctored']}); a submission the server failed after receiving shows "
        f"'{UNKNOWN}': {e['unknown']}, answer kept: {e['answer_kept']}, server text hidden: {e['server_text_hidden']}; submissions (procedure, status, Authorization) {submits}; "
        f"'Check submission status' reads Academy's state and shows its pending review ('{PENDING}'): {e['pending_review']}; answer keys or canaries in AX snapshots: {o['truth_in_trees']}",
        [o["tree_exam_intro"], o["tree_exam_runner"], o["tree_exam_unknown"], o["tree_exam_pending"]])

    kb = o["exam_keyboard"]
    add("C06-N03", all(kb[k] for k in ("option_focused", "option_selected", "submit_focused", "unknown_announced", "unknown_in_alert", "check_focused", "pending_review"))
        and kb["submits"] == 1,
        f"Tab reached the first option: {kb['option_focused']}; Space selected it: {kb['option_selected']}; Tab reached 'See results': {kb['submit_focused']}; "
        f"Return submitted once ({kb['submits']} submission) and the unknown outcome was shown: {kb['unknown_announced']}, inside an alert region: {kb['unknown_in_alert']}; "
        f"Tab reached 'Check submission status': {kb['check_focused']}; Return showed Academy's pending review: {kb['pending_review']}",
        [o["tree_exam_keyboard"]])

    c = o["certificate"]
    expected = f"{o['origin']}/verify/cert-certification"
    add("C06-N04", c["shown"] and c["cert_url"] == expected and len(c["prints_after"]) == len(c["prints_before"]) + 1
        and c["failure_shown"] and c["in_alert"] and c["prints_after_failure"] == c["prints_after"]
        and c["still_on_certificate"] and not [x for x in c["calls"] if x[0] not in ("certificate.mine", "certificate.get", "settings.get", "certifications.list")],
        f"certificate shown: {c['shown']}; public verify URL {c['cert_url']} (Academy web origin {expected}); print sheets opened: {len(c['prints_before'])} -> {len(c['prints_after'])}; "
        f"a sheet that cannot open says 'The print sheet could not open' in an alert region: {c['failure_shown'] and c['in_alert']}, sheets opened after it: {len(c['prints_after_failure'])}; "
        f"still on the certificate: {c['still_on_certificate']}; learner calls (no mutation) {c['calls']}",
        [o["tree_certificate"], o["tree_certificate_print_failed"]])

    lab = o["lab"]
    bodies = lab["bodies"]
    starts = [b for p_, b in bodies if p_ == "desktop.attempt.start"]
    saves = [b for p_, b in bodies if p_ == "desktop.draft.save"]
    grades = [b for p_, b in bodies if p_ == "desktop.attempt.submit"]
    web_runs = [x[0] for x in lab["calls"] if x[0].startswith("labs.") and x[0] != "labs.get"]
    lab_ok = (lab["intro"] and lab["started"] and lab["passed"] and len(starts) == 1 and starts[0].get("labId") == "m-rt-1-lab-def"
              and bool(starts[0].get("requestId")) and saves and all("baseVersion" in s and s.get("clientMutationId") for s in saves)
              and len(grades) == 1 and isinstance(grades[0].get("draftVersion"), int) and bool(grades[0].get("submissionId"))
              and not web_runs and o["lab_unsupported"]["previewed"])
    add("C06-N05", lab_ok,
        f"lab intro: {lab['intro']}; attempt started once with labId and requestId: {starts}; draft saves with baseVersion and clientMutationId: {len(saves)}; "
        f"grading request {grades}; '{LAB_PASSED}': {lab['passed']}; web lab run calls: {web_runs}; a lab without a desktop adapter is previewed: "
        f"{o['lab_unsupported']['previewed']} (calls {o['lab_unsupported']['calls']})",
        [o["tree_lab_intro"], o["tree_lab_passed"], o["tree_lab_unsupported"]])

    s = o["shift"]
    shift_ok = (s["shown"] and s["explained"] and s["serve"] and s["answer"] and s["briefing_replied"] is not None
                and max(s["serve"] + s["answer"]) < s["briefing_replied"] and s["answer_body"] == {"shiftId": prog.SHIFT, "order": 2, "values": ["0"]})
    add("C06-N06", shift_ok,
        f"Shift alert shown: {s['shown']}; serve replied at {s['serve']} s and the answer at {s['answer']} s while the optional briefing replied at {s['briefing_replied']} s; "
        f"Academy's explanation shown after {s['answered_after_s']} s: {s['explained']}; answer request {s['answer_body']}; Academy's SLA countdown present: {s['sla_from_academy']}",
        [o["tree_shift"]])

    r = o["removal"]
    add("C06-N07", r["dialog"] and r["names_certification"] and r["names_courses"] and r["certificates_stay"] and r["after_cancel"] == []
        and r["removals"] == [{"courseId": academy_catalog.COURSE}],
        f"confirmation dialog: {r['dialog']}; names the certification: {r['names_certification']} and its courses: {r['names_courses']}; "
        f"earned certificates stay valid: {r['certificates_stay']}; removals after Cancel: {r['after_cancel']}; removals after confirming: {r['removals']}",
        [o["tree_removal_dialog"]])

    a = o["account"]
    add("C06-N08", a["page"] and a["photo_here"] and a["host_appearance"] and a["no_theme_choice"]
        and a["guest_nav"] and a["shell_signed_out"] and a["items_before"] and a["items_after"] == [] and a["revoked"],
        f"account page from Academy's Settings entry: {a['page']}; photo changed here ('Change photo'): {a['photo_here']}; "
        f"appearance follows the app: {a['host_appearance']}, no theme choice: {a['no_theme_choice']}; Academy's Sign out returned the destination to guest navigation "
        f"{a['guest_nav_links']} and Settings shows 'Not signed in to Academy.': {a['shell_signed_out']}; learner keychain items {a['items_before']} -> {a['items_after']}; revoked: {a['revoked']}",
        [o["tree_account"], o["tree_after_sign_out"]])

    calls = learner_calls(o["requests"])
    unadmitted = sorted({x["procedure"] for x in calls if x["procedure"] not in ACCOUNT and x["procedure"] not in PUBLIC})
    privileged = sorted({x["procedure"] for x in calls if x["procedure"].startswith(PRIVILEGED)})
    leaked = sorted({f"{x['procedure']}:{k}" for x in calls if x["procedure"].split(".")[0] in ("exam", "shift", "desktop", "enrollment")
                     for k in keys_deep(body(x)) if k in FORBIDDEN_INPUT_KEYS})
    guest_auth = [x["procedure"] for x in calls if x["procedure"] in PUBLIC and x["procedure"] not in ACCOUNT and x["authorization"]]
    add("C06-N09", not unadmitted and not privileged and not leaked and not guest_auth,
        f"{len(calls)} learner requests; outside the allowlist and public queries: {unadmitted}; assessor, grading, award or admin procedures: {privileged}; "
        f"hosted-decision fields in learner inputs: {leaked}; public-only queries sent with Authorization: {guest_auth}",
        [])

    away = o["away"]
    add("C06-N10", all(not v["markers"] for v in away.values()) and not o["away_requests"],
        f"learner markers in Runtime Security: {away['Runtime Security']['markers']}; in Harness: {away['Harness']['markers']}; learner calls while away from Academy: {o['away_requests']}",
        [v["tree"] for v in away.values()])

    add("C06-N11", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window {o['min_size']['window']}, clear: {o['min_size']['clear']}",
        [])

    d = o["exam_deadline"]
    add("C06-N12", bool(d["countdown"]) and d["countdown"] <= "00:08" and d["unknown"] and d["submits"] == 1 and d["timer_after"] in (None, "00:00"),
        f"with Academy's deadline 8 s away the countdown read {d['countdown']}; at the deadline the runner submitted once ({d['submits']} submission, "
        f"{d['submitted_after_s']} s after the attempt was read) and reported the unknown outcome: {d['unknown']}; nothing was resent in the next 3 s; the countdown then reads {d['timer_after']}",
        [o["tree_exam_deadline"]])

    av = o["avatar"]
    put_bytes = av["put"][0][1] if av["put"] else None
    av_ok = (av["uploaded"] and not av["load_warning"] and av["image_bytes"] > 2 * 1024 * 1024
             and av["presign"] == [[{"contentType": "image/jpeg", "size": put_bytes}, True]]
             and av["put"] == [["image/jpeg", put_bytes, False, 200]] and put_bytes <= 2 * 1024 * 1024
             and av["confirm"] == [[{"ext": "jpg"}, True]]
             and av["malformed_message"] and av["malformed_in_alert"] and av["malformed_requests"] == []
             and av["over_budget_message"] and av["over_budget_requests"] == []
             and len(av["strip_put"]) == 1 and av["strip_put"][0][0] == "image/jpeg" and av["strip_put"][0][1] <= 2 * 1024 * 1024
             and av["strip_put"][0][2] == 200 and av["strip_confirmed"] == 1 and not av["strip_warning"]
             and av["lost_message"] and av["lost_put"] == [0] and av["lost_confirms"] == 0 and av["lost_read_again"]
             and av["read_back"] and all(not auth and status == 200 for _, auth, status in av["read_back"])
             and av["cancel_requests"] == [] and not av["cancel_alert"] and av["removed"] and av["removals"] == 1)
    add("C06-N13", av_ok,
        f"'Change photo' prepared the staged {av['image_bytes']} byte PNG (above Academy's 2 MiB cap) as a {put_bytes} byte JPEG: presign (input, Authorization) {av['presign']}; store PUT (type, bytes, Authorization, status) {av['put']}; "
        f"a stored photo whose reply was lost said 'Your photo may have changed, but Academy did not confirm it': {av['lost_message']} (PUT statuses {av['lost_put']}, confirmations {av['lost_confirms']}) and read the photo at the same address again: {av['lost_read_again']}; "
        f"a 1x8192 portrait was prepared from its centre and uploaded (PUT {av['strip_put']}, confirmed {av['strip_confirmed']}, load warning {av['strip_warning']}); "
        f"a 7000x7000 photo beyond the decoder memory budget said the photo is unchanged: {av['over_budget_message']} and sent {av['over_budget_requests']}; "
        f"a file with a PNG signature that does not decode said 'Upload failed. Your photo is unchanged' in an alert region: {av['malformed_message'] and av['malformed_in_alert']} and sent {av['malformed_requests']}; "
        f"confirm {av['confirm']}; read back (path, Authorization, status) {av['read_back']}; 'Remove photo' offered: {av['uploaded']}; image load warning: {av['load_warning']}; "
        f"a cancelled pick sent {av['cancel_requests']} and showed an error: {av['cancel_alert']}; 'Remove photo' removed it: {av['removed']} ({av['removals']} removal)",
        [o["tree_avatar"], o["tree_avatar_lost_reply"]])

    rs = o["reset"]
    add("C06-N14", rs["dialog"] and rs["counts"] and rs["certificates_stay"] and rs["after_cancel"] == [] and rs["resets"] == [{"courseId": academy_catalog.COURSE}],
        f"'Reset course progress' asks 'Reset your progress?': {rs['dialog']} with Academy's counts '3 units, 1 quizzes, 0 labs': {rs['counts']} and earned certificates kept: "
        f"{rs['certificates_stay']}; resets after Cancel: {rs['after_cancel']}; after confirming: {rs['resets']}",
        [o["tree_reset_dialog"]])

    fb = o["feedback"]
    add("C06-N15", not fb["not_done_asks"] and fb["widget"] and fb["thanks"]
        and fb["sent"] == [{"moduleId": "m-rt-1", "lessonId": prog.FEEDBACK_UNIT, "rating": 4}],
        f"an unfinished lesson asks for no rating: {not fb['not_done_asks']}; the completed lesson asks 'How was this module?': {fb['widget']}; "
        f"'Rate 4 stars' then 'Submit feedback' thanked the learner: {fb['thanks']} and sent {fb['sent']}",
        [o["tree_feedback"]])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C06",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against the synthetic Academy endpoint tools/native-acceptance/academy_server.py serving tools/native-acceptance/academy_catalog.py and academy_program.py; tools/native-acceptance/acceptance_c06.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Academy is a local synthetic endpoint answering Academy's learner procedures with synthetic coursework shaped after Academy's routers at the pinned commit; hosted Academy (timing, grading, awards) is qualified in C25 (L13).",
            "The fixture build records print sheets and browser pages instead of opening them; the native print sheet itself is not driven.",
            "The identity practical is reached from identity-course pages the synthetic catalog does not carry; its assessor gating is covered by the route sweep and Academy's own tests, not natively.",
            "The fixture build answers the avatar picker with an image the driver stages in its data root, and records instead of opening the print sheet; the native NSOpenPanel and print sheet are not driven.",
            "Spoken VoiceOver output was not recorded; AX roles, names, focus and live regions are inspected.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for ch in checks:
        print(ch["result"].upper(), ch["id"], ch["observation"][:220])
    return 0 if all(ch["result"] == "pass" for ch in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
