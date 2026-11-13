use jiter::JsonValue;
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

static IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.:-]{1,160}$").unwrap());
static MODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.:-]{1,80}$").unwrap());
static BRANCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_./-]{1,160}$").unwrap());
static COMMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-fA-F0-9]{7,64}$").unwrap());
static PARENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-fA-F0-9-]{36}$").unwrap());
static AGENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/root(?:/[a-zA-Z0-9_.-]{1,160}){0,12}$").unwrap());

fn fields(values: &Value) -> Map<String, Value> {
    let mut result = Map::new();
    for key in [
        "model_provider",
        "cli_version",
        "originator",
        "history_mode",
        "approval_policy",
        "agent_nickname",
        "agent_role",
        "service_tier",
        "reasoning_summary",
        "approvals_reviewer",
    ] {
        let alias = if key == "approval_policy" {
            "approval_mode"
        } else if key == "model_provider" {
            "model_provider_id"
        } else {
            key
        };
        if let Some(value) = values
            .get(key)
            .or_else(|| values.get(alias))
            .and_then(Value::as_str)
            .filter(|value| IDENTIFIER.is_match(value))
        {
            result.insert(key.into(), json!(value));
        }
    }
    for (key, source) in [
        ("sandbox_mode", "sandbox_policy"),
        ("collaboration_mode", "collaboration_mode"),
    ] {
        let mut value = values.get(source).cloned().unwrap_or(Value::Null);
        if let Some(text) = value
            .as_str()
            .filter(|text| text.starts_with('{') && text.len() <= 4096)
        {
            value = serde_json::from_str(text).unwrap_or(Value::Null);
        }
        let selected = if let Some(object) = value.as_object() {
            object.get(if key == "sandbox_mode" {
                "type"
            } else {
                "mode"
            })
        } else {
            Some(&value)
        };
        if let Some(value) = selected
            .and_then(Value::as_str)
            .filter(|value| MODE.is_match(value))
        {
            result.insert(key.into(), json!(value));
        }
    }
    if let Some(value) = values
        .get("context_window")
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
    {
        result.insert("context_window".into(), json!(value));
    }
    for (key, source, pattern) in [
        ("git_branch", "git_branch", &*BRANCH),
        ("git_commit", "git_sha", &*COMMIT),
        ("parent_thread_id", "parent_thread_id", &*PARENT),
        ("agent_path", "agent_path", &*AGENT),
    ] {
        if let Some(value) = values
            .get(source)
            .and_then(Value::as_str)
            .filter(|value| pattern.is_match(value))
        {
            result.insert(key.into(), json!(value));
        }
    }
    result
}

pub(super) fn execution(values: &Value) -> Value {
    let spawn = values
        .get("source")
        .and_then(Value::as_str)
        .filter(|value| value.len() <= 4096)
        .and_then(|value| serde_json::from_str::<Value>(value).ok());
    let mut result = spawn
        .as_ref()
        .and_then(|value| value.get("subagent"))
        .and_then(|value| value.get("thread_spawn"))
        .filter(|value| value.is_object())
        .map(fields)
        .unwrap_or_default();
    result.extend(fields(values));
    Value::Object(result)
}

