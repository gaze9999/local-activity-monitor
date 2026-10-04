"""Recognize recorded SQLite operations without executing code or retaining SQL."""
import ast
import re
import shlex

from .operation_records import shell_parts


def sql_diagnostic(module, body):
    """Project SQL diagnostic fields from a related module, never SQL text or values."""
    if not isinstance(module, str) or not re.search(r"(?:sqlx|sqlite|rusqlite|state_db|database)(?:[:._-]|$)", module, re.I) or not isinstance(body, str):
        return None
    body = body[:8192]
    statement = re.search(r'\b(?:summary|statement|query|sql)="((?:\\.|[^"\\]){0,4096})"', body)
    operations = sql_operations(statement[1]) if statement else sql_operations(body.split("\n", 1)[0])
    clean = re.sub(r'"(?:\\.|[^"\\])*"|\'(?:\'\'|[^\'])*\'', "", body)
    result = {"statement": operations[0][0] if operations else "DATABASE_EVENT", "operation": operations[0][1] if operations else "diagnostic", "engine": "SQLite" if re.search("sqlite|rusqlite|state_db", module, re.I) else "SQL", "database": None, "duration_ms": None, "rows_affected": None, "rows_returned": None}
    duration = re.search(r"\belapsed_secs=(\d+(?:\.\d+)?(?:e[+-]?\d+)?)\b", clean, re.I)
    if duration:
        value = float(duration[1])*1000
        result["duration_ms"] = round(value, 6) if 0 <= value <= 86400000 else None
    else:
        duration = re.search(r"\b(?:elapsed|duration)=(\d+(?:\.\d+)?)(ns|us|µs|ms|s)\b", clean)
        if duration:
            value = float(duration[1])*{"ns": .000001, "us": .001, "µs": .001, "ms": 1, "s": 1000}[duration[2]]
            result["duration_ms"] = round(value, 6) if 0 <= value <= 86400000 else None
    for key in ("rows_affected", "rows_returned"):
        match = re.search(r"\b"+key+r"=(\d{1,19})\b", clean)
        if match and int(match[1]) <= 2**63-1:
            result[key] = int(match[1])
    return result


def database_path(value):
    if not isinstance(value, str) or not 0 < len(value) <= 512 or any(ord(char) < 32 for char in value) or any(char in value for char in "$`*<>|"):
        return None
    if value == ":memory:":
        return value
    if value.startswith("file:"):
        return value.split("?", 1)[0] if "@" not in value else None
    return value if "?" not in value and not re.match(r"[a-zA-Z]+://", value) else None


def sql_operations(sql):
    if not isinstance(sql, str) or len(sql) > 65536:
        return []
    # Values, quoted identifiers and comments never participate in classification
    clean = re.sub(r"'(?:''|[^'])*'|\"(?:\"\"|[^\"])*\"|`(?:``|[^`])*`|\[(?:\]\]|[^\]])*\]|--[^\n]*|/\*[\s\S]*?\*/", " ", sql)
    kinds = {"select": "read", "explain": "read", "insert": "write", "update": "write", "delete": "write", "replace": "write", "create": "schema", "alter": "schema", "drop": "schema", "pragma": "pragma", "vacuum": "maintenance", "analyze": "maintenance", "reindex": "maintenance", "begin": "transaction", "commit": "transaction", "rollback": "transaction", "savepoint": "transaction", "release": "transaction", "attach": "attach", "detach": "attach"}
    result = []
    for statement in clean.split(";")[:80]:
        depth = 0
        tokens = re.findall(r"[a-zA-Z_]+|[()]", statement)
        if not tokens or tokens[0].lower() not in kinds and tokens[0].lower() != "with":
            continue
        for token in tokens:
            if token == "(":
                depth += 1
            elif token == ")":
                depth -= 1
            elif depth == 0 and token.lower() in kinds:
                result.append((token.upper(), kinds[token.lower()]))
                break
    return result


