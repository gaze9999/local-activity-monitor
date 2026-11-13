//! Project observed file operations without reading file contents.
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

static DECIMAL_DIGIT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\p{Nd}$").expect("decimal digit"));

fn decimal_number(value: &str, max_digits: Option<usize>) -> Option<u64> {
    if value.is_empty() || max_digits.is_some_and(|limit| value.chars().count() > limit) {
        return None;
    }
    let mut number = 0_u64;
    for ch in value.chars() {
        if !DECIMAL_DIGIT.is_match(ch.encode_utf8(&mut [0; 4])) {
            return None;
        }
        let code = ch as u32;
        let mut first = code;
        while let Some(previous) = first.checked_sub(1).and_then(char::from_u32) {
            if !DECIMAL_DIGIT.is_match(previous.encode_utf8(&mut [0; 4])) {
                break;
            }
            first -= 1;
        }
        number = number.checked_mul(10)?.checked_add(u64::from((code - first) % 10))?;
    }
    (number <= (1_u64 << 53)).then_some(number)
}

fn path_valid(path: &str) -> bool {
    !path.is_empty()
        && path.chars().count() <= 512
        && !path.chars().any(|ch| ch < ' ' || "$`*?<>|".contains(ch))
}

fn add(
    events: &mut Vec<Value>,
    path: &str,
    operation: &str,
    tool: &str,
    nested: bool,
    workdir: Option<&str>,
    metadata: Option<&Map<String, Value>>,
) {
    if !path_valid(path) {
        return;
    }
    let mut item = json!({"path":path,"operation":operation,"tool":tool,"nested":nested});
    if let Some(workdir) =
        workdir.filter(|value| value.chars().count() <= 512 && !value.chars().any(|ch| ch < ' '))
    {
        item["workdir"] = json!(workdir);
    }
    if let Some(metadata) = metadata {
        item.as_object_mut()
            .expect("file event")
            .extend(metadata.clone());
    }
    if !events.contains(&item) {
        events.push(item);
    }
}

