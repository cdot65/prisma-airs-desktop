"""C47-N01: retain the frozen matrix and measure only its two unfinished API focus paths."""
import copy
import hashlib
import json
from pathlib import Path

import acceptance_c29 as c29
import acceptance_p11_shell as shell
import acceptance_p11_geometry as geometry

ROOT = Path.cwd().resolve()
FLAGS = ('one_named_header_h1', 'navigation_focus', 'main_inside_window', 'academy_location')
EXPECTED_FACTS = {
    'all_destinations_have_one_named_header_h1': True,
    'navigation_focus_observed': True,
    'main_frames_measured_inside_window': True,
    'named_actions_keyboard_reachable': False,
    'academy_hierarchy_retained': True,
}
VIEWS = (('Light', 1440, 820, 0, 100), ('Dark', 900, 600, 10, 200))


def require(value, message):
    if not value:
        raise RuntimeError(message)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def bound(root, ref):
    path = (root / ref['path']).resolve()
    require(path.is_relative_to(root.resolve()) and sha(path) == ref['sha256'],
            'prior keyboard evidence escapes or changed')
    return path


def inspect_prior(ctx, prior, root=ROOT):
    raw_path = bound(root, prior['observations'])
    matrix_path = bound(root, prior['measurements'])
    raw = json.loads(raw_path.read_text())
    require(raw['schema'] == 'airs-desktop/p11-native-observations/1' and raw['subject'] == 'P11'
            and raw['status'] == 'failed' and raw['exit_code'] == 1 and raw['complete'] is False,
            'original failed matrix process must remain failed')
    require(raw['binding']['helper_closure'] == raw['helper_closure']
            and all(raw[key] == raw['binding'][key] == ctx.prepared[key]
                    for key in ('executable_sha256', 'bundle_sha256'))
            and raw['binding']['prepare']['sha256'] == ctx.binding['prepare']['sha256']
            and raw['binding']['native_inventory_sha256'] == ctx.binding['native_inventory_sha256']
            and raw['host'] == ctx.binding['conditions']['platform'],
            'prior matrix candidate, platform or binding changed')
    for name, digest in raw['helper_closure'].items():
        file = (root / name).resolve()
        require(file.is_relative_to(root) and file.is_file()
                and ctx.closure.get(name) == digest == sha(file), 'prior matrix helper bytes changed')
    for name, digest in raw['artifact_sha256'].items():
        artifact = (root / name).resolve()
        require(artifact.is_relative_to(raw_path.parent) and sha(artifact) == digest,
                'prior matrix artifact changed')
    rows = [row for row in raw['checks'] if row['id'] == 'C47-N01']
    require(len(rows) == 1 and rows[0]['facts'] == EXPECTED_FACTS,
            'prior N01 had a different failed fact')
    row = rows[0]
    relative = str(matrix_path.relative_to(root))
    require(relative in row['evidence'] and raw['artifact_sha256'].get(relative) == prior['measurements']['sha256'],
            'matrix is not bound by the original N01')
    matrix = json.loads(matrix_path.read_text())
    expected = {(page, theme, zoom) for page in shell.PAGES for theme, _, _, _, zoom in VIEWS}
    require(len(matrix) == 16 and {(r['page'], r['theme'], r['zoom']) for r in matrix} == expected,
            'frozen matrix membership changed')
    for item in matrix:
        require(all(item.get(flag) is True for flag in FLAGS), 'frozen non-keyboard measurement failed')
        require(item['requested'] == ([1440, 820] if item['theme'] == 'Light' else [900, 600]),
                'frozen matrix viewport changed')
        require(item['named_action_keyboard_visible'] is (item['page'] != 'API Client'),
                'frozen matrix has an unexpected keyboard result')
    require(len(row['visual']) == len(set(row['visual'])) == 16
            and all(name in raw['artifact_sha256'] for name in row['visual']),
            'original matrix captures missing')
    return raw, row, matrix


class TraversalFailure(RuntimeError):
    def __init__(self, reason, record):
        super().__init__('API keyboard traversal failed: ' + reason)
        self.record = dict(record, failure=reason)


def focus_key(tree, node):
    semantic = lambda value: (value.get('role'), value.get('label'), value.get('subrole'), value.get('depth'))
    peers = [value for value in tree if semantic(value) == semantic(node)]
    return (*semantic(node), next((i for i, value in enumerate(peers) if value is node), 0))


