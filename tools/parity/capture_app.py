#!/usr/bin/env python3
"""Captures the v2 app's destinations at 1440×900 in light and dark for the
parity composites (docs/planning/p09-recut.md), through the fixture window
with synthetic endpoints only, then composes each capture beside its
prototype board.

Run from the repository root after `xtask native-prepare --chunk <chunk>`.
Usage: capture_app.py --chunk C35 --screen home [--screen inbox …]
Writes docs/planning/evidence/<chunk>/parity/<screen>-<theme>.png plus the
raw app captures under parity/app/ and the prototype renders under
parity/proto/.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = os.getcwd()
TOOLS = os.path.join(ROOT, "tools/native-acceptance")
sys.path.insert(0, TOOLS)
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c28 as c28  # noqa: E402
import acceptance_c29 as c29  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
LOG = []
TITLE_BAR = 28


def png_size(path):
    import struct
    with open(path, "rb") as f:
        head = f.read(24)
    return struct.unpack(">II", head[16:24])
# Destination → (Go menu item, heading to wait for, prototype start=)
SCREENS = {
    "home": ("Home", "Home", "home"),
    "inbox": ("Inbox", "Inbox", "inbox"),
    "harness": ("Harness", "Harness", "harness"),
    "academy": ("Academy", "Academy", "academy"),
    "cases": ("Test Cases", "Test Cases", "cases"),
    "api": ("API Client", "API Client", "api"),
    "admin": ("Administration", "Administration", "admin"),
}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--chunk", required=True)
    ap.add_argument("--screen", action="append", default=[])
    ap.add_argument("--no-tenant", action="store_true", help="capture without creating a tenant")
    ap.add_argument("--prepare", help="native-prepare.json to take the bundle from (default: the chunk's)")
    ap.add_argument("--measure", action="append", default=[], help="print the geometry of nodes whose label starts with this (after sizing)")
    ap.add_argument("--read", action="store_true", help="read the tenant first (Home's Refresh), so the screens carry the synthetic plane's figures")
    ap.add_argument("--press", action="append", default=[], help="on each screen, press the first button whose name contains this before capturing (a row to open, say)")
    args = ap.parse_args()
    screens = args.screen or ["home"]
    evidence = os.path.join(ROOT, "docs/planning/evidence", args.chunk)
    parity = os.path.join(evidence, "parity")
    os.makedirs(os.path.join(parity, "app"), exist_ok=True)
    os.makedirs(os.path.join(parity, "proto"), exist_ok=True)
    prepared = json.load(open(args.prepare or os.path.join(evidence, "native-prepare.json")))
    json.dump({"bundle": prepared["bundle"], "executable_sha256": prepared["executable_sha256"], "from": os.path.relpath(args.prepare, ROOT) if args.prepare else "native-prepare.json"},
              open(os.path.join(parity, "bundle.json"), "w"), indent=2)
    bundle = os.path.join(ROOT, prepared["bundle"])
    work = tempfile.mkdtemp(prefix="airs-v2-parity-")
    c29.NATIVE = os.path.join(work, "native")
    os.makedirs(c29.NATIVE)
    if os.path.isdir(c29.CAPTURES):
        shutil.rmtree(c29.CAPTURES)
    app = fx.App(bundle, work, LOG)
    c29._APP = app
    app.quit()
    fx.sh(["pkill", "-f", fx.PROCESS])
    fx.clean_fixture_root()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    out = []
    try:
        app.launch()
        pid = app.pid
        if not args.no_tenant:
            app.open_settings()
            app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
            app.create_custom_tenant("Northwind Health", endpoint.origin)
            app.save_key(c8.SCAN_KEY)
            c8.save_client(app)
        c29.press(app, "AXCheckBox", "Use the new interface")
        app.wait_for(lambda t: c29.nav(t, "Destinations") is not None, 15)
        # The web area is the board's 1440×900: the window sits at the screen's origin
        # (so macOS does not shrink it) and is taller by its title bar.
        fx.sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid}) to set position of window 1 to {{0, 0}}'])
        c28.set_window_size(pid, 1440, 900 + TITLE_BAR)
        time.sleep(0.8)
        frame = c29.window_frame(pid)
        print("window", frame, file=sys.stderr)
        if args.measure:
            for n in app.tree():
                label = (n.get("label") or n.get("value") or "")
                if n.get("role") in ("AXWebArea",) or n.get("subrole") == "AXLandmarkNavigation" or any(label.startswith(m) for m in args.measure):
                    print("node", n.get("role"), n.get("subrole"), repr(label[:40]), n.get("pos"), n.get("size"), file=sys.stderr)
        if args.read and not args.no_tenant:
            app.menu("Go", "Home")
            app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 15)
            time.sleep(0.6)
            # Home's Refresh (R2App, C36) or the earlier Home's Read now: the same readTenant intent.
            read_button = "Refresh" if fx.App.node(app.tree(), "AXButton", "Refresh") else "Read now"
            c29.press(app, "AXButton", read_button)
            _, landed = app.wait_for(lambda t: app.has_text(t, "Read finished") and fx.App.node(t, "AXButton", read_button) is not None, 60)
            print("read landed", landed, file=sys.stderr)
            time.sleep(0.8)
        for screen in screens:
            menu, heading, start = SCREENS[screen]
            app.menu("Go", menu)
            app.wait_for(lambda t: fx.App.node(t, "AXHeading", heading) is not None, 15)
            time.sleep(1.0)
            for prefix in args.press:
                node = next((n for n in app.tree() if n.get("role") == "AXButton" and prefix in (n.get("label") or "")), None)
                if node:
                    app.ax("press", "AXButton", node["label"][:60])
                    time.sleep(0.6)
                print("pressed", prefix, node is not None, file=sys.stderr)
            for theme in ("Light", "Dark"):
                c29.appearance(app, theme)
                time.sleep(0.6)
                cap = c29.capture(app, f"{screen}-{theme.lower()}")
                if not cap:
                    raise SystemExit(f"no capture for {screen} {theme}")
                app_png = os.path.join(parity, "app", f"{screen}-{theme.lower()}.png")
                shutil.copy(os.path.join(ROOT, cap), app_png)
                # The prototype is rendered at the web area's own CSS size, read from the
                # accessibility tree: the self-capture is window-sized and its bottom strip,
                # the title bar's height, is dead space the composite crops.
                web = next((n for n in app.tree() if n.get("role") == "AXWebArea" and n.get("size")), None)
                if web:
                    web_w, web_h = int(web["size"][0]), int(web["size"][1])
                else:
                    pw, ph = png_size(app_png)
                    scale = 2 if pw > 1600 else 1
                    web_w, web_h = pw // scale, ph // scale - TITLE_BAR
                proto_png = os.path.join(parity, "proto", f"{screen}-{theme.lower()}.png")
                subprocess.run([sys.executable, "-I", os.path.join(HERE, "dc_render.py"), "--board", "R2App", "--prop", f"start={start}",
                                "--prop", f"theme={theme.lower()}", "--width", str(web_w), "--height", str(web_h), "--out", proto_png], check=True, capture_output=True)
                composite = os.path.join(parity, f"{screen}-{theme.lower()}.png")
                subprocess.run([sys.executable, "-I", os.path.join(HERE, "compose.py"), "--left", proto_png, "--right", app_png,
                                "--out", composite, "--title", f"{heading} · {theme.lower()} · {web_w}×{web_h}"], check=True, capture_output=True)
                out.append(os.path.relpath(composite, ROOT))
            c29.appearance(app, "Match System")
        app.quit()
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        endpoint.stop()
        mgmt.stop()
        fx.clean_fixture_root()
        shutil.rmtree(work, ignore_errors=True)
    for p in out:
        print(p)


if __name__ == "__main__":
    main()
