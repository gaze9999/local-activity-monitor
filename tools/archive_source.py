"""Append only verified generated UI files to the tracked source release archive."""
from pathlib import Path
import subprocess
import sys
import zipfile


def archive(root, destination):
    root = Path(root).resolve()
    bundle = root / "src/local_activity_monitor/_workbench"
    sys.path.insert(0, str(root / "src"))
    from local_activity_monitor import _workbench
    _workbench.Assets(bundle, verify=True)
    destination = Path(destination).resolve()
    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(["git", "-C", str(root), "archive", "--format=zip", "--prefix=local-activity-monitor/", "HEAD", "-o", str(destination)], check=True)
    with zipfile.ZipFile(destination, "a", compression=zipfile.ZIP_DEFLATED) as package:
        for name in _workbench.BUNDLE_FILES:
            path = bundle / name
            package.write(path, "local-activity-monitor/" + path.relative_to(root).as_posix())


if __name__ == "__main__":
    archive(Path(__file__).resolve().parents[1], sys.argv[1])
