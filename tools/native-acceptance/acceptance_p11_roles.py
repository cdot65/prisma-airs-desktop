"""One observed WebKit role adaptation; no actions run at import time."""


def press(ctx, base_press, role, label, timeout=12):
    if (role, label) != ('AXButton', 'Contents'):
        return base_press(role, label, timeout)
    tree = ctx.app.tree()  # Existing App guard binds this read to the owned PID.
    # Keep the original Button/prefix semantics whenever that target exists.
    if any(n.get('role') == role and (n.get('label') or '').startswith(label) for n in tree):
        return base_press(role, label, timeout)
    popups = [n for n in tree if n.get('role') == 'AXPopUpButton' and n.get('label') == label]
    ctx.require(len(popups) == 1 and popups[0].get('enabled') is True,
                'Contents requires one exact enabled owned AXPopUpButton when AXButton is absent')
    return base_press('AXPopUpButton', label, timeout)
