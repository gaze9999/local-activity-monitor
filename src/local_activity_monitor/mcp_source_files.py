"""Discover declared local MCP documents and edit bounded data files safely."""
from datetime import datetime, timezone
import ast
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile

from .operation_records import redact
from .payload_detail import parse_text, complete_payload

try:
    import tomllib
except ImportError:
    tomllib = None

READ_LIMIT = 65536
EDIT_LIMIT = 32768
DATA_TYPES = {".json", ".yaml", ".yml", ".toml", ".txt", ".md"}
PRIVATE = re.compile(r"(?:auth|credential|secret|token|password|private[-_]?key)|^\.env(?:\.|$)", re.I)


def safe_path(path):
    try:
        return path.is_absolute() and not str(path).startswith(("//", "\\\\")) and not PRIVATE.search(path.name) and not any(part.is_symlink() for part in (path, *path.parents))
    except OSError:
        return False


def source_roots(value):
    args = value.get("args", [])
    args = args[:32] if isinstance(args, list) else []
    candidates = [*args, value.get("command")]
    roots = []
    cwd = value.get("cwd")
    if isinstance(cwd, str) and len(cwd)<512:
        path = Path(cwd)
        if safe_path(path) and path.is_dir():
            roots.append(path)
    for item in candidates:
        if not isinstance(item, str) or len(item)>512:
            continue
        path = Path(item)
        if not safe_path(path) or not path.is_file() or path.suffix.lower() not in (".py", ".js", ".mjs", ".cjs", ".exe"):
            continue
        # Interpreter documentation is not the configured MCP's introduction.
        if path.stem.lower() in ("node", "python", "python3", "pythonw", "codex", "textlint"):
            continue
        directory = path.parent
        for _ in range(5):
            roots.append(directory)
            if any((directory/name).is_file() for name in ("package.json", "pyproject.toml")) or directory.parent==directory or directory.name in ("node_modules", "site-packages"):
                break
            if directory.name.lower() not in ("src", "scripts", "bin", "build", "dist", "server", "lib"):
                break
            directory = directory.parent
    command = value.get("command")
    if isinstance(command, str) and "-m" in args:
        index = args.index("-m")+1
        if index<len(args) and isinstance(args[index], str) and re.fullmatch(r"[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*", args[index]):
            runtime = Path(command)
            if safe_path(runtime) and runtime.is_file():
                environments = [runtime.parent.parent/"Lib/site-packages"]
                lib = runtime.parent.parent/"lib"
                if lib.is_dir():
                    environments += [path/"site-packages" for path in list(lib.glob("python*"))[:8]]
                for environment in environments:
                    module = environment.joinpath(*args[index].split("."))
                    if module.with_suffix('.py').is_file():
                        module = module.parent
                    if module.is_dir() and safe_path(module):
                        roots.extend((module, module.parent))
    return list(dict.fromkeys(roots))[:16]


def local_introduction(value):
    """Prefer package purpose metadata over README navigation or interpreter text."""
    for root in source_roots(value):
        for filename in ("package.json", "pyproject.toml"):
            path = root/filename
            try:
                if not safe_path(path):
                    continue
                with path.open('rb') as stream:
                    raw = stream.read(READ_LIMIT+1)
                if len(raw)>READ_LIMIT:
                    continue
                body = raw.decode('utf-8-sig')
                parsed = json.loads(body) if filename.endswith('.json') else tomllib.loads(body) if tomllib else {}
                text = parsed.get('description') if filename.endswith('.json') else parsed.get('project', {}).get('description')
                if isinstance(text, str) and text.strip():
                    return {'description': redact(' '.join(text.split())[:400]), 'description_source': filename, 'description_file': str(path)}
            except (OSError, UnicodeError, ValueError, AttributeError):
                continue
    args = value.get('args', [])
    if isinstance(args, list) and '-m' in args:
        index = args.index('-m')+1
        if index<len(args) and isinstance(args[index], str):
            filename = args[index].rsplit('.', 1)[-1]+'.py'
            for root in source_roots(value):
                path = root/filename
                try:
                    if not safe_path(path):
                        continue
                    with path.open('rb') as stream:
                        raw = stream.read(READ_LIMIT+1)
                    if len(raw)<=READ_LIMIT:
                        text = ast.get_docstring(ast.parse(raw.decode('utf-8-sig')))
                        if text:
                            return {'description': redact(' '.join(text.split())[:400]), 'description_source': filename, 'description_file': str(path)}
                except (OSError, UnicodeError, ValueError, SyntaxError):
                    continue
    return {}


