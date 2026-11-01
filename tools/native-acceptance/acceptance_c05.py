#!/usr/bin/env python3
"""C05 native acceptance: Academy's own learner pages (catalog, course, long
lessons with diagrams and links, knowledge check, lab preview) in the
prepared fixture app, driven through macOS Accessibility against the
synthetic Academy endpoint (academy_server.py with academy_catalog.py),
evaluated against the C05 inventory (docs/architecture/native-checks.json).

The fixture build trusts the disposable test CA, talks only to the synthetic
endpoint named in its data root, records (does not launch) browser pages and
keeps credentials in its dedicated fixture keychain (inspected by attributes
only). The fixture root is removed at the end.

One launch: guest catalog and guest knowledge-check denial; sign-in; keyboard
navigation catalog -> course -> lesson; diagrams and links; knowledge check;
lab preview; cross-feature isolation; themes; zoom and minimum size; account
change; reset.
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import academy_catalog  # noqa: E402
import fixture_app as fx  # noqa: E402
import v2_setup as v2s  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C05")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C05" else "native-C05")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C05" else "native-acceptance-C05.json")
ACADEMY_SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.academy"
CATALOG = "What will you take up next?"
COURSE = "Runtime Security Foundations"
LESSON = "Scanning at runtime: part 1"
LESSON2 = "Scanning at runtime: part 2"
SIGN_IN_GATE = "This content is part of the signed-in curriculum"
LOCKED_GATE = "This content is locked"
PROGRESSION = "Finish more of the previous path first"
FIRST_PARAGRAPH = "Lesson 1 of Scanning at runtime opens here"
GUEST_NOTE = "Browsing as a guest"
PUBLIC = set(json.load(open(os.path.join(ROOT, "packages/desktop-contracts/fixtures/academy/learner-procedures.json")))["public"])
ACCOUNT = {p["procedure"] for p in json.load(open(os.path.join(ROOT, "packages/desktop-contracts/fixtures/academy/learner-procedures.json")))["learner"]}
# Learner chrome that must never appear outside the Academy destination.
LEARNER_MARKERS = ["Run learner call probe", GUEST_NOTE, "Reading progress", "Complete & continue", " XP",
                   "Academy location", "You scored", "Start knowledge check", "Hands-on lab", "HANDS-ON LAB"]
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


def correct_result_line(tree):
    """Read the result paragraph's inline AX text within this knowledge check."""
    owner = next((i for i, n in enumerate(tree) if n.get("role") == "AXGroup"
                  and n.get("label") == "Knowledge check: Scanning at runtime"), None)
    if owner is None:
        return False
    end = next((i for i in range(owner + 1, len(tree))
                if tree[i].get("depth", 0) <= tree[owner].get("depth", 0)), len(tree))
    for i in range(owner + 1, end):
        if tree[i].get("role") != "AXGroup":
            continue
        parts = []
        for node in tree[i + 1:end]:
            if node.get("depth", 0) <= tree[i].get("depth", 0):
                break
            if node.get("role") == "AXStaticText":
                parts.append(node.get("label") or "")
        if "".join(parts).upper().startswith("1 OF 1 CORRECT"):
            return True
    return False


def heading(tree, label, level=None):
    return next((n for n in tree if n.get("role") == "AXHeading" and (n.get("label") or "") == label
                 and (level is None or n.get("value") == level)), None)


def trail_links(tree):
    """Links of the 'Academy location' trail, in order (from C06 the
    destination's own navigation also offers a 'Learn' link)."""
    out, depth = [], None
    for n in tree:
        d = n.get("depth", 0)
        if depth is not None and d <= depth:
            break
        if n.get("role") == "AXGroup" and n.get("label") == "Academy location":
            depth = d
            continue
        if depth is not None and n.get("role") == "AXLink":
            out.append(n.get("label"))
    return out


