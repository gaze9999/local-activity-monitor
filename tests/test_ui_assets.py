"""Check live and offline shared UI loading, CSP bytes and static route guards."""
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import sys
import tempfile
import types
import unittest
from unittest.mock import MagicMock, patch

import local_activity_monitor
from local_activity_monitor import ui_assets
from local_activity_monitor.server import Dashboard, handler

SOURCE = ui_assets.load_ui_assets().root
LOADER = SOURCE / "__init__.py" if (SOURCE / "__init__.py").is_file() else SOURCE.parent / "integrations/python/workbench_assets.py"
spec = importlib.util.spec_from_file_location("fixture_workbench", LOADER)
loader = importlib.util.module_from_spec(spec)
spec.loader.exec_module(loader)


class UIAssetTests(unittest.TestCase):
    def tearDown(self):
        ui_assets.load_ui_assets.cache_clear()

    def test_checkout_uses_one_shared_loader_and_does_not_download(self):
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder).resolve()
            (source / "integrations/python").mkdir(parents=True)
            (source / "src").mkdir()
            shutil.copyfile(LOADER, source / "integrations/python/workbench_assets.py")
            for name in loader.FILES:
                shutil.copyfile(SOURCE / name, source / "src" / name)
            ui_assets.load_ui_assets.cache_clear()
            with patch.dict(os.environ, {"WORKBENCH_UI_PATH": str(source)}), patch("subprocess.run") as run:
                first = ui_assets.load_ui_assets()
                self.assertIs(ui_assets.load_ui_assets(), first)
                self.assertEqual(first.path("workbench-ui.js"), source / "src/workbench-ui.js")
                run.assert_not_called()

    def test_frozen_bundle_uses_embedded_assets_and_checks_hashes(self):
        with tempfile.TemporaryDirectory() as folder:
            package = Path(folder).resolve() / "src/local_activity_monitor"
            bundle = package / "_workbench"
            bundle.mkdir(parents=True)
            hashes = {}
            for name in loader.FILES:
                shutil.copyfile(SOURCE / name, bundle / name)
                hashes[name] = hashlib.sha256((bundle / name).read_bytes()).hexdigest()
            (bundle / "manifest.json").write_text(json.dumps({"version": 1, "revision": "a" * 40, "sha256": hashes}))
            with patch.object(ui_assets, "__file__", str(package / "ui_assets.py")), patch.object(sys, "frozen", True, create=True), patch.object(local_activity_monitor, "_workbench", loader, create=True):
                ui_assets.load_ui_assets.cache_clear()
                self.assertEqual(ui_assets.load_ui_assets().path("workbench-ui.css"), bundle / "workbench-ui.css")
                (bundle / "workbench-ui.css").write_text("changed")
                ui_assets.load_ui_assets.cache_clear()
                with self.assertRaises(ValueError):
                    ui_assets.load_ui_assets()

    def test_incomplete_embedded_loader_is_preserved_without_setup(self):
        with tempfile.TemporaryDirectory() as folder:
            package = Path(folder).resolve() / "src/local_activity_monitor"
            bundle = package / "_workbench"
            bundle.mkdir(parents=True)
            marker = bundle / "user-file"
            marker.write_text("keep")
            with patch.object(ui_assets, "__file__", str(package / "ui_assets.py")), patch.object(sys, "frozen", True, create=True), patch.object(local_activity_monitor, "_workbench", types.SimpleNamespace(), create=True), patch("subprocess.run") as run:
                ui_assets.load_ui_assets.cache_clear()
                with self.assertRaisesRegex(ValueError, "incomplete"):
                    ui_assets.load_ui_assets()
                run.assert_not_called()
            self.assertEqual(marker.read_text(), "keep")

    def test_static_routes_and_inline_csp_use_shared_source(self):
        ui_assets.load_ui_assets.cache_clear()
        with tempfile.TemporaryDirectory() as folder:
            request = object.__new__(handler(Dashboard(Path(folder).resolve()), 8787))
            request.headers = {"Host": "127.0.0.1:8787"}
            request.reply = MagicMock()
            for name in ("workbench-ui.css", "workbench-ui.js"):
                request.path = "/" + name
                request.do_GET()
                self.assertEqual(request.reply.call_args.args[1], (SOURCE / name).read_bytes())
            request.path = "/"
            request.do_GET()
            result = request.reply.call_args
            self.assertEqual(result.args[0], 200)
            script = re.search(rb"<script>(.*?)</script>", result.args[1], re.S).group(1)
            style = re.search(rb"<style>(.*?)</style>", result.args[1], re.S).group(1)
            self.assertEqual(base64.b64encode(hashlib.sha256(script).digest()).decode(), result.kwargs["script_hash"])
            self.assertEqual(base64.b64encode(hashlib.sha256(style).digest()).decode(), result.kwargs["style_hash"])
            for path in ("/../workbench-ui.json", "/manifest.json", "/workbench_assets.py"):
                request.path = path
                request.do_GET()
                self.assertEqual(request.reply.call_args.args[0], 404)
            request.headers = {"Host": "attacker.invalid"}
            request.do_GET()
            self.assertEqual(request.reply.call_args.args[0], 403)

    def test_revision_detects_shared_file_updates_without_rereading_unchanged_bytes(self):
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder).resolve() / "source"
            source.mkdir()
            for name in loader.FILES:
                shutil.copyfile(SOURCE / name, source / name)
            library = loader.Assets(source)
            with patch("local_activity_monitor.server.load_ui_assets", return_value=library):
                dashboard = Dashboard(Path(folder).resolve() / "home")
                before = dashboard.web_revision()
                with patch.object(Path, "read_bytes", side_effect=AssertionError("unchanged assets reread")):
                    self.assertEqual(dashboard.web_revision(), before)
                with (source / "workbench-ui.css").open("a") as stream:
                    stream.write("\n/* changed fixture */")
                self.assertNotEqual(dashboard.web_revision(), before)


if __name__ == "__main__":
    unittest.main()
