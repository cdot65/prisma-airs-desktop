"""Historical source/builder independence and strict current reuse regressions."""
from pathlib import Path
import json,tempfile,unittest
from unittest.mock import patch
from candidate import digest,file_identity,bundle_hash,verify_candidate
from qualification import sha,read
from historical import Historical


class ArchiveNegatives(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name);self.base=self.root/'docs/planning/evidence/C45';self.source=self.base/'archive/source';self.source.mkdir(parents=True);(self.source/'helper.py').write_text('recorded helper')
        files={'helper.py':file_identity(self.source/'helper.py')};self.fp=digest(files);seal=self.base/'seal.json';seal.write_text(json.dumps(dict(files=files)))
        self.bundle=self.base/'archive/app';exe=self.bundle/'Contents/MacOS/prisma-airs-desktop';exe.parent.mkdir(parents=True);exe.write_text('recorded executable');self.value=dict(bundle_sha256=bundle_hash(self.bundle),executable_sha256=sha(exe))
        self.context=self.base/'context.json';self.context.write_text(json.dumps(dict(schema='airs-desktop/historical-qualification-context/1',sources={self.fp:dict(path=str(self.source.relative_to(self.root)),seal=dict(path=str(seal.relative_to(self.root)),sha256=sha(seal)))},applications={self.value['bundle_sha256']:dict(path=str(self.bundle.relative_to(self.root)),executable_sha256=self.value['executable_sha256'])})))
    def history(self):return Historical(self.root,dict(path=str(self.context.relative_to(self.root)),sha256=sha(self.context)))
    def test_original_source_and_complete_bundle_validate_without_a_live_checkout(self):
        h=self.history();self.assertEqual(h.source(self.fp),self.source);self.assertEqual(h.application(self.value),self.bundle)
    def test_changed_missing_or_extra_archived_source_is_rejected(self):
        p=self.source/'helper.py'
        p.write_text('changed')
        with self.assertRaisesRegex(ValueError,'archive membership/content drift'):self.history().source(self.fp)
        p.unlink()
        with self.assertRaisesRegex(ValueError,'archive membership/content drift'):self.history().source(self.fp)
        p.write_text('recorded helper');(self.source/'extra.py').write_text('extra')
        with self.assertRaisesRegex(ValueError,'archive membership/content drift'):self.history().source(self.fp)
    def test_changed_bundle_resource_or_executable_cannot_be_historical_evidence(self):
        p=self.bundle/'Contents/resource';p.write_text('extra')
        with self.assertRaisesRegex(ValueError,'complete bundle drift'):self.history().application(self.value)
        p.unlink();(self.bundle/'Contents/MacOS/prisma-airs-desktop').write_text('wrong')
        with self.assertRaisesRegex(ValueError,'complete bundle drift'):self.history().application(self.value)


class ActualQualificationRegression(unittest.TestCase):
    def setUp(self):
        self.root=Path(__file__).resolve().parents[2];self.folder=self.root/'docs/planning/evidence/C45/attempt-02-followup';self.context=dict(path=str((self.folder/'historical-context.json').relative_to(self.root)),sha256=sha(self.folder/'historical-context.json'))
    def test_actual_historical_145_qualification_survives_unrelated_checkout_and_other_builder(self):
        from packet import qualification
        with patch('candidate.source_inventory',return_value={'unrelated/new-source.rs':{'sha256':'different'}}),patch('platform.node',return_value='another-release-builder'),patch('platform.platform',return_value='another-platform'):
            result=qualification(self.root,self.root/'docs/planning/evidence/C45/attempt-01-followup/qualified-coverage.json',current=False,historical=Historical(self.root,self.context))
        self.assertTrue(result['verified']['qualified']);self.assertEqual(result['verified']['affected']['tests']['passed'],72)
    def test_current_candidate_still_rejects_source_or_builder_change(self):
        candidate=read(self.folder/'candidate.json')
        with patch('candidate.source_inventory',return_value={'unrelated/new-source.rs':{'sha256':'different'}}):
            with self.assertRaisesRegex(ValueError,'current build-reuse source changed'):verify_candidate(self.root,candidate)
        with patch('platform.node',return_value='another-release-builder'):
            with self.assertRaisesRegex(ValueError,'execution conditions changed'):verify_candidate(self.root,candidate)
    def test_historical_mode_requires_explicit_hash_bound_context(self):
        from packet import qualification
        with self.assertRaisesRegex(ValueError,'requires explicit archive context'):
            qualification(self.root,self.root/'docs/planning/evidence/C45/attempt-01-followup/qualified-coverage.json',current=False)


if __name__=='__main__':unittest.main()
