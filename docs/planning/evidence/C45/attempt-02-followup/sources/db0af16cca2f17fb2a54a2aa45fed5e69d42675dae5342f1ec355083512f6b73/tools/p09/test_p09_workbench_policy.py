"""Boundary mutation checks for the additive P09 Workbench transition."""
from pathlib import Path
import unittest,sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from p09_workbench_policy import ARCHIVE,workbench_projection
ROOT=Path(__file__).resolve().parents[2]
class Boundary(unittest.TestCase):
    def test_shared_authority_only_workbench_retirement(self):
        old=(ROOT/ARCHIVE/'reviewed-inputs/crates/desktop-core/src/authority.rs').read_text()
        self.assertEqual(old,workbench_projection((ROOT/'crates/desktop-core/src/authority.rs').read_text()))
    def test_unrelated_authority_change_rejected(self):
        old=(ROOT/ARCHIVE/'reviewed-inputs/crates/desktop-core/src/authority.rs').read_text()
        new=(ROOT/'crates/desktop-core/src/authority.rs').read_text().replace('Effect::RevealSecret {','Effect::UnexpectedSecret {',1)
        self.assertNotEqual(old,workbench_projection(new))
    def test_missing_or_ambiguous_retirement_rejected(self):
        current=(ROOT/'crates/desktop-core/src/authority.rs').read_text()
        for value in ['',current+'        state.workbench.sending.retire_renderer_review();\n']:
            with self.assertRaises(ValueError):workbench_projection(value)
if __name__=='__main__':unittest.main()
