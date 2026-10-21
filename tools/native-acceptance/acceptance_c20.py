#!/usr/bin/env python3
"""C20 native acceptance: Harness company sign-in, renewal and protected
history through the real fixture window, the bundled pinned engine and the
synthetic OpenID provider and Gateway (mgmt_server.py /idp and /rt/v1).

The browser is played by this driver: it reads the one URL the app opened
(the fixture opener's log), asks the provider's authorize endpoint for its
redirect, and requests the app's loopback callback. Conversations are
answered by the real engine; a renewal restarts it and restores the
conversation, and saved conversations are continued after a relaunch.

Run from the repository root after `xtask native-prepare --chunk C20`.
Requires the Accessibility grant. Synthetic data only; the fixture data
root is removed at the end; v1 data and the login keychain are untouched.
"""

import hashlib
import json
import os
import re
import shutil
import ssl
import stat
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c16 as c16  # noqa: E402
import acceptance_c19 as c19  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C20")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C20" else "native-C20")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C20" else "native-acceptance-C20.json")
INFERENCE_KEY = "SYNTHETIC-C20-HARNESS-KEY"
CLIENT = "native-c20-desktop"
AUDIENCE = "https://gateway.example/native-c20"
ACCOUNT = "native-c20-alice@example.com"
CANARY = "C20-CANARY periwinkle"
FIRST = f"Remember this: {CANARY}."
SECOND = "What did I ask you to remember?"
ANSWER = "Hello from the synthetic Gateway."
HISTORY = os.path.join(fx.DATA, "harness-history")
EXPORTS = os.path.join(fx.DATA, "fixture-exports")
SERVICE = "io.cdot.prisma-airs-desktop.v2.fixture.scan"
LOG = []
c19.NATIVE = NATIVE


def rel(path):
    return os.path.relpath(path, ROOT)


def save_tree(name, tree):
    return c19.save_tree(name, tree)


def browser(work, issuer, opened_before):
    """Completes the authorization the app opened; the loopback status."""
    urls = fx.opened_urls()[opened_before:]
    if len(urls) != 1:
        return {"opened": urls, "status": None}
    url = urls[0]
    query = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(url).query))
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
    result = {"opened": [url.split("?")[0]], "https_issuer": url.startswith(f"{issuer}/authorize?"),
              "pkce": query.get("code_challenge_method") == "S256" and len(query.get("code_challenge", "")) == 43,
              "loopback": query.get("redirect_uri", "").startswith("http://127.0.0.1:") and query.get("redirect_uri", "").endswith("/callback"),
              "state_nonce": len(query.get("state", "")) >= 43 and len(query.get("nonce", "")) >= 43,
              "client": query.get("client_id") == CLIENT, "status": None, "forged": None}
    if not location:
        return result
    plain = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    # Another local page cannot complete it.
    forged = location.replace("state=", "state=x")
    try:
        result["forged"] = plain.open(forged, timeout=10).status
    except urllib.error.HTTPError as e:
        result["forged"] = e.code
    try:
        result["status"] = plain.open(location, timeout=10).status
    except urllib.error.HTTPError as e:
        result["status"] = e.code
    return result


def sign_in(app, work, issuer, again=False):
    before = len(fx.opened_urls())
    app.ax("press", "AXButton", "Sign in again" if again else "Sign in with your browser")
    app.wait_for(lambda t: len(fx.opened_urls()) > before, 15)
    step = browser(work, issuer, before)
    tree, done = app.wait_for(lambda t: app.has_text(t, f"Signed in as {ACCOUNT}") and not app.has_text(t, "Continue in your browser"), 20)
    step["signed_in"] = done
    return tree, step


def saved_line(tree):
    """The conversation's "Saved <date>" line (not the list's heading)."""
    return any(re.match(r"Saved [A-Z][a-z]{2} \d", x) for x in fx.App.texts(tree))


def history_files():
    out = []
    for d, _, files in os.walk(HISTORY):
        out += [os.path.join(d, f) for f in files if f.endswith(".sealed")]
    return sorted(out)


