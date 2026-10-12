"""Bind the shared loader to checkout development or generated offline assets."""
import importlib.util
from functools import lru_cache
import os
import json
from pathlib import Path
import re
import sys


class MissingUIAssetsError(RuntimeError):
    pass


def require_ui_version(path):
    if path.stat().st_size > 8 * 1024 * 1024:
        raise ValueError("Invalid Workbench UI asset size")
    match = re.search(r'global\.WorkbenchUI\s*=\s*Object\.freeze\(\{\s*version:\s*"((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))"', path.read_text(encoding="utf-8"))
    if not match or tuple(map(int, match[1].split("."))) < (0, 13, 0):
        raise ValueError("LAM requires Workbench UI 0.13.0 or later for subscribe(options, listener); update the exact pin and verified offline assets, or select a compatible development source")


def development_source(project):
    path = project / ".local/workbench-ui-development.json"
    if not path.exists():
        return None
    if path.is_symlink() or path.stat().st_size > 4096 or path.parent.resolve() != project / ".local":
        raise ValueError("Unsafe Workbench UI development selection")
    value = json.loads(path.read_text(encoding="utf-8"))
    pin = json.loads((project / "workbench-ui.json").read_text(encoding="utf-8"))
    if not isinstance(value, dict) or set(value) != {"version", "source", "revision"} or type(value["version"]) is not int or value["version"] != 1 or value["revision"] != pin["revision"] or not isinstance(value["source"], str):
        raise ValueError("Invalid Workbench UI development selection")
    source = Path(value["source"])
    if not source.is_absolute() or source.resolve() != source or source.is_symlink():
        raise ValueError("Unsafe Workbench UI development source")
    return source


@lru_cache(maxsize=1)
def load_ui_assets():
    package = Path(__file__).resolve().parent
    project = package.parents[1]
    selected = None if getattr(sys, "frozen", False) else development_source(project)
    source = Path(os.environ.get("WORKBENCH_UI_PATH", selected or project.parent / "workbench-ui")).expanduser().resolve()
    loader = source / "integrations/python/workbench_assets.py"
    if not getattr(sys, "frozen", False) and (project / "workbench-ui.json").is_file() and (selected or "WORKBENCH_UI_PATH" in os.environ or loader.is_file() and not (package / "_workbench").exists()):
        if loader.is_symlink() or loader.resolve().parent != source / "integrations/python":
            raise ValueError("Unsafe Workbench UI loader")
        spec = importlib.util.spec_from_file_location("workbench_assets", loader)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        assets = module.Assets(source / "src")
        require_ui_version(assets.path("workbench-ui.js"))
        return assets
    try:
        from . import _workbench
    except ImportError:
        raise MissingUIAssetsError("Workbench UI is missing. Run the CLI launcher to prepare the pinned source automatically, use a complete LAM release, or set WORKBENCH_UI_PATH to an existing checkout.") from None
    if not callable(getattr(_workbench, "Assets", None)):
        raise ValueError("Embedded Workbench UI is incomplete. Existing files were preserved; restore the assets from a complete LAM release.")
    assets = _workbench.Assets(package / "_workbench", verify=True)
    require_ui_version(assets.path("workbench-ui.js"))
    return assets