/// Return at most 100 file operations from a recorded direct or nested invocation.
pub fn file_operations(tool: &str, args: &Value, nested: bool) -> Vec<Value> {
    let mut events = Vec::new();
    if matches!(tool, "apply_patch" | "functions.apply_patch") {
        let patch = args.as_str().or_else(|| {
            args.as_object()
                .and_then(|object| object.get("patch").or_else(|| object.get("input")))
                .and_then(Value::as_str)
        });
        if let Some(patch) = patch {
            for line in patch.split('\n') {
                for (prefix, operation) in [
                    ("*** Add File: ", "added"),
                    ("*** Update File: ", "modified"),
                    ("*** Delete File: ", "deleted"),
                    ("*** Move to: ", "moved"),
                ] {
                    if let Some(path) = line.strip_prefix(prefix) {
                        add(&mut events, path, operation, tool, nested, None, None);
                    }
                }
            }
        }
    } else if matches!(tool, "exec_command" | "functions.exec_command") {
        if let Some(args) = args.as_object() {
            shell_file_operations(&mut events, tool, args, nested);
        }
    } else {
        if let Some(args) = args.as_object() {
            let action = tool
                .rsplit("__")
                .next()
                .unwrap_or(tool)
                .rsplit('.')
                .next()
                .unwrap_or(tool)
                .split('_')
                .next()
                .unwrap_or(tool)
                .to_ascii_lowercase();
            let operation = match action.as_str() {
                "read" | "inspect" | "extract" | "load" | "view" => Some("read"),
                "write" | "update" | "append" | "save" => Some("write"),
                _ => None,
            };
            if let Some(operation) = operation {
                let mut ranges = Map::new();
                for key in [
                    "start_line",
                    "end_line",
                    "line_start",
                    "line_end",
                    "offset",
                    "limit",
                    "byte_offset",
                    "byte_length",
                ] {
                    if let Some(value) = args
                        .get(key)
                        .and_then(Value::as_u64)
                        .filter(|value| *value <= (1_u64 << 53))
                    {
                        ranges.insert(key.into(), json!(value));
                    }
                }
                if let Some(pages) = args.get("pages").and_then(Value::as_str).filter(|value| {
                    (1..=80).contains(&value.len())
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || b", -".contains(&byte))
                }) {
                    ranges.insert("pages".into(), json!(pages));
                }
                let mut metadata = Map::new();
                if !ranges.is_empty() {
                    metadata.insert("range".into(), Value::Object(ranges));
                }
                if operation == "write" {
                    if let Some(content) = args
                        .get("content")
                        .or_else(|| args.get("text"))
                        .and_then(Value::as_str)
                        .filter(|value| value.chars().count() <= 65536)
                    {
                        metadata.insert("submitted_utf8_bytes".into(), json!(content.len()));
                    }
                }
                let workdir = args.get("workdir").and_then(Value::as_str);
                for key in [
                    "path",
                    "file_path",
                    "filename",
                    if operation == "read" {
                        "input_path"
                    } else {
                        "output_path"
                    },
                ] {
                    if let Some(path) = args.get(key).and_then(Value::as_str) {
                        add(
                            &mut events,
                            path,
                            operation,
                            tool,
                            nested,
                            workdir,
                            Some(&metadata),
                        );
                    }
                }
            }
        }
    }
    events.truncate(100);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patch_and_tool_arguments_keep_paths_without_contents() {
        let patch = "*** Begin Patch\n*** Update File: src/a.rs\n-secret\n*** Add File: $unsafe\n*** Move to: src/b.rs\n";
        let rows = file_operations("apply_patch", &json!(patch), false);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["operation"], "modified");
        assert_eq!(rows[1]["operation"], "moved");
        assert!(!serde_json::to_string(&rows).unwrap().contains("secret"));
        let rows = file_operations(
            "mcp__fixture__write_file",
            &json!({"path":"a.txt","file_path":"a.txt","content":"漢","offset":2,"workdir":"/tmp"}),
            true,
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["submitted_utf8_bytes"], 3);
        assert_eq!(rows[0]["range"]["offset"], 2);
        assert!(!serde_json::to_string(&rows).unwrap().contains("漢"));
    }

    #[test]
    fn shell_reads_writes_ranges_and_filters_unsafe_paths() {
        let input = json!({"cmd":"head -n 5 'some file.txt'; sed -i '2,7p' src/a.rs; Set-Content output.txt -Value secret; cat '$VAR'", "workdir":"/tmp/project"});
        let rows = file_operations("functions.exec_command", &input, true);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["path"], "some file.txt");
        assert_eq!(rows[0]["range"]["first_lines"], 5);
        assert_eq!(rows[1]["operation"], "modified");
        assert_eq!(rows[1]["range"]["start_line"], 2);
        assert_eq!(rows[1]["range"]["end_line"], 7);
        assert_eq!(rows[2]["operation"], "write");
        assert!(!serde_json::to_string(&rows).unwrap().contains("secret"));
        assert!(rows.iter().all(valid_file_operation));
    }

    #[test]
    fn patch_paths_with_carriage_returns_are_rejected() {
        let patch = "*** Update File: src/unsafe.rs\r\n*** Add File: src/safe.rs\n";
        let rows = file_operations("apply_patch", &json!(patch), false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["path"], "src/safe.rs");
        assert!(rows.iter().all(valid_file_operation));
    }

    #[test]
    fn shell_keeps_non_ascii_whitespace_inside_a_filename() {
        let rows = file_operations("exec_command", &json!({"cmd":"cat a\u{00a0}b.txt"}), false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["path"], "a\u{00a0}b.txt");
    }

    #[test]
    fn shell_ranges_accept_unicode_decimal_digits() {
        let rows = file_operations("exec_command", &json!({"cmd":"head -n １２ a.txt; sed '١,٥p' b.txt"}), false);
        assert_eq!(rows[0]["range"]["first_lines"], 12);
        assert_eq!(rows[1]["range"]["start_line"], 1);
        assert_eq!(rows[1]["range"]["end_line"], 5);
        assert!(rows.iter().all(valid_file_operation));
    }
}

