//! Project recorded command text into operation names without running commands.
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::sync::LazyLock;

const COMMAND_LIMIT: usize = 65_536;
const GIT_PER_COMMAND: usize = 80;
const CHECK_LIMIT: usize = 40;
const GIT_OPERATIONS: &[&str] = &[
    "status",
    "log",
    "diff",
    "show",
    "branch",
    "rev-parse",
    "ls-files",
    "ls-tree",
    "remote",
    "fetch",
    "pull",
    "push",
    "commit",
    "add",
    "restore",
    "reset",
    "checkout",
    "switch",
    "merge",
    "rebase",
    "tag",
    "stash",
    "clean",
    "worktree",
    "submodule",
    "init",
    "clone",
    "config",
    "reflog",
    "cherry-pick",
    "revert",
    "describe",
    "blame",
    "check-ignore",
    "for-each-ref",
    "merge-base",
];
const CHECK_EXECUTABLES: &[&str] = &[
    "python", "python3", "py", "node", "npm", "pnpm", "yarn", "bun", "dotnet", "cargo", "go",
    "pytest", "mypy", "tsc", "ruff",
];

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommandMetadata {
    pub git: Vec<GitOperation>,
    pub checks: Vec<CheckOperation>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitOperation {
    pub operation: String,
    pub repository: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckOperation {
    pub operation: String,
    pub category: CheckCategory,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckCategory {
    Package,
    Python,
    Check,
    Runtime,
    Javascript,
}

static CHECK_PATTERNS: LazyLock<Vec<(Regex, CheckCategory)>> = LazyLock::new(|| {
    [
        (r"^(?:npm|pnpm|yarn|bun)\s+(?:run\s+)?(?:test|build|lint|typecheck|check)\b", CheckCategory::Package),
        (r"^(?:[.\\/\w-]*[\\/])?(?:python(?:3)?(?:\.exe)?|py)\s+-m\s+(?:unittest|pytest|compileall)\b", CheckCategory::Python),
        (r"^(?:pytest|mypy|tsc)\b", CheckCategory::Check),
        (r"^ruff\s+(?:check|format)\b", CheckCategory::Python),
        (r"^(?:dotnet|cargo|go)\s+(?:test|build|check|clippy|vet)\b", CheckCategory::Runtime),
        (r"^node\s+--check\b", CheckCategory::Javascript),
    ].into_iter().map(|(pattern, category)| (Regex::new(pattern).expect("check pattern"), category)).collect()
});
static EXECUTABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:"([^"\n]+)"|'([^'\n]+)'|([^\s]+))\s+(.*)$"#).expect("executable pattern")
});
static FLAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:(?:-[BIu]|-X\s+\S+)\s+)+").expect("interpreter flags"));

impl CommandMetadata {
    pub fn observe(&mut self, tool: &str, args: Option<&Map<String, Value>>) {
        self.observe_selected(tool, args, true, true);
    }

    pub fn observe_selected(
        &mut self,
        tool: &str,
        args: Option<&Map<String, Value>>,
        git: bool,
        checks: bool,
    ) {
        if !git && !checks {
            return;
        }
        if !matches!(tool, "exec_command" | "functions.exec_command") {
            return;
        }
        let Some(args) = args else { return };
        self.observe_text_selected(
            args.get("cmd").and_then(Value::as_str),
            args.get("workdir").and_then(Value::as_str),
            git,
            checks,
        );
    }

    pub fn observe_text(&mut self, command: Option<&str>, workdir: Option<&str>) {
        self.observe_text_selected(command, workdir, true, true);
    }

