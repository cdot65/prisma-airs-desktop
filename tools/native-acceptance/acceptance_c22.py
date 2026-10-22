#!/usr/bin/env python3
"""C22 native acceptance: workspace MCP servers for the Harness through the
real fixture window, the bundled pinned engine and the synthetic Gateway
MCP plane of mgmt_server.py (catalog, OAuth, Streamable HTTP servers).

The browser is played by this driver: it reads the one URL the app opened
(the fixture opener's log), asks the authorization endpoint for its
redirect, and requests the app's loopback callback. The synthetic Gateway
plays the model (mode "harnessMcp": the engine's own name for the server's
tool, found in the request's tools).

Run from the repository root after `xtask native-prepare --chunk C22`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import json
import os
import secrets
import shutil
import ssl
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402
import acceptance_c16 as c16  # noqa: E402
import acceptance_c19 as c19  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C22")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C22" else "native-C22")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C22" else "native-acceptance-C22.json")
INFERENCE_KEY = "SYNTHETIC-C22-HARNESS-KEY"
SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.scan"
SERVER = "Tools tools"
CANARY = "C20-CANARY heliotrope"
RESULT = "C22-NATIVE-MCP-RESULT"
TOKEN_MARKS = ("c22-mcp-access-", "c22-mcp-refresh-")
LOG = []
c19.NATIVE = NATIVE


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    return c19.save_tree(name, tree)


def rows(mgmt, since=0, **match):
    return [r for r in mgmt.requests()[since:] if all(r.get(k) == v for k, v in match.items())]


def mcp_rows(mgmt, since=0):
    return [r for r in mgmt.requests()[since:] if r.get("mcp")]


def browser(work, opened_before):
    """Completes the authorization the app opened; what it carried."""
    urls = fx.opened_urls()[opened_before:]
    if len(urls) != 1:
        return {"opened": urls, "status": None}
    url = urls[0]
    q = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(url).query))
    context = ssl.create_default_context(cafile=os.path.join(work, "endpoint", "ca.pem"))

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=context), NoRedirect)
    try:
        opener.open(url, timeout=10)
        location = None
    except urllib.error.HTTPError as e:
        location = e.headers.get("location") if e.code == 302 else None
    out = {"authorize": url.split("?")[0].endswith("/mcp-oauth/authorize"),
           "pkce": q.get("code_challenge_method") == "S256" and len(q.get("code_challenge", "")) == 43,
           "state": len(q.get("state", "")) >= 43,
           "resource": q.get("resource", "").endswith("/mcp/tools/mcp"),
           "scope": q.get("scope"),
           "client": (q.get("client_id") or "").startswith("c22-client-"),
           "loopback": q.get("redirect_uri", "").startswith("http://127.0.0.1:"),
           "status": None, "forged": None}
    if not location:
        return out
    plain = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        out["forged"] = plain.open(location.replace("state=", "state=x"), timeout=10).status
    except urllib.error.HTTPError as e:
        out["forged"] = e.code
    try:
        out["status"] = plain.open(location, timeout=10).status
    except urllib.error.HTTPError as e:
        out["status"] = e.code
    return out


def sign_in(app, work, again=False):
    before = len(fx.opened_urls())
    label = f"Sign in to {SERVER} again" if again else f"Sign in to {SERVER}"
    c9.press_exact(app, label)
    app.wait_for(lambda t: len(fx.opened_urls()) > before, 20)
    step = browser(work, before)
    tree, done = app.wait_for(lambda t: app.has_text(t, "Signed in.") and not app.has_text(t, "Continue in your browser"), 20)
    step["signed_in"] = done
    return tree, step


def conversation_gone(app):
    return app.wait_for(lambda t: c19.button(t, "Start") is not None and not app.has_text(t, "Conversation in Team A"), 20)


def answered(app, count, timeout=40):
    return app.wait_for(lambda t: sum(x.startswith("Answered.") for x in app.texts(t)) >= 1
                        and fx.App.node(t, "AXButton", "Decline") is None
                        and fx.App.node(t, "AXButton", "Approve once") is None, timeout)


def main():
    try:
        return run()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        fx.sh(["pkill", "-9", "-f", c19.ENGINE])
        fx.clean_fixture_root()


def run():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if c8.sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c22-acceptance-")
    # The engine refuses writes in system temporary folders.
    project = os.path.join(ROOT, "target", f"native-c22-{secrets.token_hex(6)}")
    os.makedirs(project)
    if os.path.isdir(NATIVE):
        shutil.rmtree(NATIVE)
    os.makedirs(NATIVE)
    app = fx.App(bundle, work, LOG)
    app.quit()
    fx.sh(["pkill", "-9", "-f", c19.ENGINE])
    fx.clean_fixture_root()
    v1_before = fx.v1_metadata()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    try:
        return exercise(app, work, endpoint, mgmt, prepared, v1_before, project)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(project, ignore_errors=True)


def exercise(app, work, endpoint, mgmt, prepared, v1_before, project):
    runtime = f"{mgmt.origin}/rt/v1"
    obs = {}
    app.launch()
    app.open_settings()
    app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
    app.create_custom_tenant("Tenant A", endpoint.origin)
    app.save_key(c8.SCAN_KEY)
    c8.save_client(app)
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 15)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    c16.tab(app, "Playground")
    app.type_into("AXTextField", "Runtime endpoint", runtime)
    app.type_into("AXTextField", "Workspace API key with completions access", INFERENCE_KEY, secure=True)
    app.ax("press", "AXButton", "Store key for Team A")
    c8.text(app, f"A key for Team A is stored (Keychain). It is sent only to {runtime}.", 15)

    # ---- N01: the catalog through the management client; the desktop's OAuth ----
    c19.open_harness(app)
    tree, listed = app.wait_for(lambda t: fx.App.node(t, "AXCheckBox", f"Use {SERVER} in new conversations") is not None, 20)
    catalog_reads = rows(mgmt, path="/ai_gw/v2/mcp-servers")
    obs["catalog"] = {"listed": listed, "docs_listed": app.has_text(tree, "Use Docs tools in new conversations"),
                      "management_read": bool(catalog_reads) and catalog_reads[-1].get("auth") == "bearer-issued",
                      "upstream_hidden": not app.has_text(tree, "upstream.example"),
                      "not_signed_in": app.has_text(tree, "Not signed in: its tools will be unavailable until you sign in.")}
    obs["tree_catalog"] = save_tree("catalog", tree)
    app.ax("press", "AXCheckBox", f"Use {SERVER} in new conversations")
    app.wait_for(lambda t: str((fx.App.node(t, "AXCheckBox", f"Use {SERVER} in new conversations") or {}).get("value")) in ("1", "True", "true"), 10)
    items_before = fx.keychain_accounts(SERVICE)
    since = len(mgmt.requests())
    tree, step = sign_in(app, work)
    state_path = os.path.join(fx.DATA, "state.json")
    state_text = open(state_path).read() if os.path.exists(state_path) else ""
    obs["sign_in"] = {**step, "registered": len(rows(mgmt, since, path="/mcp-oauth/register")) == 1,
                      "challenge": any(r.get("status") == 401 for r in mcp_rows(mgmt, since)),
                      "keychain_item_added": len(fx.keychain_accounts(SERVICE)) == len(items_before) + 1,
                      "no_token_in_state": not any(m in state_text for m in TOKEN_MARKS)}
    obs["tree_signed_in"] = save_tree("signed-in", tree)

    # ---- N02: decline runs nothing; approve once runs one call ----
    mgmt.mode(harnessMcp={"server": "tools", "tool": "lookup", "arguments": json.dumps({"query": "record-1"})})
    tree, ready = c19.start(app)
    tree, started = app.wait_for(lambda t: app.has_text(t, "MCP servers: tools (ready)"), 40)
    since = len(mgmt.requests())
    c19.say(app, "Look up record 1.")
    tree, asked = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is not None, 40)
    shown = {"title": app.has_text(tree, "Run a workspace MCP tool"), "server_tool": app.has_text(tree, "tools · lookup"),
             "arguments": app.has_text(tree, '{"query":"record-1"}')}
    obs["tree_approval"] = save_tree("approval", tree)
    app.ax("focus", "AXButton", "Approve once")
    approval_tabs = app.tab_walk(2)
    c9.press_exact(app, "Decline")
    tree, declined_done = answered(app, 1)
    calls_after_decline = len(rows(mgmt, since, mcpMethod="tools/call"))
    c19.say(app, "Look it up again.")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Approve once") is not None, 40)
    # Reached and activated by keyboard alone: back from Decline with
    # Shift-Tab, then Space.
    app.ax("focus", "AXButton", "Decline")
    app.keycode(48, "shift down")
    reached = (app.focused(app.tree()) or {}).get("label")
    app.keycode(49)
    tree, approved_done = app.wait_for(lambda t: app.has_text(t, RESULT) and fx.App.node(t, "AXButton", "Approve once") is None, 40)
    calls = rows(mgmt, since, mcpMethod="tools/call")
    inference = rows(mgmt, since, path="/rt/v1/responses")
    obs["approval"] = {"ready": ready, "started": started, "asked": asked, **shown,
                       "declined_entry": app.has_text(tree, "(declined)"), "declined_done": declined_done,
                       "calls_after_decline": calls_after_decline, "approved_done": approved_done,
                       "calls": len(calls), "issued_bearer": all(r.get("mcpBearer") == "issued" for r in calls),
                       "no_bearer_in_inference": all(not r.get("authorizationHeader") for r in inference),
                       "approval_tabs": [(o or {}).get("label") for o in approval_tabs],
                       "approve_reached_by_keyboard": reached}
    obs["tree_approved"] = save_tree("approved", tree)
    pid = c19.leader()
    app.ax("press", "AXButton", "End conversation")
    c8.text(app, "You ended this conversation.", 10)
    c19.wait(lambda: pid not in c19.engines(), 10)

    # ---- N03: an MCP-only rotation restarts and restores, without asking again ----
    with open(os.path.join(fx.DATA, "fixture-project-folder"), "w") as f:
        f.write(project)
    app.ax("press", "AXButton", "Choose a folder…")
    app.wait_for(lambda t: app.has_text(t, project), 10)
    app.ax("press", "AXCheckBox", "Allow changes in this folder")
    mgmt.mode(mcpLifetime=6)
    tree, step_again = sign_in(app, work, again=True)
    mgmt.mode(mcpLifetime=6)
    tree, ready = c19.start(app)
    app.wait_for(lambda t: app.has_text(t, "MCP servers: tools (ready)"), 40)
    c19.say(app, f"Remember this: {CANARY}.")
    c8.text(app, "Answered.", 30)
    first = c19.leader()
    refreshes_before = len(rows(mgmt, grantType="refresh_token", mcpPlane="/mcp-oauth/token"))
    since = len(mgmt.requests())
    time.sleep(4)
    c19.say(app, "What did I ask you to remember?")
    asked_again = False
    deadline = time.time() + 40
    restored = False
    while time.time() < deadline:
        tree = app.tree()
        asked_again |= app.has_text(tree, "Allow changes and continue")
        runs = rows(mgmt, since, path="/rt/v1/responses")
        if runs and app.has_text(tree, "Answered.") and c19.leader() not in (None, first):
            restored = True
            break
        time.sleep(0.3)
    runs = rows(mgmt, since, path="/rt/v1/responses")
    after = mcp_rows(mgmt, since)
    obs["rotation"] = {"signed_in": step_again.get("signed_in"), "ready": ready, "restored": restored,
                       "asked_again": asked_again,
                       "refreshes": len(rows(mgmt, grantType="refresh_token", mcpPlane="/mcp-oauth/token")) - refreshes_before,
                       "new_engine": c19.leader() not in (None, first),
                       "old_gone": c19.wait(lambda: first not in c19.engines(), 10),
                       "carries_first_turn": bool(runs) and runs[-1].get("carriesCanary") is True,
                       "renewed_bearer": any(r.get("mcpBearer") == "issued" for r in after)}
    obs["tree_rotation"] = save_tree("rotation", tree)
    # The engine's runtime home holds no MCP token while it runs.
    obs["rotation"]["home_clean"] = c19.scan_for([os.path.join(fx.DATA, "harness-runtime")], list(TOKEN_MARKS)) == []
    pid = c19.leader()
    app.ax("press", "AXButton", "End conversation")
    c8.text(app, "You ended this conversation.", 10)
    c19.wait(lambda: pid not in c19.engines(), 10)

    # ---- N04: an expired sign-in keeps inference ----
    mgmt.mode(mcpLifetime=6)
    sign_in(app, work, again=True)
    mgmt.mode(mcpLifetime=6, mcpRefresh="refuse")
    time.sleep(4)
    since = len(mgmt.requests())
    tree, ready = c19.start(app)
    tree, signed_out = app.wait_for(lambda t: app.has_text(t, "MCP servers: tools (not signed in)"), 40)
    c19.say(app, "Hello.")
    tree, still = c8.text(app, "Answered.", 30)
    obs["expired"] = {"ready": ready, "shown_signed_out": signed_out, "answered": still,
                      "no_issued_bearer": all(r.get("mcpBearer") != "issued" for r in mcp_rows(mgmt, since)),
                      "expired_text": app.has_text(tree, "The sign-in has expired. Sign in again to use its tools.")}
    obs["tree_expired"] = save_tree("expired", tree)
    pid = c19.leader()
    app.ax("press", "AXButton", "End conversation")
    c8.text(app, "You ended this conversation.", 10)
    c19.wait(lambda: pid not in c19.engines(), 10)

    # ---- N05: sign-out, sign-in again, off and a new revision each retire ----
    mgmt.mode()
    sign_in(app, work, again=True)
    retire = {}
    c19.start(app)
    pid = c19.leader()
    items = len(fx.keychain_accounts(SERVICE))
    since = len(mgmt.requests())
    c9.press_exact(app, f"Sign out of {SERVER}")
    tree, gone = conversation_gone(app)
    retire["sign_out"] = {"retired": gone, "reaped": c19.wait(lambda: pid not in c19.engines(), 10),
                          "item_deleted": c19.wait(lambda: len(fx.keychain_accounts(SERVICE)) < items, 10),
                          "revoked": c19.wait(lambda: len(rows(mgmt, since, mcpPlane="/mcp-oauth/revoke")) >= 2, 10)}
    obs["tree_signed_out"] = save_tree("signed-out", tree)
    sign_in(app, work)
    c19.start(app)
    pid = c19.leader()
    sign_in(app, work, again=True)
    tree, gone = conversation_gone(app)
    retire["sign_in_again"] = {"retired": gone, "reaped": c19.wait(lambda: pid not in c19.engines(), 10)}
    c19.start(app)
    pid = c19.leader()
    app.ax("press", "AXCheckBox", f"Use {SERVER} in new conversations")
    tree, gone = conversation_gone(app)
    retire["off"] = {"retired": gone, "reaped": c19.wait(lambda: pid not in c19.engines(), 10)}
    app.ax("press", "AXCheckBox", f"Use {SERVER} in new conversations")
    c19.start(app)
    pid = c19.leader()
    mgmt.mode(mcpUpdated="2026-10-02T00:00:00Z")
    c9.press_exact(app, "Read the servers again")
    tree, gone = conversation_gone(app)
    retire["revision"] = {"retired": gone, "reaped": c19.wait(lambda: pid not in c19.engines(), 10),
                          "stale": app.wait_for(lambda t: app.has_text(t, "Signed in for an earlier version of this server."), 10)[1]}
    obs["retire"] = retire
    obs["tree_retired"] = save_tree("retired", tree)

    # ---- N06: keyboard and accessibility ----
    tree = app.tree()
    app.ax("focus", "AXCheckBox", f"Use {SERVER} in new conversations")
    order = app.tab_walk(6)
    labels = [(o or {}).get("label") or "" for o in order]
    obs["a11y"] = {"region": fx.App.node(tree, "AXHeading", "Workspace MCP servers") is not None,
                   "tab_order": order,
                   "sign_in_by_tab": any(lbl.startswith(f"Sign in to {SERVER}") for lbl in labels),
                   "sign_out_by_tab": any(lbl == f"Sign out of {SERVER}" for lbl in labels),
                   "approval_by_tab": obs["approval"]["approval_tabs"]}
    obs["tree_a11y"] = save_tree("a11y", tree)

    app.quit()
    obs["no_engine_after_quit"] = c19.wait(lambda: c19.engines() == [], 10)
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    marks = (c8.SECRET, c8.SCAN_KEY, INFERENCE_KEY) + TOKEN_MARKS
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in marks)]
    log_text = json.dumps(mgmt.requests())
    obs["secret_in_log"] = any(s in log_text for s in (INFERENCE_KEY, c8.SECRET) + TOKEN_MARKS)
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["engines_left"] = c19.engines()
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C22"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    c, s = o["catalog"], o["sign_in"]
    add("C22-N01", c["listed"] and c["docs_listed"] and c["management_read"] and c["upstream_hidden"] and c["not_signed_in"]
        and s.get("authorize") and s.get("pkce") and s.get("state") and s.get("resource") and s.get("client") and s.get("loopback")
        and s.get("scope") == "mcp:servers:read mcp:tools:list mcp:tools:call" and s["registered"] and s["challenge"]
        and s.get("forged") == 400 and s.get("status") == 200 and s["signed_in"] and s["keychain_item_added"] and s["no_token_in_state"],
        f"catalog {c}; sign-in {s}", [o["tree_catalog"], o["tree_signed_in"]])
    a = o["approval"]
    add("C22-N02", a["ready"] and a["started"] and a["asked"] and a["title"] and a["server_tool"] and a["arguments"]
        and a["declined_entry"] and a["declined_done"] and a["calls_after_decline"] == 0 and a["approved_done"]
        and a["calls"] == 1 and a["issued_bearer"] and a["no_bearer_in_inference"],
        f"approval {a}", [o["tree_approval"], o["tree_approved"]])
    r = o["rotation"]
    add("C22-N03", r["signed_in"] and r["ready"] and r["restored"] and not r["asked_again"] and r["refreshes"] == 1
        and r["new_engine"] and r["old_gone"] and r["carries_first_turn"] and r["renewed_bearer"] and r["home_clean"],
        f"rotation {r}", [o["tree_rotation"]])
    e = o["expired"]
    add("C22-N04", e["ready"] and e["shown_signed_out"] and e["answered"] and e["no_issued_bearer"] and e["expired_text"],
        f"expired {e}", [o["tree_expired"]])
    t = o["retire"]
    add("C22-N05", all(v["retired"] and v["reaped"] for v in t.values()) and t["sign_out"]["item_deleted"]
        and t["sign_out"]["revoked"] and t["revision"]["stale"], f"retire {t}", [o["tree_signed_out"], o["tree_retired"]])
    y = o["a11y"]
    add("C22-N06", y["region"] and y["sign_in_by_tab"] and y["sign_out_by_tab"] and "Decline" in (y["approval_by_tab"] or [])
        and o["approval"]["approve_reached_by_keyboard"] == "Approve once" and o["approval"]["approved_done"],
        f"keyboard and accessibility {y}", [o["tree_a11y"]])
    add("C22-N07", not o["secret_in_trees"] and not o["secret_in_log"] and o["no_engine_after_quit"] and o["engines_left"] == []
        and o["v1_unchanged"] and o["rotation"]["home_clean"],
        f"secrets in AX snapshots: {o['secret_in_trees']}; token in the server log: {o['secret_in_log']}; "
        f"runtime home clean: {o['rotation']['home_clean']}; engines left: {o['engines_left']}; v1 unchanged: {o['v1_unchanged']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C22",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes), playing the browser against the app's loopback callback, with the synthetic Gateway playing the model's MCP tool calls, and inspecting the engine's processes, the fixture keychain and the server log, against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c22.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The Gateway's MCP catalog, its OAuth authorization server and the MCP servers are synthetic local TLS services; no product MCP server, Gateway or model is called before C25.",
            "The browser is played by the driver (an HTTPS request to the authorization endpoint and an HTTP request to the app's loopback callback); the fixture build records the URL instead of opening it.",
            "The model is played by the synthetic Gateway: it asks for the server's tool by the engine's own name for it, once per turn.",
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
