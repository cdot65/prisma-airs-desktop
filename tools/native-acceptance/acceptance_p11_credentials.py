"""Scoped scan-session precondition for the unfinished C49 credential flow."""
import acceptance_p11_settings as settings


def run(ctx, bootstrap):
    # The finish driver removes any leftover fixture scan key before entry.
    # Credential saves rotate context and remount the form with keychain as
    # its default, so select session anew before EACH synthetic secret entry.
    original = ctx.app.type_into
    submitted = 0

    def type_into(role, label, text, *args, **kwargs):
        nonlocal submitted
        if (role, label) == ('AXTextField', 'AIRS API key') and isinstance(text, str) and text.startswith('synthetic-c49-session-key-'):
            ctx.require(submitted < 2 and text == f'synthetic-c49-session-key-{submitted + 1}',
                        'unexpected C49 synthetic scan-key sequence')
            settings._radio(ctx, 'For this session only', 0)  # includes selected-state verification
            submitted += 1
            ctx.log.append({'action': 'c49-scan-session-mode-before-input', 'ordinal': submitted})
        return original(role, label, text, *args, **kwargs)

    ctx.app.type_into = type_into
    try:
        result = settings._tenant_controls(ctx, bootstrap)
        ctx.require(submitted == 2, 'C49 requires both session-key submissions')
        return result
    finally:
        ctx.app.type_into = original
