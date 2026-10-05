"""Recognize literal Git / Jev tool operations; never execute recorded code."""
import json
import math
from pathlib import PurePosixPath, PureWindowsPath
import re
import shlex

GIT_COMMANDS = {"status", "log", "diff", "show", "branch", "rev-parse", "ls-files", "ls-tree", "remote", "fetch", "pull", "push", "commit", "add", "restore", "reset", "checkout", "switch", "merge", "rebase", "tag", "stash", "clean", "worktree", "submodule", "init", "clone", "config", "reflog", "cherry-pick", "revert", "describe", "blame", "check-ignore", "for-each-ref", "merge-base"}
JEV_TOOL = re.compile(r"(?:mcp__jev__|jev[.:_])jev_?(rank|evaluate|status)\Z|(?:mcp__jev__)?jev_(rank|evaluate|status)\Z")
TOOL_CALL = re.compile(r"\btools\.([A-Za-z_][\w]*)\(\s*")


def literal(source, start=0):
    """Read only JSON-like JS literals, including unquoted object keys."""
    index = start

    def space():
        nonlocal index
        while index < len(source) and source[index].isspace():
            index += 1

    def string():
        nonlocal index
        quote = source[index]
        index += 1
        result = []
        while index < len(source):
            char = source[index]
            index += 1
            if char == quote:
                value = "".join(result)
                if quote == "`" and "${" in value:
                    raise ValueError()
                return value
            if char == "\\":
                if index >= len(source):
                    raise ValueError()
                char = source[index]
                index += 1
                if char == "u":
                    digits = source[index:index+4]
                    if not re.fullmatch(r"[0-9a-fA-F]{4}", digits):
                        raise ValueError()
                    char = chr(int(digits, 16))
                    index += 4
                else:
                    char = {"n": "\n", "r": "\r", "t": "\t", "b": "\b", "f": "\f"}.get(char, char)
            result.append(char)
        raise ValueError()

    def value(depth=0):
        nonlocal index
        if depth > 12:
            raise ValueError()
        space()
        if index >= len(source):
            raise ValueError()
        char = source[index]
        if char in "\"'`":
            return string()
        if char in "{[":
            index += 1
            closing = "}" if char == "{" else "]"
            result = {} if char == "{" else []
            space()
            while index < len(source) and source[index] != closing:
                if char == "{":
                    if source[index] in "\"'":
                        key = string()
                    else:
                        match = re.match(r"[A-Za-z_$][\w$]*", source[index:])
                        if not match:
                            raise ValueError()
                        key = match[0]
                        index += len(key)
                    space()
                    if index >= len(source) or source[index] != ":":
                        raise ValueError()
                    index += 1
                    result[key] = value(depth+1)
                else:
                    result.append(value(depth+1))
                space()
                if index < len(source) and source[index] == ",":
                    index += 1
                    space()
                elif index >= len(source) or source[index] != closing:
                    raise ValueError()
            if index >= len(source):
                raise ValueError()
            index += 1
            return result
        match = re.match(r"(?:true|false|null|undefined)\b|-?\d+(?:\.\d+)?", source[index:])
        if not match:
            raise ValueError()
        index += len(match[0])
        return {"true": True, "false": False, "null": None, "undefined": None}.get(match[0]) if match[0] in ("true", "false", "null", "undefined") else json.loads(match[0])

    result = value()
    return result, index


def qualified_tool(payload):
    tool, namespace = payload.get("name", ""), payload.get("namespace")
    if not isinstance(tool, str):
        return ""
    if isinstance(namespace, str) and re.fullmatch(r"[A-Za-z0-9_.-]{1,100}", namespace) and not tool.startswith(("mcp__", namespace+".", namespace+"__")):
        return namespace+("__" if namespace.startswith("mcp__") else ".")+tool
    return tool