    fn observe_text_selected(
        &mut self,
        command: Option<&str>,
        workdir: Option<&str>,
        git_enabled: bool,
        checks_enabled: bool,
    ) {
        let Some(command) =
            command.filter(|value| value.chars().take(COMMAND_LIMIT + 1).count() <= COMMAND_LIMIT)
        else {
            return;
        };
        let repository = workdir.and_then(repository_label);
        let mut git = 0;
        for part in shell_parts(command) {
            if git_enabled && git < GIT_PER_COMMAND {
                if let Some(operation) = git_operation(part) {
                    self.git.push(GitOperation {
                        operation: operation.into(),
                        repository: repository.clone(),
                    });
                    git += 1;
                }
            }
            if checks_enabled && self.checks.len() < CHECK_LIMIT {
                if let Some(check) = check_operation(part.trim()) {
                    self.checks.push(check);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.git.is_empty() && self.checks.is_empty()
    }

    pub fn valid(&self) -> bool {
        self.git.len() <= GIT_PER_COMMAND * crate::INVOCATION_LIMIT
            && self.git.iter().all(|event| {
                GIT_OPERATIONS.contains(&event.operation.as_str())
                    && event
                        .repository
                        .as_ref()
                        .is_none_or(|repository| valid_label(repository))
            })
            && self.checks.len() <= CHECK_LIMIT
            && self.checks.iter().all(|event| {
                check_operation(&event.operation).is_some_and(|matched| {
                    matched.operation == event.operation && matched.category == event.category
                })
            })
    }
}

fn valid_label(value: &str) -> bool {
    !value.is_empty()
        && value.chars().count() <= 160
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '/' | '\\'))
}

fn repository_label(value: &str) -> Option<String> {
    let windows = value.contains('\\');
    let name = if windows {
        let value = if value.as_bytes().get(1) == Some(&b':') {
            &value[2..]
        } else {
            value
        };
        if value.starts_with("\\\\") {
            let parts = value
                .trim_start_matches('\\')
                .split(['\\', '/'])
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            if parts.len() <= 2 {
                return None;
            }
        }
        value
            .rsplit(['/', '\\'])
            .find(|part| !part.is_empty() && *part != ".")
    } else {
        value
            .rsplit('/')
            .find(|part| !part.is_empty() && *part != ".")
    }?;
    let name: String = name.chars().take(160).collect();
    valid_label(&name).then_some(name)
}

pub(crate) fn shell_parts(command: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in command.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '`' {
            escaped = true;
        } else if let Some(end) = quote {
            if ch == end {
                quote = None;
            }
        } else if matches!(ch, '"' | '\'') {
            quote = Some(ch);
        } else if matches!(ch, ';' | '\n' | '&' | '|') {
            parts.push(&command[start..index]);
            start = index + ch.len_utf8();
        }
    }
    parts.push(&command[start..]);
    parts
}

/// Select recorded command segments that match an observed operation.
pub fn matching_command_parts(
    command: &str,
    operation: &str,
    checks: bool,
) -> (Vec<String>, usize) {
    if command.chars().take(COMMAND_LIMIT + 1).count() > COMMAND_LIMIT {
        return (Vec::new(), 0);
    }
    let parts = shell_parts(command);
    let count = parts.len();
    let selected = parts
        .into_iter()
        .filter(|part| {
            if checks {
                check_operation(part.trim()).is_some_and(|item| item.operation == operation)
            } else {
                git_operation(part).is_some_and(|item| item == operation)
            }
        })
        .map(|part| part.trim().to_owned())
        .collect();
    (selected, count)
}

fn git_operation(part: &str) -> Option<&str> {
    let tokens = crate::files::shell_tokens(part)?;
    let executable = tokens.first()?.rsplit(['/', '\\']).next()?;
    if !executable.eq_ignore_ascii_case("git") && !executable.eq_ignore_ascii_case("git.exe") {
        return None;
    }
    let mut index = 1;
    while let Some(option) = tokens.get(index).filter(|value| value.starts_with('-')) {
        index += if matches!(
            *option,
            "-C" | "-c" | "--git-dir" | "--work-tree" | "--namespace"
        ) {
            2
        } else {
            1
        };
    }
    tokens
        .get(index)
        .copied()
        .filter(|operation| GIT_OPERATIONS.contains(operation))
}

