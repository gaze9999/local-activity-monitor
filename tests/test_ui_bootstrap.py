"""Isolated UI bootstrap fixtures; no real Git remote or owner checkout writes."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import types
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("prepare_ui_fixture", Path(__file__).resolve().parents[1] / "tools/prepare_ui.py")
prepare = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(prepare)
PIN = {"version": 1, "repository": "gaze9999/workbench-ui", "revision": "7f2745430cd1e037ac43331ebff125da5b3b2020"}
FILES = ("workbench-ui.css", "workbench-ui.js", "workbench-ui.mjs", "workbench-loader.js")


def source_fixture(path):
    (path / ".git").mkdir(parents=True)
    loader = path / "integrations/python/workbench_assets.py"
    loader.parent.mkdir(parents=True)
    loader.write_text("FILES = " + repr(FILES) + "\n", encoding="utf-8")
    return loader


def offline_fixture(path, *, pin=None):
    path.mkdir(parents=True)
    (path / "__init__.py").write_text("FILES = " + repr(FILES) + "\nraise AssertionError('offline helper must not execute')\n", encoding="utf-8")
    hashes = {}
    for name in FILES:
        content = ("fixture " + name).encode()
        (path / name).write_bytes(content)
        hashes[name] = hashlib.sha256(content).hexdigest()
    (path / "manifest.json").write_text(json.dumps({**(pin or PIN), "sha256": hashes}), encoding="utf-8")


class UIBootstrapTests(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.addCleanup(self.folder.cleanup)
        self.root = Path(self.folder.name).resolve()
        (self.root / "workbench-ui.json").write_text(json.dumps(PIN))
        self.source = self.root / "owner"
        self.calls = []

    @property
    def destination(self):
        return self.root / "src/local_activity_monitor/_workbench"

    def runner(self, command, **kwargs):
        self.calls.append((command, kwargs))
        self.assertFalse(kwargs["shell"])
        self.assertEqual(kwargs["stdin"], subprocess.DEVNULL)
        self.assertEqual(kwargs["timeout"], 120)
        self.assertEqual(kwargs["encoding"], "utf-8")
        self.assertEqual(kwargs["errors"], "replace")
        env = kwargs["env"]
        self.assertEqual(env["GIT_TERMINAL_PROMPT"], "0")
        self.assertEqual(env["GCM_INTERACTIVE"], "never")
        if "clone" in command:
            self.assertIn(prepare._REMOTE, command)
            self.assertIn("--no-checkout", command)
            source_fixture(Path(command[-1]))
        elif "checkout" in command:
            self.assertEqual(command[-3:], ["checkout", "--detach", PIN["revision"]])
        elif "rev-parse" in command:
            return types.SimpleNamespace(stdout=PIN["revision"])
        elif "get-url" in command:
            return types.SimpleNamespace(stdout=prepare._REMOTE)
        elif "status" in command:
            return types.SimpleNamespace(stdout="")
        elif "--destination" in command:
            staged = Path(command[command.index("--destination") + 1])
            offline_fixture(staged)
        else:
            self.fail("Unexpected command " + repr(command))
        return types.SimpleNamespace(stdout="")

    def ensure(self, runner=None):
        with patch.object(prepare.shutil, "which", return_value="git"), patch.object(prepare.subprocess, "run", side_effect=runner or self.runner):
            return prepare.ensure_ui(self.root, self.source)

    def assert_no_temporary(self):
        self.assertFalse(list((self.root / ".local").glob("**/.prepare*")))

    def test_clone_exact_pin_then_stage_and_reuse_offline_without_commands(self):
        self.ensure()
        self.assertTrue((self.root / ".local/workbench-ui" / PIN["revision"] / ".git").is_dir())
        prepare.verify_offline(self.destination, PIN)
        self.assertEqual(sum("clone" in command for command, _ in self.calls), 1)
        self.assert_no_temporary()
        with patch.object(prepare.subprocess, "run", side_effect=AssertionError("network/process")), patch.object(prepare.shutil, "which", side_effect=AssertionError("Git lookup")):
            prepare.ensure_ui(self.root, self.source)
        self.assertEqual(json.loads((self.root / "workbench-ui.json").read_text()), PIN)

    def test_verified_cache_stages_without_clone(self):
        source_fixture(self.root / ".local/workbench-ui" / PIN["revision"])
        self.ensure()
        self.assertFalse(any("clone" in command for command, _ in self.calls))
        self.assert_no_temporary()

    def test_valid_owner_source_stages_without_clone_and_does_not_mutate_owner(self):
        loader = source_fixture(self.source)
        before = loader.read_bytes()
        self.ensure()
        self.assertFalse(any("clone" in command or "checkout" in command for command, _ in self.calls))
        self.assertEqual(loader.read_bytes(), before)
        self.assertFalse((self.root / ".local/workbench-ui" / PIN["revision"]).exists())

    def test_dirty_owner_uses_private_cache_without_touching_owner(self):
        loader = source_fixture(self.source)
        before = loader.read_bytes()
        def runner(command, **kwargs):
            if "status" in command and str(self.source) in command:
                self.calls.append((command, kwargs))
                return types.SimpleNamespace(stdout=" M owner-file")
            return self.runner(command, **kwargs)
        self.ensure(runner)
        self.assertEqual(loader.read_bytes(), before)
        self.assertFalse(any("checkout" in command and str(self.source) in command for command, _ in self.calls))

    def test_invalid_pin_never_runs_git(self):
        for pin in ({**PIN, "revision": "main"}, {**PIN, "repository": "attacker/repo"}, {**PIN, "version": True}):
            (self.root / "workbench-ui.json").write_text(json.dumps(pin))
            with patch.object(prepare.subprocess, "run", side_effect=AssertionError("Git")):
                with self.assertRaises(prepare.PreparationError):
                    prepare.ensure_ui(self.root, self.source)

    def test_auth_failure_is_redacted_and_temporary_clone_cleaned(self):
        def failure(command, **kwargs):
            if "clone" in command:
                Path(command[-1]).mkdir()
                raise subprocess.CalledProcessError(128, command, stderr="https://token-secret@github.com private credentials")
            return self.runner(command, **kwargs)
        with self.assertRaises(prepare.PreparationError) as caught:
            self.ensure(failure)
        self.assertNotIn("token-secret", str(caught.exception))
        self.assert_no_temporary()
        self.assertFalse(self.destination.exists())

    def test_missing_git_and_timeout_have_safe_messages(self):
        with patch.object(prepare.shutil, "which", return_value=None):
            with self.assertRaisesRegex(prepare.PreparationError, "Git"):
                prepare.ensure_ui(self.root, self.source)
        def failure(command, **kwargs):
            raise subprocess.TimeoutExpired(command, 120, stderr="private-token")
        with self.assertRaisesRegex(prepare.PreparationError, "逾時"):
            self.ensure(failure)
        self.assert_no_temporary()

    def test_dirty_wrong_revision_or_wrong_remote_cache_preserved(self):
        cache = self.root / ".local/workbench-ui" / PIN["revision"]
        loader = source_fixture(cache)
        before = loader.read_bytes()
        for wrong, output in (("status", " M modified"), ("rev-parse", "a" * 40), ("get-url", "https://attacker.invalid/repo.git")):
            def runner(command, **kwargs):
                if wrong in command:
                    return types.SimpleNamespace(stdout=output)
                return self.runner(command, **kwargs)
            with self.assertRaises(prepare.PreparationError):
                self.ensure(runner)
            self.assertEqual(loader.read_bytes(), before)
            self.assertFalse(any("clone" in command for command, _ in self.calls))

    def test_partial_offline_directory_never_overwritten_or_fetched(self):
        self.destination.mkdir(parents=True)
        marker = self.destination / "user-file"
        marker.write_text("keep")
        with patch.object(prepare.subprocess, "run", side_effect=AssertionError("Git")):
            with self.assertRaises(prepare.PreparationError):
                prepare.ensure_ui(self.root, self.source)
        self.assertEqual(marker.read_text(), "keep")
        self.assertFalse((self.destination / "__init__.py").exists())

    def test_corrupt_offline_and_wrong_manifest_pin_preserved_without_network(self):
        offline_fixture(self.destination)
        css = self.destination / "workbench-ui.css"
        css.write_text("modified")
        with patch.object(prepare.subprocess, "run", side_effect=AssertionError("Git")):
            with self.assertRaises(prepare.PreparationError):
                prepare.ensure_ui(self.root, self.source)
        self.assertEqual(css.read_text(), "modified")
        manifest = self.destination / "manifest.json"
        value = json.loads(manifest.read_text())
        value["revision"] = "a" * 40
        manifest.write_text(json.dumps(value))
        with self.assertRaises(prepare.PreparationError):
            prepare.verify_offline(self.destination, PIN)

    def test_stage_failure_does_not_publish_partial_assets(self):
        def failure(command, **kwargs):
            if "--destination" in command:
                staged = Path(command[command.index("--destination") + 1])
                staged.mkdir()
                (staged / "partial").write_text("incomplete")
                raise subprocess.CalledProcessError(1, command, stderr="private-data")
            return self.runner(command, **kwargs)
        with self.assertRaises(prepare.PreparationError):
            self.ensure(failure)
        self.assertFalse(self.destination.exists())
        self.assert_no_temporary()

    def test_ensure_and_update_are_mutually_exclusive(self):
        with patch.object(prepare.sys, "argv", ["prepare_ui.py", "--ensure", "--update"]), patch("sys.stderr"):
            with self.assertRaises(SystemExit) as caught:
                prepare.main()
        self.assertEqual(caught.exception.code, 2)

    def test_default_missing_source_never_downloads(self):
        with patch.object(prepare, "__file__", str(self.root / "tools/prepare_ui.py")), patch.object(prepare.sys, "argv", ["prepare_ui.py", "--source", str(self.source)]), patch.object(prepare.subprocess, "run") as run, patch("sys.stderr"):
            with self.assertRaises(SystemExit) as caught:
                prepare.main()
        self.assertEqual(caught.exception.code, 1)
        run.assert_not_called()

    def test_original_source_and_update_modes_keep_helper_interface(self):
        source_fixture(self.source)
        for flags in ([], ["--update"]):
            with patch.object(prepare, "__file__", str(self.root / "tools/prepare_ui.py")), patch.object(prepare.sys, "argv", ["prepare_ui.py", "--source", str(self.source), *flags]), patch.object(prepare.subprocess, "run") as run:
                prepare.main()
            command = run.call_args.args[0]
            self.assertEqual(command[2], str(self.source / "integrations/python/workbench_assets.py"))
            self.assertEqual("--update" in command, bool(flags))
            self.assertNotIn("clone", command)

    def test_bad_staged_hash_never_publishes_assets(self):
        def corrupt(command, **kwargs):
            result = self.runner(command, **kwargs)
            if "--destination" in command:
                staged = Path(command[command.index("--destination") + 1])
                (staged / "workbench-ui.css").write_text("changed")
            return result
        with self.assertRaises(prepare.PreparationError):
            self.ensure(corrupt)
        self.assertFalse(self.destination.exists())
        self.assert_no_temporary()

    def test_cache_parent_resolved_outside_project_fails_before_clone(self):
        original = Path.resolve
        cache_parent = self.root / ".local/workbench-ui"
        outside = self.root.parent / "outside-workbench"
        def redirected(path, *args, **kwargs):
            return outside if path == cache_parent else original(path, *args, **kwargs)
        with patch.object(Path, "resolve", redirected), patch.object(prepare.subprocess, "run") as run:
            with self.assertRaisesRegex(prepare.PreparationError, "快取位置"):
                self.ensure()
        run.assert_not_called()
        self.assertFalse(cache_parent.exists())

    def test_staging_parent_resolved_outside_project_fails_before_staging(self):
        source_fixture(self.source)
        original = Path.resolve
        staging_parent = self.root / ".local"
        outside = self.root.parent / "outside-staging"
        def redirected(path, *args, **kwargs):
            return outside if path == staging_parent else original(path, *args, **kwargs)
        with patch.object(Path, "resolve", redirected):
            with self.assertRaisesRegex(prepare.PreparationError, "暫存位置"):
                self.ensure()
        self.assertFalse(any("--destination" in command for command, _ in self.calls))
        self.assertFalse(staging_parent.exists())

    def test_cache_resolution_is_rechecked_after_directory_creation(self):
        original = Path.resolve
        cache_parent = self.root / ".local/workbench-ui"
        outside = self.root.parent / "outside-created-cache"
        def redirected(path, *args, **kwargs):
            return outside if path == cache_parent and path.exists() else original(path, *args, **kwargs)
        with patch.object(Path, "resolve", redirected):
            with self.assertRaisesRegex(prepare.PreparationError, "快取位置"):
                self.ensure()
        self.assertEqual(self.calls, [])
        self.assertFalse(outside.exists())


if __name__ == "__main__":
    unittest.main()
