"""C49 affected native observations; invoked only by acceptance_p11.run.

All input belongs to the orchestrator's fixture. Captures require independent
visual review. Native observations do not prove before-await secret clearing,
identity authority, rollback, contrast ratios, or earned-skin authorization;
the finalizer must also bind the named production-interface regressions.
This module has no standalone execution path and never reads a real vault.
"""

import time


SOURCE_REQUIREMENTS = {
    "frontend": [
        "src/v2/settings/refinement-settings.test.tsx",
        "src/modules/tenants/credential-entry.test.tsx",
        "src/v2/settings/management-credential.test.tsx",
        "src/shell/appearance.test.ts",
        "src/shell/palette-contrast.test.ts",
        "src/v2/academy/academy.test.tsx",
    ],
    "rust": ["refinement_metadata", "color_theme", "ui_v2_setup_reads", "ui_v2_onboarding"],
    "not_claimed_by_native": [
        "secret clearing occurs before awaiting dispatch",
        "stale context and cross-profile authority are rejected",
        "failed persistence and cancellation roll back safely",
        "measured palette contrast and Academy earned-skin authority",
    ],
}


def _node(ctx, role, label, tree=None, nth=0):
    node = ctx.app.node(ctx.app.tree() if tree is None else tree, role, label, nth)
    ctx.require(node is not None, f"C49 missing {role} {label}")
    return node


def _value(ctx, role, label, nth=0):
    return str(_node(ctx, role, label, nth=nth).get("value") or "")


def _select(ctx, label, options, selected):
    """Choose a native HTML select with real keys, then verify the AX value."""
    ctx.require(selected in options, "unknown C49 select option")
    _node(ctx, "AXPopUpButton", label)
    ctx.app.ax("focus", "AXPopUpButton", label)
    ctx.app.keycode(49)  # Space opens the focused select's native menu.
    ctx.app.keycode(115)  # Home, followed by the declared option index.
    for _ in range(options.index(selected)):
        ctx.app.keycode(125)
    ctx.app.keycode(36)
    tree, ready = ctx.app.wait_for(
        lambda t: str((ctx.app.node(t, "AXPopUpButton", label) or {}).get("value") or "") == selected
    )
    ctx.require(ready, f"C49 {label} did not select {selected}")
    return tree


def _replace_text(ctx, label, value):
    ctx.app.ax("focus", "AXTextField", label)
    ctx.app.keys("a", "command down")
    ctx.app.keycode(51)
    ctx.app.type_into("AXTextField", label, value)


def _radio(ctx, label, nth=0):
    node = _node(ctx, "AXRadioButton", label, nth=nth)
    ctx.require(node.get("enabled") is not False, f"disabled radio {label}")
    ctx.app.ax("press", "AXRadioButton", label, nth)
    tree, selected = ctx.app.wait_for(
        lambda t: (ctx.app.node(t, "AXRadioButton", label, nth) or {}).get("value") in (1, "1")
    )
    ctx.require(selected, f"radio not selected: {label}")
    return tree


def _resize(ctx, width, height):
    ctx.resize(width, height)
    time.sleep(.6)


def _zoom(ctx, doubled=False):
    ctx.app.menu("View", "Actual Size")
    if doubled:
        # Host::zoom increments by 0.1 from reset 1.0; ten steps request 2.0.
        for _ in range(10):
            ctx.app.menu("View", "Zoom In")


def _inside(window, control):
    if not all(n.get("pos") and n.get("size") for n in (window, control)):
        return False
    x, y = window["pos"]
    w, h = window["size"]
    px, py = control["pos"]
    pw, ph = control["size"]
    return pw > 0 and ph > 0 and x - 2 <= px and y - 2 <= py and px + pw <= x + w + 2 and py + ph <= y + h + 2


def _keyboard_cards(ctx):
    wanted = {"Theme", "AIRS API key", "Client ID", "Client secret", "Save key",
              "Change destination…", "Tenant to update", "Edit", "Delete", "Add tenant…"}
    reached, trace = set(), []
    ctx.app.ax("focus", "AXPopUpButton", "Appearance")
    for _ in range(70):
        ctx.app.keycode(48)
        tree = ctx.app.tree()
        focused = ctx.app.focused(tree)
        window = ctx.app.node(tree, "AXWindow", "") or {}
        if focused:
            label = focused.get("label") or ""
            matches = {label_ for label_ in wanted if label == label_ or label.startswith(label_ + " ")}
            visible = _inside(window, focused)
            trace.append({"role": focused.get("role"), "label": label, "inside_window": visible,
                          "pos": focused.get("pos"), "size": focused.get("size")})
            if visible:
                reached.update(matches)
        if reached == wanted:
            break
    evidence = ctx.save("c49-keyboard-200-percent", {"requested_zoom": 2.0, "wanted": sorted(wanted),
        "reached": sorted(reached), "trace": trace, "meaning": "AX geometry and keyboard reach; pixels require review"})
    ctx.require(reached == wanted, f"C49 unreachable controls at narrow/200%: {sorted(wanted - reached)}")
    return evidence, reached == wanted


