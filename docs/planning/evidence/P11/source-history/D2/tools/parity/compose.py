#!/usr/bin/env python3
"""Composes a parity image: the prototype board on the left, the app's capture
on the right, each shown at 1440×900, with a caption. Rendered by the headless
browser (no image library is needed on this Mac).

Usage: compose.py --left proto.png --right app.png --out composite.png
       --title "Home · light" [--note "accessibility deviations…"]
"""

import argparse
import base64
import os
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from dc_render import find_browser  # noqa: E402

GAP, CAP = 24, 44


def png_size(path):
    import struct
    with open(path, "rb") as f:
        head = f.read(24)
    return struct.unpack(">II", head[16:24])


def data_url(path):
    return "data:image/png;base64," + base64.b64encode(open(path, "rb").read()).decode("ascii")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--left", required=True)
    ap.add_argument("--right", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--title", default="")
    ap.add_argument("--note", default="")
    ap.add_argument("--browser")
    args = ap.parse_args()
    # Each side is shown at the prototype's CSS size; the app capture (2x, window-sized) is scaled to the
    # same width and its dead strip below the web area is cropped.
    W, H = png_size(args.left)
    width = W * 2 + GAP * 3
    height = H + CAP * 2 + GAP
    html = f"""<!doctype html><html><head><meta charset='utf-8'><style>
html,body{{margin:0;background:#111;color:#eee;font:14px -apple-system,system-ui}}
.row{{display:flex;gap:{GAP}px;padding:{GAP}px}}
figure{{margin:0}} img{{display:block;width:{W}px;height:{H}px;object-fit:cover;object-position:top left;background:#fff}}
figcaption{{height:{CAP}px;line-height:{CAP}px;font-weight:600}}
.note{{padding:0 {GAP}px;height:{CAP}px;line-height:{CAP}px;color:#bbb;white-space:nowrap;overflow:hidden}}
</style></head><body>
<div class='row'>
<figure><figcaption>Prototype · {args.title}</figcaption><img src='{data_url(args.left)}'></figure>
<figure><figcaption>App · {args.title}</figcaption><img src='{data_url(args.right)}'></figure>
</div>
<div class='note'>{args.note}</div>
</body></html>"""
    with tempfile.TemporaryDirectory(prefix="parity-") as d:
        path = os.path.join(d, "composite.html")
        open(path, "w", encoding="utf-8").write(html)
        cmd = [find_browser(args.browser), "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
               f"--window-size={width},{height}", f"--screenshot={os.path.abspath(args.out)}",
               "--virtual-time-budget=1500", "--user-data-dir=" + os.path.join(d, "profile"), "file://" + path]
        r = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
        if r.returncode != 0 or not os.path.exists(args.out):
            sys.stderr.write(r.stderr[-2000:])
            raise SystemExit("compose failed")
    print(args.out)


if __name__ == "__main__":
    main()
