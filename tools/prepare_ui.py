"""Prepare pinned shared UI; network access requires explicit --ensure or --update."""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import tempfile

_REPOSITORY = "gaze9999/workbench-ui"
_REMOTE = "https://github.com/" + _REPOSITORY + ".git"
_TIMEOUT = 120
_GUIDANCE = "可使用內附 UI 資產的 LAM 官方發行包, 或透過 --source 指定可用的 Workbench UI 來源"


class PreparationError(Exception):
    """Safe failure that never contains subprocess output."""


def read_pin(root):
    try:
        value = json.loads((root / "workbench-ui.json").read_text(encoding="utf-8"))
    except (OSError, ValueError):
        raise PreparationError("無法讀取 Workbench UI 版本 pin") from None
    if (not isinstance(value, dict) or set(value) != {"version", "repository", "revision"}
            or type(value.get("version")) is not int or value["version"] != 1
            or value.get("repository") != _REPOSITORY
            or not isinstance(value.get("revision"), str) or not re.fullmatch(r"[a-f0-9]{40}", value["revision"])):
        raise PreparationError("Workbench UI 版本 pin 格式無效")
    return value


def run_command(command, *, env=None, timeout=_TIMEOUT):
    try:
        return subprocess.run(command, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout,
                              check=True, shell=False, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0)).stdout.strip()
    except subprocess.TimeoutExpired:
        raise PreparationError("取得或準備 Workbench UI 逾時") from None
    except FileNotFoundError:
        raise PreparationError("找不到 Git 或必要的本機執行程式") from None
    except (OSError, subprocess.CalledProcessError):
        raise PreparationError("無法取得或準備 Workbench UI, 請確認既有 Git 認證與網路連線") from None


def git_command(git, source, *args, env=None):
    return run_command([git, "-c", "protocol.file.allow=never", "-c", "protocol.ext.allow=never",
                        "-c", "core.hooksPath=" + os.devnull, "-C", str(source), *args], env=env)