pub(crate) fn shell_tokens(part: &str) -> Option<Vec<&str>> {
    let whitespace = |ch| matches!(ch, ' ' | '\t' | '\r' | '\n');
    let mut tokens = Vec::new();
    let mut rest = part.trim_start_matches(whitespace);
    while !rest.is_empty() {
        let end = if rest.starts_with(['"', '\'']) {
            let quote = rest.as_bytes()[0] as char;
            rest[1..].find(quote)? + 2
        } else {
            rest.find(whitespace).unwrap_or(rest.len())
        };
        tokens.push(rest[..end].trim_matches(['"', '\'']));
        rest = rest[end..].trim_start_matches(whitespace);
    }
    Some(tokens)
}

fn shell_file_operations(
    events: &mut Vec<Value>,
    tool: &str,
    args: &Map<String, Value>,
    nested: bool,
) {
    let Some(command) = args
        .get("cmd")
        .and_then(Value::as_str)
        .filter(|value| value.chars().count() <= 65536)
    else {
        return;
    };
    let workdir = args.get("workdir").and_then(Value::as_str);
    for part in crate::commands::shell_parts(command) {
        let Some(tokens) = shell_tokens(part) else {
            continue;
        };
        let Some(executable) = tokens.first().map(|value| value.to_ascii_lowercase()) else {
            continue;
        };
        if ![
            "get-content",
            "cat",
            "type",
            "more",
            "head",
            "tail",
            "sed",
            "set-content",
            "add-content",
            "out-file",
        ]
        .contains(&executable.as_str())
        {
            continue;
        }
        let write = ["set-content", "add-content", "out-file"].contains(&executable.as_str());
        let sed = executable == "sed";
        let mut ranges = Map::new();
        for pair in tokens.windows(2) {
            let option = pair[0];
            let value = pair[1];
            if let Some(number) = decimal_number(value, Some(15)) {
                let label = if option.eq_ignore_ascii_case("-totalcount") {
                    Some("first_lines")
                } else if option.eq_ignore_ascii_case("-tail") {
                    Some("last_lines")
                } else if executable == "head" && option == "-n" {
                    Some("first_lines")
                } else if executable == "head" && option == "-c" {
                    Some("first_bytes")
                } else if executable == "tail" && option == "-n" {
                    Some("last_lines")
                } else if executable == "tail" && option == "-c" {
                    Some("last_bytes")
                } else {
                    None
                };
                if let Some(label) = label {
                    ranges.insert(label.into(), json!(number));
                }
            }
        }
        if sed {
            for token in tokens.iter().skip(1) {
                if let Some((start, end)) = token
                    .strip_suffix('p')
                    .and_then(|value| value.split_once(','))
                {
                    if let (Some(start), Some(end)) =
                        (decimal_number(start, None), decimal_number(end, None))
                    {
                        ranges.insert("start_line".into(), json!(start));
                        ranges.insert("end_line".into(), json!(end));
                    }
                }
            }
        }
        let mut metadata = Map::new();
        if !ranges.is_empty() {
            metadata.insert("range".into(), Value::Object(ranges));
        }
        let mut expression = false;
        let mut index = 1;
        while index < tokens.len() {
            let value = tokens[index];
            if value.starts_with('-') {
                if sed && ["-e", "--expression", "-f", "--file"].contains(&value) {
                    expression = true;
                    if matches!(value, "-f" | "--file") {
                        if let Some(path) = tokens.get(index + 1) {
                            add(events, path, "read", tool, nested, workdir, None);
                        }
                    }
                    index += 2;
                    continue;
                }
                if sed
                    && (value.starts_with("--expression=")
                        || value.starts_with("-e") && value.len() > 2)
                {
                    expression = true;
                }
                if sed
                    && (value.starts_with("--file=") || value.starts_with("-f") && value.len() > 2)
                {
                    expression = true;
                    let path = value.strip_prefix("--file=").unwrap_or_else(|| &value[2..]);
                    add(events, path, "read", tool, nested, workdir, None);
                }
                if sed && value == "-i" && tokens.get(index + 1) == Some(&"") {
                    index += 2;
                    continue;
                }
                let option = value.to_ascii_lowercase();
                let takes_value = [
                    "-encoding",
                    "-totalcount",
                    "-tail",
                    "-readcount",
                    "-delimiter",
                    "-value",
                    "-width",
                ]
                .contains(&option.as_str())
                    || ["head", "tail"].contains(&executable.as_str())
                        && ["-n", "-c"].contains(&value);
                index += if takes_value { 2 } else { 1 };
                continue;
            }
            if sed && !expression {
                expression = true;
                index += 1;
                continue;
            }
            let in_place = sed
                && tokens
                    .iter()
                    .skip(1)
                    .any(|item| item.starts_with("-i") || item.starts_with("--in-place"));
            add(
                events,
                value,
                if write {
                    "write"
                } else if in_place {
                    "modified"
                } else {
                    "read"
                },
                tool,
                nested,
                workdir,
                Some(&metadata),
            );
            if write {
                break;
            }
            index += 1;
        }
    }
}

