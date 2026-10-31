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

import fixture_app as fx
import v2_setup as v2s

GO = {
    "Sessions &": "Sessions & Reports",
    "Sessions & Reports": "Sessions & Reports",
    "Import from v1": "Import from v1",
    "Runtime Security": "Runtime Security",
    "Runtime": "Runtime Security",
    "AI Gateway": "AI Gateway",
    "Red Team & Models": "Red Team & Models",
    "Administration": "Administration",
}

_open_settings = fx.App.open_settings
_create = fx.App.create_custom_tenant
_ax = fx.App.ax


def open_settings(self):
    if v2s.wizard_showing(self, timeout=3):
        v2s.first_run(self, None, label=v2s.PLACEHOLDER, client=False)
    return _open_settings(self)


def create_custom_tenant(self, label, origin):
    out = _create(self, label, origin)
    self.make_active(label)
    # The switch leaves Settings for Home: the pre-P09 driver's next step types the credentials into Settings, so it
    # is opened again here (the first run on this build found Home where it expected the key field).
    _open_settings(self)
    return out


def ax(self, command, *args):
    if command in ("press", "focus") and len(args) >= 2 and args[0] == "AXButton" and args[1] in GO and self.node(self.tree(), "AXButton", args[1]) is None:
        self.menu("Go", GO[args[1]])
        return ""
    return _ax(self, command, *args)


fx.App.open_settings = open_settings
fx.App.create_custom_tenant = create_custom_tenant
fx.App.ax = ax
