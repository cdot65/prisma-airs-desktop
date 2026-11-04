"""Controlled filesystem regression for the isolated-worktree path alias."""
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
with patch.dict(os.environ,{"AIRS_C32_EVIDENCE":"synthetic-no-UI"}):
    from acceptance_c32_current import native_project_root


class ProjectPathTests(unittest.TestCase):
    def test_symlinked_target_matches_the_native_canonical_selected_folder(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory).resolve();physical=root/"shared-target";physical.mkdir();work=root/"isolated-worktree";work.mkdir();(work/"target").symlink_to(physical,target_is_directory=True)
            old_alias=work/"target/native-c32-000000000001/agent-demo"
            expected=physical/"native-c32-000000000001/agent-demo"
            current=Path(native_project_root(str(work),"000000000001"))/"agent-demo";current.mkdir(parents=True)
            self.assertNotEqual(str(old_alias),str(expected),"old exact text assertion did not handle the path alias")
            self.assertEqual(current,expected)
            self.assertTrue(os.path.samefile(old_alias,current),"same selected owned folder, no assertion retirement")

    def test_regular_target_also_uses_its_exact_owned_folder(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory).resolve();(root/"target").mkdir()
            self.assertEqual(native_project_root(str(root),"000000000002"),str(root/"target/native-c32-000000000002"))


if __name__=='__main__':unittest.main()
