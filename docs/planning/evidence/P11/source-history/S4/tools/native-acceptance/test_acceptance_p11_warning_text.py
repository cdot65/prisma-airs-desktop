"""Pure replay of actual native11 text fragmentation; no app/endpoint actions."""
import copy
import json
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import patch

import acceptance_p11_warning_text as warning_text

RAW = Path('docs/planning/evidence/P11/native-attempt-11-vm/failure-tree.json')


def old_has_text(tree, fragment):
    texts = [n.get('label') or '' for n in tree]
    texts += [str(n.get('value')) for n in tree if n.get('value') is not None]
    return any(fragment in text for text in texts)


def paragraph(*parts):
    return [{'role': 'AXGroup', 'depth': 9, 'label': '', 'subrole': None},
            *[{'role': 'AXStaticText', 'depth': 10, 'label': part, 'value': part}
              for part in parts]]


class WarningParagraphAdmission(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.raw = json.loads(RAW.read_text())

    def test_actual11_old_matcher_is_red_and_bounded_matcher_is_green(self):
        self.assertFalse(old_has_text(self.raw, 'Latest check: Warning'))
        self.assertTrue(warning_text.has_text(old_has_text, self.raw, 'Latest check: Warning'))
        self.assertFalse(warning_text.has_text(old_has_text, self.raw, 'Latest check: Passed'))

    def test_passed_status_has_same_exact_paragraph_contract(self):
        tree = copy.deepcopy(self.raw)
        index = next(i for i, node in enumerate(tree) if node.get('label') == 'Latest check: ')
        self.assertEqual(tree[index + 1]['label'], 'Warning')
        tree[index + 1].update(label='Passed', value='Passed')
        self.assertTrue(warning_text.has_text(old_has_text, tree, 'Latest check: Passed'))
        self.assertFalse(warning_text.has_text(old_has_text, tree, 'Latest check: Warning'))

    def test_split_across_groups_or_nested_status_is_never_joined(self):
        trees = [
            paragraph('Latest check: ') + paragraph('Warning'),
            [*paragraph('Latest check: '),
             {'role': 'AXGroup', 'depth': 10, 'label': ''},
             {'role': 'AXStaticText', 'depth': 11, 'label': 'Warning'}],
        ]
        for tree in trees:
            with self.subTest(tree=tree):
                self.assertFalse(warning_text.has_text(old_has_text, tree, 'Latest check: Warning'))

    def test_ambiguous_conflicting_wrong_status_and_suffix_are_rejected(self):
        trees = [
            paragraph('Latest check: Warning') + paragraph('Latest check: Warning'),
            paragraph('Latest check: Warning') + paragraph('Latest check: Passed'),
            paragraph('Latest check: ', 'Warning', 'Passed', ' · ', '.'),
            paragraph('Latest check: ', 'Failed', ' · ', '.'),
            paragraph('Latest check: ', 'Warning', ' · ', 'Passed'),
            paragraph('Latest check: Warning extra'),
            [{'role': 'AXHeading', 'depth': 9, 'label': 'Latest check: Warning'}],
        ]
        for tree in trees:
            with self.subTest(tree=tree):
                self.assertFalse(warning_text.has_text(old_has_text, tree, 'Latest check: Warning'))

    def test_normal_unsplit_paragraph_and_other_text_behavior(self):
        self.assertTrue(warning_text.has_text(old_has_text, paragraph('Latest check: Warning'), 'Latest check: Warning'))
        calls = []
        def base(tree, fragment):
            calls.append((tree, fragment))
            return 'unchanged'
        tree = []
        self.assertEqual(warning_text.has_text(base, tree, 'Skill statistics:'), 'unchanged')
        self.assertEqual(calls, [(tree, 'Skill statistics:')])

    def test_scoped_run_preserves_frozen_flow_and_restores_on_failure(self):
        for fail in (False, True):
            app = types.SimpleNamespace(has_text=old_has_text)
            ctx = types.SimpleNamespace(app=app)
            original = app.has_text
            def flow(actual):
                self.assertIs(actual, ctx)
                self.assertTrue(actual.app.has_text(self.raw, 'Latest check: Warning'))
                if fail:
                    raise ValueError('owned-warning-flow-failure')
                return 'flow-result'
            with patch.dict(sys.modules, {'acceptance_p11_warnings': types.SimpleNamespace(run=flow)}):
                if fail:
                    with self.assertRaisesRegex(ValueError, 'owned-warning-flow-failure'):
                        warning_text.run(ctx)
                else:
                    self.assertEqual(warning_text.run(ctx), 'flow-result')
            self.assertIs(app.has_text, original)


if __name__ == '__main__':
    unittest.main()
