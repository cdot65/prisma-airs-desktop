"""Runs a pre-P09 native driver on the one presentation (C44).

Imported before the driver, it patches the fixture-app helpers the legacy
scripts call so their assertions run unchanged against the v2 shell:

- `open_settings`: the first run of a fresh store is the wizard; it is
  walked once with a placeholder tenant (no credential) before Settings.
- `create_custom_tenant`: the core keeps the active tenant on create, so a
  tenant the script creates is made active at once, as the legacy form did.
- `ax press/focus` of a legacy sidebar entry (Sessions & Reports, Import
  from v1, a product page): the v2 shell lists these in the Go menu.
"""

import time

import fixture_app as fx
import v2_setup as v2s

GO = {
    "Sessions &": "Sessions & Reports",
    "Sessions & Reports": "Sessions & Reports",
    "Runtime Security": "Runtime Security",
    "Runtime": "Runtime Security",
    "AI Gateway": "AI Gateway",
    "Red Team & Models": "Red Team & Models",
    "Administration": "Administration",
    # The legacy Case Library is the Test Cases destination.
    "Case Library": "Test Cases",
    "Academy": "Academy",
    "Harness": "Harness",
    "Home": "Home",
    "API Client": "API Client",
}

# The legacy Administration sidebar's inventories: on the v2 shell they are the AI Runtime Security card's DLP tab
# (its DLP inventory segments) and the card's own tabs.
DLP_INVENTORIES = ("Data patterns", "Dictionaries", "Data profiles", "Data filtering profiles")
RUNTIME_TABS = ("Deployment profiles", "Security profiles", "Custom topics", "Applications", "API keys", "Overview", "Playground", "DLP")

_open_settings = fx.App.open_settings
_create = fx.App.create_custom_tenant
_ax = fx.App.ax


# The placeholder tenant's TSG: not the drivers' shared TSG (c8.TSG), which the v1 import fixture gives its Prod tenant;
# the importer maps a v1 tenant onto an existing tenant with the same TSG, and the C23 run saw Prod land on the
# placeholder instead of becoming its own tenant.
PLACEHOLDER_TSG = "9000000009"


def open_settings(self):
    if v2s.wizard_showing(self, timeout=3):
        v2s.first_run(self, None, label=v2s.PLACEHOLDER, tsg=PLACEHOLDER_TSG, client=False)
    return _open_settings(self)


def create_custom_tenant(self, label, origin):
    out = _create(self, label, origin)
    self.make_active(label)
    # The switch leaves Settings for Home: the pre-P09 driver's next step types the credentials into Settings, so it
    # is opened again here (the first run on this build found Home where it expected the key field).
    _open_settings(self)
    return out


def _runtime_card(self):
    """Administration with the AI Runtime Security card open (Expand all opens every card)."""
    tree = self.tree()
    if self.node(tree, "AXHeading", "Administration") is None:
        self.menu("Go", "Administration")
        tree, _ = self.wait_for(lambda t: self.node(t, "AXHeading", "Administration") is not None, 12)
    if not any(n.get("role") == "AXRadioButton" and (n.get("label") or "") == "DLP" for n in tree):
        _ax(self, "press", "AXButton", "Expand all")
        self.wait_for(lambda t: any(n.get("role") == "AXRadioButton" and (n.get("label") or "") == "DLP" for n in t), 10)


def _runtime_tab(self, label):
    """Selects a tab of the Runtime card. Pressed every time: the tree reports a value of 1 on tabs that are not
    selected (the C10 run read Data filtering profiles where it expected Deployment profiles), and pressing the
    selected tab again changes nothing."""
    _runtime_card(self)
    _ax(self, "press", "AXRadioButton", label)
    time.sleep(0.8)


def _playground(self):
    """The Runtime card's Playground tab with its scan form shown: the legacy Runtime Security view's scan form is the
    AI Runtime Security card's Playground tab on the one presentation (Go › Runtime Security lands on the card's
    default tab)."""
    _runtime_tab(self, "Playground")
    self.wait_for(lambda t: self.node(t, "AXTextField", "Profile name or ID") is not None, 12)


def open_runtime(self):
    self.menu("Go", "Runtime Security")
    self.wait_for(lambda t: self.node(t, "AXHeading", "Administration") is not None, 12)
    _playground(self)
    return self.tree()


