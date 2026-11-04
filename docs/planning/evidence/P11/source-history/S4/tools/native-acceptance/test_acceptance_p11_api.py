"""Pure replay of native AX evidence; never launches apps or endpoints."""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import unittest

import acceptance_p11_api as api

RAW_TREE = Path('docs/planning/evidence/P11/native-attempt-06-vm/failure-tree.json')

class App:
    def __init__(self, tree): self.value = tree
    def wait_for(self, predicate, timeout): return self.value, bool(predicate(self.value))
    @staticmethod
    def has_text(tree, text): return any(text in str(n.get('label') or '') or text in str(n.get('value') or '') for n in tree)

class Context:
    def __init__(self, tree): self.app = App(tree); self.state = {}; self.saved = []
    def save(self, name, value): self.saved.append((name, value)); return 'new-attempt/' + name + '.json'
    @staticmethod
    def require(condition, message):
        if not condition: raise RuntimeError(message)

class FooterReplay(unittest.TestCase):
    @classmethod
    def setUpClass(cls): cls.tree = json.loads(RAW_TREE.read_text())

    def test_captured_split_footer_names_current_record(self):
        self.assertIs(api._metrics(Context(self.tree), 'My request', 200), self.tree)

    def test_wrong_request_is_not_accepted(self):
        with self.assertRaises(RuntimeError): api._metrics(Context(self.tree), 'Other request', 200)

    def test_wrong_status_is_not_accepted(self):
        with self.assertRaises(RuntimeError): api._metrics(Context(self.tree), 'My request', 201)

    def test_current_record_is_not_history(self):
        with self.assertRaises(RuntimeError): api._metrics(Context(self.tree), 'My request', 200, True)

    def test_history_prefix_keeps_its_own_binding(self):
        tree = copy.deepcopy(self.tree)
        node = next(n for n in tree if n.get('label') == 'My request')
        node['label'] = node['value'] = 'History · My request'
        self.assertIs(api._metrics(Context(tree), 'My request', 200, True), tree)
        with self.assertRaises(RuntimeError): api._metrics(Context(tree), 'My request', 200, False)

    def test_status_from_another_region_cannot_satisfy_name(self):
        tree = [
            {'depth': 0, 'subrole': 'AXApplicationStatus'},
            {'depth': 1, 'label': 'My request'},
            {'depth': 1, 'label': ' · '},
            {'depth': 1, 'label': '201'},
            {'depth': 0, 'subrole': 'AXApplicationStatus'},
            {'depth': 1, 'label': 'Other request · 15:23'},
            {'depth': 1, 'label': '200 OK'},
        ]
        with self.assertRaises(RuntimeError): api._metrics(Context(tree), 'My request', 200)
        self.assertIs(api._metrics(Context(tree), 'My request', 201), tree)

class FirstUseCarry(unittest.TestCase):
    def setUp(self):
        observations = RAW_TREE.parent / 'observations.json'
        evidence = RAW_TREE.parent / 'c48-first-use.json'
        self.binding = {kind: {'path': path.as_posix(), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}
                        for kind, path in [('observations', observations), ('evidence', evidence)]}
        original = json.loads(observations.read_text())
        self.ctx = Context([])
        self.ctx.prepared = {key: original[key] for key in ('executable_sha256', 'bundle_sha256')}

    def test_actual_first_use_keeps_original_provenance(self):
        passed, local = api._prior_first_use(self.ctx, self.binding)
        self.assertTrue(passed)
        self.assertEqual(local, 'new-attempt/c48-first-use-carry.json')
        carry = self.ctx.saved[-1][1]
        self.assertEqual(carry['observations'], self.binding['observations'])
        self.assertEqual(carry['evidence'], self.binding['evidence'])
        self.assertEqual(carry['original']['execution_id'], RAW_TREE.parent.name)
        self.assertIn('not a new first-use execution', carry['meaning'])

    def test_hash_drift_is_rejected(self):
        self.binding['evidence']['sha256'] = '0' * 64
        with self.assertRaises(RuntimeError): api._prior_first_use(self.ctx, self.binding)
        self.assertEqual(self.ctx.saved, [])

    def test_other_application_artifact_is_rejected(self):
        self.ctx.prepared['executable_sha256'] = '0' * 64
        with self.assertRaises(RuntimeError): api._prior_first_use(self.ctx, self.binding)
        self.assertEqual(self.ctx.saved, [])

    def test_non_first_use_tree_is_rejected(self):
        self.binding['evidence'] = {'path': RAW_TREE.as_posix(), 'sha256': hashlib.sha256(RAW_TREE.read_bytes()).hexdigest()}
        with self.assertRaises(RuntimeError): api._prior_first_use(self.ctx, self.binding)
        self.assertEqual(self.ctx.saved, [])

    def test_escape_reference_is_rejected(self):
        self.binding['evidence']['path'] = '../not-P11-evidence.json'
        with self.assertRaises(RuntimeError): api._prior_first_use(self.ctx, self.binding)
        self.assertEqual(self.ctx.saved, [])

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--tree', type=Path, default=RAW_TREE)
    args, remaining = parser.parse_known_args()
    RAW_TREE = args.tree
    unittest.main(argv=[__file__, *remaining])