def mode_of(path):
    return stat.S_IMODE(os.stat(path).st_mode)


def runs(mgmt):
    return c19.harness_runs(mgmt)


def idp_rows(mgmt, path):
    return [r for r in mgmt.requests() if r.get("idp") == path]


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
    work = tempfile.mkdtemp(prefix="airs-v2-c20-acceptance-")
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
        return exercise(app, work, endpoint, mgmt, prepared, v1_before)
    finally:
        endpoint.stop()
        mgmt.stop()
        shutil.rmtree(work, ignore_errors=True)


def exercise(app, work, endpoint, mgmt, prepared, v1_before):
    runtime = f"{mgmt.origin}/rt/v1"
    issuer = f"{mgmt.origin}/idp"
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

    # ---- N01: company sign-in through the browser and the loopback ----
    c19.open_harness(app)
    app.ax("press", "AXButton", "Set up company sign-in")
    app.wait_for(lambda t: fx.App.node(t, "AXTextField", "Identity provider") is not None, 10)
    # An http issuer is refused and nothing is kept.
    for label, value in (("Gateway endpoint", runtime), ("Identity provider", "http://localhost/idp"),
                         ("Client ID", CLIENT), ("Gateway audience", AUDIENCE)):
        app.type_into("AXTextField", label, value)
    app.ax("press", "AXButton", "Save company sign-in")
    tree, refused = c8.text(app, "That was not done. Check the values and try again.", 10)
    app.type_into("AXTextField", "Identity provider", issuer)
    app.ax("press", "AXButton", "Save company sign-in")
    tree, saved = c8.text(app, "Sign in with your browser", 10)
    obs["tree_configured"] = save_tree("configured", tree)
    before_items = fx.keychain_accounts(SERVICE)
    # Eight-second tokens, renewed four seconds after issue (N03, N04).
    mgmt.mode(idpLifetime=8)
    tree, step = sign_in(app, work, issuer)
    after_items = fx.keychain_accounts(SERVICE)
    state_text = open(os.path.join(fx.DATA, "state.json")).read() if os.path.exists(os.path.join(fx.DATA, "state.json")) else ""
    token_rows = idp_rows(mgmt, "/token")
    obs["sign_in"] = {"refused_http": refused, "saved": saved, **step,
                      "provider_shown": app.has_text(tree, issuer), "account_shown": app.has_text(tree, f"Company sign-in ({ACCOUNT})"),
                      "keychain_item_added": len(after_items) == len(before_items) + 1,
                      "token_exchanges": [r.get("grantType") for r in token_rows],
                      "no_token_in_state": "c20-refresh-" not in state_text and "eyJ" not in state_text}
    obs["tree_signed_in"] = save_tree("signed-in", tree)

    # ---- N02: a conversation answered with the verified access token ----
    mgmt.mode(idpLifetime=8)
    tree, ready = c19.start(app)
    c19.say(app, FIRST)
    tree, answered = c8.text(app, "Answered.", 25)
    first = runs(mgmt)
    pid_first = c19.leader()
    obs["conversation"] = {"ready": ready, "answered": answered, "sent": len(first),
                           "token_from_idp": bool(first) and first[-1].get("keyIssuedByIdp") is True,
                           "not_the_key": bool(first) and first[-1].get("inferenceKeySha256") != hashlib.sha256(INFERENCE_KEY.encode()).hexdigest(),
                           "saved_shown": app.wait_for(saved_line, 10)[1]}
    obs["tree_conversation"] = save_tree("conversation", tree)

    # ---- N03: a renewal restarts the engine and restores the conversation ----
    refreshed_before = len([r for r in idp_rows(mgmt, "/token") if r.get("grantType") == "refresh_token"])
    time.sleep(5)
    c19.say(app, SECOND)
    tree, renewed = app.wait_for(lambda t: sum(ANSWER in x for x in app.texts(t)) >= 2 and app.has_text(t, "Answered."), 30)
    second = runs(mgmt)
    pid_second = c19.leader()
    obs["renewal"] = {"answered": renewed, "refreshes": len([r for r in idp_rows(mgmt, "/token") if r.get("grantType") == "refresh_token"]) - refreshed_before,
                      "new_engine": pid_second not in (None, pid_first), "old_gone": c19.wait(lambda: pid_first not in c19.engines(), 10),
                      "restored": len(second) == 2 and second[-1].get("carriesCanary") is True and second[-1].get("keyIssuedByIdp") is True,
                      "one_home": len(c19.homes()) == 1, "messages": app.has_text(tree, SECOND) and app.has_text(tree, FIRST)}
    obs["tree_renewed"] = save_tree("renewed", tree)

    # ---- N04: a refused renewal sends nothing and gives the message back ----
    mgmt.mode(idpLifetime=8, idpRefresh="refuse")
    time.sleep(5)
    c19.say(app, "Third message.")
    tree, refused_renewal = c8.text(app, "Your sign-in has expired. Sign in again to send. Your message was not sent; it is back in the box.", 20)
    box = fx.App.node(tree, "AXTextArea", "Message") or {}
    obs["refused_renewal"] = {"shown": refused_renewal, "nothing_sent": len(runs(mgmt)) == 2,
                              "message_back": str(box.get("value") or "") == "Third message."}
    obs["tree_refused_renewal"] = save_tree("refused-renewal", tree)
    mgmt.mode(idpLifetime=3600)
    tree, step_again = sign_in(app, work, issuer, again=True)
    app.ax("focus", "AXTextArea", "Message")
    app.keycode(36, "command down")
    tree, continued = app.wait_for(lambda t: sum(ANSWER in x for x in app.texts(t)) >= 3, 30)
    third = runs(mgmt)
    obs["refused_renewal"].update({"signed_in_again": step_again.get("signed_in"), "continued": continued,
                                   "restored": len(third) == 3 and third[-1].get("carriesCanary") is True})

    # ---- N06: saved, sealed, and listed again after a relaunch ----
    files = history_files()
    leaks = c19.scan_for([fx.DATA], [CANARY, ANSWER, INFERENCE_KEY, "c20-refresh-"])
    tree = app.tree()
    title = FIRST[:80]
    obs["history"] = {"files": len(files), "file_modes": sorted({oct(mode_of(f)) for f in files}),
                      "dir_modes": sorted({oct(mode_of(os.path.dirname(f))) for f in files}),
                      "plaintext": leaks, "listed": app.has_text(tree, title)}
    pid_quit = c19.leader()
    app.quit()
    obs["history"]["engine_gone_at_quit"] = c19.wait(lambda: c19.engines() == [], 10) and pid_quit is not None
    app.launch()
    c16.open_gateway(app)
    c8.text(app, "2 workspaces", 25)
    app.ax("press", "AXRadioButton", "Team A")
    c8.text(app, "2 providers · complete", 15)
    c19.open_harness(app)
    tree, relisted = app.wait_for(lambda t: fx.App.node(t, "AXButton", f"Open {title}") is not None, 15)
    obs["history"]["relisted"] = relisted
    obs["tree_relisted"] = save_tree("relisted", tree)

    # ---- N07: opened and continued in a new engine after the relaunch ----
    app.ax("press", "AXButton", f"Open {title}")
    tree, opened = app.wait_for(lambda t: any(n.get("role") == "AXList" and n.get("label") == "Saved messages" for n in t), 10)
    obs["tree_opened"] = save_tree("opened", tree)
    before_runs = len(runs(mgmt))
    app.ax("press", "AXButton", "Continue this conversation")
    tree, resumed = app.wait_for(lambda t: fx.App.node(t, "AXTextArea", "Message") is not None and app.has_text(t, "Ready."), 30)
    c19.say(app, "One more, please.")
    tree, answered_again = app.wait_for(lambda t: len(runs(mgmt)) > before_runs and app.has_text(t, "Answered."), 30)
    after = runs(mgmt)
    obs["continue"] = {"opened": opened, "resumed": resumed, "answered": answered_again,
                       "restored": len(after) == before_runs + 1 and after[-1].get("carriesCanary") is True,
                       "same_record": len(history_files()) == 1}
    obs["tree_continued"] = save_tree("continued", tree)
    app.ax("press", "AXButton", "End conversation")
    c8.text(app, "You ended this conversation.", 10)
    c19.wait(lambda: c19.engines() == [], 10)

    # ---- N08: export the transcript, then delete it ----
    app.ax("press", "AXButton", f"Open {title}")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Export…") is not None, 10)
    app.ax("press", "AXButton", "Export…")
    tree, exported = app.wait_for(lambda t: app.has_text(t, "Saved harness-"), 15)
    exports = [os.path.join(EXPORTS, n) for n in os.listdir(EXPORTS)] if os.path.isdir(EXPORTS) else []
    markdown = open(exports[0]).read() if exports else ""
    app.ax("press", "AXButton", "Delete…")
    tree, asked = c8.text(app, "Delete this saved conversation from this Mac? It cannot be recovered.", 10)
    app.ax("press", "AXButton", "Delete permanently")
    tree, deleted = c8.text(app, "No saved conversations in this workspace.", 10)
    obs["export_delete"] = {"exported": exported, "files": [os.path.basename(e) for e in exports],
                            "transcript": CANARY in markdown and "## Harness" in markdown,
                            "no_secrets": all(s not in markdown for s in (INFERENCE_KEY, "c20-refresh-", "eyJ")),
                            "asked": asked, "deleted": deleted, "files_left": len(history_files())}
    obs["tree_deleted"] = save_tree("deleted", tree)

    # ---- N05: signing out ends the grant and its conversation ----
    tree, _ = c19.start(app)
    pid_out = c19.leader()
    items_before = fx.keychain_accounts(SERVICE)
    app.ax("press", "AXButton", "Sign out")
    tree, out = app.wait_for(lambda t: app.has_text(t, "Not signed in") and not app.has_text(t, "Conversation in Team A"), 15)
    obs["sign_out"] = {"signed_out": out, "engine_gone": c19.wait(lambda: pid_out not in c19.engines(), 10) and pid_out is not None,
                       "revoked": c19.wait(lambda: len(idp_rows(mgmt, "/revoke")) >= 1, 10),
                       "item_deleted": c19.wait(lambda: len(fx.keychain_accounts(SERVICE)) < len(items_before), 10)}
    obs["tree_signed_out"] = save_tree("signed-out", tree)

    # ---- N09: keyboard and accessibility ----
    tree = app.tree()
    radios = [n for n in tree if n.get("role") == "AXRadioButton" and (n.get("label") or "").startswith(("The workspace inference key", "Company sign-in"))]
    obs["a11y"] = {"credential_radios": len(radios), "history_heading": fx.App.node(tree, "AXHeading", "Saved conversations") is not None}
    app.ax("focus", "AXRadioButton", "The workspace inference key")
    order = app.tab_walk(8)
    obs["a11y"]["tab_order"] = order
    obs["a11y"]["sign_in_by_tab"] = any((o or {}).get("label", "") and (o or {}).get("label", "").startswith("Sign in with your browser") for o in order)
    obs["tree_a11y"] = save_tree("a11y", tree)

    app.quit()
    trees = [os.path.join(NATIVE, n) for n in os.listdir(NATIVE) if n.startswith("tree-")]
    secrets = (c8.SECRET, c8.SCAN_KEY, INFERENCE_KEY, "c20-refresh-", "eyJ")
    obs["secret_in_trees"] = [rel(p) for p in trees if any(s in open(p).read() for s in secrets)]
    log_text = json.dumps(mgmt.requests())
    obs["secret_in_log"] = any(s in log_text for s in (INFERENCE_KEY, "c20-refresh-", "eyJ", CANARY))
    obs["v1_unchanged"] = fx.v1_metadata() == v1_before
    obs["engines_left"] = c19.engines()
    with open(os.path.join(NATIVE, "observations.json"), "w") as f:
        json.dump({"observations": obs, "log": LOG}, f, indent=1)
    return evaluate(prepared, obs)