def ax(self, command, *args):
    # The first v2 shell's drivers (C28–C33) switched the window to v2 through Settings' interface switch; on the one
    # presentation there is no switch and the window is already v2.
    if command == "press" and len(args) >= 2 and args[0] == "AXCheckBox" and args[1] == "Use the new interface" and self.node(self.tree(), "AXCheckBox", args[1]) is None:
        return ""
    if command in ("press", "focus") and len(args) >= 2 and args[0] == "AXButton" and args[1] in GO and self.node(self.tree(), "AXButton", args[1]) is None:
        # A pre-P09 driver may open a destination before any tenant exists (the legacy shell allowed it); on the one
        # presentation the first run is the wizard, walked once with the placeholder tenant first.
        if v2s.wizard_showing(self, timeout=1):
            v2s.first_run(self, None, label=v2s.PLACEHOLDER, tsg=PLACEHOLDER_TSG, client=False)
        self.menu("Go", GO[args[1]])
        if GO[args[1]] == "Runtime Security":
            self.wait_for(lambda t: self.node(t, "AXHeading", "Administration") is not None, 12)
            _playground(self)
        return ""
    if command == "press" and len(args) == 2 and args[0] == "AXButton" and args[1] in DLP_INVENTORIES:
        # A legacy DLP inventory: the Runtime card's DLP tab, then its segment.
        _runtime_tab(self, "DLP")
        _ax(self, "press", "AXRadioButton", args[1])
        time.sleep(0.8)
        return ""
    if command == "press" and len(args) == 2 and args[0] == "AXButton" and args[1] in RUNTIME_TABS and self.node(self.tree(), "AXButton", args[1]) is None:
        _runtime_tab(self, args[1])
        return ""
    if command == "press" and len(args) >= 2 and args[0] == "AXButton" and args[1] == "Close" and not any(n.get("role") == "AXButton" and (n.get("label") or "") == "Close" for n in self.tree()):
        # The v2 review sheet's close control is "Close review".
        if self.node(self.tree(), "AXButton", "Close review") is not None:
            return _ax(self, "press", "AXButton", "Close review")
    if command in ("press", "focus") and len(args) == 2 and isinstance(args[1], str):
        # The probe matches a label by prefix, in tree order. The v2 shell's rail puts "Search or go to… Command K"
        # ahead of a page's own "Search" button, so a legacy driver's exact label is preferred when one exists
        # (the first run on this build opened the palette instead of searching).
        role, label = args
        matches = [n for n in self.tree() if n.get("role") == role and (n.get("label") or "").startswith(label)]
        exact = [i for i, n in enumerate(matches) if (n.get("label") or "") == label]
        if exact and exact[0] != 0:
            return _ax(self, command, role, label, exact[0])
    return _ax(self, command, *args)


fx.App.open_settings = open_settings
fx.App.open_runtime = open_runtime
fx.App.create_custom_tenant = create_custom_tenant
fx.App.ax = ax

# The C8 reach: the legacy shell drew a 60 CSS px fade over the bottom of every page, so a control reached by Tab
# had to end above it; the v2 shell has no such fade, and a control scrolled into view is clear when it lies inside
# the window.
import acceptance_c08 as _c8

_reach = _c8.reach


def reach(app, zoom, label):
    out = _reach(app, zoom, label)
    f, w = out.get("found"), out.get("window")
    if f and w:
        out["clear"] = f["pos"][1] >= w["pos"][1] and f["pos"][1] + f["size"][1] <= w["pos"][1] + w["size"][1] + 1
        out["criterion"] = "inside the window (the v2 shell has no bottom fade)"
    return out


_c8.reach = reach

# The C23 keyboard run reaches the legacy sidebar's "Import from v1" by Tab and presses Space on it. The v2 shell has
# no sidebar entry for it; its keyboard route is the command palette (⌘K, the destination's name, Return), used here
# so the run stays keyboard-only.
try:
    import acceptance_c23 as _c23

    _space_on = _c23.space_on

    def space_on(app, role, prefix):
        if role == "AXButton" and prefix in GO and app.node(app.tree(), "AXButton", prefix) is None:
            app.keys("k", "command down")
            time.sleep(0.6)
            app.keys(GO[prefix])
            time.sleep(0.6)
            app.keycode(36)
            time.sleep(0.8)
            return GO[prefix]
        return _space_on(app, role, prefix)

    _c23.space_on = space_on
except ImportError:
    pass

# The C9 helpers' exact Close: the v2 review sheet's control is "Close review".
try:
    import acceptance_c09 as _c9

    _press_exact = _c9.press_exact

    def press_exact(app, label):
        if label == "Close" and not any(n.get("role") == "AXButton" and (n.get("label") or "") == "Close" for n in app.tree()):
            return _press_exact(app, "Close review")
        return _press_exact(app, label)

    _c9.press_exact = press_exact
except ImportError:  # a driver that does not use the C9 helpers
    pass
