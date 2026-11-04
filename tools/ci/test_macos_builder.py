"""Focused authorization/input/env checks; never compiles or opens the app."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("macos_builder", Path(__file__).with_name("macos_builder.py"))
builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(builder)


class BuilderBoundaryTests(unittest.TestCase):
    def test_application_change_cannot_hide_in_ci_delta(self):
        builder.validate_delta([("A", "tools/ci/macos_builder.py")])
        for change in [("M", "apps/desktop/package.json"), ("A", "apps/desktop/src/extra.ts"),
                       ("D", "tools/ci/macos_builder.py"), ("A", "tools/ci/other.py")]:
            with self.assertRaises(RuntimeError):
                builder.validate_delta([("A", "tools/ci/macos_builder.py"), change])

    def test_only_exact_push_checkout_is_authorized(self):
        context = {"GITHUB_REPOSITORY": builder.REPOSITORY, "GITHUB_REF": builder.REF,
                   "GITHUB_EVENT_NAME": "push", "GITHUB_SHA": "a" * 40}
        builder.validate_context(context, "a" * 40)
        for key, value in [("GITHUB_REPOSITORY", "fork/repo"), ("GITHUB_REF", "refs/heads/main"),
                           ("GITHUB_EVENT_NAME", "pull_request"), ("GITHUB_SHA", "b" * 40)]:
            with self.assertRaises(RuntimeError):
                builder.validate_context(dict(context, **{key: value}), "a" * 40)

    def test_build_children_receive_no_action_or_signing_credentials(self):
        supplied = {"HOME": "/Users/cdot", "USER": "cdot", "SSH_AUTH_SOCK": "/tmp/agent",
                    "GITHUB_TOKEN": "private", "ACTIONS_RUNTIME_TOKEN": "private",
                    "APPLE_ID": "private", "APPLE_PASSWORD": "private", "APPLE_SIGNING_IDENTITY": "private",
                    "TAURI_SIGNING_PRIVATE_KEY": "private", "NODE_OPTIONS": "--import unwanted", "RUSTFLAGS": "unwanted"}
        env = builder.child_environment(supplied)
        self.assertEqual(env["HOME"], supplied["HOME"])
        for key in supplied.keys() - {"HOME", "USER"}:
            self.assertNotIn(key, env)
        transport = builder.child_environment(supplied, transport=True)
        self.assertNotIn("SSH_AUTH_SOCK", transport)
        self.assertNotIn("GITHUB_TOKEN", transport)
        self.assertIn("BatchMode=yes", transport["GIT_SSH_COMMAND"])
        self.assertIn("IdentityAgent=none", transport["GIT_SSH_COMMAND"])

    def test_input_drift_stops_before_fetch_or_build(self):
        academy = {"origin": builder.ACADEMY_ORIGIN, "commit": builder.ACADEMY_COMMIT, "tree_sha256": builder.ACADEMY_TREE}
        harness = {"archive": builder.HARNESS_PATH, "archiveSha256": builder.HARNESS_SHA, "archiveBytes": 113497192}
        builder.validate_pins(academy, harness)
        with self.assertRaises(RuntimeError):
            builder.validate_pins(dict(academy, commit="b" * 40), harness)
        with self.assertRaises(RuntimeError):
            builder.validate_pins(academy, dict(harness, archiveSha256="c" * 64))


class P11CheckoutAdmissionTests(unittest.TestCase):
    """Real disposable Git objects exercise admission, never the app or CI."""

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="airs-ci-admission-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.env = {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(self.root),
            "LANG": "C", "LC_ALL": "C",
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
            "GIT_TERMINAL_PROMPT": "0",
        }
        self.git("init", "-q")
        for path in builder.CI_FILES:
            self.write(path, "initial CI fixture\n")
        self.write("apps/desktop/src/product.ts", "export const product = 1\n")
        self.commit("accepted application fixture")
        self.base = self.git("rev-parse", "HEAD")
        self.tree = self.git("rev-parse", "HEAD^{tree}")

    def git(self, *args):
        result = subprocess.run(
            ["git", "-c", "user.name=AIRS admission fixture",
             "-c", "user.email=admission-fixture@example.invalid", *args],
            cwd=self.root, env=self.env, text=True, check=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        return result.stdout.strip()

    def write(self, path, text):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def commit(self, message):
        self.git("add", "--all")
        self.git("commit", "-q", "-m", message)

    def ci_commit(self):
        self.write("tools/ci/macos_builder.py", "reviewed CI-only fixture\n")
        self.commit("reviewed CI-only delta")

    def context(self):
        return {"GITHUB_REPOSITORY": builder.REPOSITORY, "GITHUB_REF": builder.P11_REF,
                "GITHUB_EVENT_NAME": "push", "GITHUB_SHA": self.git("rev-parse", "HEAD")}

    def admit(self, **changes):
        pins = {"P11_ACCEPTED_SOURCE": self.base, "P11_ACCEPTED_TREE": self.tree,
                "P11_PROVISIONAL_ANCESTOR": self.base}
        pins.update(changes)
        with patch.multiple(builder, **pins):
            return builder.admit_checkout(self.context(), self.env, cwd=self.root)

    def test_exact_accepted_base_plus_reviewed_ci_modification(self):
        self.ci_commit()
        result = self.admit()
        self.assertEqual(result["base"], self.base)
        self.assertEqual(result["tree"], self.tree)
        self.assertEqual(result["head"], self.git("rev-parse", "HEAD"))
        self.assertEqual(result["lane"], "p11-accepted-source")
        self.assertEqual(result["delta"], [["M", "tools/ci/macos_builder.py"]])

    def test_provisional_qualification_never_substitutes_missing_acceptance(self):
        self.ci_commit()
        for pins in ({"P11_ACCEPTED_SOURCE": None}, {"P11_ACCEPTED_TREE": None},
                     {"P11_ACCEPTED_SOURCE": builder.P11_REF},
                     {"P11_ACCEPTED_TREE": "not-a-tree"}):
            with self.subTest(pins=pins):
                with self.assertRaisesRegex(RuntimeError, "P11 lane is disabled"):
                    self.admit(**pins)

    def test_wrong_source_tree_and_unrelated_same_tree_commit_are_rejected(self):
        self.ci_commit()
        with self.assertRaisesRegex(RuntimeError, "accepted P11 tree"):
            self.admit(P11_ACCEPTED_TREE="b" * 40)
        unrelated = self.git("commit-tree", self.tree, "-m", "unrelated same-tree source")
        with self.assertRaises(subprocess.CalledProcessError):
            self.admit(P11_ACCEPTED_SOURCE=unrelated)
        with self.assertRaises(subprocess.CalledProcessError):
            self.admit(P11_PROVISIONAL_ANCESTOR=unrelated)

    def test_triggering_checkout_must_still_match_exact_commit(self):
        self.ci_commit()
        with patch.multiple(builder, P11_ACCEPTED_SOURCE=self.base, P11_ACCEPTED_TREE=self.tree):
            with self.assertRaisesRegex(RuntimeError, "checkout differs"):
                builder.admit_checkout(dict(self.context(), GITHUB_SHA=self.base), self.env, cwd=self.root)

    def test_product_change_cannot_hide_beside_reviewed_ci_delta(self):
        self.ci_commit()
        self.write("apps/desktop/src/product.ts", "export const product = 2\n")
        self.commit("product change outside CI authority")
        with self.assertRaisesRegex(RuntimeError, "non-CI change"):
            self.admit()

    def test_dirty_tracked_and_untracked_checkout_are_rejected(self):
        self.ci_commit()
        self.write("tools/ci/macos_builder.py", "dirty CI worktree\n")
        with self.assertRaisesRegex(RuntimeError, "checkout is not clean"):
            self.admit()
        self.git("checkout", "--", "tools/ci/macos_builder.py")
        self.write("unexpected-product-file.ts", "untracked input\n")
        with self.assertRaisesRegex(RuntimeError, "checkout is not clean"):
            self.admit()

    def test_empty_delta_is_not_a_candidate_controller(self):
        with self.assertRaisesRegex(RuntimeError, "expected declared CI-only"):
            self.admit()

    def test_ci_mode_change_and_symlink_are_rejected(self):
        target = self.root / "tools/ci/macos_builder.py"
        target.chmod(0o755)
        self.commit("CI mode change")
        with self.assertRaisesRegex(RuntimeError, "changed type or mode"):
            self.admit()
        self.git("reset", "--hard", self.base)
        target.unlink()
        target.symlink_to("test_macos_builder.py")
        self.commit("CI symlink")
        with self.assertRaisesRegex(RuntimeError, "non-CI change"):
            self.admit()

    def test_old_rc1_admission_remains_additions_only(self):
        builder.validate_delta([("A", "tools/ci/macos_builder.py")])
        with self.assertRaises(RuntimeError):
            builder.validate_delta([("M", "tools/ci/macos_builder.py")])
        builder.validate_delta([("M", "tools/ci/macos_builder.py")], builder.P11_REF)


if __name__ == "__main__":
    unittest.main()
