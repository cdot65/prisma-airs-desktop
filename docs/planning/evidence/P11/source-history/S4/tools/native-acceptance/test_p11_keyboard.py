"""Pure driver regressions; these fixtures never execute UI or claim native passes."""
import unittest
from unittest.mock import MagicMock, call, patch

import acceptance_p11_keyboard as keyboard
import acceptance_p11_shell as original


def node(role, label, pos=(10, 10)):
    return {'role': role, 'label': label, 'pos': list(pos), 'size': [100, 20],
            'depth': 5, 'focused': False, 'enabled': True}


class FakeApp:
    def __init__(self, nodes):
        self.nodes, self.index = nodes, -1

    def ax(self, *_):
        self.index = -1

    def keycode(self, code):
        assert code == 48
        self.index = (self.index + 1) % len(self.nodes)

    def tree(self):
        for i, item in enumerate(self.nodes):
            item['focused'] = i == self.index
        return self.nodes

    @staticmethod
    def focused(tree):
        return next((item for item in tree if item['focused']), None)


class KeyboardTests(unittest.TestCase):
    @staticmethod
    def frame():
        return [0, 0, 500, 500]

    def walk(self, app):
        return keyboard.traverse(app.tree(), lambda: app.keycode(48), app.tree,
                                 app.focused, self.frame)

    def test_actual_frozen_24_tab_cap_misses_later_request_action(self):
        controls = [node('AXPopUpButton', 'Collection P11 API'),
                    node('AXPopUpButton', 'Environment')]
        controls += [node('AXButton', 'Close tab owned-%02d' % i) for i in range(26)]
        controls += [node('AXTextField', 'URL')]
        app = FakeApp(controls)
        context = type('Context', (), {'app': app, 'frame': self.frame})()
        old_passed, old_focus = original.focus_visible(context, 'API Client')
        self.assertFalse(old_passed)
        self.assertTrue(old_focus['label'].startswith('Close tab'))
        app.ax('focus', 'AXHeading', 'API Client')
        trace, focused = self.walk(app)
        self.assertEqual(trace['steps'], 29)
        self.assertGreater(trace['steps'], 24)
        self.assertEqual(set(trace['reached']), {'collection', 'environment', 'request_action'})
        self.assertEqual(focused['label'], 'URL')
        self.assertTrue(all(row['inside_window'] for row in trace['reached'].values()))

    def test_wrong_label_does_not_count_and_a_real_cycle_stops(self):
        app = FakeApp([node('AXPopUpButton', 'Collection P11 API'),
                       node('AXPopUpButton', 'Environment'), node('AXButton', 'URL details')])
        with self.assertRaises(keyboard.TraversalFailure) as caught:
            self.walk(app)
        self.assertEqual(caught.exception.record['failure'], 'focus_cycle')
        self.assertNotIn('request_action', caught.exception.record['reached'])

    def test_focused_goal_must_have_positive_geometry_inside_actual_window(self):
        for bad in [node('AXPopUpButton', 'Collection P11 API', (600, 10)),
                    dict(node('AXPopUpButton', 'Collection P11 API'), size=[0, 0])]:
            app = FakeApp([bad, node('AXPopUpButton', 'Environment'), node('AXTextField', 'URL')])
            with self.subTest(bad=bad), self.assertRaises(keyboard.TraversalFailure) as caught:
                self.walk(app)
            self.assertEqual(caught.exception.record['failure'], 'focused_goal_outside_window')

    def test_changing_focus_nodes_still_have_finite_ax_element_bound(self):
        initial = [node('AXPopUpButton', 'Collection P11 API'), node('AXPopUpButton', 'Environment')]
        state = {'step': 0}
        def tab():
            state['step'] += 1
        def tree():
            return [dict(node('AXButton', 'unrelated-%s' % state['step']), focused=True)]
        with self.assertRaises(keyboard.TraversalFailure) as caught:
            keyboard.traverse(initial, tab, tree, FakeApp.focused, self.frame)
        self.assertEqual(caught.exception.record['failure'], 'ax_element_bound')
        self.assertEqual(state['step'], 2 * len(initial))


    def test_run_routes_both_viewports_through_measured_geometry_not_old_shell_sizer(self):
        ctx = MagicMock()
        ctx.app.focused.return_value = {'role': 'AXHeading', 'label': 'API Client'}
        ctx.save.side_effect = lambda name, *_: name + '.json'
        ctx.capture.side_effect = lambda name: name + '.png'
        old = {'facts': keyboard.EXPECTED_FACTS, 'visual': []}
        matrix = [dict(page='API Client', theme=theme, zoom=zoom,
                       named_action_keyboard_visible=False, **{flag: True for flag in keyboard.FLAGS})
                  for theme, zoom in [('Light', 100), ('Dark', 200)]]
        prior = {'observations': {'path': 'original.json'}, 'measurements': {'path': 'matrix.json'}}
        with patch.object(keyboard, 'inspect_prior', return_value=(
                {'execution_id': 'pure-route-fixture', 'exit_code': 1}, old, matrix)), \
             patch.object(keyboard, 'traverse', return_value=({}, node('AXTextField', 'URL'))), \
             patch.object(keyboard.c29, 'appearance'), patch.object(keyboard.c29, 'zoom'), \
             patch.object(keyboard.geometry, 'size_window') as measured, \
             patch.object(keyboard.shell, 'size_window', side_effect=AssertionError('old sizing bug replayed')):
            keyboard.run(ctx, prior)
        self.assertEqual(measured.call_args_list, [call(ctx, 1440, 820), call(ctx, 900, 600)])


if __name__ == '__main__':
    unittest.main()
