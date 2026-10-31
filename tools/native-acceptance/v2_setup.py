"""The v2 tenant setup for native drivers (C44): the first run opens the
wizard (there is one presentation), so a driver that needs a tenant walks
the wizard once — Connect your tenant with a custom endpoint, the scan key,
the management client, the products, the checks, Skip for now twice, Open
Prisma AIRS — and lands on Home. Drivers written before C44 called
`open_settings` + `create_custom_tenant` + `save_key` + `save_client` and
pressed "Use the new interface"; `tenant_on_home` does the same work on
either build.
"""

import time

import fixture_app as fx
import acceptance_c08 as c8

WELCOME = "Welcome to Prisma AIRS Desktop"
PLACEHOLDER = "Setup tenant"


def _press(app, role, label, timeout=10):
    app.wait_for(lambda t: fx.App.node(t, role, label) is not None, timeout)
    app.ax("press", role, label)
    time.sleep(0.5)


def _press_any(app, label, roles=("AXRadioButton", "AXCheckBox", "AXButton"), timeout=8):
    tree, _ = app.wait_for(lambda t: any((n.get("label") or "") == label and n.get("role") in roles for n in t), timeout)
    node = next((n for n in tree if (n.get("label") or "") == label and n.get("role") in roles), None)
    if node:
        app.ax("press", node["role"], label)
        time.sleep(0.5)
    return node is not None


def wizard_showing(app, timeout=8):
    tree, shown = app.wait_for(lambda t: fx.App.node(t, "AXHeading", WELCOME) is not None or fx.App.node(t, "AXHeading", "Connect your tenant") is not None, timeout)
    return shown


def first_run(app, endpoint_origin, label="Northwind Health", tsg=c8.TSG, scan_key=c8.SCAN_KEY, client=True, environment="Staging"):
    """Walks the first-run wizard to Home with one tenant on a custom endpoint."""
    tree, _ = app.wait_for(lambda t: fx.App.node(t, "AXHeading", WELCOME) is not None or fx.App.node(t, "AXHeading", "Connect your tenant") is not None, 15)
    if fx.App.node(tree, "AXHeading", WELCOME) is not None:
        _press(app, "AXButton", "Set up a tenant")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Connect your tenant") is not None, 10)
    app.type_into("AXTextField", "Display name", label)
    app.type_into("AXTextField", "TSG ID", tsg)
    _press_any(app, environment)
    # The custom endpoint: the Region select has no custom entry; the fixture
    # build reads the endpoint origin from the destination the driver stages.
    _press(app, "AXButton", "Create tenant")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Save scan key") is not None, 12)
    app.type_into("AXTextField", "Scan API key", scan_key, secure=True)
    _press(app, "AXButton", "Save scan key")
    app.wait_for(lambda t: fx.App.node(t, "AXButton", "Replace scan key") is not None, 12)
    if client:
        app.type_into("AXTextField", "Client ID", c8.CLIENT_ID)
        app.type_into("AXTextField", "Client secret", c8.SECRET, secure=True)
        _press(app, "AXButton", "Save management client")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Replace management client") is not None, 12)
    _press(app, "AXButton", "Continue")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Which products will you manage?") is not None, 10)
    _press(app, "AXButton", "Continue")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Check the connection") is not None, 10)
    _press(app, "AXButton", "Run the checks")
    tree, _ = app.wait_for(lambda t: any((fx.App.node(t, "AXButton", l) or {}).get("enabled") is not False and fx.App.node(t, "AXButton", l) is not None for l in ("Continue", "Continue with warnings")) and not app.has_text(t, "Checking…"), 45)
    _press(app, "AXButton", "Continue with warnings" if fx.App.node(tree, "AXButton", "Continue with warnings") else "Continue")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Load starter test cases") is not None, 10)
    _press(app, "AXButton", "Skip for now")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Join the AI Security Academy") is not None, 10)
    _press(app, "AXButton", "Skip for now")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", f"{label} is ready") is not None, 10)
    _press(app, "AXButton", "Open Prisma AIRS")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 25)


def tenant_on_home(app, endpoint_origin, label="Northwind Health", client=True, extra=()):
    """One tenant on the driver's synthetic endpoint, active, with Home open.

    On this build the first run is the wizard: it is walked once with a
    placeholder region tenant (no live call is made: the scan key is stored,
    not checked, and no management client is saved), then the driver's
    tenants are added through Settings' tenant form (a custom HTTPS endpoint
    is the only way to reach a synthetic service) and the first is made
    active. On a pre-C44 build the legacy form and the interface switch are
    used as before. `extra` names further tenants to add on the same endpoint.
    """
    if wizard_showing(app, timeout=6):
        first_run(app, endpoint_origin, label=PLACEHOLDER, client=False)
    app.open_settings()
    results = {}
    for name in (label, *extra):
        app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
        results[f"created {name}"] = bool(app.create_custom_tenant(name, endpoint_origin)[1])
        # The core keeps the active tenant on create (the wizard's placeholder here): the new tenant is made
        # active before its credentials are saved, so Settings attaches them to it and not to the placeholder.
        results[f"active {name}"] = bool(app.make_active(name)[1])
        # A tenant switch leaves Settings (the shell returns to Home): Settings is opened again for the credentials
        # and for the next tenant's form (the first run after the switch found no key field).
        app.open_settings()
        if name == label:
            results["scan key"] = bool(app.save_key(c8.SCAN_KEY)[1])
            if client:
                results["management client"] = bool(c8.save_client(app)[1])
    results["active again"] = bool(app.make_active(label)[1])
    missing = [k for k, v in results.items() if not v]
    if missing:
        raise RuntimeError(f"tenant setup did not complete: {missing}")
    tree = app.tree()
    if fx.App.node(tree, "AXCheckBox", "Use the new interface") is not None:
        app.ax("press", "AXCheckBox", "Use the new interface")
    app.menu("Go", "Home")
    return app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 15)