def invocations(payload):
    tool = qualified_tool(payload)
    raw = payload.get("arguments", payload.get("input"))
    if tool in ("exec", "functions.exec") and isinstance(raw, str):
        result, index = [], 0
        while index < len(raw):
            if raw[index:index+2] == "//":
                end = raw.find("\n", index+2)
                index = len(raw) if end < 0 else end+1
                continue
            if raw[index:index+2] == "/*":
                end = raw.find("*/", index+2)
                index = len(raw) if end < 0 else end+2
                continue
            if raw[index] in "\"'`":
                quote = raw[index]
                index += 1
                while index < len(raw):
                    if raw[index] == "\\":
                        index += 2
                    elif raw[index] == quote:
                        index += 1
                        break
                    else:
                        index += 1
                continue
            match = TOOL_CALL.match(raw, index)
            if match:
                start = index+len(match[0])
                try:
                    args, end = literal(raw, start)
                    result.append((match[1], args if isinstance(args, dict) or match[1] == "apply_patch" and isinstance(args, str) else None, True))
                    index = end
                    continue
                except (ValueError, RecursionError):
                    result.append((match[1], None, True))
                    index = start
                    continue
            index += 1
        return result
    try:
        args = json.loads(raw) if isinstance(raw, str) else raw
    except (ValueError, RecursionError):
        args = None
    return [(tool, args if isinstance(args, dict) else raw if tool in ("apply_patch", "functions.apply_patch") and isinstance(raw, str) else None, False)]


def file_operations(calls):
    changes = []
    def add(path, operation, tool, nested, workdir=None, metadata=None):
        if isinstance(path, str) and 0 < len(path) <= 512 and not any(ord(char) < 32 for char in path) and not any(char in path for char in "$`*?<>|"):
            item = {"path": path, "operation": operation, "tool": tool, "nested": nested}
            if isinstance(workdir, str) and len(workdir) <= 512 and not any(ord(char) < 32 for char in workdir):
                item["workdir"] = workdir
            if metadata:
                item.update(metadata)
            if item not in changes:
                changes.append(item)
    for tool, args, nested in calls:
        if tool in ("apply_patch", "functions.apply_patch"):
            patch = args if isinstance(args, str) else args.get("patch", args.get("input")) if isinstance(args, dict) else None
            if isinstance(patch, str):
                for operation, path in re.findall(r"^\*\*\* (Add File|Update File|Delete File|Move to): (.+)$", patch, re.M):
                    if len(path) <= 512 and not any(ord(char) < 32 for char in path):
                        add(path, {"Add File": "added", "Update File": "modified", "Delete File": "deleted", "Move to": "moved"}[operation], tool, nested)
        elif tool in ("exec_command", "functions.exec_command") and isinstance(args, dict):
            command = args.get("cmd")
            if not isinstance(command, str):
                continue
            for part in shell_parts(command):
                try:
                    tokens = [item.strip("\"'") for item in shlex.split(part, posix=False)]
                except ValueError:
                    continue
                if not tokens or tokens[0].lower() not in ("get-content", "cat", "type", "more", "head", "tail", "sed", "set-content", "add-content", "out-file"):
                    continue
                write = tokens[0].lower() in ("set-content", "add-content", "out-file")
                metadata, ranges = {}, {}
                for index, option in enumerate(tokens[:-1]):
                    value = tokens[index+1]
                    if value.isdecimal() and len(value)<=15:
                        if option.lower() in ('-totalcount', '-tail'):
                            ranges['first_lines' if option.lower()=='-totalcount' else 'last_lines'] = int(value)
                        elif tokens[0].lower() in ('head', 'tail') and option in ('-n', '-c'):
                            ranges[('first_' if tokens[0].lower()=='head' else 'last_')+('lines' if option=='-n' else 'bytes')] = int(value)
                if tokens[0].lower()=='sed':
                    for token in tokens[1:]:
                        match = re.fullmatch(r'(\d+),(\d+)p', token)
                        if match:
                            ranges.update(start_line=int(match[1]), end_line=int(match[2]))
                if ranges:
                    metadata['range'] = ranges
                sed, expression, index = tokens[0].lower() == "sed", False, 1
                while index < len(tokens):
                    value = tokens[index]
                    if value.startswith("-"):
                        if sed and value in ("-e", "--expression", "-f", "--file"):
                            expression = True
                            if value in ("-f", "--file") and index + 1 < len(tokens):
                                add(tokens[index + 1], "read", tool, nested, args.get("workdir"))
                            index += 2
                            continue
                        if sed and (value.startswith("--expression=") or value.startswith("-e") and len(value) > 2):
                            expression = True
                        if sed and (value.startswith("--file=") or value.startswith("-f") and len(value) > 2):
                            expression = True
                            add(value.partition("=")[2] if value.startswith("--file=") else value[2:], "read", tool, nested, args.get("workdir"))
                        if sed and value == "-i" and index + 1 < len(tokens) and not tokens[index + 1]:
                            index += 2
                            continue
                        index += 2 if value.lower() in ("-encoding", "-totalcount", "-tail", "-readcount", "-delimiter", "-value", "-width") or tokens[0].lower() in ("head", "tail") and value in ("-n", "-c") else 1
                        continue
                    if sed and not expression:
                        expression = True
                        index += 1
                        continue
                    add(value, "write" if write else "modified" if sed and any(item.startswith("-i") or item.startswith("--in-place") for item in tokens[1:]) else "read", tool, nested, args.get("workdir"), metadata)
                    if write:
                        break
                    index += 1
        elif isinstance(args, dict):
            action = tool.rsplit("__", 1)[-1].rsplit(".", 1)[-1].split("_", 1)[0].lower()
            operation = "read" if action in ("read", "inspect", "extract", "load", "view") else "write" if action in ("write", "update", "append", "save") else None
            if operation:
                ranges = {key: value for key in ('start_line', 'end_line', 'line_start', 'line_end', 'offset', 'limit', 'byte_offset', 'byte_length') if type(value:=args.get(key)) is int and 0<=value<=2**53}
                if isinstance(args.get('pages'), str) and re.fullmatch(r'[0-9, -]{1,80}', args['pages']):
                    ranges['pages'] = args['pages']
                metadata = {'range': ranges} if ranges else {}
                text = args.get('content', args.get('text'))
                if operation=='write' and isinstance(text, str) and len(text)<=65536:
                    metadata['submitted_utf8_bytes'] = len(text.encode('utf-8'))
                for key in ("path", "file_path", "filename", "input_path" if operation == "read" else "output_path"):
                    if key in args:
                        add(args[key], operation, tool, nested, args.get('workdir'), metadata)
    return changes[:100]


