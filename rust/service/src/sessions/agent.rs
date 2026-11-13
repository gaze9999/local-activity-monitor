use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_./:@-]{1,256}$").unwrap());
static HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\AMessage Type: (MESSAGE|NEW_TASK|FINAL_ANSWER)\r?\nTask name: ([A-Za-z0-9_./:@-]{1,256})\r?\nSender: ([A-Za-z0-9_./:@-]{1,256})\r?\nPayload:\r?\n").unwrap()
});

fn valid_id(value: Option<&str>) -> Option<&str> {
    value.filter(|value| ID.is_match(value))
}
fn action(tool: &str) -> Option<&'static str> {
    for prefix in [
        "",
        "collaboration.",
        "functions.",
        "functions.collaboration.",
        "collaboration__",
    ] {
        if let Some(name) = tool.strip_prefix(prefix) {
            match name {
                "send_message" => return Some("send_message"),
                "spawn_agent" => return Some("spawn_agent"),
                "followup_task" => return Some("followup_task"),
                _ => {}
            }
        }
    }
    None
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Outgoing {
    action: String,
    tool: String,
    target: Option<String>,
    task_name: Option<String>,
    sender: Option<String>,
    nested: bool,
    index: usize,
    invocation_index: usize,
}
impl Outgoing {
    pub fn metadata(&self) -> Value {
        json!({"action":self.action,"tool":self.tool,"target":self.target,"task_name":self.task_name,"sender":self.sender,"direction":"outgoing","nested":self.nested,"index":self.index,"invocation_index":self.invocation_index,"recognition":if self.nested {"literal_call_site"}else{"direct_tool_call"}})
    }
    pub fn is_nested(&self) -> bool {
        self.nested
    }
    pub fn tool(&self) -> &str {
        &self.tool
    }
    pub fn invocation_index(&self) -> usize {
        self.invocation_index
    }
    pub fn new(
        tool: &str,
        args: Option<&Map<String, Value>>,
        sender: Option<&str>,
        nested: bool,
        index: usize,
        invocation_index: usize,
    ) -> Option<Self> {
        let action = action(tool)?;
        let pick = |key| {
            args.and_then(|args| args.get(key))
                .and_then(Value::as_str)
                .and_then(|value| valid_id(Some(value)))
                .map(str::to_owned)
        };
        Some(Self {
            action: action.into(),
            tool: tool.into(),
            target: pick("target").or_else(|| pick("recipient")),
            task_name: if action == "spawn_agent" {
                pick("task_name")
            } else {
                None
            },
            sender: valid_id(sender).map(str::to_owned),
            nested,
            index,
            invocation_index,
        })
    }
    pub fn valid(&self, index: usize) -> bool {
        action(&self.tool) == Some(self.action.as_str())
            && self.index == index
            && self.invocation_index < lam_session_core::INVOCATION_LIMIT
            && (self.nested || self.invocation_index == 0)
            && [&self.target, &self.task_name, &self.sender]
                .iter()
                .all(|value| {
                    value
                        .as_deref()
                        .is_none_or(|value| valid_id(Some(value)).is_some())
                })
            && (self.action == "spawn_agent" || self.task_name.is_none())
    }
    pub fn render(
        &self,
        thread: &str,
        call: &str,
        time: Option<DateTime<Utc>>,
        completed: Option<DateTime<Utc>>,
        duration: Option<i64>,
    ) -> Value {
        json!({"action":self.action,"tool":self.tool,"target":self.target,"task_name":self.task_name,"sender":self.sender,"direction":"outgoing","nested":self.nested,"index":self.index,"invocation_index":self.invocation_index,"recognition":if self.nested {"literal_call_site"}else{"direct_tool_call"},"thread_id":thread,"thread_name":null,"call_id":call,"timestamp":time.map(super::project::stamp),"completed_at":completed.map(super::project::stamp),"duration_ms":duration})
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Incoming {
    message_type: String,
    target: String,
    sender: String,
    timestamp: Option<DateTime<Utc>>,
}
impl Incoming {
    pub fn parse(text: &str, timestamp: Option<DateTime<Utc>>) -> Option<Self> {
        let found = HEADER.captures(text)?;
        Some(Self {
            message_type: found[1].into(),
            target: found[2].into(),
            sender: found[3].into(),
            timestamp,
        })
    }
    pub fn valid(&self) -> bool {
        matches!(
            self.message_type.as_str(),
            "MESSAGE" | "NEW_TASK" | "FINAL_ANSWER"
        ) && valid_id(Some(&self.target)).is_some()
            && valid_id(Some(&self.sender)).is_some()
    }
    pub fn time(&self) -> Option<DateTime<Utc>> {
        self.timestamp
    }
    pub fn render(&self, thread: &str) -> Value {
        json!({"action":"receive_message","message_type":self.message_type,"target":self.target,"sender":self.sender,"task_name":self.target,"direction":"incoming","nested":false,"index":0,"invocation_index":null,"recognition":"recorded_message_header","timestamp":self.timestamp.map(super::project::stamp),"thread_id":thread})
    }
}