def _settings(ctx):
    return ctx.navigate("Settings")


def _academy_reader(ctx):
    """From the current synthetic catalog, through guest learner links only."""
    ctx.press("AXCheckBox", "Courses")
    ctx.press("AXLink", "COURSE Runtime Security Foundations")
    ctx.text("Runtime Security Foundations")
    ctx.press("AXLink", "Scanning at runtime: part 1")
    return ctx.text("Lesson 1 of Scanning at runtime opens here")


def _themes(ctx, original_appearance, original_theme):
    evidence, captures = [], []
    options = ["Match system", "Light", "Dark"]
    for theme in ("Prisma AIRS", "Gruvbox"):
        for appearance in ("Light", "Dark"):
            _settings(ctx)
            _select(ctx, "Appearance", options, appearance)
            tree = _select(ctx, "Theme", ["Prisma AIRS", "Gruvbox"], theme)
            name = f"c49-{theme.lower().replace(' ', '-')}-{appearance.lower()}"
            evidence.append(ctx.save(name, tree))
            captures.append(ctx.capture(name))
    # The alternate theme and explicit appearance must survive a real restart.
    old_pid = ctx.app.pid
    ctx.app.quit()
    ctx.app.launch()
    _settings(ctx)
    persisted = _value(ctx, "AXPopUpButton", "Theme") == "Gruvbox" and _value(ctx, "AXPopUpButton", "Appearance") == "Dark"
    ctx.require(persisted and ctx.app.pid != old_pid, "C49 palette/appearance did not survive restart")
    evidence.append(ctx.save("c49-persisted-alternate-theme"))
    ctx.navigate("Home")
    captures.append(ctx.capture("c49-product-gruvbox-dark"))

    portal_observed, desktop_portals, academy_inputs = [], [], []
    for theme in ("Prisma AIRS", "Gruvbox"):
        _settings(ctx)
        _select(ctx, "Theme", ["Prisma AIRS", "Gruvbox"], theme)
        # Desktop-owned modal must follow the selected desktop palette.
        ctx.press("AXButton", "Add tenant…")
        tree = ctx.text("Add a tenant")
        desktop_portals.append(ctx.app.has_text(tree, "Add a tenant"))
        captures.append(ctx.capture(f"c49-desktop-portal-{theme.lower().replace(' ', '-')}"))
        ctx.press("AXButton", "Cancel")
        _resize(ctx, 1100, 800)
        _zoom(ctx, doubled=True)
        ctx.navigate("Academy")
        # Academy owns this ordinary in-page search input too. Capture its
        # actual focus treatment, not only a portal or static learner text.
        label = "Search paths, courses, and labs"
        tree, ready = ctx.app.wait_for(lambda t: any(ctx.app.node(t, role, label)
            for role in ("AXSearchField", "AXTextField")))
        ctx.require(ready, "Academy catalog search input missing")
        role = next(role for role in ("AXSearchField", "AXTextField") if ctx.app.node(tree, role, label))
        ctx.app.ax("focus", role, label)
        tree, focused = ctx.app.wait_for(lambda t: (ctx.app.focused(t) or {}).get("label") == label)
        ctx.require(focused, "Academy catalog search did not receive focus")
        academy_inputs.append(focused)
        evidence.append(ctx.save(f"c49-academy-search-focused-{theme.lower().replace(' ', '-')}", tree))
        captures.append(ctx.capture(f"c49-academy-search-focused-{theme.lower().replace(' ', '-')}"))
        tree = _academy_reader(ctx)
        evidence.append(ctx.save(f"c49-guest-reader-{theme.lower().replace(' ', '-')}", tree))
        captures.append(ctx.capture(f"c49-guest-reader-{theme.lower().replace(' ', '-')}"))
        # Academy's narrow reader Modal is a body portal with Academy tokens.
        ctx.press("AXButton", "Contents")
        tree = ctx.text("Course contents")
        portal_observed.append(ctx.app.has_text(tree, "Course contents"))
        evidence.append(ctx.save(f"c49-academy-contents-{theme.lower().replace(' ', '-')}", tree))
        captures.append(ctx.capture(f"c49-academy-contents-{theme.lower().replace(' ', '-')}"))
        ctx.press("AXButton", "Close")
        ctx.require(not ctx.app.has_text(ctx.app.tree(), "Course contents"), "Academy contents did not close")
        _zoom(ctx)
    _settings(ctx)
    _select(ctx, "Theme", ["Prisma AIRS", "Gruvbox"], original_theme)
    _select(ctx, "Appearance", options, original_appearance)
    ctx.record("C49-N02", {"four_palette_controls_observed": len(captures) >= 4,
        "alternate_theme_and_appearance_survived_restart": persisted,
        "desktop_add_portal_opened_in_both_themes": all(desktop_portals) and len(desktop_portals) == 2,
        "guest_academy_in_page_search_focus_observed_in_both_desktop_themes": all(academy_inputs) and len(academy_inputs) == 2,
        "guest_academy_reader_and_contents_portal_opened_in_both_themes": all(portal_observed) and len(portal_observed) == 2},
        evidence, captures)


