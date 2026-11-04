"""Mandatory C32 native facts; absent facts never count as coverage."""
BOOLEAN_FACTS = {
    'n2': ('project', 'asked', 'names_call', 'guard_switcher', 'guard_isolated', 'guard_focus_returned', 'guard_page_back', 'guard_palette', 'guard_menu', 'still_tenant_a', 'decision_kept', 'declined', 'approved', 'may_change_shown', 'native_sheet', 'refused_kept_pending', 'ran_after_native'),
    'n3': ('read_asked', 'read_only_shown', 'read_done', 'staged', 'review', 'proposed_title', 'apply_gated', 'closed', 'ended', 'rail_ended', 'ended_announced', 'engine_gone', 'new_conversation'),
    'n4': ('send_reached',),
}
MOTION = ('active', 'harness_ready', 'answered', 'toast', 'persists', 'dismissed', 'restored')


def problems(obs):
    errors = []
    for group, fields in BOOLEAN_FACTS.items():
        for field in fields:
            if obs.get(group, {}).get(field) is not True:
                errors.append(group + '.' + field)
    n2, n3, n4 = (obs.get(n, {}) for n in ('n2', 'n3', 'n4'))
    if n2.get('guard_initial_focus') != 'Stay': errors.append('guard initial focus')
    if set(n2.get('guard_buttons', [])) != {'Stay', 'Discard and switch', 'Archive and switch'}: errors.append('guard buttons')
    for field in ('writes_after_staging', 'writes_after_review'):
        if n3.get(field) != 0: errors.append(field)
    for field in MOTION:
        if n4.get('reduce_motion', {}).get(field) is not True: errors.append('reduce_motion.' + field)
    zoom = n4.get('zoom', {})
    for theme in ('light', 'dark'):
        row = zoom.get(theme, {})
        if row.get('overflow_px') != 0 or row.get('horizontal_scrollbars') != 0: errors.append('layout.' + theme)
    ring = n4.get('ring', {})
    if ring.get('focused') != 'New investigation' or ring.get('diff', 0) <= 150: errors.append('new investigation focus ring')
    return errors
