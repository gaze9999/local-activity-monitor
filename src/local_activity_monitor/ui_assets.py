"""Bind the shared loader to checkout development or generated offline assets."""
import importlib.util
from functools import lru_cache
import os
from pathlib import Path
import sys


@lru_cache(maxsize=1)
def load_ui_assets():
    package = Path(__file__).resolve().parent
    project = package.parents[1]
    source = Path(os.environ.get("WORKBENCH_UI_PATH", project.parent / "workbench-ui")).expanduser().resolve()
    loader = source / "integrations/python/workbench_assets.py"
    if not getattr(sys, "frozen", False) and (project / "workbench-ui.json").is_file() and (loader.is_file() or "WORKBENCH_UI_PATH" in os.environ):
        if loader.is_symlink() or loader.resolve().parent != source / "integrations/python":
            raise ValueError("Unsafe Workbench UI loader")
        spec = importlib.util.spec_from_file_location("workbench_assets", loader)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module.Assets(source / "src")
    try:
        from . import _workbench
    except ImportError:
        raise RuntimeError("Workbench UI is missing. Clone workbench-ui next to this checkout, set WORKBENCH_UI_PATH, or prepare offline assets with tools/prepare_ui.py") from None
    return _workbench.Assets(package / "_workbench", verify=True)