def documents(value):
    """List only local documents and data settings associated with this launcher."""
    candidates = {}
    args = value.get('args', [])
    args = args[:32] if isinstance(args, list) else []
    env = value.get('env', {})
    entries = [*args, *(list(env.values())[:64] if isinstance(env, dict) else [])]
    parents = []
    for entry in entries:
        if not isinstance(entry, str) or len(entry)>512:
            continue
        path = Path(entry)
        if safe_path(path) and path.is_file() and (path.suffix.lower() in DATA_TYPES or re.search(r'(?:config|rc)\.(?:cjs|js)$', path.name)):
            candidates[path] = path.suffix.lower() in DATA_TYPES
            parents.append(path.parent)
            if path.suffix.lower() in ('.cjs', '.js'):
                try:
                    with path.open('rb') as stream:
                        body = stream.read(READ_LIMIT).decode('utf-8-sig')
                    for name in re.findall(r'''['"]([A-Za-z0-9_.-]{1,160}\.(?:yml|yaml|json|toml|txt|md))['"]''', body)[:32]:
                        candidates[path.parent/name] = True
                except (OSError, UnicodeError):
                    pass
    roots = source_roots(value)
    for root in roots:
        for filename in ('README.md', 'readme.md', 'package.json', 'pyproject.toml'):
            candidates.setdefault(root/filename, False)
    for root in list(dict.fromkeys(parents+roots))[:16]:
        for directory in (root, root/'config', root/'rules'):
            try:
                with os.scandir(directory) as items:
                    for index, entry in enumerate(items):
                        if index>=200:
                            break
                        path = directory/entry.name
                        if path.suffix.lower() in DATA_TYPES and re.search(r'(?:terms|rules|config|settings)', path.stem, re.I):
                            candidates.setdefault(path, True)
            except OSError:
                pass
    result = {}
    for path, editable in list(candidates.items())[:64]:
        try:
            if not safe_path(path):
                continue
            info = path.stat()
            if not stat.S_ISREG(info.st_mode) or info.st_nlink>1:
                continue
            key = hashlib.sha256(str(path).encode()).hexdigest()[:32]
            result[key] = {'id': key, 'name': path.name, 'path': str(path), 'bytes': info.st_size,
                           'modified_at': datetime.fromtimestamp(info.st_mtime, timezone.utc).isoformat(),
                           'format': path.suffix.lstrip('.'), 'editable': editable and info.st_size<=READ_LIMIT}
        except OSError:
            continue
    return result


def read_document(value, identity):
    entry = documents(value).get(identity)
    if not entry:
        return None
    path = Path(entry['path'])
    try:
        with path.open('rb') as stream:
            raw = stream.read(READ_LIMIT+1)
        text = raw[:READ_LIMIT].decode('utf-8-sig')
        clean = redact(text, limit=None)
        # Compare structured data without treating whitespace formatting as masking.
        if path.suffix.lower()=='.json':
            try:
                parsed = json.loads(text)
                masked = redact(parsed, limit=None)!=parsed
                clean = json.dumps(redact(parsed, limit=None), ensure_ascii=False, indent=2) if masked else text
            except ValueError:
                masked = clean!=text
        else:
            masked = clean!=text
        entry.update(text=clean, sha256=hashlib.sha256(raw).hexdigest(),
                     truncated=len(raw)>READ_LIMIT,
                     editable=entry['editable'] and not masked and len(raw)<=READ_LIMIT and len(text)<=EDIT_LIMIT,
                     masked=masked)
        parsed = parse_text(clean, entry['format'], full=True)
        if parsed:
            display = complete_payload(parsed['value'])
            entry.update(parsed=display['value'], parsed_format=parsed['format'], parsed_truncated=display['truncated'])
        if entry['format'] in ('yaml', 'yml'):
            parser = yaml_parser(value)
            entry['validation'] = 'syntax' if parser else 'basic'
            if parser:
                parsed = parse_yaml(clean, parser)
                if parsed is not None:
                    display = complete_payload(parsed)
                    entry.update(parsed=display['value'], parsed_format='YAML', parsed_truncated=display['truncated'])
        return entry
    except (OSError, UnicodeError, ValueError):
        return entry | {'health': 'unavailable', 'editable': False}


