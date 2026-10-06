"""Run checkout source with the selected interpreter, without installing packages."""
from pathlib import Path
import subprocess
import sys


def main(args=None):
    args = sys.argv[1:] if args is None else args
    if sys.version_info < (3, 10):
        print("Python 3.10 or newer is required.", file=sys.stderr)
        return 1
    root = Path(__file__).resolve().parents[1]
    try:
        sys.path.insert(0, str(root / "src"))
        from local_activity_monitor.ui_assets import MissingUIAssetsError, load_ui_assets
        try:
            load_ui_assets()
        except MissingUIAssetsError:
            print("找不到 Workbench UI, 正在用既有 Git 認證準備指定版本的離線資產", file=sys.stderr)
            prepared = subprocess.run([sys.executable, "-I", "-B", str(root / "tools/prepare_ui.py"), "--ensure"], cwd=root)
            if prepared.returncode:
                return prepared.returncode
        return subprocess.run(
            [sys.executable, "-I", "-B", str(root/"tools/watch.py"), "--codex", "--open", *args],
            cwd=root,
        ).returncode
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Startup failed: {error}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    raise SystemExit(main())