def traverse(initial, tab, tree_now, focused, frame):
    """One finite measured focus cycle, bounded by the actual AX tree size."""
    collection = [n for n in initial if n.get('role') == 'AXPopUpButton'
                  and (n.get('label') or '').startswith('Collection ')]
    require(len(collection) == 1, 'API Collection selector is missing or ambiguous')
    require(sum(n.get('role') == 'AXPopUpButton' and n.get('label') == 'Environment'
                for n in initial) == 1, 'API Environment selector is missing or ambiguous')
    labels = {'collection': ('AXPopUpButton', collection[0]['label']),
              'environment': ('AXPopUpButton', 'Environment')}
    limit = 2 * max(1, len(initial))
    trace, seen, reached = [], set(), {}
    record = {'initial_ax_elements': len(initial), 'element_bound': limit,
              'expected_header_selectors': labels, 'trace': trace, 'reached': reached}
    for step in range(1, limit + 1):
        tab()
        tree = tree_now()
        node = focused(tree)
        if not node:
            trace.append({'step': step, 'focused': None})
            continue
        key = focus_key(tree, node)
        if key in seen:
            raise TraversalFailure('focus_cycle', record)
        seen.add(key)
        viewport = frame()
        inside = bool(node.get('enabled') is not False and node.get('size')
                      and all(value > 0 for value in node['size']) and c29.inside(viewport, node))
        entry = {'step': step, 'role': node.get('role'), 'label': node.get('label'),
                 'pos': node.get('pos'), 'size': node.get('size'), 'window': viewport,
                 'inside_window': inside}
        trace.append(entry)
        pair = (node.get('role'), node.get('label'))
        goals = [name for name, expected in labels.items() if pair == expected]
        if pair in (('AXTextField', 'URL'), ('AXButton', 'Send')):
            goals.append('request_action')
        for goal in goals:
            if not inside:
                raise TraversalFailure('focused_goal_outside_window', record)
            reached[goal] = dict(entry)
        if set(reached) == {'collection', 'environment', 'request_action'}:
            record['steps'] = step
            return record, node
    raise TraversalFailure('ax_element_bound', record)


def run(ctx, prior):
    raw, old_row, matrix = inspect_prior(ctx, prior)
    proof = ctx.save('c47-keyboard-prior-binding', {
        'prior_raw': prior['observations'], 'prior_measurements': prior['measurements'],
        'original_execution_id': raw['execution_id'], 'original_process_exit_code': raw['exit_code'],
        'original_failed_facts': old_row['facts'],
        'disposition': 'Retain original non-keyboard and fourteen keyboard measurements; '
                       'replace only the two failed API keyboard measurements with new observations.'})
    corrected = copy.deepcopy(matrix)
    evidence, captures, replacements = [prior['measurements']['path'], proof], [], []
    for theme, width, height, zoom, percent in VIEWS:
        name = 'c47-api-keyboard-' + theme.lower() + '-' + str(percent)
        ctx.app.menu('View', 'Actual Size')
        ctx.app.front()
        geometry.size_window(ctx, width, height)
        c29.appearance(ctx.app, theme)
        if zoom:
            c29.zoom(ctx.app, zoom)
        ctx.navigate('API Client')
        ctx.app.ax('focus', 'AXHeading', 'API Client')
        initial = ctx.app.tree()
        start = ctx.app.focused(initial) or {}
        ctx.require(start.get('role') == 'AXHeading' and start.get('label') == 'API Client',
                    'API header did not receive actual native focus')
        try:
            trace, node = traverse(initial, lambda: ctx.app.keycode(48), ctx.app.tree,
                                   ctx.app.focused, ctx.frame)
        except TraversalFailure as error:
            ctx.save(name + '-trace', error.record)
            raise
        trace_ref = ctx.save(name + '-trace', trace)
        evidence += [trace_ref, ctx.save(name + '-focused', ctx.app.tree())]
        captures.append(ctx.capture(name))
        item = next(r for r in corrected if r['page'] == 'API Client'
                    and r['theme'] == theme and r['zoom'] == percent)
        item['named_action_keyboard_visible'] = True
        item['focused_action'] = node
        replacements.append({'page': 'API Client', 'theme': theme, 'zoom': percent,
                             'prior_keyboard_visible': False, 'new_trace': trace_ref,
                             'collection_and_environment_keyboard_visible': True})
    evidence.append(ctx.save('c47-keyboard-supplement-measurements', {
        'prior_raw': prior['observations'], 'prior_measurements': prior['measurements'],
        'replacements': replacements, 'derived_measurements': corrected,
        'meaning': 'New derived N01 evidence. The original failed raw row and matrix are unchanged.'}))
    facts = {'all_destinations_have_one_named_header_h1': all(r['one_named_header_h1'] for r in corrected),
             'navigation_focus_observed': all(r['navigation_focus'] for r in corrected),
             'main_frames_measured_inside_window': all(r['main_inside_window'] for r in corrected),
             'named_actions_keyboard_reachable': all(r['named_action_keyboard_visible'] for r in corrected),
             'academy_hierarchy_retained': all(r['academy_location'] for r in corrected),
             'api_header_selectors_keyboard_visible': len(replacements) == 2}
    ctx.app.menu('View', 'Actual Size')
    ctx.resize(1440, 900)
    return {'id': 'C47-N01', 'facts': facts, 'evidence': evidence,
            'visual': [*old_row['visual'], *captures]}
