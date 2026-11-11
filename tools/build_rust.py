"""Build the optional session scanner for this source checkout."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import sysconfig
import tempfile


ROOT = Path(__file__).resolve().parents[1]
MODULE = "_session_scan"


def main():
    if sys.implementation.name != "cpython" or sysconfig.get_config_var("Py_GIL_DISABLED"):
        raise SystemExit("The Rust session scanner requires a GIL-enabled CPython 3.10+.")
    if sys.platform == "win32":
        library, extension = f"{MODULE}.dll", f"{MODULE}.pyd"
    elif sys.platform == "darwin":
        library, extension = f"lib{MODULE}.dylib", f"{MODULE}.abi3.so"
    elif sys.platform == "linux":
        library = f"lib{MODULE}.so"
        extension = f"{MODULE}.abi3.so"
    else:
        raise SystemExit(f"Unsupported platform: {sys.platform}")
    target = ROOT / "build/rust"
    subprocess.run(
        ["cargo", "build", "--locked", "--release", "--manifest-path", "rust/session-scan/Cargo.toml",
         "--target-dir", str(target)],
        cwd=ROOT, env=os.environ | {"PYO3_PYTHON": sys.executable}, check=True,
    )
    destination = ROOT / "src/local_activity_monitor" / extension
    with tempfile.TemporaryDirectory(dir=destination.parent) as directory:
        staged = Path(directory) / extension
        shutil.copyfile(target / "release" / library, staged)
        staged.replace(destination)
    print(destination)


if __name__ == "__main__":
    main()