def shell_parts(command):
    if not isinstance(command, str) or len(command) > 65536:
        return []
    parts, current, quote, escaped = [], [], None, False
    for char in command:
        if escaped:
            current.append(char)
            escaped = False
        elif char == "`":
            current.append(char)
            escaped = True
        elif quote:
            current.append(char)
            if char == quote:
                quote = None
        elif char in "\"'":
            current.append(char)
            quote = char
        elif char in ";\n&|":
            parts.append("".join(current))
            current = []
        else:
            current.append(char)
    parts.append("".join(current))
    return parts


def git_commands(command):
    operations = []
    parts = shell_parts(command)
    for part in parts:
        try:
            tokens = [item.strip("\"'") for item in shlex.split(part, posix=False)]
        except ValueError:
            continue
        if not tokens or tokens[0].lower().replace("\\", "/").rsplit("/", 1)[-1] not in ("git", "git.exe"):
            continue
        index = 1
        while index < len(tokens) and tokens[index].startswith("-"):
            option = tokens[index]
            index += 2 if option in ("-C", "-c", "--git-dir", "--work-tree", "--namespace") else 1
        if index < len(tokens) and tokens[index] in GIT_COMMANDS:
            operations.append(tokens[index])
    return operations[:80]


def repository_label(value):
    if not isinstance(value, str):
        return None
    return (PureWindowsPath(value) if "\\" in value else PurePosixPath(value)).name[:160] or None


