#!/usr/bin/env python3
"""Renders a board of the frozen design prototype to a PNG with a headless
browser, for the parity composites (docs/planning/p09-recut.md).

The prototype files are Claude Design components whose runtime is not in the
repository; tools/parity/dc-shim.js is a static stand-in that expands the
template once from `renderVals()`. Nothing is interactive in the output.

Usage: dc_render.py --board R2App --out home-light.png [--prop start=home]
       [--prop theme=light] [--width 1440 --height 900]
       [--browser /path/to/chrome-headless-shell]
"""

import argparse
import glob
import json
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PROTOTYPE = os.path.join(ROOT, "docs/architecture/ui-v2/prototype")
SHIM = os.path.join(os.path.dirname(os.path.abspath(__file__)), "dc-shim.js")


def find_browser(explicit):
    if explicit:
        return explicit
    candidates = glob.glob(os.path.expanduser("~/Library/Caches/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-mac-arm64/chrome-headless-shell"))
    candidates += ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"]
    for c in sorted(candidates, reverse=True):
        if os.path.exists(c):
            return c
    raise SystemExit("no headless browser found; pass --browser")


def component(name):
    """A component's template (inside <x-dc>) and logic (the x-dc script)."""
    text = open(os.path.join(PROTOTYPE, f"{name}.dc.html"), encoding="utf-8").read()
    template = re.search(r"<x-dc>(.*)</x-dc>", text, re.S)
    script = re.search(r"<script type=\"text/x-dc\"[^>]*>(.*?)</script>", text, re.S)
    if not template or not script:
        raise SystemExit(f"{name}: not a dc component")
    logic = script.group(1)
    if "class Component" not in logic:
        raise SystemExit(f"{name}: no Component class")
    imports = set(re.findall(r'<dc-import name="([A-Za-z0-9_]+)"', template.group(1)))
    return template.group(1), logic, imports


def page(root, props, width, height):
    components = {}
    pending = [root]
    while pending:
        name = pending.pop()
        if name in components:
            continue
        template, logic, imports = component(name)
        components[name] = (template, logic)
        pending.extend(imports)
    parts = [
        "<!doctype html><html><head><meta charset='utf-8'>",
        # The board's root is fixed at 1440×900 in its own style; the viewport's size wins so a
        # capture of a shorter window compares like for like (the board lays out by flex).
        "<style>html,body{margin:0;padding:0;background:#fff} #dc-root{width:%dpx;height:%dpx;overflow:hidden} #dc-root>[data-theme]{width:%dpx!important;height:%dpx!important;border-radius:0!important;border:0!important}</style>" % (width, height, width, height),
        "<script>", open(SHIM, encoding="utf-8").read(), "</script>",
        "</head><body><div id='dc-root'></div>",
    ]
    for name, (template, _) in components.items():
        parts.append(f"<template id='tpl-{name}'>{template}</template>")
    parts.append("<script>const COMPONENTS = {};</script>")
    for name, (_, logic) in components.items():
        # Each component's `class Component extends DCLogic` is scoped per IIFE.
        parts.append("<script>(function(){ %s\nCOMPONENTS[%s] = { template: document.getElementById('tpl-%s').innerHTML, logic: Component }; })();</script>" % (logic, json.dumps(name), name))
    parts.append("<script>window.dcRender({ root: %s, props: %s, components: COMPONENTS });</script>" % (json.dumps(root), json.dumps(props)))
    parts.append("</body></html>")
    return "\n".join(parts)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--board", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--prop", action="append", default=[])
    ap.add_argument("--width", type=int, default=1440)
    ap.add_argument("--height", type=int, default=900)
    ap.add_argument("--browser")
    ap.add_argument("--keep-html", help="also write the generated page here")
    args = ap.parse_args()
    props = {}
    for p in args.prop:
        k, _, v = p.partition("=")
        props[k] = v
    html = page(args.board, props, args.width, args.height)
    if args.keep_html:
        open(args.keep_html, "w", encoding="utf-8").write(html)
    with tempfile.TemporaryDirectory(prefix="dc-render-") as d:
        path = os.path.join(d, "board.html")
        open(path, "w", encoding="utf-8").write(html)
        browser = find_browser(args.browser)
        cmd = [browser, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
               f"--window-size={args.width},{args.height}", f"--screenshot={os.path.abspath(args.out)}",
               "--virtual-time-budget=2000", "--user-data-dir=" + os.path.join(d, "profile"), "file://" + path]
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
        if r.returncode != 0 or not os.path.exists(args.out):
            sys.stderr.write(r.stderr[-2000:])
            raise SystemExit(f"render failed ({r.returncode})")
    print(args.out)


if __name__ == "__main__":
    main()
