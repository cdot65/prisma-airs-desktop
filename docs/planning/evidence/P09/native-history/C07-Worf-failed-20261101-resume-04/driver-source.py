#!/usr/bin/env python3
"""C07 native acceptance: Academy offline work in the prepared fixture app,
driven through macOS Accessibility against the synthetic Academy endpoint
(academy_server.py serving academy_catalog.py, academy_program.py and
academy_offline.py), evaluated against the C07 inventory
(docs/architecture/native-checks.json).

The fixture build trusts the disposable test CA, talks only to the synthetic
endpoint named in its data root and keeps credentials and offline keys in
its dedicated fixture keychain (inspected by attributes only). "Offline" is
the endpoint process stopped: every connection is refused. The fixture root
is removed at the end.

Launch 1 (online): sign in; the Downloads page and its disclosure; a course
downloaded by keyboard; the sealed files and the offline key; a course past
Academy's bundle cap refused beyond the record limit; the lab attempt read
once. Launch 2 (offline): the notice, the download, two lessons read and an
edit to the lab kept on this Mac. Launch 3 (offline): the edit back. Then
Academy returns: nothing sent by itself, one explicit save, the local copy
cleared, the download re-checked (newer revision) and updated. Sign-out,
another account, the first account again. A short lease ending. Launch 4:
a course Academy withdrew. Zoom and minimum size. Reset.
"""

import json
import os
import shutil
import stat
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import academy_catalog as cat  # noqa: E402
import academy_offline as off  # noqa: E402
import academy_program as prog  # noqa: E402
import fixture_app as fx  # noqa: E402

ROOT = os.getcwd()
# A phase containing C07 runs this driver against its own prepared bundle
# (AIRS_NATIVE_SUBJECT=Pxx); tools/native-acceptance/merge_phase.py combines
# the member records.
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C07")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C07" else "native-C07")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C07" else "native-acceptance-C07.json")
ACADEMY_SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.academy"
OFFLINE_ROOT = os.path.join(fx.DATA, "academy-offline")
COURSE = "Runtime Security Foundations"
LESSON_1 = "Scanning at runtime: part 1"
LESSON_2 = "Scanning at runtime: part 2"
LESSON_TEXT = "Lesson 1 of Scanning at runtime opens here"
LAB_HEADING = "Triage a red-team campaign"
UNREACHABLE = "Academy can't be reached"
LOCAL_STATUS = "Saved on this device only"
SAVED_STATUS = "All changes saved to your Academy account."
ONE_DRAFT = "One lab draft is saved on this Mac only"
NOT_ENOUGH_ROOM = "There is not enough room for this course in offline storage"
ENDED = "Offline access ended. Connect to renew it."
WITHDRAWN = "No longer part of your account"
NEWER = "A newer version is on Academy"
FIRST = "user_2fixturelearner"
OTHER = "user_2otherlearner"
PROCEDURES = json.load(open(os.path.join(ROOT, "packages/desktop-contracts/fixtures/academy/learner-procedures.json")))
ADMITTED = {p["procedure"] for p in PROCEDURES["learner"]} | set(PROCEDURES["public"])
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


def nav_links(tree):
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


def body(call):
    try:
        return json.loads(call.get("input") or "{}").get("json") or {}
    except ValueError:
        return {}


def inventory():
    """Every file under the offline root: path relative to it, mode and a
    content digest, plus directory modes. Contents are never printed."""
    files, dirs = [], []
    if not os.path.isdir(OFFLINE_ROOT):
        return {"exists": False, "files": files, "dirs": dirs}
    for here, subdirs, names in os.walk(OFFLINE_ROOT):
        dirs.append({"path": os.path.relpath(here, OFFLINE_ROOT), "mode": oct(stat.S_IMODE(os.stat(here).st_mode))})
        for name in sorted(names):
            path = os.path.join(here, name)
            files.append({"path": os.path.relpath(path, OFFLINE_ROOT), "mode": oct(stat.S_IMODE(os.stat(path).st_mode)),
                          "bytes": os.path.getsize(path), "sha256": sha256(path)})
    return {"exists": True, "files": files, "dirs": dirs}


