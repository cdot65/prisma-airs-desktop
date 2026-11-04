import unittest
from c32_transition_policy import validate_input_delta, require_bundle_equivalence

class ReuseProof(unittest.TestCase):
    def test_only_declared_c32_driver_change(self):
        p='tools/native-acceptance/acceptance_c32_current.py'
        self.assertEqual(validate_input_delta({'apps/source':'same'},{'apps/source':'same',p:'new'},{p:{}}),{p})
    def test_production_or_shared_helper_change_rejected(self):
        for p in ['apps/desktop/src/v2/harness/harness.tsx','crates/desktop-core/src/authority.rs','tools/native-acceptance/fixture_app.py','tools/native-acceptance/mgmt_server.py','Cargo.lock']:
            with self.subTest(path=p),self.assertRaises(ValueError): validate_input_delta({p:'before'},{p:'after'},{p:{}})
    def test_undeclared_new_driver_rejected(self):
        with self.assertRaises(ValueError): validate_input_delta({}, {'tools/native-acceptance/acceptance_c32_current.py':'new'}, {})
    def test_application_mode_symlink_and_bytes_all_bound(self):
        expected={'binary':{'sha256':'same','mode':493},'link':{'target':'binary','mode':511}}
        require_bundle_equivalence(expected,expected)
        for changed in [{'binary':{'sha256':'changed','mode':493},'link':expected['link']},{'binary':{'sha256':'same','mode':420},'link':expected['link']},{'binary':expected['binary'],'link':{'target':'other','mode':511}}]:
            with self.assertRaises(ValueError): require_bundle_equivalence(expected,changed)

if __name__=='__main__': unittest.main()
