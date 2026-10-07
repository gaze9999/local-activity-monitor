"""Build LAM's dependency-free frontend with verified shared UI assets."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from prepare_ui import PreparationError, ensure_ui, latest_pin, read_pin, remove_temporary, unsafe_path, verify_offline
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))
from local_activity_monitor.frontend_assets import FILES, FrontendAssets


def build(root, source=None, *, latest=False, revision=None):
    root = Path(root).resolve()
    source = Path(source or os.environ.get("WORKBENCH_UI_PATH", root.parent / "workbench-ui"))
    pin = read_pin(root)
    library, destination = root / "src/local_activity_monitor/_workbench", root / "src/local_activity_monitor/_web"
    if revision is not None and (latest or not re.fullmatch(r"[a-f0-9]{40}", revision)):
        raise PreparationError("明確 revision 需為完整 SHA, 並與 --latest 分開使用")
    if library.exists() or library.is_symlink():
        verify_offline(library, pin)
    if destination.exists() or destination.is_symlink():
        if unsafe_path(destination):
            raise PreparationError("LAM 頁面建置位置不安全")
        existing = FrontendAssets(destination)
    else:
        existing = None
    if latest and not library.exists():
        # Prepare the committed baseline first, so a consumer-authorized newer
        # commit cannot be downgraded to an older tag on a clean checkout.
        ensure_ui(root, source)
    selected = {**pin, "revision": revision} if revision is not None else pin
    if latest:
        try:
            selected = latest_pin(root)
        except PreparationError as error:
            print(str(error) + ", 沿用已記錄版本", file=sys.stderr)
    inputs = {}
    for name in FILES:
        path = root / "frontend" / name
        if unsafe_path(path) or path.resolve().parent != root / "frontend" or not path.is_file() or path.stat().st_size > 8 * 1024 * 1024:
            raise PreparationError("LAM 頁面來源不完整或不安全: " + name)
        inputs[name] = path.read_bytes()
    # Parse application data before replacing any known-good bundle.
    json.loads(inputs["locales.json"].decode("utf-8"))
    for name in ("index.html", "app.js", "style.css", "favicon.svg"):
        inputs[name].decode("utf-8")
    fingerprint = hashlib.sha256(json.dumps({"format": 1, "revision": selected["revision"],
                            "sources": {name: hashlib.sha256(content).hexdigest() for name, content in inputs.items()}}, sort_keys=True).encode()).hexdigest()
    if existing and existing.manifest["inputs"] == fingerprint and library.is_dir():
        return "LAM 頁面已是最新建置, Workbench UI " + selected["revision"][:12]
    staging = root / ".local"
    if unsafe_path(staging) or staging.resolve() != root / ".local":
        raise PreparationError("頁面建置暫存位置不安全")
    staging.mkdir(exist_ok=True)
    if staging.resolve() != root / ".local":
        raise PreparationError("頁面建置暫存位置不安全")
    temporary = Path(tempfile.mkdtemp(prefix=".build-frontend-", dir=staging))
    installed = []
    try:
        candidate = library
        if selected != pin or not library.exists():
            candidate = temporary / "_workbench"
            try:
                ensure_ui(root, source, pin=selected, destination=candidate)
            except (PreparationError, OSError):
                if selected == pin or not library.exists():
                    raise
                print("無法準備新版 Workbench UI, 保留目前版本", file=sys.stderr)
                return build(root, source)
        app = temporary / "_web"
        app.mkdir()
        for name, content in inputs.items():
            (app / name).write_bytes(content)
        manifest = {"version": 1, "inputs": fingerprint, "workbench_revision": selected["revision"],
                    "sha256": {name: hashlib.sha256(content).hexdigest() for name, content in inputs.items()}}
        (app / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        FrontendAssets(app)
        if read_pin(root) != pin:
            raise PreparationError("建置期間 WBUI 版本已變更, 保留既有內容")
        if library.exists():
            verify_offline(library, pin)
        if destination.exists():
            FrontendAssets(destination)
        # The server composes the validated JS/CSS into one CSP-hashed response.
        # Publish only complete directories, restoring the old pair if a move fails.
        for target, prepared in ((library, candidate), (destination, app)):
            if target == prepared:
                continue
            if unsafe_path(target.parent) or not target.parent.resolve().is_relative_to(root):
                raise PreparationError("頁面建置位置不安全")
            target.parent.mkdir(parents=True, exist_ok=True)
            backup = temporary / (target.name + "-previous")
            if target.exists():
                target.rename(backup)
            installed.append((target, backup))
            prepared.rename(target)
        if selected != pin:
            lock = temporary / "workbench-ui.json"
            lock.write_text(json.dumps(selected, indent=2) + "\n", encoding="utf-8")
            os.replace(lock, root / "workbench-ui.json")
        return "已建置 LAM 頁面, Workbench UI " + selected["revision"][:12]
    except BaseException:
        for target, backup in reversed(installed):
            if target.exists():
                target.rename(temporary / (target.name + "-failed"))
            if backup.exists():
                backup.rename(target)
        raise
    finally:
        remove_temporary(temporary)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--latest", action="store_true", help="Check the latest stable WBUI tag using existing Git authentication")
    selection.add_argument("--revision", help="Build an explicitly selected full WBUI commit SHA")
    parser.add_argument("--source", type=Path)
    args = parser.parse_args()
    try:
        print(build(Path(__file__).resolve().parents[1], args.source, latest=args.latest, revision=args.revision))
    except (PreparationError, OSError, ValueError, RuntimeError) as error:
        parser.exit(1, "頁面建置失敗: " + str(error) + "\n")


if __name__ == "__main__":
    main()
