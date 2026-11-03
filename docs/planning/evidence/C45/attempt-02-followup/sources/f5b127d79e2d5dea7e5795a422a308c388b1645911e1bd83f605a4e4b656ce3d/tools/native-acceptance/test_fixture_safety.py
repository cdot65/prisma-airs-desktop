"""No UI, keychain or real user-data operations: isolated helper boundary tests."""
from pathlib import Path
import os
import tempfile
import unittest
from unittest.mock import patch
import fixture_app as fx


class FixtureSafetyTests(unittest.TestCase):
    def test_rejects_old_or_foreign_root_before_any_cleanup(self):
        with patch.object(fx,"DATA","/synthetic/old-v2"), patch.object(fx,"sh") as run, patch.object(fx.shutil,"rmtree") as remove:
            with self.assertRaisesRegex(RuntimeError,"exact new fixture root"):fx.clean_fixture_root()
            run.assert_not_called();remove.assert_not_called()

    def test_symlinked_root_and_keychain_cannot_escape(self):
        with tempfile.TemporaryDirectory() as tmp:
            home=Path(tmp).resolve();expected=home/"Library/Application Support/io.cdot.airs-desktop.fixture";expected.parent.mkdir(parents=True)
            other=home/"foreign";other.mkdir();expected.symlink_to(other,target_is_directory=True)
            with patch.object(fx.os.path,"expanduser",return_value=str(home)),patch.object(fx,"DATA",str(expected)),patch.object(fx,"KEYCHAIN",str(expected/"fixture-credentials.keychain-db")),patch.object(fx,"sh") as run:
                with self.assertRaisesRegex(RuntimeError,"exact new fixture root"):fx.clean_fixture_root()
                run.assert_not_called()
            expected.unlink();expected.mkdir();(expected/"fixture-credentials.keychain-db").symlink_to(other/"secret")
            with patch.object(fx.os.path,"expanduser",return_value=str(home)),patch.object(fx,"DATA",str(expected)),patch.object(fx,"KEYCHAIN",str(expected/"fixture-credentials.keychain-db")),patch.object(fx,"sh") as run:
                with self.assertRaisesRegex(RuntimeError,"symlinked"):fx.clean_fixture_root()
                run.assert_not_called()

    def test_canary_digest_detects_same_size_same_timestamp_mutation(self):
        with tempfile.TemporaryDirectory() as tmp,patch.object(fx,"V1_DATA",tmp):
            p=Path(tmp)/"old-state";p.write_text("before");stamp=p.stat().st_mtime_ns;before=fx.v1_metadata()
            p.write_text("after!");os.utime(p,ns=(stamp,stamp));after=fx.v1_metadata()
            self.assertEqual(before[0][:3],after[0][:3]);self.assertNotEqual(before[0][3],after[0][3])

    def test_process_name_signaling_rejected_before_subprocess(self):
        with patch.object(fx.subprocess,"run") as run:
            for command in [["pkill","app"],["/usr/bin/killall","app"]]:
                with self.assertRaisesRegex(RuntimeError,"forbidden"):fx.sh(command)
            run.assert_not_called()


if __name__ == "__main__":unittest.main()
