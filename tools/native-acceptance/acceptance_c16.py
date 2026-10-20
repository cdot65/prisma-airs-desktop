#!/usr/bin/env python3
"""C16 native acceptance: AI Gateway workspaces and configuration through
the real fixture window against the synthetic Gateway planes
(mgmt_server.py: /ai_gw/v2 data, /ai_gw/admin/v2 admin, /iam/v1 IAM).

Run from the repository root after `xtask native-prepare --chunk C16`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import json
import os
import shutil
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C16")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C16" else "native-C16")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C16" else "native-acceptance-C16.json")
GW_A = "16f7e90d-382a-4e78-b577-1b01eb5f8297"
GW_B = "16f7e90d-382a-4e78-b577-1b01eb5f8298"
# Markers the synthetic planes put in key values, provider keys, model
# configuration, integration secrets, credential-bearing routing and
# guardrail settings and error bodies; none may ever reach the window.
PRIVATE = ("GW-PRIVATE", "SERVER-TEXT-NEVER-SHOWN")
HEADING = "Gateway workspaces and configuration"
LOG = []


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    path = os.path.join(NATIVE, f"tree-{name}.json")
    with open(path, "w") as f:
        json.dump(tree, f, indent=0)
    return rel(path)


def leaks(app, tree):
    return sorted({p for p in PRIVATE if app.has_text(tree, p)})


def open_gateway(app):
    app.ax("press", "AXButton", "AI Gateway")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", HEADING) is not None)


def radio(tree, prefix):
    return fx.App.node(tree, "AXRadioButton", prefix)


def checked(tree, prefix):
    node = radio(tree, prefix)
    return bool(node) and str(node.get("value")) in ("1", "True", "true")


def tab(app, label):
    """Presses a tab (WebKit exposes role=tab as a radio button)."""
    tree = app.tree()
    for role in ("AXRadioButton", "AXTab", "AXButton"):
        matches = [n for n in tree if n.get("role") == role and (n.get("label") or "").startswith(label)]
        for nth, n in enumerate(matches):
            if n.get("label") == label:
                app.ax("press", role, label, nth)
                return role
    raise RuntimeError(f"no tab labelled {label!r}")


def gateway_requests(mgmt):
    return [r for r in mgmt.requests() if r["path"].startswith(("/ai_gw/", "/iam/v1/"))]


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c16-acceptance-")
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
    mgmt.mode()
    obs = {}

    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)

    # ---- N01: no management client, nothing read ----
    open_gateway(app)
    tree, asked = c8.text(app, "Configure one in Settings to see the workspaces", 8)
    obs["no_client"] = {"asked": asked, "requests": len(gateway_requests(mgmt))}
    app.open_settings()
    c8.save_client(app)

    # ---- N02: workspaces listed; one chosen natively ----
    open_gateway(app)
    tree, listed = c8.text(app, "2 workspaces", 15)
    nothing_chosen = not checked(tree, "Team A") and not checked(tree, "Team B") \
        and app.has_text(tree, "Choose a workspace to read its configuration.")
    app.ax("press", "AXRadioButton", "Team A")
    tree, providers = c8.text(app, "2 providers · complete", 15)
    reads = gateway_requests(mgmt)
    provider_reads = [r for r in reads if r["path"] == "/ai_gw/v2/providers"]
    obs["choose"] = {
        "listed": listed,
        "nothing_chosen_first": nothing_chosen,
        "providers": providers,
        "checked": checked(tree, "Team A"),
        "workspace_query": [r["query"].get("workspace_id") for r in provider_reads],
        "tsg_header": sorted({str(r.get("xTsgId")) for r in reads}),
        "bearer": sorted({r.get("auth") for r in reads}),
        "leaks": leaks(app, tree),
    }
    obs["tree_providers"] = save_tree("providers", tree)

    # ---- N03: a credential-bearing route, focus, integrations and models ----
    tab(app, "Routes")
    c8.text(app, "1 routes · complete", 12)
    app.ax("press", "AXButton", "Details for team-a-route")
    tree, opened = app.wait_for(lambda t: app.has_text(t, "This configuration carries credentials. Their values are not shown."), 12)
    focus = fx.App.focused(tree)
    heading_focused = bool(focus) and focus.get("role") == "AXHeading" and focus.get("label") == "team-a-route"
    route_tree = save_tree("route-detail", tree)
    route_leaks = leaks(app, tree)
    app.ax("press", "AXButton", "Close details")
    time.sleep(0.6)
    back = fx.App.focused(app.tree())
    tab(app, "Integrations")
    tree, tenant_wide = c8.text(app, "Integrations are tenant-wide", 12)
    app.ax("press", "AXButton", "Details for OpenAI")
    c8.text(app, "Read models", 12)
    app.ax("press", "AXButton", "Read models")
    tree, models = c8.text(app, "2 models", 12)
    obs["details"] = {
        "route_opened": opened,
        "heading_focused": heading_focused,
        "focus_returned": bool(back) and back.get("label") == "Details for team-a-route",
        "tenant_wide": tenant_wide,
        "models": models,
        "protected_listed": app.has_text(tree, "protected_fields"),
        "leaks": sorted(set(route_leaks + leaks(app, tree))),
    }
    obs["tree_route"] = route_tree
    obs["tree_integration"] = save_tree("integration", tree)
    app.ax("press", "AXButton", "Close details")

    # ---- N04: another workspace while the first's read is in flight ----
    mgmt.mode(delayMs={"/ai_gw/v2/guardrails": 4000})
    tab(app, "Guardrails")
    time.sleep(0.8)
    app.ax("press", "AXRadioButton", "Team B")
    seen_a = False
    deadline = time.time() + 12
    tree, shown_b = app.tree(), False
    while time.time() < deadline:
        tree = app.tree()
        seen_a = seen_a or app.has_text(tree, "team-a-guardrail")
        if app.has_text(tree, "team-b-guardrail"):
            shown_b = True
            break
        time.sleep(0.3)
    time.sleep(1.5)
    tree = app.tree()
    seen_a = seen_a or app.has_text(tree, "team-a-guardrail")
    mgmt.mode()
    tab(app, "Providers")
    tree, partial = c8.text(app, "1 of 5+ providers read. More exist; they are not shown and not assumed absent.", 12)
    guardrail_reads = [r["query"].get("workspace_id") for r in gateway_requests(mgmt) if r["path"] == "/ai_gw/v2/guardrails"]
    obs["switch"] = {"b_shown": shown_b, "a_late_reply_shown": seen_a, "checked_b": checked(tree, "Team B"),
                     "partial": partial, "guardrail_reads": guardrail_reads, "leaks": leaks(app, tree)}
    obs["tree_switch"] = save_tree("switch", tree)

    # ---- N05: prerequisites as read ----
    tab(app, "Inference prerequisites")
    tree, unbound = c8.text(app, "scope-team-b · The workspace’s IAM scope exists but does not list this workspace.", 15)
    statement = app.has_text(tree, "No Gateway inference credential is configured in the desktop yet")
    keys = app.has_text(tree, "1 of the service keys read may call inference")
    mgmt.mode(gwAdminStatus=403, gwIamStatus=403)
    app.ax("press", "AXButton", "Refresh prerequisites")
    tree, denied = c8.text(app, "This management client may not read IAM scopes.", 15)
    obs["prerequisites"] = {
        "unbound": unbound,
        "inference_statement": statement,
        "inference_keys": keys,
        "scope_denied": denied,
        "admin_denied": app.has_text(tree, "Not allowed for this management client"),
        "integrations_unavailable": app.has_text(tree, "Unavailable: This management client is not allowed to read this."),
        "data_readable": app.has_text(tree, "Readable"),
        "leaks": leaks(app, tree),
    }
    obs["tree_prerequisites"] = save_tree("prerequisites", tree)
    mgmt.mode()

    # ---- N06: a pending Runtime change review stays protected ----
    c8.open_admin(app)
    c8.text(app, "Items 1–25 of 25+")
    c9.open_change(app, "Custom topics", "Details for Finance", "Edit…")
    app.type_into("AXTextArea", "custom topic draft (JSON)",
                  '{"topic_name":"Finance","description":"Reviewed before Gateway use","examples":[],"revision":1}')
    _, reviewed = c9.review(app, "Applies only to this tenant")
    open_gateway(app)
    c8.text(app, "2 workspaces", 12)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 12)
    app.ax("press", "AXRadioButton", "Team B")
    c8.text(app, "1 of 5+ providers read", 12)
    c8.open_admin(app)
    c8.text(app, "Items 1–25 of 25+")
    c9.open_change(app, "Custom topics", "Details for Finance", "Edit…")
    tree, held = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Review again") is not None, 10)
    obs["review"] = {"reviewed": reviewed, "held_after_gateway": held,
                     "writes": [r["path"] for r in mgmt.requests() if r["method"] == "PUT"]}
    obs["tree_review"] = save_tree("review-held", tree)

    # ---- N08: keyboard reach at 200% zoom and the minimum size ----
    open_gateway(app)
    c8.text(app, "1 of 5+ providers read", 12)
    for _ in range(10):
        app.keys("=", "command down")
    obs["zoom_refresh"] = c8.reach(app, 2, "Refresh workspaces")
    app.keys("0", "command down")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{900, 600}}'])
    time.sleep(0.8)
    obs["min_details"] = c8.reach(app, 1, "Details for team-b-provider-1")
    fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {app.pid}) to set size of window 1 to {{1280, 820}}'])

    # ---- N07: restart restores only a listed workspace ----
    app.quit()
    app.launch()
    open_gateway(app)
    tree, restored = c8.text(app, "Restored the workspace last chosen for this tenant: it is still listed.", 15)
    restore = {"restored": restored, "checked_b": checked(tree, "Team B"),
               "read_b": c8.text(app, "1 of 5+ providers read", 12)[1]}
    mgmt.mode(gwWorkspaces="onlyA")
    app.ax("press", "AXButton", "Refresh workspaces")
    tree, missing = c8.text(app, "The workspace last chosen for this tenant is no longer listed. Choose another.", 15)
    restore.update({"missing": missing, "cleared": not checked(tree, "Team A") and radio(tree, "Team B") is None
                    and app.has_text(tree, "Choose a workspace to read its configuration.")})
    obs["restore"] = restore
    obs["tree_restore"] = save_tree("restore-missing", tree)
    mgmt.mode()

    app.quit()
    requests = mgmt.requests()
    obs["writes"] = [{k: r.get(k) for k in ("method", "path")} for r in gateway_requests(mgmt) if r["method"] != "GET"]
    obs["gateway_requests"] = len(gateway_requests(mgmt))
    obs["all_non_get"] = sorted({r["path"] for r in requests if r["method"] != "GET" and r["path"] != "/oauth2/access_token"})
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in (c8.SECRET, c8.SCAN_KEY) + PRIVATE)]
    endpoint.stop()
    mgmt.stop()
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    shutil.rmtree(work, ignore_errors=True)
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C16"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    nc = o["no_client"]
    add("C16-N01", nc["asked"] and nc["requests"] == 0, f"without a management client: {nc}", [])
    ch = o["choose"]
    add("C16-N02", ch["listed"] and ch["nothing_chosen_first"] and ch["providers"] and ch["checked"]
        and ch["workspace_query"] == [GW_A] and ch["tsg_header"] == [c8.TSG] and ch["bearer"] == ["bearer-issued"]
        and not ch["leaks"], f"choose {ch}", [o["tree_providers"]])
    d = o["details"]
    add("C16-N03", d["route_opened"] and d["heading_focused"] and d["focus_returned"] and d["tenant_wide"] and d["models"]
        and d["protected_listed"] and not d["leaks"], f"details {d}", [o["tree_route"], o["tree_integration"]])
    sw = o["switch"]
    add("C16-N04", sw["b_shown"] and not sw["a_late_reply_shown"] and sw["checked_b"] and sw["partial"]
        and sw["guardrail_reads"][:2] == [GW_A, GW_B] and not sw["leaks"], f"switch {sw}", [o["tree_switch"]])
    p = o["prerequisites"]
    add("C16-N05", p["unbound"] and p["inference_statement"] and p["inference_keys"] and p["scope_denied"]
        and p["admin_denied"] and p["integrations_unavailable"] and p["data_readable"] and not p["leaks"],
        f"prerequisites {p}", [o["tree_prerequisites"]])
    rv = o["review"]
    add("C16-N06", rv["reviewed"] and rv["held_after_gateway"] and not rv["writes"], f"review {rv}", [o["tree_review"]])
    rs = o["restore"]
    add("C16-N07", rs["restored"] and rs["checked_b"] and rs["read_b"] and rs["missing"] and rs["cleared"],
        f"restore {rs}", [o["tree_restore"]])
    reach_ok = o["zoom_refresh"]["clear"] and o["min_details"]["clear"]
    add("C16-N08", reach_ok, f"200% zoom: {o['zoom_refresh']['found']}; minimum size: {o['min_details']['found']}", [])
    add("C16-N09", not o["writes"] and o["gateway_requests"] > 0 and not o["secret_in_trees"] and o["v1_unchanged"],
        f"non-GET Gateway requests: {o['writes']} (non-GET requests overall, the Runtime review excluded: {o['all_non_get']}); "
        f"{o['gateway_requests']} Gateway reads; secrets or private markers in AX snapshots: {o['secret_in_trees']}; "
        f"v1 unchanged: {o['v1_unchanged']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C16",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes) against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c16.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The AI Gateway data, admin and IAM planes are a synthetic local TLS endpoint at the production paths; no product endpoint is called before C25.",
            "The synthetic replies follow the SDK's Gateway contract shapes; live Gateway qualification (C25) is not claimed.",
            "Spoken VoiceOver output was not recorded; AX roles, names and focus are inspected.",
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