fn check_operation(command: &str) -> Option<CheckOperation> {
    if let Some(parts) = EXECUTABLE.captures(command) {
        let executable = (1..=3)
            .find_map(|index| parts.get(index))?
            .as_str()
            .rsplit(['/', '\\'])
            .next()?;
        let lowercase = executable.to_ascii_lowercase();
        let executable = [".exe", ".cmd", ".ps1"]
            .into_iter()
            .find_map(|suffix| lowercase.strip_suffix(suffix))
            .unwrap_or(&lowercase);
        if CHECK_EXECUTABLES.contains(&executable) {
            let flags = FLAGS.replace(parts.get(4)?.as_str(), "");
            return match_check(&format!("{executable} {flags}"));
        }
    }
    match_check(command)
}

fn match_check(command: &str) -> Option<CheckOperation> {
    CHECK_PATTERNS.iter().find_map(|(pattern, category)| {
        let operation = pattern.find(command)?.as_str();
        if operation.chars().count() > 160
            || operation.chars().any(|ch| ch.is_control() && ch != '\t')
        {
            return None;
        }
        Some(CheckOperation {
            operation: operation.into(),
            category: *category,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_git_options_and_repository_basenames_without_arguments() {
        let mut output = CommandMetadata::default();
        output.observe_text(
            Some(r#"echo "git push"; git --no-optional-locks -c safe.directory=/repo log; git -C "/repo path" diff; & "C:\Program Files\Git\git.exe" status --short"#),
            Some(r#"D:\PRIVATE_PARENT\repo name\"#),
        );
        let operations: Vec<_> = output
            .git
            .iter()
            .map(|event| event.operation.as_str())
            .collect();
        assert_eq!(operations, ["log", "diff", "status"]);
        assert!(output
            .git
            .iter()
            .all(|event| event.repository.as_deref() == Some("repo name")));
        assert!(output.checks.is_empty());
        assert!(output.valid());
        assert!(!serde_json::to_string(&output)
            .unwrap()
            .contains("PRIVATE_PARENT"));
    }

    #[test]
    fn git_commands_keep_non_ascii_whitespace_inside_tokens() {
        let mut output = CommandMetadata::default();
        output.observe_text(Some("git\u{00a0}status; git \u{00a0}status; \u{00a0}git status; git status"), None);
        assert_eq!(output.git.len(), 1);
        assert_eq!(output.git[0].operation, "status");
        assert_eq!(matching_command_parts("git\u{00a0}status; git status", "status", false).0, ["git status"]);
    }

    #[test]
    fn normalizes_check_executables_and_keeps_only_recognized_prefixes() {
        let mut output = CommandMetadata::default();
        output.observe_text(
            Some(r#"& 'C:\runtime folder\python.exe' -X utf8 -I -B -m unittest PRIVATE_TARGET; echo "node --check"; pnpm run typecheck --project PRIVATE_PROJECT; node --check PRIVATE_FILE; cargo clippy --all-targets; ruff format --check PRIVATE_PATH"#),
            None,
        );
        let operations: Vec<_> = output
            .checks
            .iter()
            .map(|event| event.operation.as_str())
            .collect();
        assert_eq!(
            operations,
            [
                "python -m unittest",
                "pnpm run typecheck",
                "node --check",
                "cargo clippy",
                "ruff format"
            ]
        );
        assert!(output.valid());
        assert!(!serde_json::to_string(&output).unwrap().contains("PRIVATE_"));
    }

    #[test]
    fn applies_command_and_check_limits() {
        let mut output = CommandMetadata::default();
        output.observe_text(Some(&"git status; ".repeat(90)), None);
        output.observe_text(Some(&"cargo test; ".repeat(45)), None);
        output.observe_text(
            Some(&format!("git diff; #{}", "x".repeat(COMMAND_LIMIT))),
            None,
        );
        assert_eq!(output.git.len(), GIT_PER_COMMAND);
        assert_eq!(output.checks.len(), CHECK_LIMIT);
    }
}
