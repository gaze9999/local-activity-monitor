//! Recognize recorded skill document reads without opening files.
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

static READER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:Get-Content|cat|type|sed|more)\b").unwrap());
static PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)"([^"\n]*[/\\]SKILL\.md)"|'([^'\n]*[/\\]SKILL\.md)'|([^\s"'`|;]+[/\\]SKILL\.md)"#,
    )
    .unwrap()
});

pub fn skill_reads(tool: &str, args: Option<&Map<String, Value>>) -> Vec<String> {
    skill_read_paths(tool, args)
        .into_iter()
        .map(|item| item.name)
        .collect()
}

pub struct SkillRead {
    pub name: String,
    pub path: String,
    pub workdir: Option<String>,
}

pub fn skill_read_paths(tool: &str, args: Option<&Map<String, Value>>) -> Vec<SkillRead> {
    if !matches!(tool, "exec_command" | "functions.exec_command") {
        return Vec::new();
    }
    let Some(command) = args
        .and_then(|args| args.get("cmd"))
        .and_then(Value::as_str)
        .filter(|command| command.chars().take(65_537).count() <= 65_536)
    else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for part in crate::commands::shell_parts(command) {
        if !READER.is_match(part.trim()) {
            continue;
        }
        for captures in PATH.captures_iter(part) {
            let Some(path) =
                (1..=3).find_map(|index| captures.get(index).map(|value| value.as_str()))
            else {
                continue;
            };
            let Some((directory, _)) = path.rsplit_once(['/', '\\']) else {
                continue;
            };
            let name = directory.rsplit(['/', '\\']).next().unwrap_or("");
            if (1..=160).contains(&name.chars().count())
                && !name
                    .chars()
                    .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
                && name != "."
                && name != ".."
            {
                result.push(SkillRead {
                    name: name.to_owned(),
                    path: path.to_owned(),
                    workdir: args
                        .and_then(|args| args.get("workdir"))
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                });
                if result.len() >= 40 {
                    return result;
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn recognizes_only_bounded_read_commands_and_omits_paths() {
        let value = json!({"cmd":"cat '/private/alpha/SKILL.md' && sed -n '1,4p' C:\\skills\\beta\\SKILL.md && echo /tmp/no/SKILL.md"});
        assert_eq!(
            skill_reads("exec_command", value.as_object()),
            ["alpha", "beta"]
        );
        assert!(skill_reads("other", value.as_object()).is_empty());
    }
}
