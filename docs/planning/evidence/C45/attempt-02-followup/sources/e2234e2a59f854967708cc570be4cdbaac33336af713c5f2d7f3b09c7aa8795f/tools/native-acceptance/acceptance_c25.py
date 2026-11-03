#!/usr/bin/env python3
"""C25 native acceptance: the user-visible changes from the controlled live
run, through the real fixture window, its native confirmation sheet and
its own WebView captures (Fixture > Capture Window):

- a data filtering profile edited as the live service reads it back
  (mgmt_server.py's "Live shape": nulls and the service's own fields);
- the inactive-profile impact in a security profile review;
- T09 on the change review (Tab order, focus ring, names, Escape);
- an Academy sign-in whose desktop.me names the account by a UUID, as live
  Academy does (academy_server.py mode accountId).

Run from the repository root after `xtask native-prepare --chunk C25`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import glob
import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c01 as c1  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C25")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C25" else "native-C25")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C25" else "native-acceptance-C25.json")
LOG = []
FILTER = "Live shape"
FILTER_PATH = "/dlp/v2/api/data-filtering-profiles/filter-live"
# What the service adds to a filtering profile, and the fields it reads back null.
SERVICE_FIELDS = ["id", "name", "tenant_id", "type", "version", "audit_metadata", "is_parent_managed"]
NULL_FIELDS = ["description", "euc_template_id", "is_end_user_coaching_enabled", "rule2"]
INACTIVE = ("Saves this profile inactive. The service does not list inactive profiles, so it will not appear "
            "in the profile list or read back after saving; note its name and ID first.")
ORDINARY = "Applies only to this tenant"
ACCOUNT = "0b7c9d2e-4f1a-4c3b-9e8d-1a2b3c4d5e6f"
LEARNER = "Grace Hopper"


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def capture(app, name):
    """The window's own WebView snapshot (fixture build), kept as evidence."""
    before = set(glob.glob(os.path.join(c1.CAPTURES, "capture-*.png")))
    app.menu("Fixture", "Capture Window")
    deadline = time.time() + 8
    while time.time() < deadline:
        new = sorted(set(glob.glob(os.path.join(c1.CAPTURES, "capture-*.png"))) - before)
        if new:
            time.sleep(0.3)
            path = os.path.join(NATIVE, f"capture-{name}.png")
            shutil.copy(new[-1], path)
            return path
        time.sleep(0.2)
    return None


def ring(app, tree, label, image):
    """Warm accent pixels around the button labelled `label` in `image`.
    The snapshot shows the visible viewport (the web area's scroll area);
    a scrolled page's web area spans the whole document."""
    at = next(i for i, n in enumerate(tree) if n.get("role") == "AXWebArea")
    view = next((n for n in reversed(tree[:at]) if n.get("role") == "AXScrollArea"), tree[at])
    node = fx.App.node(tree, "AXButton", label)
    if image is None or node is None:
        return None
    img = c1.png_pixels(image)
    scale = img[0] / view["size"][0]
    box = [node["pos"][0] - view["pos"][0], node["pos"][1] - view["pos"][1], node["size"][0], node["size"][1]]
    return c1.ring_pixels(img, box, scale)


def confirm_label(tree):
    field = next((n for n in tree if n.get("role") == "AXTextField"
                  and (n.get("label") or "").startswith("Type ") and (n.get("label") or "").endswith(" to confirm")), None)
    return field and field["label"]


def walk(app, steps, back=False):
    order = []
    for _ in range(steps):
        app.keycode(48, "shift down" if back else None)
        f = app.focused(app.tree())
        order.append(None if f is None else {"role": f["role"], "label": f.get("label")})
    return order


def in_order(order, wanted):
    labels = [o and o.get("label") for o in order]
    at = 0
    for label in labels:
        if at < len(wanted) and label == wanted[at]:
            at += 1
    return at == len(wanted)