def _tenant_controls(ctx, bootstrap):
    evidence, captures = [], []
    _settings(ctx)
    selected_before = _value(ctx, "AXPopUpButton", "Tenant to update")
    ctx.require(selected_before == bootstrap + " (active)", "C49 must begin on its owned bootstrap tenant")
    client_before = _value(ctx, "AXTextField", "Client ID")
    # Keep the management client intact: later C46 warning flows use it.
    _node(ctx, "AXButton", "Replace client")
    _node(ctx, "AXButton", "Remove management client")
    storage_choices = []
    for nth in (0, 1):  # Scan and management cards expose both storage choices.
        tree = _radio(ctx, "For this session only", nth)
        storage_choices.append((ctx.app.node(tree, "AXRadioButton", "For this session only", nth) or {}).get("value") in (1, "1"))
        tree = _radio(ctx, "In secure storage", nth)
        storage_choices.append((ctx.app.node(tree, "AXRadioButton", "In secure storage", nth) or {}).get("value") in (1, "1"))
    ctx.app.type_into("AXTextField", "Client secret", "synthetic-c49-unsaved-secret", secure=True)
    ctx.navigate("Home")
    _settings(ctx)
    cleared_on_leave = _value(ctx, "AXTextField", "Client secret") == ""
    ctx.require(cleared_on_leave, "unsaved management secret survived leaving Settings")

    # Actual credential writes are confined to the fixture vault/session. No scan
    # or product request is made. Remove the key again before restoring Home.
    _radio(ctx, "For this session only", 0)
    for index, button in enumerate(("Save key", "Replace key"), start=1):
        ctx.app.type_into("AXTextField", "AIRS API key", f"synthetic-c49-session-key-{index}", secure=True)
        ctx.press("AXButton", button)
        tree = ctx.text("Scan key held for this session only.")
        ctx.require(_value(ctx, "AXTextField", "AIRS API key") == "", "submitted scan secret remained visible")
        evidence.append(ctx.save(f"c49-session-key-{index}", tree))
    ctx.press("AXButton", "Remove scan key")
    tree = ctx.text("No scan key.")
    evidence.append(ctx.save("c49-session-key-removed", tree))

    ctx.press("AXButton", "Change destination…")
    _radio(ctx, "Custom HTTPS endpoint")
    custom_destination = _node(ctx, "AXTextField", "Endpoint origin") is not None
    tree = ctx.text("HTTPS only, with no path, query or credentials.")
    evidence.append(ctx.save("c49-custom-destination-review-surface", tree))
    captures.append(ctx.capture("c49-custom-destination-card"))
    ctx.press("AXButton", "Cancel")

    ctx.press("AXButton", "Edit")
    original_environment = _value(ctx, "AXPopUpButton", "Environment")
    tree = ctx.app.tree()
    metadata_only = ctx.app.node(tree, "AXTextField", "TSG ID") is None and ctx.app.node(tree, "AXTextField", "Endpoint origin") is None
    ctx.require(metadata_only, "metadata editor unexpectedly exposes tenant identity/destination")
    _replace_text(ctx, "Display name", bootstrap + " Edited")
    _select(ctx, "Environment", ["Production", "Staging", "Other"], "Staging" if original_environment != "Staging" else "Other")
    ctx.press("AXButton", "Save changes")
    ctx.text(bootstrap + " Edited")
    evidence.append(ctx.save("c49-metadata-edited"))
    ctx.press("AXButton", "Edit")
    _replace_text(ctx, "Display name", bootstrap)
    _select(ctx, "Environment", ["Production", "Staging", "Other"], original_environment)
    ctx.press("AXButton", "Save changes")
    ctx.text(bootstrap + " (active)")
    ctx.press("AXButton", "Delete")
    tree = ctx.app.tree()
    bound_delete = ctx.app.node(tree, "AXButton", f"Confirm deleting {bootstrap} and its credentials") is not None
    ctx.require(bound_delete, "Delete confirmation did not name the selected tenant")
    evidence.append(ctx.save("c49-delete-target-confirmation", tree))
    ctx.press("AXButton", "Cancel")

    ctx.press("AXButton", "Add tenant…")
    tree = ctx.text("Add a tenant")
    region_only = ctx.app.node(tree, "AXRadioButton", "Custom HTTPS endpoint") is None and ctx.app.node(tree, "AXPopUpButton", "Region") is not None
    _replace_text(ctx, "Display name", "P11 Cancelled Draft")
    _replace_text(ctx, "TSG ID", "1234567899")
    ctx.press("AXButton", "Draft tenant")
    tree = ctx.text("P11 Cancelled Draft is drafted.")
    blocked = _node(ctx, "AXButton", "Save the management client to continue", tree).get("enabled") is False
    ctx.require(region_only and blocked, "Add must use region-only draft with required management client")
    ctx.app.type_into("AXTextField", "Client secret", "synthetic-c49-cancelled-draft-secret", secure=True)
    evidence.append(ctx.save("c49-add-required-oauth-draft"))
    captures.append(ctx.capture("c49-add-required-oauth-draft"))
    ctx.press("AXButton", "Cancel")
    tree, cancelled = ctx.app.wait_for(lambda t: ctx.app.node(t, "AXTextField", "TSG ID") is None)
    ctx.require(cancelled, "Add draft did not close after cancellation")
    selected_after = _value(ctx, "AXPopUpButton", "Tenant to update")
    management_retained = _value(ctx, "AXTextField", "Client ID") == client_before and ctx.app.node(tree, "AXButton", "Remove management client") is not None
    restored = selected_after == selected_before
    ctx.press("AXButton", "Edit")
    restored_environment = _value(ctx, "AXPopUpButton", "Environment") == original_environment
    ctx.press("AXButton", "Cancel")
    ctx.require(restored and management_retained, "C49 did not restore bootstrap profile/management controls")
    ctx.record("C49-N03", {"scan_and_management_secure_session_choices_selectable": all(storage_choices) and len(storage_choices) == 4,
        "scan_session_save_replace_remove_observed": ctx.app.has_text(tree, "No scan key."),
        "unsaved_management_secret_cleared_on_navigation": cleared_on_leave,
        "editor_exposed_metadata_fields_only": metadata_only,
        "selected_profile_label_and_environment_restored": restored and restored_environment,
        "delete_confirmation_named_selected_target_and_was_cancelled": bound_delete,
        "custom_https_controls_retained_in_destination_card": custom_destination,
        "add_offered_region_only": region_only,
        "add_draft_required_management_before_continue_and_cancelled": blocked and cancelled,
        "bootstrap_management_controls_retained": management_retained}, evidence, captures)


