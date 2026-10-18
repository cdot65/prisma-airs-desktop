#!/usr/bin/env python3
"""C01 native acceptance: drive the prepared fixture app and evaluate criteria.

Run from the repository root after `xtask native --chunk C01` prepared the
fixture bundle. Requires the macOS Accessibility grant for the invoking
terminal. No screen-recording grant is needed: pixels come from the fixture
build's own WebView self-capture (Fixture > Capture Window).

Steps:
1. launch the prepared bundle and sample startup sockets (app + WebKit);
2. run the AX driver (tools/native-acceptance/c01_ax_driver.js) with captures;
3. read AX text colors in Dark and Light themes (ax_colors.swift);
4. enable macOS Reduce Motion, relaunch, rerun the driver, then restore the
   setting to its exact prior state;
5. evaluate the C01 criteria and write evidence plus
   docs/planning/evidence/C01/native-acceptance.json.

Every check's result is computed from recorded observations; nothing here
asserts a pass that the observations do not support.
"""

import glob
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import time
import zlib

ROOT = os.getcwd()
# A phase review runs every member chunk's checks into the phase's own
# evidence (AIRS_NATIVE_SUBJECT=Pxx); tools/native-acceptance/merge_phase.py
# then combines the per-chunk records.
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C01")
# Academy's catalog heading (the destination is available since C05).
ACADEMY_HEADING = "What will you take up next?"
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
NATIVE = os.path.join(EVIDENCE, "native" if SUBJECT == "C01" else "native-C01")
RECORD = os.path.join(EVIDENCE, "native-acceptance.json" if SUBJECT == "C01" else "native-acceptance-C01.json")
TOOLS = os.path.join(ROOT, "tools/native-acceptance")
PROCESS = "Fixture.app/Contents/MacOS/prisma-airs-desktop-v2"
CAPTURES = os.path.join(subprocess.run(["getconf", "DARWIN_USER_TEMP_DIR"], capture_output=True, text=True).stdout.strip(),
                        "prisma-airs-v2-fixture-captures")


def sh(argv, **kw):
    return subprocess.run(argv, capture_output=True, text=True, **kw)


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def rel(path):
    return os.path.relpath(path, ROOT)


# ---------- PNG decoding (8-bit RGB/RGBA, non-interlaced) ----------

