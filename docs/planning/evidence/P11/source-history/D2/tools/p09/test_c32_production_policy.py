from pathlib import Path
import unittest
from c32_production_policy import authority_projection,bundle_digest
import tempfile

ROOT=Path(__file__).resolve().parents[2]
OLD=ROOT/'docs/planning/evidence/P09/native-history/Worf-generation-09-complete-before-C32-scoped-confirmation-20261102/reviewed-source/authority.rs'

class HarnessBoundary(unittest.TestCase):
    def test_actual_delta_only_harness(self):
        self.assertEqual(authority_projection(OLD.read_text()),authority_projection((ROOT/'crates/desktop-core/src/authority.rs').read_text()))
    def test_other_presentation_branch_change_rejected(self):
        old=OLD.read_text();new=(ROOT/'crates/desktop-core/src/authority.rs').read_text()
        new=new.replace('Effect::RevealSecret {','Effect::UnexpectedSecret {',1)
        self.assertNotEqual(authority_projection(old),authority_projection(new))
    def test_unrelated_state_change_rejected(self):
        old=OLD.read_text();new=(ROOT/'crates/desktop-core/src/authority.rs').read_text().replace('    cleaning: bool,','    cleaning: u64,',1)
        self.assertNotEqual(authority_projection(old),authority_projection(new))
    def test_missing_or_ambiguous_harness_marker_rejected(self):
        for text in ['',OLD.read_text()+'\n    ConfirmHarness { nonce: u64 }']:
            with self.assertRaises(ValueError):authority_projection(text)

class BundleIntegrity(unittest.TestCase):
    def test_bytes_mode_and_symlink_targets_bound(self):
        with tempfile.TemporaryDirectory() as work:
            root=Path(work);file=root/'app';file.write_text('first');file.chmod(0o755);link=root/'link';link.symlink_to('app');before=bundle_digest(root)
            file.write_text('second');self.assertNotEqual(before,bundle_digest(root));file.write_text('first');self.assertEqual(before,bundle_digest(root))
            file.chmod(0o644);self.assertNotEqual(before,bundle_digest(root));file.chmod(0o755);link.unlink();link.symlink_to('other');self.assertNotEqual(before,bundle_digest(root))

if __name__=='__main__':unittest.main()
