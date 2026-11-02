#!/usr/bin/env python3
"""Scratch-only stdlib tests of the final prepared-bundle boundary."""
from pathlib import Path
import hashlib
import importlib.util
import json
import os
import plistlib
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
SCRATCH = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location(
    "proposed_phase_inputs", Path(os.environ.get("P09_BUNDLE_PREDICATE_SOURCE", str(SCRATCH / "proposal/tools/p09/phase_inputs.py"))))
predicate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(predicate)


class PreparedBundleBoundary(unittest.TestCase):
    def setUp(self):
        data = SCRATCH / "test-data"
        data.mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="case-", dir=data)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bundle = self.root / "Fixture.app"
        self.exe = self.bundle / "Contents/MacOS/prisma-airs-desktop-v2"
        self.exe.parent.mkdir(parents=True)
        self.exe.write_bytes(b"synthetic executable bytes\n")
        self.exe.chmod(0o755)
        self.resources = self.bundle / "Contents/Resources"
        self.resources.mkdir()
        self.resource = self.resources / "payload.txt"
        self.resource.write_bytes(b"resource bytes\n")
        self.resource.chmod(0o644)
        (self.resources / "same-bytes.txt").write_bytes(self.resource.read_bytes())
        (self.resources / "same-bytes.txt").chmod(0o644)
        self.link = self.resources / "current"
        self.link.symlink_to("payload.txt")
        self.identifier = "io.cdot.fixture.test"
        self.write_identifier(self.identifier)
        manifest = self.root / "docs/architecture/native-checks.json"
        manifest.parent.mkdir(parents=True)
        manifest.write_text(json.dumps({"fixture_identifier": self.identifier}))
        self.prepared = {
            "bundle": "Fixture.app",
            "identifier": self.identifier,
            "executable_sha256": predicate.sha(self.exe),
            "bundle_sha256": predicate.bundle_tree_hash(self.bundle),
        }

    def write_identifier(self, identifier):
        (self.bundle / "Contents/Info.plist").write_bytes(
            plistlib.dumps({"CFBundleIdentifier": identifier}, sort_keys=True))
        (self.bundle / "Contents/Info.plist").chmod(0o644)

    def assert_executable_unchanged(self):
        self.assertEqual(predicate.sha(self.exe), self.prepared["executable_sha256"])

    def test_valid_bound_bundle_is_accepted(self):
        self.assertIsNone(predicate.verify_current_bundle(self.root, self.prepared))

    def test_resource_drift_with_identical_executable_is_rejected(self):
        self.resource.write_bytes(b"changed resource bytes\n")
        self.assert_executable_unchanged()
        with self.assertRaisesRegex(ValueError, "prepared bundle drift"):
            predicate.verify_current_bundle(self.root, self.prepared)

    def test_symlink_target_change_is_rejected_even_for_equal_target_bytes(self):
        self.link.unlink()
        self.link.symlink_to("same-bytes.txt")
        self.assert_executable_unchanged()
        self.assertEqual(self.link.read_bytes(), self.resource.read_bytes())
        with self.assertRaisesRegex(ValueError, "prepared bundle drift"):
            predicate.verify_current_bundle(self.root, self.prepared)

    def test_executable_bit_change_is_rejected_without_byte_change(self):
        before = self.resource.read_bytes()
        self.resource.chmod(0o645)  # Any execute bit changes the canonical mode.
        self.assertEqual(self.resource.read_bytes(), before)
        self.assert_executable_unchanged()
        with self.assertRaisesRegex(ValueError, "prepared bundle drift"):
            predicate.verify_current_bundle(self.root, self.prepared)

    def test_wrong_identifier_is_rejected_even_with_matching_live_bundle_hash(self):
        self.write_identifier("io.cdot.unexpected.production")
        # Rebind the tree hash deliberately: identity must have its own guard.
        self.prepared["bundle_sha256"] = predicate.bundle_tree_hash(self.bundle)
        self.assert_executable_unchanged()
        with self.assertRaisesRegex(ValueError, "prepared fixture identity drift"):
            predicate.verify_current_bundle(self.root, self.prepared)

    def test_non_rust_whitespace_controls_in_binary_plist_never_become_fixture_identity(self):
        for control in ["\x1c", "\x1d", "\x1e", "\x1f"]:
            for position in ["prefix", "suffix"]:
                with self.subTest(control=f"U+{ord(control):04X}", position=position):
                    malformed = control + self.identifier if position == "prefix" else self.identifier + control
                    (self.bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(
                        {"CFBundleIdentifier": malformed}, fmt=plistlib.FMT_BINARY))
                    # Isolate identity normalization: all artifact hashes match.
                    self.prepared["bundle_sha256"] = predicate.bundle_tree_hash(self.bundle)
                    self.assert_executable_unchanged()
                    with self.assertRaisesRegex(ValueError, "prepared fixture identity drift"):
                        predicate.verify_current_bundle(self.root, self.prepared)

    def test_canonical_inventory_vector_sorted_modes_links_and_no_directories(self):
        mini = self.root / "Canonical.app"
        (mini / "a").mkdir(parents=True)
        (mini / "empty").mkdir()
        (mini / "a-z").write_bytes(b"A")
        (mini / "a-z").chmod(0o640)
        (mini / "a/x").write_bytes(b"B")
        (mini / "a/x").chmod(0o701)
        (mini / "z-link").symlink_to("a")  # A directory link is not traversed.
        # Literal canonical entries: final global ordering puts a-z before a/x,
        # ignores empty directories and normalizes any executable permission.
        inventory = (
            "a-z\0" "100644\0" "559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd\n"
            "a/x\0" "100755\0" "df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c\n"
            "z-link\0" "symlink\0" "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb\n"
        )
        expected = hashlib.sha256(inventory.encode("utf-8")).hexdigest()
        self.assertEqual(predicate.bundle_tree_hash(mini), expected)
        (mini / "a-z").chmod(0o644)
        (mini / "a/x").chmod(0o710)
        self.assertEqual(predicate.bundle_tree_hash(mini), expected)


if __name__ == "__main__":
    unittest.main(verbosity=2)
