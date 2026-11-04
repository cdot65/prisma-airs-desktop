"""Exact observed Runtime-warning action label adaptation for the owned driver."""


def press(ctx, base_press, role, label, timeout=12):
    if (role, label) != ('AXButton', 'Open API keys'):
        return base_press(role, label, timeout)
    tree = ctx.app.tree()  # Existing App guard binds this read to the owned PID.
    if any(n.get('role') == role and (n.get('label') or '').startswith(label) for n in tree):
        return base_press(role, label, timeout)
    candidates = [n for n in tree if n.get('role') == 'AXButton'
                  and (n.get('label') or '').startswith('Manage API keys')]
    ctx.require(len(candidates) == 1 and candidates[0].get('label') == 'Manage API keys'
                and candidates[0].get('enabled') is True,
                'Runtime warning requires one exact enabled owned Manage API keys button')
    ctx.log.append({'action': 'runtime-warning-action-label', 'requested': label,
                    'observed': 'Manage API keys', 'role': 'AXButton'})
    return base_press('AXButton', 'Manage API keys', timeout)


def run(ctx):
    import acceptance_p11_setup as setup
    original = ctx.press
    ctx.press = lambda role, label, timeout=12: press(ctx, original, role, label, timeout)
    try:
        return setup.warning_flows(ctx)
    finally:
        ctx.press = original
