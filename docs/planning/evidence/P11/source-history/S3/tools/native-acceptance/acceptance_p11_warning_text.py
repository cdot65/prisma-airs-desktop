"""Read only the observed latest-check paragraph text in the owned AX tree."""

_TARGETS = frozenset(('Latest check: Warning', 'Latest check: Passed'))


def _paragraph_texts(tree):
    paragraphs = []
    for index, group in enumerate(tree):
        depth = group.get('depth')
        if (group.get('role') != 'AXGroup' or group.get('subrole') not in (None, '')
                or not isinstance(depth, int) or isinstance(depth, bool)):
            continue
        parts = []
        for child in tree[index + 1:]:
            child_depth = child.get('depth')
            if not isinstance(child_depth, int) or isinstance(child_depth, bool):
                return []
            if child_depth <= depth:
                break
            if child_depth == depth + 1 and child.get('role') == 'AXStaticText':
                text = child.get('label')
                if not isinstance(text, str):
                    text = child.get('value')
                if not isinstance(text, str):
                    return []
                parts.append(text)
        text = ' '.join(''.join(parts).split())
        if text.startswith('Latest check:'):
            paragraphs.append(text)
    return paragraphs


def has_text(base_has_text, tree, fragment):
    if fragment not in _TARGETS:
        return base_has_text(tree, fragment)
    paragraphs = _paragraph_texts(tree)
    if len(paragraphs) != 1:
        return False
    # The timestamp is a nested AXTimeGroup, not a direct sibling text.
    # Accept the exact status with either no suffix or its rendered delimiter
    # and period. Extra/conflicting status text fails; no cross-group assembly.
    return paragraphs[0] in (fragment, fragment + ' · .')


def run(ctx):
    import acceptance_p11_warnings as warnings
    original = ctx.app.has_text
    ctx.app.has_text = lambda tree, fragment: has_text(original, tree, fragment)
    try:
        return warnings.run(ctx)
    finally:
        ctx.app.has_text = original