def evaluate(prepared, o):
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C20"]}
    obs_path = rel(os.path.join(NATIVE, "observations.json"))
    checks = []

    def add(cid, passed, observation, evidence):
        checks.append({"id": cid, "description": inventory[cid], "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": [obs_path] + [e for e in evidence if e]})

    s = o["sign_in"]
    add("C20-N01", s["refused_http"] and s["saved"] and s.get("https_issuer") and s.get("pkce") and s.get("loopback")
        and s.get("state_nonce") and s.get("client") and s.get("forged") == 400 and s.get("status") == 200 and s["signed_in"]
        and s["account_shown"] and s["keychain_item_added"] and s["token_exchanges"] == ["authorization_code"] and s["no_token_in_state"],
        f"sign-in {s}", [o["tree_configured"], o["tree_signed_in"]])
    c = o["conversation"]
    add("C20-N02", c["ready"] and c["answered"] and c["sent"] == 1 and c["token_from_idp"] and c["not_the_key"] and c["saved_shown"],
        f"conversation {c}", [o["tree_conversation"]])
    r = o["renewal"]
    add("C20-N03", r["answered"] and r["refreshes"] == 1 and r["new_engine"] and r["old_gone"] and r["restored"] and r["one_home"]
        and r["messages"], f"renewal {r}", [o["tree_renewed"]])
    f = o["refused_renewal"]
    add("C20-N04", f["shown"] and f["nothing_sent"] and f["message_back"] and f["signed_in_again"] and f["continued"] and f["restored"],
        f"refused renewal {f}", [o["tree_refused_renewal"]])
    so = o["sign_out"]
    add("C20-N05", so["signed_out"] and so["engine_gone"] and so["revoked"] and so["item_deleted"], f"sign-out {so}", [o["tree_signed_out"]])
    h = o["history"]
    add("C20-N06", h["files"] == 1 and h["file_modes"] == ["0o600"] and h["dir_modes"] == ["0o700"] and h["plaintext"] == []
        and h["listed"] and h["engine_gone_at_quit"] and h["relisted"], f"history {h}", [o["tree_relisted"]])
    k = o["continue"]
    add("C20-N07", k["opened"] and k["resumed"] and k["answered"] and k["restored"] and k["same_record"],
        f"continue {k}", [o["tree_opened"], o["tree_continued"]])
    e = o["export_delete"]
    add("C20-N08", e["exported"] and len(e["files"]) == 1 and e["files"][0].endswith(".md") and e["transcript"] and e["no_secrets"]
        and e["asked"] and e["deleted"] and e["files_left"] == 0, f"export and delete {e}", [o["tree_deleted"]])
    y = o["a11y"]
    add("C20-N09", y["credential_radios"] == 2 and y["history_heading"] and y["sign_in_by_tab"], f"keyboard and accessibility {y}", [o["tree_a11y"]])
    add("C20-N10", not o["secret_in_trees"] and not o["secret_in_log"] and o["v1_unchanged"] and o["engines_left"] == [],
        f"secrets in AX snapshots: {o['secret_in_trees']}; secret or prompt in the server log: {o['secret_in_log']}; "
        f"engines left: {o['engines_left']}; v1 unchanged: {o['v1_unchanged']}", [])

    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C20",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (tools/native-acceptance/ax.swift, System Events keystrokes), playing the browser against the app's loopback callback, and inspecting the engine's processes, the fixture keychain and the sealed history files, against tools/native-acceptance/mgmt_server.py; tools/native-acceptance/acceptance_c20.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "The identity provider and the Gateway are synthetic local TLS services; no product identity provider or endpoint is called before C25.",
            "The browser is played by the driver (an HTTPS request to the provider's authorize endpoint and an HTTP request to the app's loopback callback); no real browser session is involved, and the fixture build records the URL instead of opening it.",
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
