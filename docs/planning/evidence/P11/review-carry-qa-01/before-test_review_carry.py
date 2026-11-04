"""Pure review-carry adversarial fixtures; never actual review evidence."""
from pathlib import Path
import copy
import json
import tempfile
import unittest
from unittest.mock import patch

import review_carry as carry
import review


class ReviewCarryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.name = 'apps/desktop/src/v2/workbench/api-client.tsx'
        self.patch = patch.object(carry, 'RETRY_DELTA', {self.name: ('old', 'new')})
        self.patch.start(); self.addCleanup(self.patch.stop)

    def test_only_exact_declared_retry_delta_can_carry(self):
        before = {self.name: {'sha256': 'old'}, 'Cargo.lock': {'sha256': 'same'}}
        after = {self.name: {'sha256': 'new'}, 'Cargo.lock': {'sha256': 'same'}}
        self.assertEqual(carry.impact_delta(before, after, [self.name]), [self.name])
        for kind in ('undeclared', 'different-fix', 'unrelated-behavior', 'configuration', 'deleted'):
            changed, declared = copy.deepcopy(after), [self.name]
            if kind == 'undeclared': declared = []
            elif kind == 'different-fix': changed[self.name]['sha256'] = 'other'
            elif kind == 'unrelated-behavior':
                changed['apps/desktop/src/v2/settings.tsx'] = {'sha256': 'other'}
                declared = sorted([self.name, 'apps/desktop/src/v2/settings.tsx'])
            elif kind == 'configuration':
                changed['Cargo.lock']['sha256'] = 'other'; declared = sorted([self.name, 'Cargo.lock'])
            else: changed.pop(self.name)
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                carry.impact_delta(before, changed, declared)

    def test_dependency_closure_keeps_all_resources_and_unmodified_consumers(self):
        names = [self.name, 'resources/harness/pin', 'resources/another-package.bin',
                 'apps/desktop/src/app.tsx', 'crates/desktop-core/tests/harness_tools/main.rs',
                 'packages/desktop-contracts/src/index.ts', 'Cargo.lock']
        selected = carry.dependencies(dict.fromkeys(names, {}))
        self.assertNotIn(self.name, selected)
        self.assertEqual(set(selected), set(names)-{self.name})
        files = {n: {'sha256': 'unchanged-'+n} for n in names}
        identities = {n: files[n] for n in selected}
        carry.unchanged(files, files, selected, identities)
        altered = copy.deepcopy(files); altered['resources/another-package.bin']['sha256'] = 'changed'
        with self.assertRaises(ValueError): carry.unchanged(files, altered, selected, identities)
        with self.assertRaises(ValueError): carry.unchanged(files, files, selected[:-1], identities)

    def test_qualification_and_subject_set_are_bound(self):
        qualified = {'record': 'fixture-current', 'verified': {'qualified': True}}
        original = {'record': carry.OLD_QUALIFICATION, 'sha256': carry.OLD_QUALIFICATION_SHA}
        record = {'schema': 'airs-desktop/p11-member-review-carry/1',
                  'members': dict.fromkeys(carry.CARRIED, {}), 'current_qualification': qualified,
                  'original_qualification': original}
        self.assertEqual(carry.qualification_bindings(record, qualified), original)
        for kind in ('affected-subject', 'phase-subject', 'old-qualification', 'current-qualification', 'unqualified'):
            altered = copy.deepcopy(record)
            if kind == 'affected-subject': altered['members']['C48'] = altered['members'].pop('C49')
            elif kind == 'phase-subject': altered['members']['P11'] = altered['members'].pop('C49')
            elif kind == 'old-qualification': altered['original_qualification']['sha256'] = 'foreign'
            elif kind == 'current-qualification': altered['current_qualification']['record'] = 'foreign'
            else: altered['current_qualification']['verified']['qualified'] = False
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                carry.qualification_bindings(altered, qualified)

    def test_original_full_nine_member_cannot_be_rescored_or_retargeted(self):
        qualification = {'record': 'fixture-original-qualification'}
        scores = dict.fromkeys(('architecture', 'behavior', 'authority_security', 'validation', 'overall'), 9)
        original = {'subject': 'C46', 'attempt': 'attempt-02', 'scores': scores,
                    'verdict_sha256': 'fixture-original-verdict'}
        entry = {'review': original, 'reason': 'Pure validator fixture: scoped C48-only retry change.'}
        folder = self.root/'docs/planning/reviews/C46/attempt-02'; folder.mkdir(parents=True)
        request = {'subject': 'C46', 'qualification': qualification}
        def save(value): (folder/'request.json').write_text(json.dumps(value))
        save(request)
        with patch.object(carry, 'actual_review', return_value=original) as actual:
            self.assertEqual(carry.member(self.root, 'C46', entry, qualification, self.root/'old-source'), original)
            self.assertFalse(actual.call_args.kwargs['evidence'])
        for kind in ('C48', 'P11', 'wrong-subject', 'wrong-qualification', 'changed-original', 'score-eight'):
            altered, outcome, subject = copy.deepcopy(entry), copy.deepcopy(original), 'C46'; save(request)
            if kind in ('C48', 'P11'): subject = kind
            elif kind == 'wrong-subject': altered['review']['subject'] = 'C47'
            elif kind == 'wrong-qualification': save(dict(request, qualification={'record': 'foreign'}))
            elif kind == 'changed-original': outcome['verdict_sha256'] = 'changed'
            else: altered['review']['scores']['overall'] = outcome['scores']['overall'] = 8
            with self.subTest(kind=kind), patch.object(carry, 'actual_review', return_value=outcome), self.assertRaises(ValueError):
                carry.member(self.root, subject, altered, qualification, self.root/'old-source')

    def test_phase_packet_describes_mixed_original_members_without_retagging(self):
        # Temporary metadata-only fixture; no actual review/qualification is fabricated.
        folder = self.root/'docs/planning/evidence/P11'; folder.mkdir(parents=True)
        qualified = {'record': 'fixture-qualified', 'meaning': 'pure validator fixture'}
        qualification = self.root/'qualification.json'
        qualification.write_text(json.dumps({'runner_inputs': {'dependencies': 'fixture', 'build_flags': 'fixture'}}))
        carry_path = self.root/'carry.json'; carry_path.write_text('pure validator fixture')
        with patch.object(review.proof, 'verify', return_value=qualified), \
             patch.object(review, 'predecessors', return_value={'meaning': 'pure predecessor fixture'}), \
             patch.object(review.proof, 'source_inventory', return_value={}), \
             patch.object(review.subprocess, 'run'):
            request = review.make(self.root, 'P11', 'attempt-01', qualification, review_carry=carry_path)
        message = (self.root/'docs/planning/reviews/P11/attempt-01/message.txt').read_text()
        self.assertEqual(request['review_carry'], review.proof.reference(self.root, carry_path))
        self.assertIn('qualification03', request['limitations'][1])
        self.assertIn('three explicitly carried original qualification03', message)
        self.assertIn('do not relabel those members current', message)


if __name__ == '__main__':
    unittest.main()