def unsafe_path(path):
    if path.is_symlink():
        return True
    try:
        return bool(getattr(path.lstat(), "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 0))
    except FileNotFoundError:
        return False


def remove_temporary(path):
    def writable_remove(function, filename, error):
        target = Path(filename)
        if unsafe_path(target) or not target.resolve().is_relative_to(path.resolve()):
            raise error[1]
        target.chmod(stat.S_IWRITE | stat.S_IREAD)
        function(filename)
    shutil.rmtree(path, onerror=writable_remove)


def verify_checkout(git, source, pin, env):
    if unsafe_path(source) or not source.is_dir() or unsafe_path(source / ".git") or not (source / ".git").is_dir():
        raise PreparationError("Workbench UI 來源或快取位置無效")
    if git_command(git, source, "rev-parse", "HEAD", env=env) != pin["revision"]:
        raise PreparationError("Workbench UI 來源或快取與固定 revision 不符")
    if git_command(git, source, "remote", "get-url", "origin", env=env) not in (_REMOTE, _REMOTE.removesuffix(".git")):
        raise PreparationError("Workbench UI 來源或快取的 origin 不符")
    if git_command(git, source, "status", "--porcelain", "--untracked-files=all", env=env):
        raise PreparationError("Workbench UI 來源或快取含未提交修改, 已保留原內容")
    loader = source / "integrations/python/workbench_assets.py"
    if loader.is_symlink() or not loader.is_file() or loader.resolve().parent != source.resolve() / "integrations/python":
        raise PreparationError("固定 revision 缺少安全的 UI 資產準備程式")
    return loader


def bundle_files(loader):
    """Read the pinned helper's static allowlist without executing it."""
    if loader.is_symlink() or not loader.is_file() or loader.stat().st_size > 256 * 1024:
        raise PreparationError("離線 UI loader 不完整或不安全")
    try:
        tree = ast.parse(loader.read_text(encoding="utf-8"))
        files = next(ast.literal_eval(item.value) for item in tree.body if isinstance(item, ast.Assign)
                     and any(isinstance(target, ast.Name) and target.id == "FILES" for target in item.targets))
        if (not isinstance(files, tuple) or not 1 <= len(files) <= 16 or len(set(files)) != len(files)
                or any(not isinstance(name, str) or not re.fullmatch(r"workbench-[a-z-]+\.(?:js|mjs|css)", name) for name in files)):
            raise ValueError()
    except (OSError, ValueError, SyntaxError, TypeError, StopIteration, RecursionError):
        raise PreparationError("無法確認固定版本的 UI 資產白名單") from None
    return files


def verify_offline(destination, pin, *, loader=None):
    if unsafe_path(destination) or not destination.is_dir():
        raise PreparationError("既有離線 UI 目錄無效, 已保留原內容")
    files = bundle_files(loader or destination / "__init__.py")
    allowed = {"__init__.py", "manifest.json", *files, "__pycache__"}
    if any(path.name not in allowed or unsafe_path(path) for path in destination.iterdir()):
        raise PreparationError("既有離線 UI 含未知檔案, 已保留原內容")
    try:
        path = destination / "manifest.json"
        if not path.is_file() or path.stat().st_size > 65536:
            raise ValueError()
        manifest = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(manifest, dict) or any(manifest.get(key) != value for key, value in pin.items()):
            raise ValueError()
        hashes = manifest.get("sha256")
        if not isinstance(hashes, dict) or set(hashes) != set(files):
            raise ValueError()
        for name in files:
            path, digest = destination / name, hashes[name]
            if (not path.is_file() or path.stat().st_size > 8 * 1024 * 1024
                    or not isinstance(digest, str) or not re.fullmatch(r"[a-f0-9]{64}", digest)
                    or hashlib.sha256(path.read_bytes()).hexdigest() != digest):
                raise ValueError()
        if not (destination / "__init__.py").is_file():
            raise ValueError()
    except (OSError, ValueError, KeyError, RecursionError):
        raise PreparationError("既有離線 UI 不完整或 checksum/pin 不符, 已保留原內容") from None


def offline_pin(destination, pin):
    """Validate an existing bundle against its own revision before rebuilding."""
    try:
        path = destination / "manifest.json"
        if unsafe_path(destination) or unsafe_path(path) or not path.is_file() or path.stat().st_size > 65536:
            raise ValueError()
        revision = json.loads(path.read_text(encoding="utf-8"))["revision"]
        if not isinstance(revision, str) or not re.fullmatch(r"[a-f0-9]{40}", revision):
            raise ValueError()
    except (OSError, ValueError, KeyError, TypeError, RecursionError):
        raise PreparationError("既有離線 UI 版本資訊無效, 已保留原內容") from None
    recorded = {**pin, "revision": revision}
    verify_offline(destination, recorded)
    return recorded


def latest_pin(root):
    """Resolve the highest stable version tag, including annotated tag peeling."""
    pin = read_pin(root)
    current = None
    library = Path(root) / "src/local_activity_monitor/_workbench"
    if library.is_dir():
        offline_pin(library, pin)
        match = re.search(r'global\.WorkbenchUI\s*=\s*Object\.freeze\(\{\s*version:\s*"((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))"', (library / "workbench-ui.js").read_text(encoding="utf-8"))
        if not match:
            raise PreparationError("無法確認目前 WBUI 版本, 沿用已記錄版本")
        current = tuple(map(int, match[1].split(".")))
    git = shutil.which("git")
    if not git:
        raise PreparationError("找不到 Git, 無法檢查 Workbench UI 最新版本")
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0", GCM_INTERACTIVE="never")
    output = run_command([git, "-c", "protocol.file.allow=never", "-c", "protocol.ext.allow=never",
                          "-c", "core.hooksPath=" + os.devnull, "ls-remote", "--tags", "--", _REMOTE], env=env, timeout=20)
    tags = {}
    for line in output.splitlines():
        match = re.fullmatch(r"([a-f0-9]{40})\s+refs/tags/v((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))(\^\{\})?", line)
        if match:
            revision, version, peeled = match.groups()
            key = tuple(map(int, version.split(".")))
            if key not in tags or peeled:
                tags[key] = revision
    if not tags:
        raise PreparationError("Workbench UI 尚未提供正式版本 tag")
    if current is not None and max(tags) < current:
        return pin
    development = {key: revision for key, revision in tags.items() if key[0] == 0}
    if any(key[0] >= 1 for key in tags):
        gh = shutil.which("gh")
        if not gh:
            raise PreparationError("Workbench UI 1.0 以上需用已認證的 GitHub CLI 確認已發布 Release")
        env["GH_PROMPT_DISABLED"] = "1"
        try:
            release = json.loads(run_command([gh, "api", "--hostname", "github.com", "repos/" + _REPOSITORY + "/releases/latest"], env=env, timeout=20))
            if not isinstance(release, dict) or release.get("draft") is not False or release.get("prerelease") is not False:
                raise ValueError()
            match = re.fullmatch(r"v((?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*))", str(release.get("tag_name", "")))
            key = tuple(map(int, match[1].split("."))) if match else None
            if key not in tags or not release.get("published_at"):
                raise ValueError()
            if key[0] >= 1:
                if current is not None and key < current:
                    return pin
                return {**pin, "revision": tags[key]}
        except (ValueError, TypeError, KeyError):
            raise PreparationError("Workbench UI Release 資訊無效, 保留已驗證版本") from None
    if not development:
        raise PreparationError("Workbench UI 尚未提供可用的正式 Release")
    if current is not None and max(development) < current:
        return pin
    return {**pin, "revision": development[max(development)]}


def ensure_ui(root, source, *, pin=None, destination=None):
    root, source = Path(root).resolve(), Path(source).expanduser().resolve()
    recorded = read_pin(root)
    pin = recorded if pin is None else pin
    destination = Path(destination) if destination is not None else root / "src/local_activity_monitor/_workbench"
    if unsafe_path(destination) or not destination.resolve().is_relative_to(root):
        raise PreparationError("離線 UI 位置不安全")
    if destination.exists() or destination.is_symlink():
        verify_offline(destination, pin)
        return "已驗證既有離線 Workbench UI " + pin["revision"]
    git = shutil.which("git")
    if not git:
        raise PreparationError("找不到 Git, 無法自動取得 Workbench UI")
    env = dict(os.environ)
    env.update(GIT_TERMINAL_PROMPT="0", GCM_INTERACTIVE="never")
    loader = source / "integrations/python/workbench_assets.py"
    usable = False
    if loader.is_file():
        try:
            loader = verify_checkout(git, source, pin, env)
            usable = True
        except PreparationError:
            pass  # A neighboring owner's checkout is never mutated.
    cache_parent, temporary = root / ".local/workbench-ui", None
    cache = cache_parent / pin["revision"]
    try:
        if not usable:
            if unsafe_path(cache_parent) or unsafe_path(root / ".local") or cache_parent.resolve() != root / ".local/workbench-ui":
                raise PreparationError("UI 快取位置不安全")
            cache_parent.mkdir(parents=True, exist_ok=True)
            if cache_parent.resolve() != root / ".local/workbench-ui":
                raise PreparationError("UI 快取位置不安全")
            if cache.exists() or cache.is_symlink():
                loader = verify_checkout(git, cache, pin, env)
            else:
                temporary = Path(tempfile.mkdtemp(prefix=".prepare-", dir=cache_parent))
                cloned = temporary / "source"
                run_command([git, "-c", "protocol.file.allow=never", "-c", "protocol.ext.allow=never",
                             "-c", "core.hooksPath=" + os.devnull, "clone", "--no-checkout", "--filter=blob:none",
                             "--", _REMOTE, str(cloned)], env=env)
                git_command(git, cloned, "checkout", "--detach", pin["revision"], env=env)
                verify_checkout(git, cloned, pin, env)
                if cache.exists() or cache.is_symlink():
                    raise PreparationError("UI 快取位置已存在, 已保留原內容")
                cloned.rename(cache)
                loader = cache / "integrations/python/workbench_assets.py"
            source = cache
        if temporary is None:
            staging_parent = root / ".local"
            if unsafe_path(staging_parent) or staging_parent.resolve() != root / ".local":
                raise PreparationError("UI 暫存位置不安全")
            staging_parent.mkdir(exist_ok=True)
            if staging_parent.resolve() != root / ".local":
                raise PreparationError("UI 暫存位置不安全")
            temporary = Path(tempfile.mkdtemp(prefix=".prepare-ui-", dir=staging_parent))
        project = root
        if pin != recorded:
            project = temporary / "project"
            project.mkdir()
            (project / "workbench-ui.json").write_text(json.dumps(pin), encoding="utf-8")
        staged = project / ".local/staged-ui" if project != root else temporary / "assets"
        run_command([sys.executable, "-B", str(loader), "--project", str(project),
                     "--destination", str(staged), "--source", str(source)], env=env)
        verify_offline(staged, pin, loader=loader)
        if destination.exists() or destination.is_symlink():
            raise PreparationError("離線 UI 位置已存在, 已保留原內容")
        if not destination.parent.resolve().is_relative_to(root):
            raise PreparationError("離線 UI 位置不安全")
        destination.parent.mkdir(parents=True, exist_ok=True)
        if unsafe_path(destination.parent) or not destination.parent.resolve().is_relative_to(root):
            raise PreparationError("離線 UI 位置不安全")
        staged.rename(destination)
        return "已準備固定版本 Workbench UI " + pin["revision"]
    finally:
        if temporary is not None and temporary.resolve().is_relative_to(root / ".local") and not temporary.is_symlink():
            remove_temporary(temporary)


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(os.environ.get("WORKBENCH_UI_PATH", root.parent / "workbench-ui")))
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--update", action="store_true", help="Explicitly pull the latest shared UI and update workbench-ui.json")
    mode.add_argument("--ensure", action="store_true", help="Obtain only the pinned UI using existing Git authentication if needed")
    args = parser.parse_args()
    if args.ensure:
        try:
            print(ensure_ui(root, args.source))
        except (PreparationError, OSError) as error:
            message = str(error) if isinstance(error, PreparationError) else "無法準備本機 UI 資產"
            parser.exit(1, message + ". " + _GUIDANCE + "\n")
        return
    loader = args.source.expanduser().resolve() / "integrations/python/workbench_assets.py"
    if not loader.is_file():
        if not args.update:
            try:
                pin = read_pin(root)
                verify_offline(root / "src/local_activity_monitor/_workbench", pin)
                print("Verified existing offline Workbench UI " + pin["revision"])
                return
            except PreparationError as error:
                parser.exit(1, "Workbench UI source or valid offline assets are required: " + str(error) + "\n")
        parser.exit(1, "Workbench UI source is missing. Clone it next to this checkout or provide --source. No assets were downloaded.\n")
    subprocess.run([sys.executable, "-B", str(loader), "--project", str(root), "--destination", str(root / "src/local_activity_monitor/_workbench"), "--source", str(args.source.resolve()), *(["--update"] if args.update else [])], check=True)


if __name__ == "__main__":
    main()