def yaml_parser(value):
    command = value.get('command', '')
    runtime = Path(command) if isinstance(command, str) else Path()
    if safe_path(runtime) and runtime.is_file() and runtime.stem.lower()=='node':
        for root in source_roots(value):
            for parent in (root, *list(root.parents)[:4]):
                package = parent/'node_modules/js-yaml'
                if safe_path(package) and (package/'package.json').is_file():
                    return runtime, package
    return None


def parse_yaml(text, parser):
    runtime, package = parser
    # Bound aliases, object depth and output before serialization by the installed parser.
    code = 'try{let n=0;function p(v,d=0){if(++n>200||d>8)return "[內容截斷]";if(typeof v==="string")return v;if(Array.isArray(v))return v.slice(0,100).map(x=>p(x,d+1));if(v&&typeof v==="object")return Object.fromEntries(Object.entries(v).slice(0,100).map(([k,x])=>[k.slice(0,160),p(x,d+1)]));return v}process.stdout.write(JSON.stringify(p(require('+json.dumps(str(package))+').load(require("fs").readFileSync(0,"utf8")))))}catch{process.exit(1)}'
    try:
        result = subprocess.run([str(runtime), '-e', code], input=text.encode(), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=2)
        return json.loads(result.stdout) if not result.returncode and len(result.stdout)<=1024*1024 else None
    except (OSError, subprocess.TimeoutExpired, ValueError, RecursionError):
        return None


def validate_document(path, text, value):
    if path.suffix.lower()=='.json':
        json.loads(text, parse_constant=lambda _: (_ for _ in ()).throw(ValueError('Invalid JSON')))
    elif path.suffix.lower()=='.toml':
        if not tomllib:
            raise ValueError('TOML validation unavailable')
        tomllib.loads(text)
    elif path.suffix.lower() in ('.yml', '.yaml'):
        parser = yaml_parser(value)
        if parser:
            runtime, package = parser
            code = 'try{require('+json.dumps(str(package))+').load(require("fs").readFileSync(0,"utf8"));}catch{process.exit(1)}'
            try:
                result = subprocess.run([str(runtime), '-e', code], input=text.encode(), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=2)
            except (OSError, subprocess.TimeoutExpired):
                raise ValueError('YAML validation unavailable')
            if result.returncode:
                raise ValueError('Invalid YAML')
            return
        # Text editing stays available when no existing YAML parser is installed.
        if '\x00' in text or '\t' in '\n'.join(line[:len(line)-len(line.lstrip())] for line in text.splitlines()):
            raise ValueError('Invalid YAML indentation')


def write_document(value, identity, text, expected):
    entry = read_document(value, identity)
    if not entry or not entry.get('editable'):
        raise ValueError('Document is not editable')
    if entry.get('sha256')!=expected:
        raise FileExistsError('Document changed')
    if not isinstance(text, str) or len(text)>EDIT_LIMIT or len(text.encode('utf-8'))>READ_LIMIT or '\x00' in text:
        raise ValueError('Document limit')
    path = Path(entry['path'])
    validate_document(path, text, value)
    if path.suffix.lower()=='.json':
        parsed = json.loads(text)
        if redact(parsed)!=parsed:
            raise ValueError('Sensitive data is not editable')
    elif len(text)<=EDIT_LIMIT and redact(text)!=text:
        raise ValueError('Sensitive data is not editable')
    info = path.stat()
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.lam-', suffix='.tmp', delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(text.encode('utf-8'))
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temporary, stat.S_IMODE(info.st_mode))
        latest = read_document(value, identity)
        if not latest or latest.get('sha256')!=expected or not safe_path(path):
            raise FileExistsError('Document changed')
        os.replace(temporary, path)
        temporary = None
        return read_document(value, identity)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
