"""Validated generated application assets; runtime never retrieves or builds sources."""
import hashlib
from functools import lru_cache
import json
from pathlib import Path
import re
import stat

FILES = ("index.html", "app.js", "style.css", "locales.json", "favicon.svg", "favicon.ico")


class FrontendAssets:
    def __init__(self, root=None):
        target = Path(root or Path(__file__).parent / "_web")
        if target.exists() and self.unsafe(target):
            raise ValueError("Unsafe frontend directory")
        self.root = target.resolve()
        manifest = self.root / "manifest.json"
        if not manifest.is_file():
            raise RuntimeError("LAM 頁面尚未建置, 請使用 launch-cli 或執行 python tools/build_frontend.py")
        if self.unsafe(manifest) or manifest.stat().st_size > 65536:
            raise ValueError("Invalid frontend manifest")
        value = json.loads(manifest.read_text(encoding="utf-8"))
        if (not isinstance(value, dict) or type(value.get("version")) is not int or value["version"] != 1
                or not re.fullmatch(r"[a-f0-9]{64}", str(value.get("inputs", "")))
                or not re.fullmatch(r"[a-f0-9]{40}", str(value.get("workbench_revision", "")))
                or not isinstance(value.get("sha256"), dict) or set(value["sha256"]) != set(FILES)):
            raise ValueError("Invalid frontend manifest")
        if any(path.name not in {*FILES, "manifest.json"} or self.unsafe(path) for path in self.root.iterdir()):
            raise ValueError("Unknown or unsafe generated frontend files; existing contents preserved")
        for name in FILES:
            path = self.path(name)
            if path.stat().st_size > 8 * 1024 * 1024 or hashlib.sha256(path.read_bytes()).hexdigest() != value["sha256"][name]:
                raise ValueError("Frontend checksum mismatch: " + name)
        self.manifest = value

    @staticmethod
    def unsafe(path):
        return path.is_symlink() or bool(getattr(path.lstat(), "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))

    def path(self, name):
        if name not in FILES:
            raise ValueError("Unsupported frontend asset")
        path = self.root / name
        if self.unsafe(path) or path.resolve().parent != self.root or not path.is_file():
            raise ValueError("Missing or unsafe frontend asset: " + name)
        return path


@lru_cache(maxsize=1)
def load_frontend_assets():
    return FrontendAssets()
