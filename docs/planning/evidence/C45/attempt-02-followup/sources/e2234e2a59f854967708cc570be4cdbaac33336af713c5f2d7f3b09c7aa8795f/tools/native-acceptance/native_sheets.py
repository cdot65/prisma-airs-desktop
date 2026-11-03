"""Drives the production rfd sheets (NSOpenPanel / NSSavePanel) in the
fixture build, which uses them when `fixture-native-sheets` exists in its
data root. The panels list the operator's own folders, so callers must not
save Accessibility trees while a panel is shown; every path typed here is a
driver-owned temporary directory.
"""

import os
import time

import fixture_app as fx

FLAG = os.path.join(fx.DATA, "fixture-native-sheets")


def enable():
    os.makedirs(fx.DATA, exist_ok=True)
    open(FLAG, "w").close()


def disable():
    if os.path.exists(FLAG):
        os.remove(FLAG)


def sheets(tree):
    return sum(1 for n in tree if n.get("role") == "AXSheet")


def wait_sheet(app, count=1, timeout=10):
    return app.wait_for(lambda t: sheets(t) >= count, timeout)[1]


def wait_closed(app, timeout=10):
    return app.wait_for(lambda t: sheets(t) == 0, timeout)[1]


def in_sheet(tree, label):
    """The `ax press` index of the last enabled button labelled `label`
    inside a sheet, or None. `ax press` matches by label prefix across every
    window in tree order, so a window button with the same label (Settings
    has its own Save) is counted but never chosen."""
    found, matched, sheet = None, 0, None
    for n in tree:
        if sheet is not None and n.get("depth", 0) <= sheet:
            sheet = None
        if n.get("role") == "AXSheet":
            sheet = n.get("depth", 0)
        elif n.get("role") == "AXButton" and (n.get("label") or "").startswith(label):
            if sheet is not None and n.get("label") == label and n.get("enabled"):
                found = matched
            matched += 1
    return found


def press(app, label, attempts=4):
    """AXPresses a panel button once it is enabled (panels settle late)."""
    for _ in range(attempts):
        tree, ready = app.wait_for(lambda t: in_sheet(t, label) is not None, 6)
        nth = in_sheet(tree, label)
        if nth is None:
            continue
        try:
            app.ax("press", "AXButton", label, nth)
            return True
        except RuntimeError:
            time.sleep(1.0)
            # A press that opens a confirmation (Replace) can report an
            # error although it took effect: the button is then held.
            if in_sheet(app.tree(), label) is None:
                return True
    return False


def go_to(app, path):
    """Opens Go to Folder in the shown panel and goes to `path`."""
    app.keys("g", "command down, shift down")
    if not wait_sheet(app, 2, 6):
        raise RuntimeError("Go to Folder did not open")
    app.keys("a", "command down")
    app.keys(path)
    time.sleep(1.5)
    app.keycode(36)
    app.wait_for(lambda t: sheets(t) == 1, 6)
    time.sleep(1.0)


def save_as(app, directory, name=None, replace=False):
    """Saves into `directory` (optionally as `name`) through the shown save
    panel; with `replace`, confirms replacing an existing file."""
    if not wait_sheet(app):
        raise RuntimeError("no save panel")
    if name:
        app.keys("a", "command down")
        app.keys(name)
    go_to(app, directory.rstrip("/") + "/")
    if not press(app, "Save"):
        raise RuntimeError("Save could not be pressed")
    if replace:
        tree, shown = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Replace") is not None, 6)
        if not shown:
            raise RuntimeError("no replace confirmation")
        press(app, "Replace")
    return wait_closed(app)


def open_file(app, path):
    """Chooses `path` in the shown open panel."""
    if not wait_sheet(app):
        raise RuntimeError("no open panel")
    go_to(app, path)
    if not press(app, "Open"):
        raise RuntimeError("Open could not be pressed")
    return wait_closed(app)


def cancel(app):
    if not wait_sheet(app):
        raise RuntimeError("no panel")
    press(app, "Cancel")
    return wait_closed(app)
