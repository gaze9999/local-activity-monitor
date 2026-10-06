import importlib.util
import os
from pathlib import Path
import plistlib
import struct
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import Mock, patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("monitor_release", ROOT / "tools/build_release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseAssemblyTests(unittest.TestCase):
    def test_local_build_cannot_use_release_mode(self):
        with patch.dict(os.environ, {"GITHUB_ACTIONS": "false"}), patch.object(sys, "argv", ["build_release.py", "--release"]), self.assertRaises(SystemExit) as result:
            release.main()
        self.assertEqual(result.exception.code, 2)