def run(ctx):
    """Observe C49 against the orchestrator's already bootstrapped fixture only."""
    bootstrap = ctx.state["bootstrap_tenant"]
    tree = _settings(ctx)
    original_window = _node(ctx, "AXWindow", "", tree).get("size")
    original_appearance = _value(ctx, "AXPopUpButton", "Appearance")
    original_theme = _value(ctx, "AXPopUpButton", "Theme")
    requirements = ctx.save("c49-source-boundary-requirements", SOURCE_REQUIREMENTS)
    ctx.state["c49_source_requirements"] = SOURCE_REQUIREMENTS
    cards = ("Appearance", "Academy account", "Diagnostics", "Tenants", "Scan key", "Management client",
             "Scan destination", "Update Tenant", "Add Tenant")
    present = all(ctx.app.has_text(tree, card) for card in cards)
    ctx.require(present, "C49 Settings cards missing")
    captures = [ctx.capture("c49-settings-cards-100-percent")]
    evidence = [requirements, ctx.save("c49-settings-cards", tree)]
    _resize(ctx, 1100, 800)
    _zoom(ctx, doubled=True)
    keyboard_evidence, keyboard_reached = _keyboard_cards(ctx)
    evidence.append(keyboard_evidence)
    captures.append(ctx.capture("c49-settings-keyboard-200-percent"))
    ctx.record("C49-N01", {"expected_settings_and_tenant_cards_present": present,
        "independent_appearance_theme_controls_present": bool(ctx.app.node(tree, "AXPopUpButton", "Appearance") and ctx.app.node(tree, "AXPopUpButton", "Theme")),
        "changed_card_controls_keyboard_reachable_inside_narrow_window_at_requested_200_percent": keyboard_reached}, evidence, captures)
    _zoom(ctx)
    _themes(ctx, original_appearance, original_theme)
    _tenant_controls(ctx, bootstrap)
    _zoom(ctx)
    if original_window:
        _resize(ctx, *original_window)
    ctx.navigate("Home")
    ctx.text(bootstrap)