def python_operations(code):
    try:
        tree = ast.parse(code)
    except (SyntaxError, ValueError, RecursionError):
        return []
    nodes = sorted(ast.walk(tree), key=lambda node:(getattr(node, "lineno", 0), getattr(node, "col_offset", 0)))
    modules = {"sqlite3"} | {alias.asname or alias.name for node in nodes if isinstance(node, ast.Import) for alias in node.names if alias.name == "sqlite3"}
    connectors = {alias.asname or alias.name for node in nodes if isinstance(node, ast.ImportFrom) and node.module == "sqlite3" for alias in node.names if alias.name == "connect"}
    def connection(node):
        if not isinstance(node, ast.Call):
            return None
        function = node.func
        match = isinstance(function, ast.Attribute) and isinstance(function.value, ast.Name) and function.value.id in modules and function.attr == "connect" or isinstance(function, ast.Name) and function.id in connectors
        if not match:
            return None
        value = node.args[0] if node.args else next((keyword.value for keyword in node.keywords if keyword.arg == "database"), None)
        return {"database": database_path(value.value) if isinstance(value, ast.Constant) else None, "statement": "CONNECT", "operation": "open"}
    bindings, result = {}, []
    for node in nodes:
        if isinstance(node, ast.Assign):
            targets, value = node.targets, node.value
        elif isinstance(node, (ast.With, ast.AsyncWith)):
            for item in node.items:
                calls = [call for call in ast.walk(item.context_expr) if connection(call)]
                if isinstance(item.optional_vars, ast.Name) and len(calls) == 1:
                    bindings[item.optional_vars.id] = connection(calls[0])["database"]
            continue
        else:
            continue
        connected = connection(value)
        if connected:
            for target in targets:
                if isinstance(target, ast.Name):
                    path = connected["database"]
                    bindings[target.id] = path if target.id not in bindings or bindings[target.id] == path else None
        elif isinstance(value, ast.Call) and isinstance(value.func, ast.Attribute) and isinstance(value.func.value, ast.Name) and value.func.attr == "cursor" and value.func.value.id in bindings:
            for target in targets:
                if isinstance(target, ast.Name):
                    bindings[target.id] = bindings[value.func.value.id]
    for node in nodes:
        connected = connection(node)
        if connected:
            result.append(connected)
        elif isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and isinstance(node.func.value, ast.Name) and node.func.value.id in bindings:
            function, target = node.func.attr, bindings[node.func.value.id]
            if function in ("execute", "executemany", "executescript") and node.args and isinstance(node.args[0], ast.Constant):
                result.extend({"database": target, "statement": statement, "operation": operation} for statement, operation in sql_operations(node.args[0].value))
            elif function in ("commit", "rollback", "close"):
                result.append({"database": target, "statement": function.upper(), "operation": "close" if function == "close" else "transaction"})
    return result[:80]


def sqlite_operations(calls):
    result = []
    for tool, args, nested in calls:
        if not isinstance(args, dict):
            continue
        events = []
        if "sqlite" in tool.lower():
            path = next((database_path(args[key]) for key in ("database", "db_path", "database_path", "path") if key in args), None)
            sql = args.get("query", args.get("sql"))
            events = [{"database": path, "statement": statement, "operation": operation} for statement, operation in sql_operations(sql)]
            if not events:
                suffix = tool.split("__")[-1]
                events = [{"database": path, "statement": suffix, "operation": "operation"}]
        elif tool in ("exec_command", "functions.exec_command") and isinstance(args.get("cmd"), str) and len(args["cmd"]) <= 65536:
            command = args["cmd"]
            if "sqlite" in command:
                candidates = [command]
                candidates += re.findall(r"@['\"]\r?\n([\s\S]*?)\r?\n['\"]@", command)
                candidates += [match[1] for match in re.findall(r"<<['\"]?(\w+)['\"]?\r?\n([\s\S]*?)\r?\n\1(?:\r?\n|$)", command)]
                for part in shell_parts(command):
                    try:
                        tokens = shlex.split(part)
                    except ValueError:
                        continue
                    for index, token in enumerate(tokens):
                        binary = token.replace("\\", "/").rsplit("/", 1)[-1].lower()
                        if re.fullmatch(r"python(?:3(?:\.\d+)?)?(?:\.exe)?", binary) and tokens[index+1:index+2] == ["-c"] and len(tokens) > index+2:
                            candidates.append(tokens[index+2])
                        if binary not in ("sqlite3", "sqlite3.exe"):
                            continue
                        arguments = tokens[index+1:]
                        while arguments and arguments[0] in ("-readonly", "-readwrite", "-batch", "-json", "-csv", "-header", "-noheader"):
                            arguments.pop(0)
                        if not arguments or arguments[0].startswith("-"):
                            continue
                        path = database_path(arguments[0])
                        events.append({"database": path, "statement": "CONNECT", "operation": "open"})
                        if len(arguments) > 1:
                            events.extend({"database": path, "statement": statement, "operation": operation} for statement, operation in sql_operations(arguments[1]))
                for code in dict.fromkeys(candidates):
                    events.extend(python_operations(code))
        for event in events[:80]:
            result.append(event | {"engine": "SQLite", "tool": tool, "nested": nested, "workdir": database_path(args.get("workdir")), "recognition": "mcp_call" if "sqlite" in tool.lower() and not nested else "recorded_code"})
        if len(result) >= 80:
            break
    return result[:80]
