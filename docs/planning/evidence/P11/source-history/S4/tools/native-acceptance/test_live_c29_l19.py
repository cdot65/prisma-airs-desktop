"""Offline regressions for live-driver admission; never resolve credentials."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import live_c29_l19 as driver


class Settlement(unittest.TestCase):
    def test_the_actual_pending_tree_is_not_a_finished_verification(self):
        root = Path(__file__).resolve().parents[2]
        pending = json.loads((root / 'docs/planning/evidence/C29/live/L19/attempt-01/wizard-verify.json').read_text())
        self.assertTrue(any(n.get('label') == 'Pending' for n in pending))
        self.assertFalse(driver.verify_finished(pending))

    def test_terminal_success_and_failure_are_finished_but_reading_is_not(self):
        for label in ['Checks finished; nothing failed.', 'Checks finished with warnings.', 'A required check failed.', 'An optional check failed. You can fix it and verify again, or continue with it recorded as a warning.']:
            tree = [{'role': 'AXStaticText', 'label': label}]
            self.assertTrue(driver.verify_finished(tree))
            self.assertFalse(driver.verify_finished(tree + [{'role': 'AXStaticText', 'label': 'Checking…'}]))


class Budget(unittest.TestCase):
    def test_interrupted_first_attempt_stays_charged_and_a_third_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'ledger.jsonl'
            self.assertEqual(driver.reserve_read(path, 'scope', 'attempt-01', 2), 1)
            self.assertEqual(driver.reserve_read(path, 'scope', 'attempt-02', 2), 2)
            before = path.read_bytes()
            with self.assertRaises(AssertionError):
                driver.reserve_read(path, 'scope', 'attempt-03', 2)
            self.assertEqual(path.read_bytes(), before)

    def test_duplicate_attempt_changed_scope_and_one_read_cap_refuse(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'ledger.jsonl'
            driver.reserve_read(path, 'scope', 'attempt-01', 1)
            for scope, attempt, limit in [('scope', 'attempt-01', 2), ('other', 'attempt-02', 2), ('scope', 'attempt-02', 1)]:
                with self.assertRaises(AssertionError):
                    driver.reserve_read(path, scope, attempt, limit)


if __name__ == '__main__':
    unittest.main()
