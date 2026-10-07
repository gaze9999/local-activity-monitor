"""Build, update and recovery fixtures, without real Git or account data."""
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from local_activity_monitor.frontend_assets import FILES, FrontendAssets
from local_activity_monitor.ui_assets import load_ui_assets

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("lam_frontend_builder", ROOT / "tools/build_frontend.py")
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)
archive_spec = importlib.util.spec_from_file_location("lam_source_archive", ROOT / "tools/archive_source.py")
source_archive = importlib.util.module_from_spec(archive_spec)
archive_spec.loader.exec_module(source_archive)


class FrontendBuildTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="LAM frontend 中文 ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.pin = json.loads((ROOT / "workbench-ui.json").read_text())
        (self.root / "workbench-ui.json").write_text(json.dumps(self.pin))
        shutil.copytree(ROOT / "frontend", self.root / "frontend")
        self.library = self.root / "src/local_activity_monitor/_workbench"
        shutil.copytree(load_ui_assets().root, self.library, ignore=shutil.ignore_patterns("__pycache__"))
        self.app = self.root / "src/local_activity_monitor/_web"

    def test_first_build_then_no_rewrite_without_git(self):
        with patch.object(builder, "ensure_ui", side_effect=AssertionError("unexpected Git")), patch.object(builder, "latest_pin", side_effect=AssertionError("unexpected network")):
            builder.build(self.root)
            before = {path.name: path.stat().st_mtime_ns for path in self.app.iterdir()}
            builder.build(self.root)
        self.assertEqual(before, {path.name: path.stat().st_mtime_ns for path in self.app.iterdir()})
        FrontendAssets(self.app)
        for name in FILES:
            self.assertEqual((self.root / "frontend" / name).read_bytes(), (self.app / name).read_bytes())

    def test_edit_rebuild_changes_revision_and_preserves_input_bytes(self):
        builder.build(self.root)
        before = FrontendAssets(self.app).manifest["inputs"]
        with (self.root / "frontend/app.js").open("ab") as stream:
            stream.write(b"\n/* fixture edit */\n")
        builder.build(self.root)
        self.assertNotEqual(before, FrontendAssets(self.app).manifest["inputs"])
        self.assertEqual((self.root / "frontend/app.js").read_bytes(), (self.app / "app.js").read_bytes())

    def test_latest_network_failure_keeps_usable_bundle(self):
        builder.build(self.root)
        before = (self.app / "manifest.json").read_bytes()
        with patch.object(builder, "latest_pin", side_effect=builder.PreparationError("offline")):
            builder.build(self.root, latest=True)
        self.assertEqual((self.app / "manifest.json").read_bytes(), before)

    def test_unknown_files_and_corruption_are_preserved_before_network(self):
        builder.build(self.root)
        marker = self.app / "user-file"
        marker.write_text("keep")
        with patch.object(builder, "latest_pin") as remote:
            with self.assertRaises(ValueError):
                builder.build(self.root, latest=True)
            remote.assert_not_called()
        self.assertEqual(marker.read_text(), "keep")
        marker.unlink()
        (self.app / "app.js").write_text("corrupt")
        with self.assertRaises(ValueError):
            builder.build(self.root)
        self.assertEqual((self.app / "app.js").read_text(), "corrupt")

    def test_invalid_source_keeps_previous_build(self):
        builder.build(self.root)
        before = (self.app / "manifest.json").read_bytes()
        (self.root / "frontend/locales.json").write_text("{invalid")
        with self.assertRaises(ValueError):
            builder.build(self.root)
        self.assertEqual((self.app / "manifest.json").read_bytes(), before)

    def test_latest_failure_preserves_pin_and_current_library(self):
        updated = {**self.pin, "revision": "a" * 40}
        with patch.object(builder, "latest_pin", return_value=updated), patch.object(builder, "ensure_ui", side_effect=builder.PreparationError("unavailable")):
            builder.build(self.root, latest=True)
        self.assertEqual(json.loads((self.root / "workbench-ui.json").read_text()), self.pin)
        self.assertEqual(FrontendAssets(self.app).manifest["workbench_revision"], self.pin["revision"])

    def test_latest_success_publishes_complete_pair_and_exact_pin(self):
        builder.build(self.root)
        updated = {**self.pin, "revision": "a" * 40}
        def prepare(root, source, *, pin, destination):
            shutil.copytree(self.library, destination)
            manifest = json.loads((destination / "manifest.json").read_text())
            manifest.update(pin)
            (destination / "manifest.json").write_text(json.dumps(manifest))
        with patch.object(builder, "latest_pin", return_value=updated), patch.object(builder, "ensure_ui", side_effect=prepare):
            builder.build(self.root, latest=True)
        self.assertEqual(json.loads((self.root / "workbench-ui.json").read_text()), updated)
        self.assertEqual(FrontendAssets(self.app).manifest["workbench_revision"], updated["revision"])
        builder.verify_offline(self.library, updated)
        self.assertFalse(list((self.root / ".local").glob(".build*")))

    def test_source_archive_adds_complete_verified_assets_and_rejects_corruption(self):
        builder.build(self.root)
        destination = self.root / "release/source.zip"
        def git_archive(command, **kwargs):
            with zipfile.ZipFile(command[-1], "w") as archive:
                archive.writestr("local-activity-monitor/README.md", "fixture")
        with patch.object(source_archive.subprocess, "run", side_effect=git_archive) as git:
            source_archive.archive(self.root, destination)
            git.assert_called_once()
            with zipfile.ZipFile(destination) as archive:
                names = archive.namelist()
                self.assertEqual(len(names), len(set(names)))
                for name in (*FILES, "manifest.json"):
                    self.assertEqual(archive.read("local-activity-monitor/src/local_activity_monitor/_web/" + name), (self.app / name).read_bytes())
                self.assertIn("local-activity-monitor/src/local_activity_monitor/_workbench/workbench-ui.js", names)
            before = destination.read_bytes()
            (self.app / "app.js").write_text("corrupt")
            with self.assertRaises(ValueError):
                source_archive.archive(self.root, destination)
            self.assertEqual(destination.read_bytes(), before)
            git.assert_called_once()

    def test_stable_tags_sort_numerically_and_peel_annotated_tags(self):
        rows = ["1"*40 + "\trefs/tags/v0.9.0", "2"*40 + "\trefs/tags/v0.10.0", "3"*40 + "\trefs/tags/v0.10.0^{}", "4"*40 + "\trefs/tags/v1.0.0-rc.1", "5"*40 + "\trefs/heads/main"]
        with patch.object(builder.latest_pin.__globals__["shutil"], "which", side_effect=lambda name: name), patch.dict(builder.latest_pin.__globals__, {"run_command": lambda *args, **kwargs: "\n".join(rows)}):
            self.assertEqual(builder.latest_pin(self.root)["revision"], "3"*40)

    def test_one_point_zero_requires_a_published_non_preview_release(self):
        rows = "1"*40 + "\trefs/tags/v0.3.0\n" + "2"*40 + "\trefs/tags/v1.0.0\n" + "3"*40 + "\trefs/tags/v1.1.0"
        release = {"tag_name": "v1.0.0", "draft": False, "prerelease": False, "published_at": "2026-10-07T00:00:00Z"}
        def command(args, **kwargs):
            return rows if "ls-remote" in args else json.dumps(release)
        with patch.object(builder.latest_pin.__globals__["shutil"], "which", side_effect=lambda name: name), patch.dict(builder.latest_pin.__globals__, {"run_command": command}):
            self.assertEqual(builder.latest_pin(self.root)["revision"], "2"*40)
            for field in ("draft", "prerelease"):
                release[field] = True
                with self.assertRaises(builder.PreparationError):
                    builder.latest_pin(self.root)
                release[field] = False
            release["tag_name"] = "v0.3.0"
            self.assertEqual(builder.latest_pin(self.root), self.pin)

    def test_latest_tags_do_not_downgrade_authorized_newer_commit(self):
        rows = "1"*40 + "\trefs/tags/v0.3.0"
        with patch.object(builder.latest_pin.__globals__["shutil"], "which", side_effect=lambda name: name), patch.dict(builder.latest_pin.__globals__, {"run_command": lambda *args, **kwargs: rows}):
            self.assertEqual(builder.latest_pin(self.root), self.pin)

    def test_explicit_revision_is_validated_and_does_not_resolve_tags(self):
        with patch.object(builder, "latest_pin") as remote:
            with self.assertRaises(builder.PreparationError):
                builder.build(self.root, revision="invalid")
            with self.assertRaises(builder.PreparationError):
                builder.build(self.root, revision="a"*40, latest=True)
            remote.assert_not_called()
