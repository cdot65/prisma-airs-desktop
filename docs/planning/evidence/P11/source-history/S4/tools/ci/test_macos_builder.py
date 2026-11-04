"""Focused authorization/input/env checks; never compiles or opens the app."""
import importlib.util
from pathlib import Path
import unittest

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


if __name__ == "__main__":
    unittest.main()