/// Validate checkpoint metadata before it can re-enter a snapshot.
pub fn valid_file_operation(value: &Value) -> bool {
    let Some(item) = value.as_object() else {
        return false;
    };
    if item.keys().any(|key| {
        !matches!(
            key.as_str(),
            "path"
                | "operation"
                | "tool"
                | "nested"
                | "workdir"
                | "range"
                | "submitted_utf8_bytes"
                | "read_bytes"
                | "write_bytes"
        )
    }) {
        return false;
    }
    if !item
        .get("path")
        .and_then(Value::as_str)
        .is_some_and(path_valid)
        || !item
            .get("operation")
            .and_then(Value::as_str)
            .is_some_and(|operation| {
                matches!(
                    operation,
                    "added" | "modified" | "deleted" | "moved" | "read" | "write"
                )
            })
        || !item
            .get("tool")
            .and_then(Value::as_str)
            .is_some_and(|tool| {
                !tool.is_empty()
                    && tool.len() <= 160
                    && tool
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
            })
        || item.get("nested").and_then(Value::as_bool).is_none()
        || item.get("workdir").is_some_and(|value| {
            !value
                .as_str()
                .is_some_and(|path| path.chars().count() <= 512 && !path.chars().any(|ch| ch < ' '))
        })
        || item
            .get("submitted_utf8_bytes")
            .is_some_and(|value| !value.as_u64().is_some_and(|size| size <= 262144))
        || ["read_bytes", "write_bytes"].iter().any(|key| {
            item.get(*key)
                .is_some_and(|value| !value.as_u64().is_some_and(|size| size <= (1_u64 << 53)))
        })
    {
        return false;
    }
    item.get("range").is_none_or(|range| {
        range.as_object().is_some_and(|ranges| {
            ranges.iter().all(|(key, value)| {
                if key == "pages" {
                    return value.as_str().is_some_and(|pages| {
                        (1..=80).contains(&pages.len())
                            && pages
                                .bytes()
                                .all(|byte| byte.is_ascii_digit() || b", -".contains(&byte))
                    });
                }
                matches!(
                    key.as_str(),
                    "start_line"
                        | "end_line"
                        | "line_start"
                        | "line_end"
                        | "offset"
                        | "limit"
                        | "byte_offset"
                        | "byte_length"
                        | "first_lines"
                        | "last_lines"
                        | "first_bytes"
                        | "last_bytes"
                ) && value.as_u64().is_some_and(|number| number <= (1_u64 << 53))
            })
        })
    })
}