fn field<'a, 'data>(object: &'a JsonValue<'data>, key: &str) -> Option<&'a JsonValue<'data>> {
    let JsonValue::Object(entries) = object else {
        return None;
    };
    entries
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn selected(value: &JsonValue<'_>) -> Map<String, Value> {
    let mut values = Map::new();
    for key in [
        "model_provider",
        "model_provider_id",
        "cli_version",
        "originator",
        "history_mode",
        "approval_policy",
        "approval_mode",
        "agent_nickname",
        "agent_role",
        "service_tier",
        "reasoning_summary",
        "approvals_reviewer",
        "git_branch",
        "git_sha",
        "parent_thread_id",
        "agent_path",
    ] {
        if let Some(JsonValue::Str(text)) = field(value, key) {
            if text.len() <= 4096 {
                values.insert(key.into(), json!(text.as_ref()));
            }
        }
    }
    if let Some(JsonValue::Int(number)) = field(value, "context_window") {
        values.insert("context_window".into(), json!(number));
    }
    for (key, member) in [("sandbox_policy", "type"), ("collaboration_mode", "mode")] {
        match field(value, key) {
            Some(JsonValue::Object(_)) => {
                if let Some(JsonValue::Str(text)) = field(field(value, key).unwrap(), member) {
                    if text.len() <= 80 {
                        values.insert(key.into(), json!({(member):text.as_ref()}));
                    }
                }
            }
            Some(JsonValue::Str(text)) if text.len() <= 4096 => {
                values.insert(key.into(), json!(text.as_ref()));
            }
            _ => {}
        }
    }
    fields(&Value::Object(values))
}

pub(super) fn session(value: &JsonValue<'_>, include_spawn: bool) -> Map<String, Value> {
    let mut result = selected(value);
    if include_spawn {
        match field(value, "source") {
            Some(source @ JsonValue::Object(_)) => {
                if let Some(spawn) =
                    field(source, "subagent").and_then(|value| field(value, "thread_spawn"))
                {
                    result.extend(selected(spawn));
                }
            }
            Some(JsonValue::Str(source)) if source.len() <= 4096 => {
                if let Ok(source) = serde_json::from_str::<Value>(source) {
                    if let Some(spawn) = source
                        .get("subagent")
                        .and_then(|value| value.get("thread_spawn"))
                        .filter(|value| value.is_object())
                    {
                        result.extend(fields(spawn));
                    }
                }
            }
            _ => {}
        }
        if let Some(git) = field(value, "git") {
            let mut values = Map::new();
            for (source, target) in [("branch", "git_branch"), ("commit_hash", "git_sha")] {
                if let Some(JsonValue::Str(text)) = field(git, source) {
                    if text.len() <= 160 {
                        values.insert(target.into(), json!(text.as_ref()));
                    }
                }
            }
            result.extend(fields(&Value::Object(values)));
        }
    }
    result
}

pub(super) fn valid_checkpoint(values: &std::collections::BTreeMap<String, Value>) -> bool {
    values.iter().all(|(key, value)| match key.as_str() {
        "model_provider" | "cli_version" | "originator" | "history_mode" | "approval_policy"
        | "agent_nickname" | "agent_role" | "service_tier" | "reasoning_summary"
        | "approvals_reviewer" => value.as_str().is_some_and(|text| IDENTIFIER.is_match(text)),
        "sandbox_mode" | "collaboration_mode" => {
            value.as_str().is_some_and(|text| MODE.is_match(text))
        }
        "git_branch" => value.as_str().is_some_and(|text| BRANCH.is_match(text)),
        "git_commit" => value.as_str().is_some_and(|text| COMMIT.is_match(text)),
        "parent_thread_id" => value.as_str().is_some_and(|text| PARENT.is_match(text)),
        "agent_path" => value.as_str().is_some_and(|text| AGENT.is_match(text)),
        "context_window" => value.as_i64().is_some_and(|number| number > 0),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_preserves_plain_and_structured_modes_without_private_fields() {
        let plain =
            execution(&json!({"sandbox_policy":"workspace-write","collaboration_mode":"default"}));
        assert_eq!(plain["sandbox_mode"], "workspace-write");
        assert_eq!(plain["collaboration_mode"], "default");
        let object = execution(
            &json!({"sandbox_policy":{"type":"read-only","roots":["PRIVATE"]},"collaboration_mode":"{\"mode\":\"plan\",\"instructions\":\"PRIVATE\"}"}),
        );
        assert_eq!(object["sandbox_mode"], "read-only");
        assert_eq!(object["collaboration_mode"], "plan");
        assert!(!object.to_string().contains("PRIVATE"));
        let invalid =
            execution(&json!({"sandbox_policy":"{broken","collaboration_mode":"private value"}));
        assert_eq!(invalid, json!({}));
    }
}
