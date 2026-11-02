import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


GUARD = Path(__file__).resolve().parents[1] / "tools" / "check-public-content.py"
SPEC = importlib.util.spec_from_file_location("public_content", GUARD)
GUARD_MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GUARD_MODULE)


def workspace():
    return "Q:" + "/" + "projects" + "/private-example/src/app"


def pem(kind="PRIVATE KEY"):
    return ("-----BEGIN " + kind + "-----\nsynthetic\n-----END " + kind + "-----\n")


class ContentTests(unittest.TestCase):
    def test_pem_encodings_and_public_controls(self):
        for kind in ["PRIVATE KEY", "RSA PRIVATE KEY", "OPENSSH PRIVATE KEY", "ENCRYPTED PRIVATE KEY", "PGP PRIVATE KEY BLOCK"]:
            for encoding in ["utf-8", "utf-16", "utf-16-be", "utf-32"]:
                with self.subTest(kind=kind, encoding=encoding):
                    self.assertIn("private key material", GUARD_MODULE.content_problems(pem(kind).encode(encoding)))
        for content in [pem("PUBLIC KEY"), pem("CERTIFICATE"), "node tool.cjs --project PROJECT_PATH --scope src/app"]:
            self.assertEqual([], GUARD_MODULE.content_problems(content.encode()))

    def test_workspace_encodings_and_public_example(self):
        original = workspace()
        variants = [original, original.lower(), original.replace("/", "\\"),
                    original.replace("/", "\\\\"), original.replace("/", "\\/"),
                    original.replace("/", "\\u002f"), original.replace(":", "%3A").replace("/", "%2F"),
                    original.replace(":", "&#58;").replace("/", "&#47;")]
        for content in variants:
            for encoding in ["utf-8", "utf-16", "utf-32"]:
                with self.subTest(encoding=encoding):
                    self.assertIn("absolute local workspace path", GUARD_MODULE.content_problems(content.encode(encoding)))
        self.assertEqual([], GUARD_MODULE.content_problems(b"C:/path/to/project; ./src/app; PROJECT_PATH"))

    def test_standalone_der_private_and_public_controls(self):
        def tlv(tag, data):
            return bytes([tag, len(data)]) + data
        integer = tlv(2, b"\x00")
        private = tlv(0x30, integer * 9)
        self.assertTrue(GUARD_MODULE.der_private_key(private))
        self.assertTrue(GUARD_MODULE.der_private_key(tlv(0x30, integer + tlv(0x30, b"") + tlv(4, b"synthetic"))))
        self.assertTrue(GUARD_MODULE.der_private_key(tlv(0x30, tlv(2, b"\x01") + tlv(4, b"synthetic"))))
        self.assertFalse(GUARD_MODULE.der_private_key(tlv(0x30, tlv(2, b"\x23") * 2)))
        self.assertFalse(GUARD_MODULE.der_private_key(private[:-1]))
        self.assertFalse(GUARD_MODULE.der_private_key(tlv(0x30, tlv(0x30, b"") + tlv(4, b"public"))))


class GitBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="public-content-test-")
        self.root = Path(self.temp.name)
        self.env = os.environ.copy()
        for key in list(self.env):
            if key.startswith("GIT_"):
                self.env.pop(key)
        self.env.update({"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull})
        self.git("init", "-b", "main")
        self.git("config", "user.name", "Fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.write("README.md", b"Public fixture\n")
        self.git("add", "README.md")
        self.git("commit", "-m", "Initial public fixture")

    def tearDown(self):
        # Git for Windows can leave readonly object files in temporary fixture repositories.
        if os.name == "nt":
            for path in self.root.rglob("*"):
                if path.is_file():
                    path.chmod(0o600)
        self.temp.cleanup()

    def git(self, *args):
        result = subprocess.run(["git", *args], cwd=self.root, env=self.env, capture_output=True)
        self.assertEqual(0, result.returncode, result.stderr.decode("utf-8", "replace"))
        return result.stdout.decode().strip()

    def write(self, name, data):
        (self.root / name).write_bytes(data)

    def check(self, *args, stdin=None, expected=0):
        result = subprocess.run([sys.executable, str(GUARD), *args], cwd=self.root, env=self.env, input=stdin,
                                capture_output=True, encoding="utf-8")
        self.assertEqual(expected, result.returncode, result.stdout + result.stderr)
        self.assertNotIn("synthetic", result.stdout + result.stderr)
        self.assertNotIn(workspace(), result.stdout + result.stderr)
        return result

    def test_index_checks_staged_bytes(self):
        self.write("renamed.txt", pem().encode())
        self.git("add", "renamed.txt")
        self.write("renamed.txt", b"Clean working copy\n")
        self.check("--index", expected=1)
        self.git("add", "renamed.txt")
        self.check("--index")

    def test_old_removed_blob_still_blocks_history_and_push(self):
        self.write("old.txt", workspace().encode("utf-16"))
        self.git("add", "old.txt")
        self.git("commit", "-m", "Old private fixture")
        self.git("rm", "old.txt")
        self.git("commit", "-m", "Remove fixture")
        self.check("--index")
        self.check("--history", expected=1)
        sha = self.git("rev-parse", "HEAD")
        self.check("--pre-push", stdin=f"refs/heads/main {sha} refs/heads/main {'0' * 40}\n", expected=1)

    def test_all_outgoing_refs_and_tag_annotation(self):
        clean = self.git("rev-parse", "HEAD")
        self.check("--pre-push", stdin=f"refs/heads/main {clean} refs/heads/main {'0' * 40}\n")
        self.git("tag", "-a", "test-private", "-m", workspace())
        tag = self.git("rev-parse", "test-private")
        self.check("--pre-push", stdin=f"refs/heads/main {clean} refs/heads/main {'0' * 40}\nrefs/tags/test-private {tag} refs/tags/test-private {'0' * 40}\n", expected=1)
        self.check("--history", expected=1)
        self.check("--pre-push", stdin=f"(delete) {'0' * 40} refs/heads/old {clean}\n")
        self.check("--pre-push", stdin="invalid\n", expected=2)

    def test_detached_head_is_checked(self):
        self.git("checkout", "--detach")
        self.write("detached.txt", pem().encode())
        self.git("add", "detached.txt")
        self.git("commit", "-m", "Detached fixture")
        self.check("--history", expected=1)

    def test_binary_key_and_reserved_filename(self):
        self.write("renamed.bin", b"\xff\xfe\x01" + pem().encode("utf-16-be"))
        self.git("add", "renamed.bin")
        self.check("--index", expected=1)
        self.git("reset", "--", "renamed.bin")
        self.write("build.pem", b"Reserved local file\n")
        self.git("add", "build.pem")
        self.check("--index", expected=1)
        self.git("commit", "-m", "Reserved filename fixture")
        self.check("--history", expected=1)

    def install_hooks(self):
        (self.root / "tools").mkdir()
        shutil.copy2(GUARD, self.root / "tools" / GUARD.name)
        shutil.copytree(GUARD.parents[1] / ".githooks", self.root / ".githooks")
        for hook in (self.root / ".githooks").iterdir():
            hook.chmod(0o755)
        self.git("config", "core.hooksPath", ".githooks")

    def test_real_pre_commit_hook(self):
        self.install_hooks()
        before = self.git("rev-parse", "HEAD")
        self.write("unsafe.txt", pem().encode())
        self.git("add", "unsafe.txt")
        result = subprocess.run(["git", "commit", "-m", "Blocked fixture"], cwd=self.root, env=self.env, capture_output=True)
        self.assertNotEqual(0, result.returncode)
        self.assertIn(b"private key material", result.stderr)
        self.assertEqual(before, self.git("rev-parse", "HEAD"))
        self.write("unsafe.txt", b"Public content\n")
        self.git("add", "unsafe.txt")
        self.git("commit", "-m", "Allowed public fixture")

    def test_real_pre_push_hook(self):
        self.install_hooks()
        remote = self.root / "remote.git"
        self.git("init", "--bare", str(remote))
        self.git("push", str(remote), "main")
        self.git("tag", "-a", "test-private", "-m", workspace())
        result = subprocess.run(["git", "push", str(remote), "refs/tags/test-private"], cwd=self.root, env=self.env, capture_output=True)
        self.assertNotEqual(0, result.returncode)
        self.assertIn(b"absolute local workspace path", result.stderr)
        self.assertEqual("", self.git("ls-remote", "--tags", str(remote)))

    def test_exact_fixture_exception_does_not_allow_changed_content(self):
        self.write("fixture.txt", workspace().encode())
        self.git("add", "fixture.txt")
        blob = self.git("rev-parse", ":fixture.txt")
        policy = [{"blob": blob, "path": "fixture.txt", "reason": "Synthetic path regression fixture"}]
        self.write(".public-content-fixtures.json", json.dumps(policy).encode())
        self.git("add", ".public-content-fixtures.json")
        self.check("--index")
        self.git("commit", "-m", "Reviewed synthetic fixture")
        self.check("--history")
        self.write("fixture.txt", (workspace() + "/changed").encode())
        self.git("add", "fixture.txt")
        self.check("--index", expected=1)
        self.git("tag", "-a", "unsafe-note", "-m", workspace())
        self.check("--history", expected=1)
        self.write("key-fixture.txt", pem().encode())
        self.git("add", "key-fixture.txt")
        policy.append({"blob": self.git("rev-parse", ":key-fixture.txt"),
                       "path": "key-fixture.txt", "reason": "Must never allow private keys"})
        self.write(".public-content-fixtures.json", json.dumps(policy).encode())
        self.git("add", ".public-content-fixtures.json")
        self.check("--index", expected=1)

    def test_invalid_fixture_policy_fails_closed(self):
        self.write(".public-content-fixtures.json", b'[{"blob": "*"}]')
        self.git("add", ".public-content-fixtures.json")
        self.check("--index", expected=2)

    def test_shallow_history_fails_closed(self):
        clone = self.root / "shallow"
        self.git("clone", "--depth=1", self.root.as_uri(), str(clone))
        result = subprocess.run([sys.executable, str(GUARD), "--history"], cwd=clone, env=self.env, capture_output=True)
        self.assertEqual(2, result.returncode)
        self.assertIn(b"Full history required", result.stderr)


if __name__ == "__main__":
    unittest.main()