def main():
    try:
        return run()
    finally:
        fx.stop_app()
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c25-acceptance-")
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    obs = {}

    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c8.open_admin(app)
    c8.text(app, "Items 1–25 of 25+")

    # ---- N01: a filtering profile edited as read ----
    c9.open_change(app, "Data filtering profiles", f"Details for {FILTER}", "Edit…")
    draft = str((app.field("AXTextArea", "data filtering profile draft (JSON)") or {}).get("value") or "")
    try:
        shown = json.loads(draft)
    except ValueError:
        shown = {}
    obs["draft_as_read"] = {"service_fields": [k for k in SERVICE_FIELDS if k in shown],
                            "null_fields": [k for k in NULL_FIELDS if k in shown and shown[k] is None]}
    tree, reviewed = c9.review(app, ORDINARY)
    obs["filter_reviewed"] = reviewed and not app.has_text(tree, "does not match")
    obs["tree_filter_review"] = save_tree("filter-review", tree)
    label = confirm_label(tree)
    obs["filter_confirmation"] = label

    # ---- N04: T09 on the review ----
    app.ax("focus", "AXTextArea", "data filtering profile draft (JSON)")
    time.sleep(0.4)
    forward = walk(app, 6)
    focused = app.focused(app.tree()) or {}
    # Back to Apply… by keyboard, then capture its ring and one without it.
    for _ in range(12):
        if (app.focused(app.tree()) or {}).get("label") == "Apply…":
            break
        app.keycode(48, "shift down")
    tree = app.tree()
    on_apply = (app.focused(tree) or {}).get("label") == "Apply…"
    shot_focus = capture(app, "apply-focused")
    tree_focus = save_tree("apply-focused", tree)
    ring_focus = ring(app, tree, "Apply…", shot_focus)
    backward = walk(app, 4, back=True)
    tree = app.tree()
    shot_other = capture(app, "apply-unfocused")
    tree_other = save_tree("apply-unfocused", tree)
    ring_other = ring(app, tree, "Apply…", shot_other)
    names = [n.get("label") for n in tree if n.get("role") == "AXButton"
             and n.get("label") in ("Review again", "Close", "Apply…")]
    obs["t09"] = {"forward": forward, "backward": backward, "last_forward": focused.get("label"),
                  "on_apply": on_apply, "ring_focused": ring_focus, "ring_unfocused": ring_other,
                  "captures": [rel(p) for p in (shot_focus, shot_other) if p] + [tree_focus, tree_other],
                  "button_names": names, "impact_named": app.has_text(tree, ORDINARY)}

    # Apply it unchanged.
    if label:
        app.type_into("AXTextField", label, label[len("Type "):-len(" to confirm")])
    before = len(c9.writes(mgmt, "PUT", FILTER_PATH))
    app.ax("press", "AXButton", "Apply…")
    c9.wait_sheet(app)
    app.keycode(36)
    tree, applied = c8.text(app, "Applied. The item was read back.", 15)
    puts = c9.writes(mgmt, "PUT", FILTER_PATH)[before:]
    obs["filter_applied"] = applied
    obs["filter_puts"] = [{k: p.get(k) for k in ("path", "status", "dlpBody")} for p in puts]
    obs["tree_filter_applied"] = save_tree("filter-applied", tree)

    # ---- N02: the inactive-profile impact; N04: Escape on the sheet ----
    c9.close_workbench(app)
    app.ax("press", "AXButton", "Security profiles")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "New security profile") is not None)
    app.ax("press", "AXButton", "New security profile")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review change") is not None)
    app.type_into("AXTextArea", "security profile draft (JSON)",
                  '{"profile_name":"C25 active check","active":true,"policy":{"ai-security-profiles":[]}}')
    tree, active_ok = c9.review(app, ORDINARY)
    obs["active_ordinary"] = active_ok and not app.has_text(tree, "Saves this profile inactive")
    app.type_into("AXTextArea", "security profile draft (JSON)",
                  '{"profile_name":"C25 inactive check","active":false,"policy":{"ai-security-profiles":[]}}')
    tree, inactive_ok = c9.review(app, INACTIVE)
    impact = [n for n in tree if INACTIVE in ((n.get("label") or "") + str(n.get("value") or ""))]
    obs["inactive_impact"] = inactive_ok
    obs["inactive_impact_node"] = [{"role": n.get("role"), "label": n.get("label")} for n in impact][:2]
    obs["tree_inactive"] = save_tree("profile-inactive", tree)
    app.type_into("AXTextField", "Type CREATE to confirm", "CREATE")
    writes_before = len(c9.writes(mgmt))
    app.ax("press", "AXButton", "Apply…")
    _, sheet_shown = c9.wait_sheet(app)
    app.keycode(53)
    tree, sheet_gone = c9.wait_no_sheet(app)
    time.sleep(1.5)
    obs["escape"] = {"sheet_shown": sheet_shown, "sheet_gone": sheet_gone,
                     "writes_after": len(c9.writes(mgmt)) - writes_before,
                     "profile_posts": [r["path"] for r in c9.writes(mgmt, "POST", "/aisec/v1/mgmt/profile")]}
    obs["tree_escaped"] = save_tree("profile-escaped", tree)
    c9.close_workbench(app)
    app.quit()
    endpoint.stop()
    mgmt.stop()
    fx.clean_fixture_root()

    # ---- N03: an Academy sign-in named by a UUID ----
    academy = fx.AcademyEndpoint(os.path.join(work, "academy"))
    academy.mode(approval="pending", accountId=ACCOUNT, displayName=LEARNER)
    academy.install()
    academy.start()
    app.launch()
    app.open_settings()
    app.ax("press", "AXButton", "Sign in to Academy")
    _, awaiting = c8.text(app, "WDJB-MJHT")
    app.ax("press", "AXButton", "Open sign-in page")
    time.sleep(1)
    academy.mode(approval="granted", accountId=ACCOUNT, displayName=LEARNER)
    tree, signed_in = app.wait_for(lambda t: app.has_text(t, "Signed in to Academy as") and app.has_text(t, LEARNER), 20)
    me = [r for r in academy.requests() if r.get("path") == "/api/trpc/desktop.me"]
    obs["academy"] = {"awaiting": awaiting, "signed_in": signed_in, "desktop_me_calls": len(me),
                      "failed_shown": app.has_text(tree, "could not") or app.has_text(tree, "failed")}
    obs["tree_academy_signed_in"] = save_tree("academy-signed-in", tree)
    app.quit()
    app.launch()
    app.open_settings()
    tree, restored = app.wait_for(lambda t: app.has_text(t, "Signed in to Academy as") and app.has_text(t, LEARNER), 15)
    obs["academy"]["restored"] = restored
    obs["tree_academy_restored"] = save_tree("academy-restored", tree)
    if fx.App.node(tree, "AXButton", "Sign out of Academy"):
        app.ax("press", "AXButton", "Sign out of Academy")
        app.wait_for(lambda t: app.has_text(t, "Not signed in to Academy."), 12)
    app.quit()
    academy.stop()

    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if c8.SECRET in open(p).read() or c8.SCAN_KEY in open(p).read()]
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C25"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    puts = o["filter_puts"]
    body = puts[0]["dlpBody"] if len(puts) == 1 else {}
    stripped = bool(body) and not any(k in body for k in SERVICE_FIELDS + NULL_FIELDS)
    kept = bool(body) and body.get("direction") == "BOTH" and body.get("rule1", {}).get("action") == "alert"
    as_read = o["draft_as_read"]
    add("C25-N01", len(as_read["service_fields"]) == len(SERVICE_FIELDS) and len(as_read["null_fields"]) == len(NULL_FIELDS)
        and o["filter_reviewed"] and o["filter_applied"] and len(puts) == 1 and stripped and kept,
        f"draft as read holds service fields {as_read['service_fields']} and nulls {as_read['null_fields']}; reviewed: "
        f"{o['filter_reviewed']}; applied and read back: {o['filter_applied']}; PUTs {puts}",
        [o["tree_filter_review"], o["tree_filter_applied"]])
    add("C25-N02", o["active_ordinary"] and o["inactive_impact"],
        f"active draft shows the ordinary impact only: {o['active_ordinary']}; inactive draft shows the inactive impact: "
        f"{o['inactive_impact']} ({o['inactive_impact_node']})", [o["tree_inactive"]])
    a = o["academy"]
    add("C25-N03", a["awaiting"] and a["signed_in"] and a["restored"] and a["desktop_me_calls"] >= 1,
        f"code shown: {a['awaiting']}; signed in as {LEARNER} with desktop.me accountId {ACCOUNT}: {a['signed_in']} "
        f"(desktop.me calls {a['desktop_me_calls']}); kept across a relaunch: {a['restored']}",
        [o["tree_academy_signed_in"], o["tree_academy_restored"]])
    t = o["t09"]
    wanted = ["Review again", "Close", o["filter_confirmation"], "Apply…"]
    rings = t["ring_focused"] is not None and t["ring_unfocused"] is not None \
        and t["ring_focused"] > 150 and t["ring_unfocused"] < t["ring_focused"] / 4
    esc = o["escape"]
    add("C25-N04", in_order(t["forward"], wanted) and in_order(list(reversed(t["backward"])) + [{"label": "Apply…"}], wanted[1:])
        and t["on_apply"] and rings and len(t["button_names"]) == 3 and t["impact_named"]
        and esc["sheet_shown"] and esc["sheet_gone"] and esc["writes_after"] == 0 and not esc["profile_posts"],
        f"Tab from the draft: {t['forward']}; Shift-Tab from Apply…: {t['backward']}; focus ring pixels around Apply… "
        f"{t['ring_focused']} focused vs {t['ring_unfocused']} not; named buttons {t['button_names']}; impact named: "
        f"{t['impact_named']}; Escape on the sheet: {esc}", t["captures"])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C25",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window and its AppKit sheet through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes, the fixture build's own WebView capture) against tools/native-acceptance/mgmt_server.py and academy_server.py; tools/native-acceptance/acceptance_c25.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The endpoints are local synthetic fixtures shaped after the live behaviours found in C25 (live receipts under evidence/C25/live). The fixture filtering profile uses the request contract's direction BOTH and log_severity LOW: the live service stores c2s and low, which the SDK contract refuses (an SDK follow-up recorded in blocked-gaps.md).",
            "Focus visibility is measured on the fixture build's own WebView snapshot; spoken VoiceOver output was not recorded; AX roles, names and focus are inspected.",
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