def operations(payload, include_git=True, include_jev=True, calls=None):
    git, jev = [], []
    for tool, args, nested in invocations(payload) if calls is None else calls:
        if include_git and tool in ("exec_command", "functions.exec_command") and args:
            git.extend({"operation": operation, "repository": repository_label(args.get("workdir"))} for operation in git_commands(args.get("cmd")))
        match = JEV_TOOL.fullmatch(tool)
        if include_jev and match:
            jev.append({"operation": next(group for group in match.groups() if group), "nested": nested, "arguments": args})
    return git, jev


def workflow_operations(payload, include_skills=True, include_checks=True, calls=None, include_paths=False):
    skills, checks = [], []
    for tool, args, _ in invocations(payload) if calls is None else calls:
        if tool not in ("exec_command", "functions.exec_command") or not args or not isinstance(args.get("cmd"), str):
            continue
        for command in shell_parts(args["cmd"]):
            command = command.strip()
            if include_skills and re.match(r"(?:Get-Content|cat|type|sed|more)\b", command, re.I):
                names = dict.fromkeys(re.findall(r"[/\\]([^/\\\s\"']+)[/\\]SKILL\.md\b", command, re.I))
                if include_paths:
                    paths = re.findall(r'''"([^"\n]*[/\\]SKILL\.md)"|'([^'\n]*[/\\]SKILL\.md)'|([^\s"'`|;]+[/\\]SKILL\.md)''', command, re.I)
                    for values in paths:
                        path = next(value for value in values if value)
                        skill = repository_label(path.replace("\\", "/").rsplit("/", 1)[0])
                        if skill:
                            skills.append({"skill": skill, "doc_path": path, "workdir": args.get("workdir") if isinstance(args.get("workdir"), str) else None})
                else:
                    skills.extend(names)
            patterns = ((r"(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?(test|build|lint|typecheck|check)\b", "package"), (r"(?:[.\\/\w-]*[\\/])?(?:python(?:3)?(?:\.exe)?|py)\s+-m\s+(unittest|pytest|compileall)\b", "python"), (r"(pytest|mypy|tsc)\b", "check"), (r"(ruff)\s+(check|format)\b", "python"), (r"(?:dotnet|cargo|go)\s+(test|build|check|clippy|vet)\b", "runtime"), (r"node\s+--check\b", "javascript"))
            for pattern, category in patterns if include_checks else ():
                match = re.match(pattern, command)
                if match:
                    checks.append({"operation": match[0], "category": category})
    return skills[:40], checks[:40]


def redact(value, depth=0, limit=32768):
    if depth > (64 if limit is None else 10):
        return "[內容過深]"
    if isinstance(value, dict):
        clean = {}
        for key, item in (value.items() if limit is None else list(value.items())[:100]):
            label = str(key) if limit is None else str(key)[:160]
            normalized = re.sub(r"[^a-z]", "", label.lower())
            sensitive = normalized.endswith(("apikey", "password", "secret", "credential", "authorization", "cookie", "header", "headers", "token"))
            clean[label] = "[已隱藏]" if sensitive else redact(item, depth+1, limit)
        return clean
    if isinstance(value, list):
        return [redact(item, depth+1, limit) for item in (value if limit is None else value[:100])]
    if isinstance(value, str):
        if value.lstrip().startswith(("{", "[")) and (limit is None or len(value) <= 1024*1024):
            try:
                decoded = json.loads(value)
                if isinstance(decoded, (dict, list)):
                    return json.dumps(redact(decoded, depth+1, limit), ensure_ascii=False)[:limit]
            except (ValueError, RecursionError):
                pass
        value = re.sub(r"(?i)\bBearer\s+[A-Za-z0-9._~+/-]+=*|\bsk-[A-Za-z0-9_-]{12,}", "[已隱藏]", value)
        value = re.sub(r'''(?i)\b(?:api[_-]?key|password|secret|authorization|access[_-]?token|token)["']?\s*[:=]\s*(?:"[^"]*"|'[^']*'|[^\s,;]+)''', "[已隱藏]", value)
        return value[:limit]
    return value if value is None or type(value) in (bool, int) or type(value) is float and math.isfinite(value) else None