def learner_calls(requests):
    return [r for r in requests if r.get("procedure")]


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
    work = tempfile.mkdtemp(prefix="airs-v2-c05-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.AcademyEndpoint(os.path.join(work, "academy"))
    with open(os.path.join(endpoint.dir, "catalog.json"), "w") as f:
        json.dump(academy_catalog.build(ROOT), f)
    endpoint.install()
    endpoint.start()
    obs = {"origin": endpoint.origin}

    def wait(predicate, timeout=12):
        return app.wait_for(predicate, timeout)

    def go_academy():
        app.ax("press", "AXButton", "Academy")
        return wait(lambda t: heading(t, CATALOG) is not None)

    def to_course():
        app.ax("press", "AXCheckBox", "Courses")
        wait(lambda t: app.node(t, "AXLink", f"COURSE {COURSE}") is not None, 5)
        app.ax("press", "AXLink", f"COURSE {COURSE}")
        return wait(lambda t: heading(t, COURSE, 1) is not None)

    def open_unit(prefix, title):
        app.ax("press", "AXLink", prefix)
        return wait(lambda t: heading(t, title, 1) is not None)

    def reenter_academy():
        # Leaving the destination unmounts it; it reopens at the catalog.
        app.ax("press", "AXButton", "Runtime Security")
        wait(lambda t: heading(t, "Scan a prompt or response", 1) is not None, 6)
        return go_academy()

    def gated(label, ready):
        """Follows a lesson link to gated or retired content and returns
        what Academy showed and the endpoint's reply for it."""
        reenter_academy()
        to_course()
        open_unit(f"{LESSON2} 5 min", LESSON2)
        mark = len(endpoint.requests())
        app.ax("press", "AXLink", label)
        tree, ok = wait(ready, 8)
        calls = [c for c in learner_calls(endpoint.requests()[mark:]) if c["procedure"] == "units.get"]
        return tree, ok, [[c["status"], c["authorization"]] for c in calls]

    app.launch()

    # ---- guest ----
    tree, ok = go_academy()
    obs["guest_catalog"] = ok and shows(tree, GUEST_NOTE)
    obs["tree_guest_catalog"] = save_tree("guest-catalog", tree)
    obs["guest_requests"] = endpoint.requests()
    to_course()
    tree, _ = open_unit("Knowledge check: Scanning", "Knowledge check: Scanning at runtime")
    before = len(endpoint.requests())
    app.ax("press", "AXButton", "Start knowledge check")
    tree, ok = wait(lambda t: shows(t, "Sign in to take this knowledge check."), 8)
    obs["guest_kc_sign_in"] = ok
    obs["guest_kc_requests"] = endpoint.requests()[before:]
    obs["tree_guest_kc"] = save_tree("guest-knowledge-check", tree)

    # Guest completion: asked to sign in, nothing sent.
    reenter_academy()
    to_course()
    open_unit(f"{LESSON} 5 min", LESSON)
    mark = len(endpoint.requests())
    app.ax("press", "AXButton", "Complete & continue")
    tree, ok = wait(lambda t: shows(t, "Sign in to Academy to save your progress. You can keep reading as a guest."), 8)
    obs["guest_complete"] = {"sign_in": ok, "still_on_lesson": heading(tree, LESSON, 1) is not None,
                             "sent": [c["procedure"] for c in learner_calls(endpoint.requests()[mark:])]}
    obs["tree_guest_complete"] = save_tree("guest-complete", tree)

    # Guest at gated content: Academy's sign-in gate, not a lock or not found.
    denial = {}
    for key, label in (("members", "members-only notes"), ("locked", "the advanced path's first lesson")):
        tree, ok, calls = gated(label, lambda t: shows(t, SIGN_IN_GATE))
        denial[f"guest_{key}"] = {"sign_in_gate": ok, "locked_gate": shows(tree, LOCKED_GATE), "calls": calls}
        obs[f"tree_guest_{key}"] = save_tree(f"guest-{key}", tree)

    # ---- sign in ----
    endpoint.mode(approval="granted", displayName="Ada Lovelace")
    app.open_settings()
    app.ax("press", "AXButton", "Sign in to Academy")
    _, obs["signed_in"] = wait(lambda t: app.has_text(t, "Signed in to Academy as"), 15)
    mark = len(endpoint.requests())
    tree, ok = go_academy()
    time.sleep(1)
    tree = app.tree()
    obs["account_catalog"] = ok and not shows(tree, GUEST_NOTE)
    obs["tree_account_catalog"] = save_tree("account-catalog", tree)
    obs["account_requests"] = endpoint.requests()[mark:]

    # ---- keyboard: catalog -> course -> lesson ----
    kb = {"courses_chip": False, "course_link": False, "course_heading_focused": False,
          "lesson_link": False, "lesson_heading_focused": False}
    for _ in range(60):
        app.keycode(48)
        f = app.focused(app.tree()) or {}
        if f.get("role") == "AXCheckBox" and f.get("label") == "Courses":
            kb["courses_chip"] = True
            break
    if kb["courses_chip"]:
        app.keycode(49)
        time.sleep(0.6)
        for _ in range(12):
            app.keycode(48)
            f = app.focused(app.tree()) or {}
            if f.get("role") == "AXLink" and (f.get("label") or "").startswith(f"COURSE {COURSE}"):
                kb["course_link"] = True
                break
    if kb["course_link"]:
        app.keycode(36)
        tree, ok = wait(lambda t: heading(t, COURSE, 1) is not None)
        time.sleep(0.8)
        f = app.focused(app.tree()) or {}
        kb["course_heading_focused"] = ok and f.get("role") == "AXHeading" and f.get("label") == COURSE
        for _ in range(30):
            app.keycode(48)
            f = app.focused(app.tree()) or {}
            if f.get("role") == "AXLink" and (f.get("label") or "").startswith(LESSON):
                kb["lesson_link"] = True
                break
    if kb["lesson_link"]:
        app.keycode(36)
        tree, ok = wait(lambda t: heading(t, LESSON, 1) is not None)
        time.sleep(0.8)
        f = app.focused(app.tree()) or {}
        kb["lesson_heading_focused"] = ok and f.get("role") == "AXHeading" and f.get("label") == LESSON
    else:
        # Continue the run by Accessibility press so later checks still observe.
        if heading(app.tree(), COURSE, 1) is None:
            to_course()
        open_unit(LESSON, LESSON)
    obs["keyboard"] = kb
    tree = app.tree()
    obs["tree_lesson"] = save_tree("lesson", tree)

    # ---- long lesson, trail, diagrams ----
    lesson_bytes = len(academy_catalog.lesson_html("m-rt-1", 1).encode())
    obs["lesson"] = {
        "heading": heading(tree, LESSON, 1) is not None,
        "first_paragraph": shows(tree, FIRST_PARAGRAPH),
        "section_headings": sum(1 for n in tree if n.get("role") == "AXHeading" and (n.get("label") or "").startswith("Section ")),
        "bytes": lesson_bytes,
        "trail": trail_links(tree),
    }
    image = next((n for n in tree if (n.get("label") or "") == "Lesson diagram"), None)
    obs["diagram"] = {
        "image": None if image is None else {"role": image.get("role"), "size": image.get("size")},
        "source_disclosure": any(n.get("role") == "AXDisclosureTriangle" and n.get("label") == "Diagram source" for n in tree),
        "artifactless_source": any(n.get("label") == "Diagram source" and n.get("role") == "AXGroup" for n in tree)
        and shows(tree, "graph LR; Verdict-->Log"),
    }
    # External, in-page and learner-path links in the lesson body.
    app.ax("press", "AXLink", "product documentation")
    tree, ok = wait(lambda t: shows(t, "This link goes to docs.example.com, outside the desktop app, and was not opened"), 5)
    obs["external_link"] = {"notice": ok, "still_on_lesson": heading(tree, LESSON, 1) is not None,
                            "opened": fx.opened_urls()}
    app.ax("press", "AXLink", "section 3")
    time.sleep(0.8)
    obs["anchor_link_stays"] = heading(app.tree(), LESSON, 1) is not None
    app.ax("press", "AXLink", "the learning catalog")
    tree, obs["internal_link_routes"] = wait(lambda t: heading(t, CATALOG) is not None, 6)
    obs["tree_after_internal_link"] = save_tree("after-internal-link", tree)

    # ---- trail back from a lesson ----
    to_course()
    open_unit(LESSON, LESSON)
    app.ax("press", "AXLink", COURSE)
    _, obs["trail_to_course"] = wait(lambda t: heading(t, COURSE, 1) is not None, 6)

    # ---- knowledge check ----
    mark = len(endpoint.requests())
    open_unit("Knowledge check: Scanning", "Knowledge check: Scanning at runtime")
    app.ax("press", "AXButton", "Start knowledge check")
    wait(lambda t: app.node(t, "AXRadioButton", "Retrieved web content") is not None, 8)
    app.ax("press", "AXRadioButton", "Retrieved web content")
    time.sleep(0.5)
    app.ax("press", "AXButton", "Submit")
    tree, ok = wait(lambda t: shows(t, "You scored 100%"), 8)
    obs["kc"] = {"graded": ok, "correct_line": correct_result_line(tree)}
    obs["kc_requests"] = endpoint.requests()[mark:]
    obs["tree_kc"] = save_tree("knowledge-check-graded", tree)

    # ---- lab preview ----
    app.ax("press", "AXLink", COURSE)
    wait(lambda t: heading(t, COURSE, 1) is not None, 6)
    mark = len(endpoint.requests())
    tree, _ = open_unit("Lab: Scanning", "Lab: Scanning at runtime")
    # From C06 the lab runs through Academy's desktop attempt contract: its intro offers 'Start lab' and nothing
    # starts until pressed (the C05-era 'no lab sandbox was started' preview is gone from the app; run as C05 on the
    # one presentation the same fact is asserted: the lab unit opens and no lab call is made).
    tree, ok = wait(lambda t: app.node(t, "AXButton", "Start lab") is not None, 6)
    obs["lab_preview"] = ok and heading(tree, "Block an injected prompt", 2) is not None
    obs["lab_requests"] = endpoint.requests()[mark:]
    obs["tree_lab"] = save_tree("lab-preview", tree)

    # Signed in: a progression lock shows Academy's locked gate with its
    # reason; members-only content opens; retired content is not found.
    tree, ok, calls = gated("the advanced path's first lesson", lambda t: shows(t, LOCKED_GATE))
    denial["account_locked"] = {"locked_gate": ok, "reason": shows(tree, PROGRESSION), "sign_in_gate": shows(tree, SIGN_IN_GATE), "calls": calls}
    obs["tree_account_locked"] = save_tree("account-locked", tree)
    tree, ok, calls = gated("members-only notes", lambda t: shows(t, "Notes for signed-in learners."))
    denial["account_members"] = {"opened": ok, "calls": calls}
    tree, ok, calls = gated("a retired lesson", lambda t: shows(t, "Unit not found."))
    denial["account_missing"] = {"not_found": ok, "gate": shows(tree, LOCKED_GATE) or shows(tree, SIGN_IN_GATE), "calls": calls}
    obs["tree_account_missing"] = save_tree("account-missing", tree)
    obs["denial"] = denial

    # A knowledge-check submission the server fails after receiving it.
    reenter_academy()
    to_course()
    app.ax("press", "AXButton", "2\n")
    wait(lambda t: app.node(t, "AXLink", "Knowledge check: Responding") is not None, 5)
    open_unit("Knowledge check: Responding", "Knowledge check: Responding to findings")
    app.ax("press", "AXButton", "Start knowledge check")
    wait(lambda t: app.node(t, "AXRadioButton", "Retrieved web content") is not None, 8)
    app.ax("press", "AXRadioButton", "Retrieved web content")
    time.sleep(0.5)
    mark = len(endpoint.requests())
    app.ax("press", "AXButton", "Submit")
    tree, ok = wait(lambda t: shows(t, "Academy may have recorded this submission"), 10)
    time.sleep(2)
    tree = app.tree()
    radio = app.node(tree, "AXRadioButton", "Retrieved web content")
    submits = [c for c in learner_calls(endpoint.requests()[mark:]) if c["procedure"] == "units.submitKnowledgeCheck"]
    obs["kc_unknown"] = {"may_have_recorded": ok, "answer_kept": bool(radio) and radio.get("value") == 1,
                         "server_text_hidden": not shows(tree, "Internal server error"),
                         "submits": [[c["status"], c["authorization"]] for c in submits]}
    obs["tree_kc_unknown"] = save_tree("knowledge-check-unknown-outcome", tree)

    # ---- cross-feature isolation ----
    to_lesson = lambda: (app.ax("press", "AXLink", COURSE), wait(lambda t: heading(t, COURSE, 1) is not None, 6), open_unit(LESSON, LESSON))  # noqa: E731
    to_lesson()
    mark = len(endpoint.requests())
    away = {}
    for label, title in (("Runtime Security", "Scan a prompt or response"), ("Harness", "Harness")):
        app.ax("press", "AXButton", label)
        tree, _ = wait(lambda t, title=title: heading(t, title, 1) is not None, 6)
        time.sleep(2)
        tree = app.tree()
        away[label] = {"tree": save_tree(f"away-{label.split()[0].lower()}", tree),
                       "markers": [m for m in LEARNER_MARKERS if shows(tree, m)]}
    obs["away"] = away
    obs["away_requests"] = learner_calls(endpoint.requests()[mark:])

    # ---- themes ----
    tree, _ = go_academy()
    to_course()
    open_unit(LESSON, LESSON)
    colors = {}
    for theme in ("Dark", "Light"):
        app.front()
        fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid})\n'
               f'set frontmost to true\nclick menu item "{theme}" of menu 1 of menu item "Appearance" of menu 1 of menu bar item "View" of menu bar 1\nend tell'])
        time.sleep(1.5)
        out = fx.sh(["swift", os.path.join(fx.TOOLS, "ax_colors.swift"), str(app.pid), FIRST_PARAGRAPH])
        path = os.path.join(NATIVE, f"colors-{theme.lower()}.json")
        with open(path, "w") as f:
            f.write(out.stdout)
        try:
            found = json.loads(out.stdout)
        except ValueError:
            found = {}
        colors[theme] = {"file": rel(path), "lesson_ink": found.get("foreground") if found.get("found") else None}
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid})\n'
           'set frontmost to true\nclick menu item "Match System" of menu 1 of menu item "Appearance" of menu 1 of menu bar item "View" of menu bar 1\nend tell'])
    obs["colors"] = colors

    # ---- zoom and minimum size ----
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = reach(app, 2)
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = reach(app, 1)
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    time.sleep(0.5)

    # ---- account change: sign out from a lesson, sign in again ----
    open_unit(LESSON, LESSON) if heading(app.tree(), LESSON, 1) is None else None
    app.open_settings()
    app.ax("press", "AXButton", "Sign out of Academy")
    _, out_ok = wait(lambda t: app.has_text(t, "Not signed in to Academy."), 10)
    mark = len(endpoint.requests())
    tree, ok = go_academy()
    time.sleep(1)
    tree = app.tree()
    calls = learner_calls(endpoint.requests()[mark:])
    obs["sign_out"] = {"signed_out": out_ok, "guest_catalog": ok and shows(tree, GUEST_NOTE),
                       "back_at_catalog": heading(tree, CATALOG) is not None and heading(tree, LESSON) is None,
                       "calls": [[c["procedure"], c["authorization"]] for c in calls]}
    obs["tree_after_sign_out"] = save_tree("after-sign-out", tree)
    app.open_settings()
    app.ax("press", "AXButton", "Sign in to Academy")
    _, again = wait(lambda t: app.has_text(t, "Signed in to Academy as"), 15)
    mark = len(endpoint.requests())
    tree, ok = go_academy()
    time.sleep(1)
    calls = learner_calls(endpoint.requests()[mark:])
    obs["sign_in_again"] = {"signed_in": again, "account_catalog": ok and not shows(app.tree(), GUEST_NOTE),
                            "calls": [[c["procedure"], c["authorization"]] for c in calls]}

    # ---- reset ----
    app.open_settings()
    app.ax("press", "AXButton", "Reset local data…")
    app.wait_for(lambda t: app.node(t, "AXButton", "Confirm reset") is not None)
    app.ax("press", "AXButton", "Confirm reset")
    app.wait_for(lambda t: app.has_text(t, "No tenants yet"))
    # On the one presentation Reset local data returns to the first-run wizard: the wizard showing is the signed-out state.
    _, reset_out = wait(lambda t: app.has_text(t, "Not signed in to Academy."), 10)
    reset_out = reset_out or v2s.wizard_showing(app, timeout=5)
    tree, ok = go_academy()
    obs["reset"] = {"signed_out": reset_out, "guest_catalog": ok and shows(app.tree(), GUEST_NOTE),
                    "academy_items": fx.keychain_accounts(ACADEMY_SERVICE)}
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["truth_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in ("answerKey", "AIRS-C05-TRUTH-CANARY"))]
    app.quit()
    endpoint.stop()
    obs["requests"] = endpoint.requests()
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def reach(app, zoom):
    """Tabs until 'Complete & continue' has focus; it must lie inside the
    window and end above the 60 CSS px bottom fade."""
    window = next(n for n in app.tree() if n.get("role") == "AXWindow")
    bottom = window["pos"][1] + window["size"][1]
    found = None
    for _ in range(90):
        app.keycode(48)
        f = app.focused(app.tree())
        if f and f.get("role") == "AXButton" and f.get("label") == "Complete & continue":
            found = {"label": f["label"], "pos": f["pos"], "size": f["size"]}
            break
    clear = bool(found) and found["pos"][1] >= window["pos"][1] and found["pos"][1] + found["size"][1] <= bottom - 60 * zoom
    return {"zoom": zoom, "window": {"pos": window["pos"], "size": window["size"]}, "found": found, "clear": clear}


def luminance(hexcolor):
    if not hexcolor or not hexcolor.startswith("#"):
        return None
    r, g, b = (int(hexcolor[i:i + 2], 16) / 255 for i in (1, 3, 5))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C05"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    guest_calls = learner_calls(o["guest_requests"])
    identity = [r for r in o["guest_requests"] if not r.get("procedure")]
    guest_ok = (o["guest_catalog"] and guest_calls and all(c["procedure"] in PUBLIC and not c["authorization"] for c in guest_calls)
                and not identity)
    add("C05-N01", guest_ok,
        f"guest catalog with the guest note: {o['guest_catalog']}; learner calls {[c['procedure'] for c in guest_calls]} all public and without Authorization: "
        f"{all(c['procedure'] in PUBLIC and not c['authorization'] for c in guest_calls)}; identity or other requests: {len(identity)}",
        [o["tree_guest_catalog"]])
    acct = learner_calls(o["account_requests"])
    acct_ok = (o["signed_in"] and o["account_catalog"] and acct
               and all(c["authorization"] == (c["procedure"] in ACCOUNT) for c in acct))
    add("C05-N02", acct_ok,
        f"signed in: {o['signed_in']}; catalog re-read under the account without the guest note: {o['account_catalog']}; calls (procedure, Authorization): "
        f"{[[c['procedure'], c['authorization']] for c in acct]}",
        [o["tree_account_catalog"]])
    lesson = o["lesson"]
    nav_ok = (lesson["heading"] and lesson["first_paragraph"] and lesson["section_headings"] >= 20 and lesson["bytes"] >= 50 * 1024
              and lesson["trail"] == ["Learn", COURSE] and o["trail_to_course"])
    add("C05-N03", nav_ok,
        f"lesson heading: {lesson['heading']}; first paragraph exposed: {lesson['first_paragraph']}; {lesson['section_headings']} section headings in a {lesson['bytes']} byte lesson; "
        f"trail links {lesson['trail']}; trail back to the course: {o['trail_to_course']}",
        [o["tree_lesson"]])
    diag = o["diagram"]
    video_calls = [c for c in learner_calls(o["requests"]) if c["procedure"] == "modules.diagramVideo"]
    ext = o["external_link"]
    links_ok = (diag["image"] is not None and (diag["image"]["size"] or [0, 0])[0] > 0 and diag["source_disclosure"] and diag["artifactless_source"]
                and not video_calls and ext["notice"] and ext["still_on_lesson"] and ext["opened"] == []
                and o["anchor_link_stays"] and o["internal_link_routes"])
    add("C05-N04", links_ok,
        f"pre-rendered diagram image 'Lesson diagram': {diag['image']}; source disclosure: {diag['source_disclosure']}; artifact-less diagram shows its source: {diag['artifactless_source']}; "
        f"diagram video requests: {len(video_calls)}; external link notice: {ext['notice']}, window still on the lesson: {ext['still_on_lesson']}, pages handed to the browser: {ext['opened']}; "
        f"in-page anchor stays on the lesson: {o['anchor_link_stays']}; learner-path link routes to the catalog: {o['internal_link_routes']}",
        [o["tree_lesson"], o["tree_after_internal_link"]])
    fixtures = os.path.join(ROOT, "packages/desktop-contracts/fixtures/academy")
    start_body = open(os.path.join(fixtures, "units-startKnowledgeCheck.input.json")).read().strip()
    submit_body = open(os.path.join(fixtures, "units-submitKnowledgeCheck.input.json")).read().strip()
    kc_calls = {c["procedure"]: c for c in learner_calls(o["kc_requests"])}
    start = kc_calls.get("units.startKnowledgeCheck")
    submit = kc_calls.get("units.submitKnowledgeCheck")
    kc_ok = (o["kc"]["graded"] and o["kc"]["correct_line"] and start and submit and start["input"] == start_body and submit["input"] == submit_body
             and start["authorization"] and submit["authorization"] and not o["truth_in_trees"])
    add("C05-N05", kc_ok,
        f"'You scored 100%': {o['kc']['graded']}; '1 of 1 correct': {o['kc']['correct_line']}; start body is the literal fixture: {bool(start) and start['input'] == start_body}; "
        f"submit body is the literal fixture: {bool(submit) and submit['input'] == submit_body}; both with Authorization: {bool(start and submit and start['authorization'] and submit['authorization'])}; "
        f"answer keys or canaries in AX snapshots: {o['truth_in_trees']}",
        [o["tree_kc"]])
    guest_kc_sent = [c["procedure"] for c in learner_calls(o["guest_kc_requests"])]
    add("C05-N06", o["guest_kc_sign_in"] and "units.startKnowledgeCheck" not in guest_kc_sent,
        f"guest 'Start knowledge check' shows 'Sign in to take this knowledge check.': {o['guest_kc_sign_in']}; learner calls sent for it: {guest_kc_sent}",
        [o["tree_guest_kc"]])
    lab_calls = [c["procedure"] for c in learner_calls(o["lab_requests"])]
    lab_runs = [p for p in lab_calls if (p.startswith("labs.") and p != "labs.get") or p in ("desktop.attempt.start", "desktop.attempt.submit")]
    add("C05-N07", o["lab_preview"] and not lab_runs,
        f"lab unit previewed with its brief and 'no lab sandbox was started': {o['lab_preview']}; learner calls {lab_calls}; lab start/step/submit calls: {lab_runs}",
        [o["tree_lab"]])
    so, si = o["sign_out"], o["sign_in_again"]
    acct_change_ok = (so["signed_out"] and so["guest_catalog"] and so["back_at_catalog"] and so["calls"] and all(not a for _, a in so["calls"])
                      and si["signed_in"] and si["account_catalog"] and si["calls"] and any(a for _, a in si["calls"]))
    add("C05-N08", acct_change_ok,
        f"signed out from a lesson: {so['signed_out']}; Academy back at the catalog as a guest: {so['guest_catalog'] and so['back_at_catalog']}; guest calls (procedure, Authorization) {so['calls']}; "
        f"signed in again: {si['signed_in']}; account catalog: {si['account_catalog']}; calls {si['calls']}",
        [o["tree_after_sign_out"]])
    away = o["away"]
    iso_ok = all(not v["markers"] for v in away.values()) and not o["away_requests"]
    add("C05-N09", iso_ok,
        f"learner markers in Runtime Security: {away['Runtime Security']['markers']}; in Harness: {away['Harness']['markers']}; learner calls while away from Academy: {[c['procedure'] for c in o['away_requests']]}",
        [v["tree"] for v in away.values()])
    kb = o["keyboard"]
    add("C05-N10", all(kb.values()),
        f"Tab reached the Courses filter: {kb['courses_chip']}; Tab reached the course link: {kb['course_link']}; Return opened the course with focus on its heading: {kb['course_heading_focused']}; "
        f"Tab reached the lesson link: {kb['lesson_link']}; Return opened the lesson with focus on its heading: {kb['lesson_heading_focused']}",
        [o["tree_lesson"]])
    dark, light = o["colors"]["Dark"], o["colors"]["Light"]
    ld, ll = luminance(dark["lesson_ink"]), luminance(light["lesson_ink"])
    add("C05-N11", ld is not None and ll is not None and ld > 0.6 and ll < 0.35,
        f"lesson paragraph ink in Dark {dark['lesson_ink']} (luminance {ld}), in Light {light['lesson_ink']} (luminance {ll})",
        [dark["file"], light["file"]])
    add("C05-N12", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window {o['min_size']['window']}, clear: {o['min_size']['clear']}",
        [])
    r = o["reset"]
    add("C05-N13", r["signed_out"] and r["guest_catalog"] and r["academy_items"] == [],
        f"after reset 'Not signed in to Academy.': {r['signed_out']}; Academy shows the guest catalog: {r['guest_catalog']}; learner namespace items {r['academy_items']}",
        [])

    d = o["denial"]
    denial_ok = (all(d[k]["sign_in_gate"] and not d[k]["locked_gate"] and d[k]["calls"] == [[401, False]] for k in ("guest_members", "guest_locked"))
                 and d["account_locked"]["locked_gate"] and d["account_locked"]["reason"] and not d["account_locked"]["sign_in_gate"]
                 and d["account_locked"]["calls"] == [[403, True]]
                 and d["account_members"]["opened"] and d["account_members"]["calls"] == [[200, True]]
                 and d["account_missing"]["not_found"] and not d["account_missing"]["gate"] and d["account_missing"]["calls"] == [[404, True]])
    add("C05-N14", denial_ok,
        f"guest at members-only and locked content: sign-in gate {d['guest_members']['sign_in_gate']}/{d['guest_locked']['sign_in_gate']}, endpoint replies (status, Authorization) {d['guest_members']['calls']}/{d['guest_locked']['calls']}; "
        f"signed in, locked content shows 'This content is locked' with the progression reason: {d['account_locked']['locked_gate'] and d['account_locked']['reason']} (no sign-in gate: {not d['account_locked']['sign_in_gate']}), replies {d['account_locked']['calls']}; "
        f"members-only content opens: {d['account_members']['opened']} {d['account_members']['calls']}; retired content 'Unit not found.' without a gate: {d['account_missing']['not_found'] and not d['account_missing']['gate']} {d['account_missing']['calls']}",
        [o["tree_guest_members"], o["tree_guest_locked"], o["tree_account_locked"], o["tree_account_missing"]])
    gc, ku = o["guest_complete"], o["kc_unknown"]
    failures_ok = (gc["sign_in"] and gc["still_on_lesson"] and "units.complete" not in gc["sent"]
                   and ku["may_have_recorded"] and ku["answer_kept"] and ku["server_text_hidden"] and ku["submits"] == [[500, True]])
    add("C05-N15", failures_ok,
        f"guest 'Complete & continue' asks to sign in and stays on the lesson: {gc['sign_in'] and gc['still_on_lesson']}; learner calls sent: {gc['sent']}; "
        f"a submission the server failed after receiving shows 'Academy may have recorded this submission': {ku['may_have_recorded']}, answer kept: {ku['answer_kept']}, server text hidden: {ku['server_text_hidden']}; submissions sent (status, Authorization): {ku['submits']}",
        [o["tree_guest_complete"], o["tree_kc_unknown"]])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C05",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, ax_colors.swift, System Events keystrokes) against the synthetic Academy endpoint tools/native-acceptance/academy_server.py serving tools/native-acceptance/academy_catalog.py; tools/native-acceptance/acceptance_c05.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Academy is a local synthetic endpoint answering Academy's learner procedures with synthetic catalog content shaped after Academy's routers at the pinned commit; hosted Academy is qualified in C25 (L11).",
            "Browser approval is simulated by the synthetic token endpoint; the fixture build records pages instead of launching a browser.",
            "Spoken VoiceOver output was not recorded; AX roles, names, focus and live status regions are inspected. Screen capture is not available to this session, so themes are checked through AX text colors.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for c in checks:
        print(c["result"].upper(), c["id"], c["observation"][:220])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
