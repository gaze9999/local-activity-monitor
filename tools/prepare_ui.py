"""Prepare pinned shared assets without downloading or installing dependencies."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(os.environ.get("WORKBENCH_UI_PATH", root.parent / "workbench-ui")))
    parser.add_argument("--update", action="store_true", help="Explicitly pull the latest shared UI and update workbench-ui.json")
    args = parser.parse_args()
    loader = args.source.expanduser().resolve() / "integrations/python/workbench_assets.py"
    if not loader.is_file():
        if not args.update:
            sys.path.insert(0, str(root / "src"))
            try:
                from local_activity_monitor import _workbench
                library = _workbench.Assets(root / "src/local_activity_monitor/_workbench", verify=True)
                pin = _workbench.read_lock(root)
                manifest = json.loads((library.root / "manifest.json").read_text(encoding="utf-8"))
                if any(pin[key] != manifest[key] for key in pin):
                    raise ValueError("Embedded UI does not match workbench-ui.json")
                print("Verified existing offline Workbench UI " + pin["revision"])
                return
            except (ImportError, OSError, ValueError, KeyError) as error:
                parser.exit(1, "Workbench UI source or valid offline assets are required: " + str(error) + "\n")
        parser.exit(1, "Workbench UI source is missing. Clone it next to this checkout or provide --source. No assets were downloaded.\n")
    subprocess.run([sys.executable, "-B", str(loader), "--project", str(root), "--destination", str(root / "src/local_activity_monitor/_workbench"), "--source", str(args.source.resolve()), *(["--update"] if args.update else [])], check=True)


if __name__ == "__main__":
    main()
