"""Bind the shared loader to checkout development or generated offline assets."""
import importlib.util
from functools import lru_cache
import os
from pathlib import Path
import sys


class MissingUIAssetsError(RuntimeError):
    pass


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
        raise MissingUIAssetsError("Workbench UI is missing. Run the CLI launcher to prepare the pinned source automatically, use a complete LAM release, or set WORKBENCH_UI_PATH to an existing checkout.") from None
    if not callable(getattr(_workbench, "Assets", None)):
        raise ValueError("Embedded Workbench UI is incomplete. Existing files were preserved; restore the assets from a complete LAM release.")
    return _workbench.Assets(package / "_workbench", verify=True)