def png_pixels(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, width = 8, b"", 0
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        chunk = data[pos + 8:pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", chunk)
            assert depth == 8 and color in (2, 6) and interlace == 0
            bpp = 4 if color == 6 else 3
        elif kind == b"IDAT":
            idat += chunk
        pos += 12 + length
    raw = zlib.decompress(idat)
    stride = width * bpp
    rows, prev = [], bytearray(stride)
    for y in range(height):
        f = raw[y * (stride + 1)]
        line = bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b = prev[i]
            c = prev[i - bpp] if i >= bpp else 0
            if f == 1:
                line[i] = (line[i] + a) & 255
            elif f == 2:
                line[i] = (line[i] + b) & 255
            elif f == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = line
    return width, height, bpp, rows


def pixel(img, x, y):
    w, h, bpp, rows = img
    o = x * bpp
    return rows[y][o], rows[y][o + 1], rows[y][o + 2]


def luminance(rgb):
    return (0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]) / 255


def mean_luminance(img, step=16):
    w, h, _, _ = img
    values = [luminance(pixel(img, x, y)) for y in range(0, h, step) for x in range(0, w, step)]
    return sum(values) / len(values)


def ring_pixels(img, box, scale):
    """Counts warm accent pixels on the border band around a control box."""
    x, y, w, h = [int(v * scale) for v in box]
    count = 0
    for yy in range(max(0, y - 8), min(img[1], y + h + 8)):
        for xx in range(max(0, x - 8), min(img[0], x + w + 8)):
            inside = x + 6 <= xx < x + w - 6 and y + 6 <= yy < y + h - 6
            if inside:
                continue
            r, g, b = pixel(img, xx, yy)
            # Warm accent of either theme: Paper's rust ring (146, 56, 19)
            # as well as brighter orange accents.
            if r > 120 and r - b > 70 and r - g > 40:
                count += 1
    return count


# ---------- app control ----------

def app_pid():
    out = sh(["pgrep", "-f", PROCESS]).stdout.split()
    return int(out[0]) if out else None


def quit_app():
    sh(["pkill", "-f", PROCESS])
    time.sleep(1)


def launch(bundle):
    quit_app()
    sh(["open", "-n", bundle])
    for _ in range(40):
        time.sleep(0.25)
        if app_pid():
            break
    time.sleep(3)
    return app_pid()


def webkit_pids():
    return set(sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def driver(pid, name):
    if os.path.isdir(CAPTURES):
        shutil.rmtree(CAPTURES)
    out = sh(["osascript", "-l", "JavaScript", os.path.join(TOOLS, "c01_ax_driver.js"), str(pid)])
    if out.returncode != 0:
        raise SystemExit(f"driver failed: {out.stderr}")
    run = json.loads(out.stdout)
    files = sorted(glob.glob(os.path.join(CAPTURES, "capture-*.png")))
    cap_dir = os.path.join(NATIVE, "captures", name)
    os.makedirs(cap_dir, exist_ok=True)
    mapped = {}
    for label, path in zip(run["captures"], files):
        dest = os.path.join(cap_dir, f"{label}.png")
        shutil.copy(path, dest)
        mapped[label] = rel(dest)
    run["capture_files"] = mapped
    path = os.path.join(NATIVE, f"driver-{name}.json")
    with open(path, "w") as f:
        json.dump(run, f, indent=1)
    return run, rel(path)


def colors(pid, theme):
    sh(["osascript", "-e", f'tell application "System Events" to tell (first application process whose unix id is {pid})\n'
        f'set frontmost to true\nclick menu item "{theme}" of menu 1 of menu item "Appearance" of menu 1 of menu bar item "View" of menu bar 1\nend tell'])
    time.sleep(1.2)
    out = sh(["swift", os.path.join(TOOLS, "ax_colors.swift"), str(pid)])
    path = os.path.join(NATIVE, f"colors-{theme.lower()}.json")
    with open(path, "w") as f:
        f.write(out.stdout)
    return json.loads(out.stdout), rel(path)


def reduce_motion():
    out = sh(["swift", "-e", "import AppKit; print(NSWorkspace.shared.accessibilityDisplayShouldReduceMotion)"])
    return out.stdout.strip().endswith("true")


def startup_network(bundle):
    quit_app()
    before = webkit_pids()
    sh(["open", "-n", bundle])
    lines, sockets = [], 0
    for i in range(30):
        pid = app_pid()
        new = sorted(webkit_pids() - before)
        lines.append(f"t={i / 2:.1f} app={pid} webkit={','.join(new)}")
        for p in ([str(pid)] if pid else []) + new:
            out = sh(["lsof", "-nP", "-a", "-p", p, "-i"]).stdout.splitlines()[1:]
            sockets += len(out)
            lines += out
        time.sleep(0.5)
    path = os.path.join(NATIVE, "startup-network-samples.log")
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")
    return sockets, rel(path), len(new)


# ---------- criteria ----------

def step(run, sid):
    return next(s for s in run["steps"] if s["id"] == sid)


def evaluate_flows(run):
    """Returns {check_id: (passed, observation)} for AX-observable checks."""
    results = {}
    launch_obs = step(run, "launch")["observation"]
    labels = [n["label"] or "" for n in step(run, "launch")["snapshot"]]
    results["C01-N01"] = (launch_obs["title"] == "Prisma AIRS Desktop v2 (Fixture)" and "FIXTURE BUILD" in labels,
                          f"window title {launch_obs['title']!r}; 'FIXTURE BUILD' shown in the rail: {'FIXTURE BUILD' in labels}")
    menus = {m["menu"]: {i["title"]: i["cmdChar"] for i in m["items"]} for m in run["menus"]}
    app_menu = next((v for k, v in menus.items() if k.startswith("Prisma AIRS")), {})
    view, go = menus.get("View", {}), menus.get("Go", {})
    want_go = ["Runtime Security", "Academy", "Administration", "Case Library", "API Client", "AI Gateway", "Harness"]
    ok = (app_menu.get("Settings…") == "," and view.get("Command Palette…") == "K" and "Appearance" in view
          and view.get("Zoom In") == "=" and view.get("Actual Size") == "0" and all(g in go for g in want_go)
          and go.get("Runtime Security") == "1" and go.get("Harness") == "9" and "Edit" in menus and "Window" in menus)
    results["C01-N02"] = (ok, f"menus {sorted(menus)}; Settings… cmd {app_menu.get('Settings…')!r}; Command Palette… {view.get('Command Palette…')!r}; Go shortcuts 1..9 present: {all(g in go for g in want_go)}")
    tabs = step(run, "tab-order")["observation"]
    enter = step(run, "enter-academy")["observation"]["focused"] or {"role": None, "label": None}
    reached = any(t and t["role"] == "AXButton" and (t["label"] or "").startswith("Academy") for t in tabs)
    current_seen = any(t and t.get("current") == "page" for t in tabs) or any(
        n.get("current") == "page" for n in step(run, "launch")["snapshot"])
    # Since C05 Academy is available: Return lands on its catalog heading.
    results["C01-N03-keyboard"] = (reached and enter["role"] == "AXHeading" and enter["label"] == ACADEMY_HEADING and current_seen,
                                   f"Tab sequence {[t and t['label'] for t in tabs]}; Return focused {enter['role']} {enter['label']!r}; AXARIACurrent=page exposed: {current_seen}")
    pal = step(run, "palette")["observation"]
    esc = step(run, "palette-escape")["observation"]
    ok = (pal["dialogOpenAfterCmdK"] and any("AI Gateway" in (o or "") for o in pal["optionsAfterTyping"])
          and not pal["dialogOpenAfterReturn"] and pal["focusAfterReturn"]["label"] == "AI Gateway"
          and esc["dialogOpened"] and not esc["dialogOpenAfterEscape"] and esc["after"] == esc["before"])
    results["C01-N04"] = (ok, f"Cmd+K opened the dialog; typing 'gate' left options {pal['optionsAfterTyping']}; Return closed it and focused heading {pal['focusAfterReturn']['label']!r}; Escape returned focus to {esc['after']['label']!r} (was {esc['before']['label']!r})")
    ga = step(run, "go-academy")["observation"]["focused"]
    gs = step(run, "go-settings-with-palette-open")["observation"]
    results["C01-N05"] = (ga["label"] == ACADEMY_HEADING and gs["focused"]["label"] == "Settings" and not gs["dialogOpen"],
                          f"Cmd+2 focused heading {ga['label']!r}; Cmd+K then Cmd+, focused {gs['focused']['label']!r} with palette open={gs['dialogOpen']}")
    # Runtime Security is available since C03 and Academy since C05 (their
    # views' headings are shown); every other product destination is still
    # unavailable.
    expected = {"Administration": "C08", "Sessions & Reports": "C11",
                "Case Library": "C12", "Red Team & Models": "C13", "API Client": "C14", "AI Gateway": "C16",
                "Harness": "C19", "Import from v1": "C23"}
    available = {"Scan a prompt or response", ACADEMY_HEADING}
    dests = step(run, "destinations")["observation"]
    unavailable = [d for d in dests if d["heading"] not in available]
    ok = (len(dests) == 10 and sum(d["heading"] in available and not d["unavailableText"] and not d["planned"] for d in dests) == 2
          and len(unavailable) == 8 and all(d["heading"] in expected and d["unavailableText"] and d["planned"] == [f"PLANNED IN {expected[d['heading']]}"] for d in unavailable))
    results["C01-N06"] = (ok, "; ".join(f"{d['shortcut']}: {d['heading']} ({', '.join(d['planned'])})" for d in dests))
    dark = step(run, "appearance-view-dark")["observation"]
    light = step(run, "appearance-view-light")["observation"]
    # Only the Appearance group: Settings also holds C02's checked
    # destination radio ("AIRS region").
    appearance = ("Match", "Light", "Dark")
    checked = lambda obs: [r["label"].split(" ")[0] for r in obs if r["checked"] == 1 and r["label"].split(" ")[0] in appearance]
    results["C01-N07-state"] = (checked(dark) == ["Dark"] and checked(light) == ["Light"],
                                f"after View > Appearance > Dark the checked radio is {checked(dark)}; after Light it is {checked(light)}")
    snap = step(run, "launch")["snapshot"]
    nav = [n for n in snap if n.get("subrole") == "AXLandmarkNavigation" and n["label"] == "Features"]
    buttons = [n for n in snap if n["role"] == "AXButton" and n["depth"] > 3]
    named = [b for b in buttons if b["label"]]
    heads = [n for n in snap if n["role"] == "AXHeading"]
    ok = bool(nav) and len(named) == len(buttons) and sum("Not available in this build" in (b["label"] or "") for b in buttons) == 8 and len(heads) == 1
    results["C01-N08"] = (ok, f"inspected AXRole, AXSubrole, AXTitle/AXDescription/AXValue, AXARIACurrent, AXFocused and tree order: 'Features' navigation landmark present={bool(nav)}; {len(named)}/{len(buttons)} buttons named; {sum('Not available in this build' in (b['label'] or '') for b in buttons)} destinations announce 'Not available in this build'; headings {[h['label'] for h in heads]}. Spoken VoiceOver output was not recorded.")
    mn = step(run, "min-size")["observation"]
    last = mn["lastDestination"]
    inside = lambda r: r and r["pos"][1] >= r["window"]["pos"][1] and r["pos"][1] + r["size"][1] <= r["window"]["pos"][1] + r["window"]["size"][1] + 1
    # The shared frame fades the last 60 CSS px of the window (over the rail
    # too below 900 CSS px); a focused control must end above it.
    clear = lambda r, zoom: r and r["pos"][1] + r["size"][1] <= r["window"]["pos"][1] + r["window"]["size"][1] - 60 * zoom
    radio = mn.get("settingsRadio")
    results["C01-N09"] = (mn["size"] == [900, 600] and inside(last) and bool(radio) and (radio["label"] or "").startswith("Light") and radio["checked"] == 1 and inside(radio) and clear(radio, 1),
                          f"window resized to {mn['size']}; last destination reachable by Tab at {last and last['pos']} size {last and last['size']} inside window {last and last['window']}; Settings radio {radio and radio['label']!r} operated by keyboard {radio and radio.get('keys')} (checked={radio and radio['checked']}) at {radio and radio['pos']} inside the window and clear of the bottom fade: {bool(clear(radio, 1))}")
    z = step(run, "zoom")["observation"]
    ratio = lambda a: a["size"][1] / z["base"]["size"][1]
    zr = z.get("settingsRadioAt200")
    ok = (abs(ratio(z["at150"]) - 1.5) < 0.15 and abs(ratio(z["at200"]) - 2.0) < 0.15 and inside(z["lastDestinationAt200"])
          and z["reset"]["size"] == z["base"]["size"] and bool(zr) and (zr["label"] or "").startswith("Dark") and zr["checked"] == 1 and inside(zr)
          and clear(z["lastDestinationAt200"], 2) and clear(zr, 2))
    results["C01-N10-ax"] = (ok, f"heading height {z['base']['size'][1]} → {z['at150']['size'][1]} (×{ratio(z['at150']):.2f}) → {z['at200']['size'][1]} (×{ratio(z['at200']):.2f}); last destination at 200% {z['lastDestinationAt200']['pos'] if z['lastDestinationAt200'] else None} size {z['lastDestinationAt200']['size'] if z['lastDestinationAt200'] else None} within the window and clear of the bottom fade: {bool(clear(z['lastDestinationAt200'], 2))}; at 200% the Settings radio {zr and zr['label']!r} was reached and operated by keyboard {zr and zr.get('keys')} (checked={zr and zr['checked']}) and scrolled into the window at {zr and zr['pos']} size {zr and zr['size']}, clear of the bottom fade: {bool(clear(zr, 2))}; Cmd+0 restored {z['reset']['size']}")
    words = ["xp", "credits", "credit", "streak", "notifications", "reward", "rank", "badge", "vegas", "phosphor"]
    found = sorted({w for s in run["steps"] if s.get("snapshot") for n in s["snapshot"] for w in words
                    if n["label"] and w in [t.strip(".,:").lower() for t in n["label"].split()]})
    results["C01-N12"] = (not found, f"scanned every AX snapshot ({sum(len(s['snapshot']) for s in run['steps'] if s.get('snapshot'))} nodes) for gamification terms {words}: found {found}")
    return results


def main():
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    os.makedirs(NATIVE, exist_ok=True)
    for old in glob.glob(os.path.join(NATIVE, "captures")):
        shutil.rmtree(old)

    sockets, net_log, webkit_count = startup_network(bundle)
    pid = launch(bundle)
    run, run_path = driver(pid, "standard")
    flows = evaluate_flows(run)
    dark_colors, dark_path = colors(pid, "Dark")
    light_colors, light_path = colors(pid, "Light")

    # Reduce Motion: enable, relaunch, rerun, restore the exact prior state.
    had_key = sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).returncode == 0
    prior = sh(["defaults", "read", "com.apple.universalaccess", "reduceMotion"]).stdout.strip()
    try:
        sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true"])
        time.sleep(1)
        rm_active = reduce_motion()
        pid_rm = launch(bundle)
        run_rm, run_rm_path = driver(pid_rm, "reduced-motion")
        flows_rm = evaluate_flows(run_rm)
    finally:
        if had_key:
            sh(["defaults", "write", "com.apple.universalaccess", "reduceMotion", "-bool", "true" if prior == "1" else "false"])
        else:
            sh(["defaults", "delete", "com.apple.universalaccess", "reduceMotion"])
        time.sleep(1)
    restored = reduce_motion() == (had_key and prior == "1")
    quit_app()

    css = sorted(glob.glob(os.path.join(ROOT, "apps/desktop/dist/assets/*.css")))
    css_text = open(css[0]).read() if css else ""
    clamp = "prefers-reduced-motion:reduce" in css_text.replace(" ", "") and "animation-duration:.001ms" in css_text.replace(" ", "")

    caps = run["capture_files"]
    scale = None
    launch_img = png_pixels(os.path.join(ROOT, caps["launch"]))
    web = next(n for n in step(run, "launch")["snapshot"] if n["role"] == "AXWebArea")
    scale = launch_img[0] / web["size"][0]
    academy = next(n for n in step(run, "launch")["snapshot"] if n["role"] == "AXButton" and (n["label"] or "").startswith("Academy"))
    box = [academy["pos"][0] - web["pos"][0], academy["pos"][1] - web["pos"][1], academy["size"][0], academy["size"][1]]
    focus_img = png_pixels(os.path.join(ROOT, caps["focus-ring-academy"]))
    ring_focus, ring_launch = ring_pixels(focus_img, box, scale), ring_pixels(launch_img, box, scale)
    lum_dark = mean_luminance(png_pixels(os.path.join(ROOT, caps["settings-dark"])))
    lum_light = mean_luminance(png_pixels(os.path.join(ROOT, caps["settings-light"])))
    ink = lambda cs: next((c["foreground"] for c in cs if c["sample"] == "Prisma AIRS"), None)
    hexlum = lambda h: luminance(tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))) if h else None

    checks = []
    def add(cid, passed, observation, evidence, desc):
        checks.append({"id": cid, "description": desc, "result": "pass" if passed else "fail",
                       "observation": observation, "evidence": evidence})
    inventory = {c["id"]: c["check"] for c in json.load(open(os.path.join(ROOT, "docs/architecture/native-checks.json")))["chunks"]["C01"]}
    add("C01-N01", flows["C01-N01"][0], flows["C01-N01"][1], [run_path, caps["launch"]], inventory["C01-N01"])
    add("C01-N02", flows["C01-N02"][0], flows["C01-N02"][1], [run_path], inventory["C01-N02"])
    ring_ok = ring_focus > 150 and ring_launch < ring_focus / 4
    add("C01-N03", flows["C01-N03-keyboard"][0] and ring_ok,
        flows["C01-N03-keyboard"][1] + f". Focus-ring pixels around the Academy control: {ring_focus} with keyboard focus vs {ring_launch} without (accent outline rendered).",
        [run_path, caps["launch"], caps["focus-ring-academy"]], inventory["C01-N03"])
    add("C01-N04", flows["C01-N04"][0], flows["C01-N04"][1], [run_path, caps["palette-filtered"]], inventory["C01-N04"])
    add("C01-N05", flows["C01-N05"][0], flows["C01-N05"][1], [run_path], inventory["C01-N05"])
    add("C01-N06", flows["C01-N06"][0], flows["C01-N06"][1], [run_path], inventory["C01-N06"])
    theme_ok = flows["C01-N07-state"][0] and lum_dark < 0.3 and lum_light > 0.7 and hexlum(ink(dark_colors)) > 0.7 and hexlum(ink(light_colors)) < 0.2
    add("C01-N07", theme_ok, flows["C01-N07-state"][1] + f". Self-capture mean luminance Dark {lum_dark:.2f}, Light {lum_light:.2f}; AX text ink for 'Prisma AIRS' Dark {ink(dark_colors)} (Gruvbox ink #ebdbb2 in sRGB conversion), Light {ink(light_colors)} (Paper ink #14141a).",
        [run_path, caps["settings-dark"], caps["settings-light"], dark_path, light_path], inventory["C01-N07"])
    add("C01-N08", flows["C01-N08"][0], flows["C01-N08"][1], [run_path], inventory["C01-N08"])
    add("C01-N09", flows["C01-N09"][0], flows["C01-N09"][1], [run_path, caps["min-size"], caps["min-size-settings"]], inventory["C01-N09"])
    add("C01-N10", flows["C01-N10-ax"][0], flows["C01-N10-ax"][1], [run_path, caps["zoom-200"], caps["zoom-200-settings"]], inventory["C01-N10"])
    rm_flows_ok = all(v[0] for k, v in flows_rm.items())
    add("C01-N11", rm_active and rm_flows_ok and clamp and restored,
        f"Reduce Motion enabled (NSWorkspace reports {rm_active}); app relaunched; all {len(flows_rm)} AX-evaluated flows passed again: {rm_flows_ok}; built stylesheet {rel(css[0]) if css else None} contains the prefers-reduced-motion clamp: {clamp}; setting restored to its prior state: {restored}.",
        [run_rm_path], inventory["C01-N11"])
    add("C01-N12", flows["C01-N12"][0], flows["C01-N12"][1], [run_path], inventory["C01-N12"])
    with open(os.path.join(NATIVE, "startup-network.json"), "w") as f:
        json.dump({"schema": "airs-v2/startup-network/1", "executable_sha256": prepared["executable_sha256"], "samples": 30,
                   "interval_seconds": 0.5, "internet_sockets_observed": sockets, "webkit_processes": webkit_count,
                   "raw_log": net_log,
                   "limitations": ["0.5s polling can miss short connections", "mDNSResponder DNS lookups are not attributed per process"]}, f, indent=2)
    add("C01-N13", sockets == 0, f"{sockets} internet sockets across 30 samples over 15s of startup for the app and {webkit_count} WebKit processes (0.5s polling; DNS via mDNSResponder not attributed).",
        [rel(os.path.join(NATIVE, "startup-network.json")), net_log], inventory["C01-N13"])

    hw = sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    os_ver = sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"
    record = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": "C01",
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "Claude Code driving the real fixture window through macOS Accessibility (System Events/JXA and AX attributed text) with fixture WebView self-capture; tools/native-acceptance/acceptance_c01.py",
        "host": f"{hw}; macOS {os_ver}",
        "app": {"bundle": prepared["bundle"], "identifier": prepared["identifier"], "executable_sha256": prepared["executable_sha256"], "fixture": True},
        "checks": checks,
        "limitations": [
            "Pixels come from the fixture build's own WKWebView snapshot (the rendered page), not a screen capture; native window chrome and menus are verified through AX.",
            "Spoken VoiceOver output was not recorded; C01-N08 inspects the accessibility attributes VoiceOver consumes.",
            "Startup network evidence is sampled lsof output, not a packet capture.",
        ],
    }
    with open(RECORD, "w") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    for c in checks:
        print(c["result"].upper(), c["id"], c["observation"][:160])
    return 0 if all(c["result"] == "pass" for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
