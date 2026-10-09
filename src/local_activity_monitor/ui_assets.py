"""Bind the shared loader to checkout development or generated offline assets."""
import importlib.util
from functools import lru_cache
import os
import json
from pathlib import Path
import sys


class MissingUIAssetsError(RuntimeError):
    pass


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
        return module.Assets(source / "src")
    try:
        from . import _workbench
    except ImportError:
        raise MissingUIAssetsError("Workbench UI is missing. Run the CLI launcher to prepare the pinned source automatically, use a complete LAM release, or set WORKBENCH_UI_PATH to an existing checkout.") from None
    if not callable(getattr(_workbench, "Assets", None)):
        raise ValueError("Embedded Workbench UI is incomplete. Existing files were preserved; restore the assets from a complete LAM release.")
    return _workbench.Assets(package / "_workbench", verify=True)