def plaintext_found(markers):
    """Which markers appear in any offline file's bytes or path."""
    found = set()
    for here, _, names in os.walk(OFFLINE_ROOT) if os.path.isdir(OFFLINE_ROOT) else []:
        for name in names:
            path = os.path.join(here, name)
            with open(path, "rb") as f:
                data = f.read()
            for m in markers:
                if m.encode() in data or m in os.path.relpath(path, OFFLINE_ROOT):
                    found.add(m)
    return sorted(found)


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
    work = tempfile.mkdtemp(prefix="airs-v2-c07-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.AcademyEndpoint(os.path.join(work, "academy"))
    catalog = cat.build(ROOT)
    catalog.update(prog.build(ROOT, endpoint.origin))
    catalog.update(off.build(catalog))
    with open(os.path.join(endpoint.dir, "catalog.json"), "w") as f:
        json.dump(catalog, f)
    endpoint.install()
    endpoint.start()
    obs = {"origin": endpoint.origin}

    def wait(predicate, timeout=12):
        return app.wait_for(predicate, timeout)

    def go_academy():
        app.ax("press", "AXButton", "Academy")
        return wait(lambda t: bool(nav_links(t)))

    def press(role, label, nth=0):
        try:
            app.ax("press", role, label, nth)
            return True
        except RuntimeError as error:
            app.note("press failed", role=role, label=label, error=str(error))
            return False

    def downloads():
        press("AXLink", "Downloads")
        return wait(lambda t: heading(t, "Downloads", 1) is not None)

    def calls_since(mark):
        return [r for r in endpoint.requests()[mark:] if r.get("procedure")]

    def focus_to(role, prefix, steps=80):
        for _ in range(steps):
            app.keycode(48)
            f = app.focused(app.tree()) or {}
            if f.get("role") == role and (f.get("label") or "").startswith(prefix):
                return True
        return False

    def outline(label):
        """Follows a link in the unit's contents (the sidebar opens first
        when it is collapsed)."""
        if app.node(app.tree(), "AXLink", label) is None:
            press("AXButton", "Expand contents sidebar")
            wait(lambda t: app.node(t, "AXLink", label) is not None, 5)
        return press("AXLink", label)

    def open_lab():
        outline("Lab: Scanning")
        return wait(lambda t: heading(t, LAB_HEADING, 2) is not None, 15)

    def relaunch():
        app.quit()
        time.sleep(1)
        app.launch()

    def sign_in(account, name):
        endpoint.mode(approval="granted", displayName=name, accountId=account)
        app.open_settings()
        press("AXButton", "Sign in to Academy")
        return wait(lambda t: app.has_text(t, f"Signed in to Academy as {name}"), 20)[1]

    app.launch()

    # ---- launch 1: sign in, download by keyboard ----
    obs["signed_in"] = sign_in(FIRST, "Ada Lovelace")
    go_academy()
    time.sleep(1)
    obs["account_nav"] = nav_links(app.tree())
    tree, page = downloads()
    tree, listed = wait(lambda t: app.node(t, "AXButton", f"Download {COURSE}") is not None)
    obs["tree_downloads_empty"] = save_tree("downloads-empty", tree)
    disclosure = "Videos, walkthroughs and live labs still need Academy"
    items_before = fx.keychain_accounts(ACADEMY_SERVICE)
    mark = len(endpoint.requests())
    kb = {"focused": focus_to("AXButton", f"Download {COURSE}")}
    if kb["focused"]:
        app.keycode(36)
    tree, kb["kept"] = wait(lambda t: shows(t, f"{COURSE} is ready offline."), 20)
    time.sleep(1)
    tree = app.tree()
    obs["tree_downloaded"] = save_tree("downloaded", tree)
    items_after = fx.keychain_accounts(ACADEMY_SERVICE)
    calls = calls_since(mark)
    obs["download"] = {
        "page": page, "listed": listed, "disclosure": shows(tree, disclosure), "keyboard": kb,
        "ready": shows(tree, "Ready offline · 28 units"),
        "calls": [[c["procedure"], c["status"], c["authorization"]] for c in calls],
        "items_before": items_before, "items_after": items_after,
    }
    obs["inventory_downloaded"] = inventory()
    obs["plaintext_downloaded"] = plaintext_found([LESSON_TEXT, cat.COURSE, FIRST, "Ada Lovelace", COURSE])
    # The redacted storage manifest: names (digests), modes, sizes and
    # content digests only; never contents, keys or account ids. As a
    # regression under a later subject it stays with C07's native records.
    manifest = os.path.join(EVIDENCE if SUBJECT == "C07" else NATIVE, "storage-manifest.json")
    with open(manifest, "w") as f:
        json.dump({"schema": "airs-v2/offline-storage-manifest/1", "subject": "C07",
                   "root": "<app data root>/academy-offline",
                   "after": f"one course downloaded ({COURSE}) for one synthetic account",
                   "keychain_items_added": [i[:3] + "<digest>" for i in sorted(set(items_after) - set(items_before))],
                   "plaintext_markers_found": obs["plaintext_downloaded"],
                   **obs["inventory_downloaded"]}, f, indent=2)
        f.write("\n")

    # Past Academy's bundle cap: assembled from unit bundles, beyond the record limit.
    mark = len(endpoint.requests())
    press("AXButton", f"Download {off.BIG_TITLE}")
    tree, refused = wait(lambda t: shows(t, NOT_ENOUGH_ROOM), 60)
    obs["too_large"] = {"message": refused,
                        "calls": [[c["procedure"], c["status"]] for c in calls_since(mark)],
                        "kept": shows(tree, f"Open {off.BIG_TITLE}") or app.node(tree, "AXLink", f"Open {off.BIG_TITLE}") is not None,
                        "inventory_unchanged": [f["path"] for f in inventory()["files"]] == [f["path"] for f in obs["inventory_downloaded"]["files"]]}
    obs["tree_too_large"] = save_tree("too-large", tree)

    # The lab attempt is read once while online (kept for offline use).
    press("AXLink", f"Open {COURSE}")
    tree, lesson_online = wait(lambda t: heading(t, LESSON_1, 1) is not None, 15)
    tree, lab_online = open_lab()
    obs["lab_online"] = {"lesson": lesson_online, "lab": lab_online}

    # ---- launch 2: Academy unreachable ----
    endpoint.stop()
    relaunch()
    mark_offline = len(endpoint.requests())
    go_academy()
    tree, notice = wait(lambda t: shows(t, UNREACHABLE), 20)
    tree, page = downloads()
    time.sleep(1)
    tree = app.tree()
    obs["tree_offline_downloads"] = save_tree("offline-downloads", tree)
    offline = {"notice": notice, "page": page, "ready": shows(tree, "Ready offline"),
               "no_download": app.node(tree, "AXButton", f"Download {off.BIG_TITLE}") is None
               and shows(tree, "Connect to Academy to download more courses.")}
    press("AXLink", f"Open {COURSE}")
    tree, offline["lesson_1"] = wait(lambda t: heading(t, LESSON_1, 1) is not None and shows(t, LESSON_TEXT), 15)
    obs["tree_offline_lesson"] = save_tree("offline-lesson", tree)
    outline(LESSON_2)
    tree, offline["lesson_2"] = wait(lambda t: heading(t, LESSON_2, 1) is not None, 15)
    tree, offline["lab"] = open_lab()
    press("AXCheckBox", "True positive", 0)
    tree, offline["kept_locally"] = wait(lambda t: shows(t, LOCAL_STATUS), 15)
    obs["tree_offline_lab"] = save_tree("offline-lab", tree)
    obs["offline"] = offline

    # ---- launch 3: still unreachable; the edit comes back ----
    relaunch()
    go_academy()
    tree, _ = downloads()
    tree, again = wait(lambda t: shows(t, ONE_DRAFT), 15)
    press("AXLink", f"Open {COURSE}")
    wait(lambda t: heading(t, LESSON_1, 1) is not None, 15)
    tree, lab = open_lab()
    tree, restored = wait(lambda t: shows(t, LOCAL_STATUS), 15)
    checked = (app.node(tree, "AXCheckBox", "True positive") or {}).get("value") == 1
    obs["tree_restored_lab"] = save_tree("restored-lab", tree)
    obs["restored"] = {"draft_listed": again, "lab": lab, "status": restored, "edit_back": checked,
                       "requests_while_offline": len(endpoint.requests()) - mark_offline}

    # ---- Academy returns: nothing sent by itself; one explicit save ----
    endpoint.mode(approval="granted", displayName="Ada Lovelace", accountId=FIRST,
                  replies={"desktop.manifest": {"by": "courseId", "values": {cat.COURSE: off.bundle(catalog, "rev-2", manifest=True)}},
                           "desktop.bundle": {"by": "courseId", "values": {cat.COURSE: off.bundle(catalog, "rev-2")}}})
    endpoint.start()
    mark = len(endpoint.requests())
    time.sleep(8)
    sent_by_itself = [c["procedure"] for c in calls_since(mark) if c["procedure"] in ("desktop.draft.save", "desktop.attempt.submit")]
    press("AXButton", "Save now")
    tree, saved = wait(lambda t: shows(t, SAVED_STATUS), 15)
    time.sleep(1)
    saves = [body(c) for c in calls_since(mark) if c["procedure"] == "desktop.draft.save"]
    go_academy()
    tree, _ = downloads()
    tree, newer = wait(lambda t: shows(t, NEWER), 20)
    obs["tree_reconnected"] = save_tree("reconnected", tree)
    draft_cleared = not shows(tree, ONE_DRAFT)
    revalidation = [c["procedure"] for c in calls_since(mark) if c["procedure"] in ("desktop.access", "desktop.manifest")]
    mark_update = len(endpoint.requests())
    press("AXButton", f"Update {COURSE}")
    tree, updated = wait(lambda t: shows(t, f"{COURSE} is ready offline.") and not shows(t, NEWER), 20)
    obs["reconnect"] = {"sent_by_itself": sent_by_itself, "saved": saved, "saves": saves, "draft_cleared": draft_cleared,
                        "newer": newer, "revalidation": revalidation, "updated": updated,
                        "update_calls": [c["procedure"] for c in calls_since(mark_update)]}

    # ---- sign-out lock and another account ----
    first_files = inventory()
    app.open_settings()
    press("AXButton", "Sign out of Academy")
    wait(lambda t: app.has_text(t, "Not signed in to Academy."), 15)
    go_academy()
    time.sleep(1)
    lock = {"guest_nav": nav_links(app.tree())}
    lock["other_signed_in"] = sign_in(OTHER, "Grace Hopper")
    go_academy()
    tree, _ = downloads()
    tree, lock["other_empty"] = wait(lambda t: shows(t, "No courses are downloaded yet."), 15)
    lock["other_sees_course"] = app.node(tree, "AXLink", f"Open {COURSE}") is not None
    obs["tree_other_account"] = save_tree("other-account", tree)
    after_other = inventory()
    first_dirs = {f["path"].split("/")[0] for f in first_files["files"]}
    lock["first_files_unchanged"] = [f for f in after_other["files"] if f["path"].split("/")[0] in first_dirs] == first_files["files"]
    app.open_settings()
    press("AXButton", "Sign out of Academy")
    wait(lambda t: app.has_text(t, "Not signed in to Academy."), 15)
    lock["first_again"] = sign_in(FIRST, "Ada Lovelace")
    go_academy()
    tree, _ = downloads()
    tree, lock["first_listed"] = wait(lambda t: app.node(t, "AXLink", f"Open {COURSE}") is not None, 15)
    obs["lock"] = lock

    # ---- a short lease ends; its lessons are not read offline ----
    endpoint.mode(approval="granted", displayName="Ada Lovelace", accountId=FIRST,
                  replies={"desktop.access": off.access(seconds=12),
                           "desktop.bundle": {"by": "courseId", "values": {cat.COURSE: off.bundle(catalog, "rev-2", seconds=12)}}})
    press("AXButton", f"Remove {COURSE}")
    wait(lambda t: shows(t, f"{COURSE} was removed from this Mac."), 15)
    tree, _ = wait(lambda t: app.node(t, "AXButton", f"Download {COURSE}") is not None, 15)
    press("AXButton", f"Download {COURSE}")
    wait(lambda t: shows(t, f"{COURSE} is ready offline."), 20)
    time.sleep(14)
    endpoint.stop()
    press("AXLink", f"Open {COURSE}")
    tree, not_read = wait(lambda t: shows(t, "Unable to load this lesson."), 20)
    lease = {"not_read": not_read, "lesson_text": shows(tree, LESSON_TEXT)}
    obs["tree_lease_ended_lesson"] = save_tree("lease-ended-lesson", tree)
    go_academy()
    tree, _ = downloads()
    tree, lease["ended"] = wait(lambda t: shows(t, ENDED), 15)
    obs["tree_lease_ended"] = save_tree("lease-ended", tree)
    obs["lease"] = lease

    # ---- launch 4: Academy withdrew the course ----
    endpoint.mode(approval="granted", displayName="Ada Lovelace", accountId=FIRST,
                  replies={"desktop.access": off.access(allowed=False)})
    endpoint.start()
    relaunch()
    go_academy()
    tree, _ = downloads()
    tree, withdrawn = wait(lambda t: shows(t, WITHDRAWN), 20)
    obs["withdrawn"] = {"shown": withdrawn, "open_offered": app.node(tree, "AXLink", f"Open {COURSE}") is not None}
    obs["tree_withdrawn"] = save_tree("withdrawn", tree)

    # ---- zoom and minimum size on Downloads ----
    endpoint.mode(approval="granted", displayName="Ada Lovelace", accountId=FIRST)
    press("AXButton", f"Remove {COURSE}")
    wait(lambda t: app.node(t, "AXButton", f"Download {COURSE}") is not None, 20)
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom"] = reach(app, 2, f"Download {COURSE}")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_size"] = reach(app, 1, f"Download {COURSE}")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])
    time.sleep(0.5)

    # ---- reset ----
    press("AXButton", f"Download {COURSE}")
    wait(lambda t: shows(t, f"{COURSE} is ready offline."), 20)
    reset = {"before": {"offline_root": os.path.isdir(OFFLINE_ROOT), "items": fx.keychain_accounts(ACADEMY_SERVICE)}}
    app.open_settings()
    press("AXButton", "Reset local data…")
    wait(lambda t: app.node(t, "AXButton", "Confirm reset") is not None)
    press("AXButton", "Confirm reset")
    tree, reset["done"] = wait(lambda t: app.node(t, "AXButton", "Confirm reset") is None and not os.path.isdir(OFFLINE_ROOT), 20)
    time.sleep(2)
    reset["after"] = {"offline_root": os.path.isdir(OFFLINE_ROOT), "items": fx.keychain_accounts(ACADEMY_SERVICE)}
    obs["reset"] = reset

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
    inventory_checks = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C07"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory_checks[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    d = o["download"]
    new_items = sorted(set(d["items_after"]) - set(d["items_before"]))
    add("C07-N01", o["signed_in"] and "Downloads" in o["account_nav"] and d["page"] and d["disclosure"] and d["keyboard"]["focused"]
        and d["keyboard"]["kept"] and d["ready"] and d["calls"] == [["desktop.access", 200, True], ["desktop.bundle", 200, True]]
        and len(new_items) == 1 and new_items[0].startswith("cr-"),
        f"signed in: {o['signed_in']}; Academy navigation {o['account_nav']}; Downloads page: {d['page']} with the disclosure that videos, walkthroughs and live labs need Academy: {d['disclosure']}; "
        f"Tab reached 'Download {COURSE}': {d['keyboard']['focused']} and Return kept it ('{COURSE} is ready offline.'): {d['keyboard']['kept']}, listed 'Ready offline · 28 units': {d['ready']}; "
        f"requests (procedure, status, Authorization) {d['calls']}; learner keychain items added {new_items}",
        [o["tree_downloads_empty"], o["tree_downloaded"]])

    inv = o["inventory_downloaded"]
    modes_ok = inv["exists"] and all(x["mode"] == "0o700" for x in inv["dirs"]) and all(x["mode"] == "0o600" for x in inv["files"])
    sealed = [x for x in inv["files"] if x["path"].endswith(".bin")]
    add("C07-N02", modes_ok and sealed and not o["plaintext_downloaded"],
        f"{len(inv['files'])} files under the offline root ({len(sealed)} sealed records), directory modes {sorted({x['mode'] for x in inv['dirs']})}, file modes {sorted({x['mode'] for x in inv['files']})}; "
        f"lesson text, course id, course title, account id or display name in any file's bytes or path: {o['plaintext_downloaded']}",
        [])

    off_ = o["offline"]
    add("C07-N03", off_["notice"] and off_["page"] and off_["ready"] and off_["no_download"] and off_["lesson_1"] and off_["lesson_2"],
        f"endpoint stopped, relaunched: '{UNREACHABLE}' shown: {off_['notice']}; Downloads lists the course ready offline: {off_['ready']} and offers no download ('Connect to Academy to download more courses.'): {off_['no_download']}; "
        f"'Open' showed '{LESSON_1}' with its text: {off_['lesson_1']}; the contents opened '{LESSON_2}': {off_['lesson_2']}",
        [o["tree_offline_downloads"], o["tree_offline_lesson"]])

    r = o["restored"]
    add("C07-N04", o["lab_online"]["lab"] and off_["lab"] and off_["kept_locally"] and r["draft_listed"] and r["lab"] and r["status"] and r["edit_back"]
        and r["requests_while_offline"] == 0,
        f"lab attempt read online: {o['lab_online']['lab']}; offline the lab opened from that copy: {off_['lab']} and an edit said '{LOCAL_STATUS}': {off_['kept_locally']}; "
        f"relaunched offline, Downloads says '{ONE_DRAFT}': {r['draft_listed']}, the lab reopened: {r['lab']} with the status: {r['status']} and the edit: {r['edit_back']}; requests reaching Academy while offline: {r['requests_while_offline']}",
        [o["tree_offline_lab"], o["tree_restored_lab"]])

    c = o["reconnect"]
    save_ok = len(c["saves"]) == 1 and c["saves"][0].get("baseVersion") == 0 and bool(c["saves"][0].get("clientMutationId")) \
        and c["saves"][0].get("draft") not in (None, prog.EMPTY_DRAFT)
    add("C07-N05", not c["sent_by_itself"] and c["saved"] and save_ok and c["draft_cleared"] and c["newer"]
        and "desktop.access" in c["revalidation"] and "desktop.manifest" in c["revalidation"] and c["updated"]
        and c["update_calls"][:2] == ["desktop.access", "desktop.bundle"],
        f"with Academy back, sent by itself in 8 s: {c['sent_by_itself']}; 'Save now' sent {len(c['saves'])} draft save {c['saves'][:1]} and showed '{SAVED_STATUS}': {c['saved']}; "
        f"the local copy cleared from Downloads: {c['draft_cleared']}; reconnecting re-checked the download ({c['revalidation']}) and showed '{NEWER}': {c['newer']}; "
        f"'Update' sent {c['update_calls']} and kept it again: {c['updated']}",
        [o["tree_reconnected"]])

    lk = o["lock"]
    # On the one presentation the Academy page's guest navigation also lists the Learning paths' "Open … Path" links
    # (C42); what the check asserts is that a guest has Learn and no Downloads.
    add("C07-N06", "Learn" in lk["guest_nav"] and "Downloads" not in lk["guest_nav"] and lk["other_signed_in"] and lk["other_empty"] and not lk["other_sees_course"]
        and lk["first_files_unchanged"] and lk["first_again"] and lk["first_listed"],
        f"signed out, Academy navigation {lk['guest_nav']} (no Downloads); another account signed in: {lk['other_signed_in']} and sees 'No courses are downloaded yet.': {lk['other_empty']}, "
        f"the first account's course: {lk['other_sees_course']}; the first account's sealed files unchanged: {lk['first_files_unchanged']}; the first account signed in again: {lk['first_again']} and its download is listed: {lk['first_listed']}",
        [o["tree_other_account"]])

    t = o["too_large"]
    ls, w = o["lease"], o["withdrawn"]
    add("C07-N07", t["message"] and [x[0] for x in t["calls"]][:3] == ["desktop.access", "desktop.bundle", "desktop.manifest"]
        and ["desktop.bundle", 413] in t["calls"] and len([x for x in t["calls"] if x[0] == "desktop.unitBundle"]) == 3
        and not t["kept"] and t["inventory_unchanged"] and ls["not_read"] and not ls["lesson_text"] and ls["ended"]
        and w["shown"] and not w["open_offered"],
        f"a course past Academy's bundle cap (four {off.PART_BYTES // 1048576} MiB unit bundles, each within Academy's 7 MiB cap): requests {t['calls']} (fetching stopped once the course could not fit one record); '{NOT_ENOUGH_ROOM}': {t['message']}; kept: {t['kept']}; offline files unchanged: {t['inventory_unchanged']}; "
        f"a 12 s lease once ended: its lesson offline said 'Unable to load this lesson.': {ls['not_read']} (lesson text shown: {ls['lesson_text']}) and Downloads says '{ENDED}': {ls['ended']}; "
        f"a course Academy withdrew shows '{WITHDRAWN}' after relaunch: {w['shown']} and offers Open: {w['open_offered']}",
        [o["tree_too_large"], o["tree_lease_ended_lesson"], o["tree_lease_ended"], o["tree_withdrawn"]])

    rs = o["reset"]
    add("C07-N08", rs["before"]["offline_root"] and any(i.startswith("cr-") for i in rs["before"]["items"]) and rs["done"]
        and not rs["after"]["offline_root"] and rs["after"]["items"] == [],
        f"before reset: offline root present {rs['before']['offline_root']}, learner keychain items {rs['before']['items']}; 'Reset local data' completed: {rs['done']}; "
        f"after: offline root present {rs['after']['offline_root']}, learner keychain items {rs['after']['items']}",
        [])

    add("C07-N09", o["zoom"]["clear"] and o["min_size"]["clear"],
        f"200% zoom: {o['zoom']['found']} in window {o['zoom']['window']}, clear: {o['zoom']['clear']}; minimum size: {o['min_size']['found']} in window {o['min_size']['window']}, clear: {o['min_size']['clear']}",
        [])

    calls = [x for x in o["requests"] if x.get("procedure")]
    outside = sorted({x["procedure"] for x in calls if x["procedure"] not in ADMITTED})
    unauthenticated = sorted({x["procedure"] for x in calls if x["procedure"].startswith("desktop.") and not x["authorization"]})
    add("C07-N10", not outside and not unauthenticated,
        f"{len(calls)} learner requests; outside the allowlist and public queries: {outside}; desktop procedures sent without the account credential: {unauthenticated}",
        [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C07",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against the synthetic Academy endpoint tools/native-acceptance/academy_server.py serving tools/native-acceptance/academy_catalog.py, academy_program.py and academy_offline.py; tools/native-acceptance/acceptance_c07.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Academy is a local synthetic endpoint answering Academy's offline contract (desktop.access, desktop.bundle, desktop.manifest, desktop.unitBundle) with synthetic coursework shaped after Academy's routers at the pinned commit; hosted Academy is qualified in C25 (L13).",
            "Offline is the synthetic endpoint stopped (connections refused); a network that drops mid-transfer or hangs is covered by the Rust offline suite, not natively.",
            "The 512 MiB account budget and 1024-record cap are covered by the store unit test an_account_is_bounded_in_bytes_and_records; natively a course past the 16 MiB record limit is refused.",
            "Keychain denial and an unreadable record are covered by the Rust offline suite; the fixture keychain is not made to refuse natively.",
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
