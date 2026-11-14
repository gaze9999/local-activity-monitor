use chrono::{DateTime, SecondsFormat, Utc};
use jiter::JsonValue;
use lam_history_store::mcp::Observed;
use lam_session_core::{nested_invocations, CommandMetadata, SqlMetadata, INVOCATION_LIMIT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::SystemTime;

pub const FILE_CALL_LIMIT: usize = 8192;
const OBSERVATION_LIMIT: usize = 100;
const COUNTERS: [&str; 6] = [
    "input_tokens",
    "cached_input_tokens",
    "cache_write_input_tokens",
    "output_tokens",
    "reasoning_output_tokens",
    "total_tokens",
];

type Time = DateTime<Utc>;

pub fn now() -> Time {
    SystemTime::now().into()
}

pub fn stamp(time: Time) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn timestamp(value: Option<&str>) -> Option<Time> {
    let value = value.filter(|text| text.len() <= 40)?;
    let time = lam_history_store::parse_utc_stamp(value)?;
    DateTime::from_timestamp_millis(time.timestamp_millis())
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

fn text<'a>(value: Option<&'a JsonValue<'_>>) -> Option<&'a str> {
    match value? {
        JsonValue::Str(value) => Some(value),
        _ => None,
    }
}

fn valid_name(value: &str, maximum: usize, colon: bool) -> bool {
    (1..=maximum).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || b"_.-".contains(&byte) || colon && byte == b':'
        })
}

fn name(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| valid_name(value, 160, true))
        .map(str::to_owned)
}

fn qualified_tool(payload: &JsonValue<'_>) -> Option<String> {
    let tool = text(field(payload, "name"))?;
    let namespace = text(field(payload, "namespace")).filter(|value| valid_name(value, 100, false));
    if let Some(namespace) = namespace.filter(|namespace| {
        !tool.starts_with("mcp__")
            && !tool
                .strip_prefix(namespace)
                .is_some_and(|tail| tail.starts_with('.') || tail.starts_with("__"))
    }) {
        let separator = if namespace.starts_with("mcp__") {
            "__"
        } else {
            "."
        };
        return name(Some(&format!("{namespace}{separator}{tool}")));
    }
    name(Some(tool))
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

pub fn filename_identity(name: &str) -> Option<String> {
    let stem = name.strip_suffix(".jsonl")?;
    let bytes = stem.as_bytes();
    let mut index = 0;
    let mut last = None;
    while index + 36 <= bytes.len() {
        let candidate = std::str::from_utf8(&bytes[index..index + 36]).ok();
        if let Some(id) = candidate.filter(|id| uuid(id)) {
            last = Some(id.to_owned());
            index += 36;
        } else {
            index += 1;
        }
    }
    last
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    tool: String,
    pub time: Option<Time>,
    completed: Option<Time>,
    sequence: u64,
    #[serde(default)]
    observation: Option<Observed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nested: Option<NestedActivity>,
    #[serde(default, skip_serializing_if = "CommandMetadata::is_empty")]
    pub commands: CommandMetadata,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    jev: Vec<JevOperation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    skills: Vec<String>,
    #[serde(default, skip_serializing_if = "SqlMetadata::is_empty")]
    sqlite: SqlMetadata,
    #[serde(default)]
    sql_failed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure: Option<ToolFailure>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    agent_messages: Vec<super::agent::Outgoing>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    files: Vec<Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    files_incomplete: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    mcp_sources_incomplete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) request_position: Option<super::details::Position>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) response_position: Option<super::details::Position>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JevOperation {
    operation: JevKind,
    nested: bool,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum JevKind {
    Rank,
    Evaluate,
    Status,
}

fn jev_operation(tool: &str, nested: bool) -> Option<JevOperation> {
    if let Some(operation) = match tool {
        "jev_rank" => Some(JevKind::Rank),
        "jev_evaluate" => Some(JevKind::Evaluate),
        "jev_status" => Some(JevKind::Status),
        _ => None,
    } {
        return Some(JevOperation { operation, nested });
    }
    let prefix = ["mcp__jev__", "jev.", "jev:", "jev_"]
        .iter()
        .find_map(|prefix| tool.strip_prefix(prefix));
    let name = prefix.unwrap_or(tool);
    let operation = match name {
        "jev_rank" => JevKind::Rank,
        "jev_evaluate" => JevKind::Evaluate,
        "jev_status" => JevKind::Status,
        "jevrank" if prefix.is_some() => JevKind::Rank,
        "jevevaluate" if prefix.is_some() => JevKind::Evaluate,
        "jevstatus" if prefix.is_some() => JevKind::Status,
        _ => return None,
    };
    Some(JevOperation { operation, nested })
}

pub(super) fn jev_kind(tool: &str) -> Option<&'static str> {
    match jev_operation(tool, false)?.operation {
        JevKind::Rank => Some("rank"),
        JevKind::Evaluate => Some("evaluate"),
        JevKind::Status => Some("status"),
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolFailure {
    code: String,
    exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cause: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    http_status: Option<u16>,
}

impl ToolFailure {
    fn valid(&self) -> bool {
        error_identifier(Some(&self.code)).is_some()
            && [
                &self.request_id,
                &self.trace_id,
                &self.error_type,
                &self.method,
                &self.cause,
            ]
            .iter()
            .all(|value| {
                value
                    .as_deref()
                    .is_none_or(|value| error_identifier(Some(value)).is_some())
            })
            && self
                .http_status
                .is_none_or(|status| (100..=599).contains(&status))
    }
}

fn tool_failure(output: &Value) -> Option<ToolFailure> {
    lam_history_store::mcp::output_error(output, |message| {
        let end = message
            .char_indices()
            .nth(8192)
            .map_or(message.len(), |(index, _)| index);
        super::core_logs::details(&message[..end])
    })
    .and_then(|value| serde_json::from_value(Value::Object(value)).ok())
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NestedEvent {
    tool: String,
    event: Observed,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NestedActivity {
    tools: BTreeMap<String, usize>,
    events: Vec<NestedEvent>,
    isolated: bool,
    truncated: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    tool_counts_incomplete: bool,
    #[serde(default)]
    single_invocation: bool,
}

impl NestedActivity {
    fn from_code(
        source: &str,
        commands: &mut CommandMetadata,
        jev: &mut Vec<JevOperation>,
        skills: &mut Vec<String>,
        sqlite: &mut SqlMetadata,
        files: &mut Vec<Value>,
        sender: Option<&str>,
        tool_events_enabled: bool,
        files_enabled: bool,
        sqlite_enabled: bool,
        command_switches: super::CommandSwitches,
        mcp_sources: &BTreeMap<String, bool>,
        mcp_switches: super::McpSwitches,
        sources_incomplete: &mut bool,
    ) -> (Self, Vec<super::agent::Outgoing>) {
        let nested = nested_invocations(source);
        let mut tools = BTreeMap::new();
        let mut required_tools = std::collections::BTreeSet::new();
        let mut events = Vec::new();
        let mut messages = Vec::new();
        for (invocation_index, invocation) in nested.calls.iter().enumerate() {
            if let Some(operation) =
                jev_operation(invocation.tool, true).filter(|_| command_switches.jev_calls)
            {
                jev.push(operation);
            }
            let args = invocation.literal.as_ref().and_then(Value::as_object);
            if command_switches.skills {
                skills.extend(
                    lam_session_core::skill_reads(invocation.tool, args)
                        .into_iter()
                        .take(40usize.saturating_sub(skills.len())),
                );
            }
            if let Some(literal) = invocation.literal.as_ref().filter(|_| files_enabled) {
                for event in lam_session_core::file_operations(invocation.tool, literal, true) {
                    if files.len() < 100 && !files.contains(&event) {
                        files.push(event);
                    }
                }
            }
            if let Some(message) = super::agent::Outgoing::new(
                invocation.tool,
                args,
                sender,
                true,
                messages.len(),
                invocation_index,
            ) {
                required_tools.insert(invocation.tool.to_owned());
                messages.push(message);
            }
            commands.observe_selected(
                invocation.tool,
                args,
                command_switches.git,
                command_switches.checks,
            );
            let before_sql = sqlite.0.len();
            if sqlite_enabled {
                sqlite.observe(invocation.tool, args, true);
            }
            if sqlite.0.len() > before_sql {
                required_tools.insert(invocation.tool.to_owned());
            }
            if let Some(tool) = name(Some(invocation.tool)) {
                if events.len() < OBSERVATION_LIMIT {
                    if let Some(mut event) = Observed::new(
                        &tool,
                        invocation.literal.as_ref().and_then(Value::as_object),
                    ) {
                        if !mcp_switches.allows(&event.server) {
                            *sources_incomplete = true;
                        } else {
                            if mcp_sources.get(&event.server) == Some(&false) {
                                event.metadata.clear();
                                *sources_incomplete = true;
                            }
                            required_tools.insert(tool.clone());
                            events.push(NestedEvent {
                                tool: tool.clone(),
                                event,
                            });
                        }
                    }
                }
                *tools.entry(tool).or_default() += 1;
            }
        }
        if !tool_events_enabled {
            tools.retain(|tool, _| required_tools.contains(tool));
        }
        let isolated = !nested.truncated && nested.calls.len() == 1 && events.len() == 1;
        (
            Self {
                tools,
                events,
                isolated,
                truncated: nested.truncated,
                tool_counts_incomplete: !tool_events_enabled,
                single_invocation: !nested.truncated && nested.calls.len() == 1,
            },
            messages,
        )
    }

    fn valid(&self) -> bool {
        let total = self
            .tools
            .iter()
            .try_fold(0_usize, |count, (tool, &calls)| {
                (valid_name(tool, 160, true) && (1..=INVOCATION_LIMIT).contains(&calls))
                    .then(|| count.checked_add(calls))
                    .flatten()
            });
        if total.is_none_or(|total| total > INVOCATION_LIMIT)
            || self.events.len() > OBSERVATION_LIMIT
        {
            return false;
        }
        if self.isolated && (self.truncated || total != Some(1) || self.events.len() != 1) {
            return false;
        }
        let mut used = BTreeMap::<&str, usize>::new();
        self.events.iter().all(|nested| {
            let count = used.entry(&nested.tool).or_default();
            *count += 1;
            self.tools
                .get(&nested.tool)
                .is_some_and(|total| *count <= *total)
                && nested.event.valid_for(&nested.tool)
                && (self.isolated || nested.event.result.is_empty())
        })
    }
}

impl Call {
    pub(super) fn needs_tool_replay(&self) -> bool {
        self.files_incomplete
            || self.mcp_sources_incomplete
            || self
                .nested
                .as_ref()
                .is_some_and(|nested| nested.tool_counts_incomplete)
    }
    pub(super) fn tool_name(&self) -> &str {
        &self.tool
    }
    fn valid_agent_messages(&self) -> bool {
        if self.agent_messages.len() > INVOCATION_LIMIT
            || self
                .agent_messages
                .windows(2)
                .any(|pair| pair[0].invocation_index() >= pair[1].invocation_index())
        {
            return false;
        }
        let nested_call = matches!(self.tool.as_str(), "exec" | "functions.exec");
        let mut counts = BTreeMap::<&str, usize>::new();
        self.agent_messages
            .iter()
            .enumerate()
            .all(|(index, message)| {
                if !message.valid(index) || message.is_nested() != nested_call {
                    return false;
                }
                let count = counts.entry(message.tool()).or_default();
                *count += 1;
                if nested_call {
                    self.nested
                        .as_ref()
                        .and_then(|nested| nested.tools.get(message.tool()))
                        .is_some_and(|limit| *count <= *limit)
                } else {
                    message.tool() == self.tool && *count == 1
                }
            })
    }
    pub fn has_failure(&self) -> bool {
        self.failure.is_some()
    }

    pub fn duration_ms(&self) -> Option<i64> {
        self.time
            .zip(self.completed)
            .map(|(start, end)| (end - start).num_milliseconds().max(0))
    }

    pub fn completed_at(&self) -> Option<Time> {
        self.completed
    }

    pub fn activities<'a>(
        &'a self,
        thread: &'a str,
        call_id: &'a str,
    ) -> impl Iterator<Item = Value> + 'a {
        let duration = self.duration_ms();
        let timestamp = self.time.map(stamp);
        let completed = self.completed.map(stamp);
        let sql_timestamp = timestamp.clone();
        let sql_completed = completed.clone();
        let result = if self.sql_failed {
            "failed"
        } else if self.completed.is_some() {
            "returned"
        } else {
            "unknown"
        };
        let sql = self.sqlite.0.iter().enumerate().map(move |(index, event)| json!({
            "statement":event.statement,"operation":event.operation,"engine":"SQLite","database":event.database,
            "workdir":event.workdir,"tool":event.tool,"recognition":event.recognition,"nested":event.nested,
            "thread_id":thread,"thread_name":null,"call_id":call_id,"index":index,"timestamp":sql_timestamp,
                "completed_at":sql_completed,"duration_ms":null,"container_duration_ms":duration,"result":result
        }));
        self.observation.iter().map(|item| (false, item)).chain(
            self.nested.iter().flat_map(|nested| nested.events.iter().map(|item| (true, &item.event)))
        ).enumerate().map(move |(index, (nested, observed))| {
            json!({
                "server":observed.server,"tool":observed.tool,"category":observed.category,"action":observed.action,
                "metadata":observed.metadata,"result":observed.result,"thread_id":thread,"thread_name":null,
                "call_id":call_id,"index":index,"timestamp":timestamp,"completed_at":completed,
                "duration_ms":if nested { None } else { duration },
                "container_duration_ms":if nested { duration } else { None },"nested":nested,
            })
        }).chain(sql)
    }

    pub fn observation_count(&self) -> usize {
        usize::from(self.observation.is_some())
            + self.nested.as_ref().map_or(0, |nested| nested.events.len())
            + self.sqlite.0.len()
            + self.skills.len()
            + usize::from(self.failure.is_some())
    }

    pub fn skill_history<'a>(
        &'a self,
        thread: &'a str,
        call_id: &'a str,
    ) -> impl Iterator<Item = Value> + 'a {
        self.skills.iter().map(move |skill| json!({"thread_id":thread,"call_id":call_id,"skill":skill,"timestamp":self.time.map(stamp)}))
    }

    pub fn failure_event(&self, thread: &str, call_id: &str) -> Option<Value> {
        let failure = self.failure.as_ref()?;
        let timestamp = self.completed.or(self.time).map(stamp);
        let observed = self.observation.as_ref().or_else(|| {
            self.nested
                .as_ref()
                .filter(|nested| nested.isolated)
                .and_then(|nested| nested.events.first())
                .map(|event| &event.event)
        });
        let mut event = serde_json::to_value(failure).ok()?;
        let fields = event.as_object_mut()?;
        fields.insert("timestamp".into(), json!(timestamp));
        fields.insert("thread_id".into(), thread.into());
        fields.insert("thread_name".into(), Value::Null);
        fields.insert("call_id".into(), call_id.into());
        fields.insert("source".into(), "tool_result".into());
        fields.insert("severity".into(), "error".into());
        fields.insert(
            "category".into(),
            if observed.is_some_and(|value| value.server != "web") {
                "mcp"
            } else {
                "tool"
            }
            .into(),
        );
        fields.insert("server".into(), json!(observed.map(|value| &value.server)));
        fields.insert(
            "tool".into(),
            observed
                .map_or(&self.tool, |value| &value.tool)
                .clone()
                .into(),
        );
        let identity = [
            Some("tool_result"),
            Some(thread),
            Some(call_id),
            timestamp.as_deref(),
            None,
            Some(failure.code.as_str()),
            None,
        ]
        .map(|field| serde_json::to_string(&field).expect("projected identifier"))
        .join(", ");
        fields.insert(
            "content_id".into(),
            format!("{:x}", Sha256::digest(format!("[{identity}]"))).into(),
        );
        Some(event)
    }

    fn complete(
        &mut self,
        output: Option<&JsonValue<'_>>,
        when: Option<Time>,
        files_enabled: bool,
        mcp_sources: &BTreeMap<String, bool>,
        mcp_switches: super::McpSwitches,
        errors_enabled: bool,
    ) {
        self.completed = when;
        if !files_enabled && !self.files.is_empty() {
            self.files_incomplete = true;
        }
        let sqlite = !self.sqlite.is_empty();
        let file_isolated = self
            .nested
            .as_ref()
            .map_or(true, |nested| nested.single_invocation);
        let disabled_source = self
            .observation
            .as_ref()
            .or_else(|| {
                self.nested
                    .as_ref()
                    .filter(|nested| nested.isolated)
                    .and_then(|nested| nested.events.first())
                    .map(|event| &event.event)
            })
            .is_some_and(|event| mcp_sources.get(&event.server) == Some(&false));
        let observed = if let Some(observed) = self.observation.as_mut() {
            Some(observed)
        } else {
            self.nested
                .as_mut()
                .filter(|nested| nested.isolated)
                .and_then(|nested| nested.events.first_mut())
                .map(|nested| &mut nested.event)
        };
        {
            let output = output.map(json_value).unwrap_or(Value::Null);
            if files_enabled && file_isolated && self.files.len() == 1 {
                let (read, written) = lam_history_store::mcp::file_byte_counts(&output);
                if let Some(value) = read {
                    self.files[0]["read_bytes"] = json!(value);
                }
                if let Some(value) = written {
                    self.files[0]["write_bytes"] = json!(value);
                }
            }
            self.failure = if !errors_enabled || disabled_source {
                None
            } else {
                tool_failure(&output)
            };
            if sqlite {
                self.sql_failed = lam_history_store::mcp::output_failed(&output);
            }
            if let Some(observed) = observed {
                if !mcp_switches.allows(&observed.server)
                    || mcp_sources.get(&observed.server) == Some(&false)
                {
                    self.mcp_sources_incomplete = true;
                } else {
                    observed.complete(&output);
                }
            }
        }
    }
}

fn json_value(value: &JsonValue<'_>) -> Value {
    match value {
        JsonValue::Null => Value::Null,
        JsonValue::Bool(value) => (*value).into(),
        JsonValue::Int(value) => (*value).into(),
        JsonValue::BigInt(value) => serde_json::from_str(&value.to_string()).unwrap_or(Value::Null),
        JsonValue::Float(value) => json!(value),
        JsonValue::Str(value) => value.as_ref().into(),
        JsonValue::Array(values) => Value::Array(values.iter().map(json_value).collect()),
        JsonValue::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.to_string(), json_value(value)))
                .collect(),
        ),
    }
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ActivityKind {
    Unknown,
    Work,
    Codex,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Status {
    Observed,
    Running,
    Completed,
}

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionError {
    pub time: Option<Time>,
    pub code: String,
    pub reason: String,
    pub severity: Severity,
    #[serde(default)]
    pub record_offset: Option<u64>,
    #[serde(default)]
    pub record_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) failure: Option<ToolFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) call_id: Option<String>,
}

pub(super) fn identifier(value: Option<&str>) -> Option<&str> {
    value.filter(|value| {
        (1..=160).contains(&value.len())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_./:-".contains(&byte))
    })
}

fn error_identifier(value: Option<&str>) -> Option<&str> {
    identifier(value).filter(|value| !value.starts_with("sk-") && !value.starts_with("sk_"))
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub id: Option<String>,
    #[serde(skip)]
    pub(super) workdir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) workdir_position: Option<super::details::Position>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub errors_incomplete: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sqlite_incomplete: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub commands_incomplete: bool,
    created: Option<Time>,
    updated: Option<Time>,
    model: Option<String>,
    effort: Option<String>,
    context_at: Option<Time>,
    context_seen: bool,
    #[serde(default)]
    execution: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    agent_incoming: Vec<super::agent::Incoming>,
    kind: Option<ActivityKind>,
    trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowance: Option<Value>,
    tokens: Option<[Option<i64>; 6]>,
    tokens_at: Option<Time>,
    token_priority: u8,
    status: Option<Status>,
    status_at: Option<Time>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    status_cached: bool,
    task_start: Option<Time>,
    intervals: BTreeMap<Time, Time>,
    #[serde(skip)]
    pub calls: BTreeMap<String, Call>,
    pub errors: Vec<SessionError>,
    #[serde(skip)]
    pub pending_errors: Vec<SessionError>,
    next_sequence: u64,
    #[serde(skip)]
    call_order: VecDeque<String>,
    #[serde(skip)]
    pub dirty_calls: BTreeSet<String>,
    #[serde(skip)]
    pub retired_calls: BTreeSet<String>,
    #[serde(skip)]
    pub retired_observations: Vec<(String, Call)>,
    #[serde(skip)]
    pub retired_activity_count: usize,
    #[serde(skip)]
    pub observation_overflow: bool,
    #[serde(skip)]
    pub newly_trimmed_calls: u64,
}

/// File-event reduction over already resolved call winners. Retain only
/// owned previews, rendering only on admission so inputs can be released.
/// Valid source file fields retain their existing limits.
pub(super) struct FileSummary {
    limit: usize,
    total: usize,
    operations: BTreeMap<String, usize>,
    preview: BTreeMap<(std::cmp::Reverse<Option<String>>, usize), Value>,
}

struct FilePreview<'a> {
    thread: Option<&'a str>,
    id: &'a str,
    call: &'a Call,
    file: &'a Value,
}

impl FilePreview<'_> {
    fn render(&self) -> Value {
        let mut event = self.file.clone();
        event["thread_id"] = json!(self.thread);
        event["thread_name"] = Value::Null;
        event["call_id"] = json!(self.id);
        event["timestamp"] = json!(self.call.time.map(stamp));
        event["completed_at"] = json!(self.call.completed.map(stamp));
        if event["nested"] == true {
            event["duration_ms"] = Value::Null;
            event["container_duration_ms"] = json!(self.call.duration_ms());
        } else {
            event["duration_ms"] = json!(self.call.duration_ms());
        }
        event
    }
}

impl FileSummary {
    pub(super) fn new(limit: usize) -> Self {
        Self { limit, total: 0, operations: BTreeMap::new(), preview: BTreeMap::new() }
    }

    /// Feed projections in the original thread traversal order. Calls must
    /// already be deduplicated by source precedence; counts are not reversible.
    /// read_filter None includes all operations, Some(true) reads only.
    pub(super) fn observe(&mut self, projection: &Projection, boundary: Option<Time>, enabled: bool, read_filter: Option<bool>) {
        if !enabled { return; }
        for (id, call) in &projection.calls {
            if !boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff)) { continue; }
            let timestamp = call.time.map(stamp);
            for file in &call.files {
                if read_filter.is_some_and(|read| (file["operation"] == "read") != read) { continue; }
                let ordinal = self.total;
                self.total += 1;
                if let Some(operation) = file["operation"].as_str() {
                    *self.operations.entry(operation.to_owned()).or_default() += 1;
                }
                if self.limit == 0 { continue; }
                let rank = (std::cmp::Reverse(timestamp.clone()), ordinal);
                if self.preview.len() == self.limit && self.preview.last_key_value().is_some_and(|(last, _)| &rank >= last) { continue; }
                if self.preview.len() == self.limit { self.preview.pop_last(); }
                self.preview.insert(rank, FilePreview { thread: projection.id.as_deref(), id, call, file }.render());
            }
        }
    }

    pub(super) fn finish(self) -> (Vec<Value>, usize, BTreeMap<String, usize>) {
        (self.preview.into_values().collect(), self.total, self.operations)
    }
}

fn retain_preview<K: Ord, V>(preview: &mut BTreeMap<K, V>, rank: K, value: V, limit: usize) {
    if limit == 0 || (preview.len() == limit && preview.last_key_value().is_some_and(|(last, _)| &rank >= last)) { return; }
    if preview.len() == limit { preview.pop_last(); }
    preview.insert(rank, value);
}

/// Current call inputs are resolved winners with lookup keys matching their
/// projection IDs, as constructed by Collector::snapshots. The history index borrows the
/// existing retained history, whose production query is limited to 500 rows.
struct SkillSummary<'a> {
    historical: BTreeMap<(&'a str, &'a str, &'a str), (&'a str, &'a Value)>,
    preview: BTreeMap<(std::cmp::Reverse<Option<String>>, String, String, String), Value>,
    counts: BTreeMap<String, usize>,
    total: usize,
    boundary: Option<Time>,
    enabled: bool,
}

impl<'a> SkillSummary<'a> {
    fn new(history: &'a [Value], names: &std::collections::HashMap<String, Value>, boundary: Option<Time>, enabled: bool) -> Self {
        let mut historical = BTreeMap::new();
        if enabled {
            for event in history {
                let (Some(thread), Some(id), Some(skill), Some(time)) = (
                    event["thread_id"].as_str(), event["call_id"].as_str(), event["skill"].as_str(), event["timestamp"].as_str(),
                ) else { continue; };
                if !names.contains_key(thread) || boundary.is_some_and(|cutoff|
                    lam_history_store::parse_utc_stamp(time).is_none_or(|time| time < cutoff)) { continue; }
                historical.insert((thread, id, skill), (time, event));
            }
        }
        Self { historical, preview: BTreeMap::new(), counts: BTreeMap::new(), total: 0, boundary, enabled }
    }

    fn admit(&mut self, thread: &str, id: &str, skill: &str, time: Option<String>, render: impl FnOnce() -> Value) {
        self.total += 1;
        *self.counts.entry(skill.to_owned()).or_default() += 1;
        let rank = (std::cmp::Reverse(time), thread.to_owned(), id.to_owned(), skill.to_owned());
        if self.preview.len() == 500 && self.preview.last_key_value().is_some_and(|(last, _)| &rank >= last) { return; }
        if self.preview.len() == 500 { self.preview.pop_last(); }
        self.preview.insert(rank, render());
    }

    fn observe(&mut self, lookup_thread: &str, thread: Option<&str>, id: &str, call: &Call) {
        if !self.enabled || !self.boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff)) { return; }
        // Source validation limits this per-call set to 40 skill names.
        for skill in call.skills.iter().map(String::as_str).collect::<BTreeSet<_>>() {
            if let Some(key) = self.historical.keys().find(|(t, i, s)| *t == lookup_thread && *i == id && *s == skill).copied() {
                self.historical.remove(&key);
            }
            let Some(thread) = thread else { continue; };
            self.admit(thread, id, skill, call.time.map(stamp), || json!({
                "thread_id":thread,"thread_name":null,"call_id":id,"skill":skill,"tool":call.tool,
                "has_document":true,"timestamp":call.time.map(stamp),"completed_at":call.completed.map(stamp),
                "duration_ms":call.duration_ms()
            }));
        }
    }

    fn finish(mut self, names: &std::collections::HashMap<String, Value>) -> (Vec<Value>, usize, BTreeMap<String, usize>) {
        for ((thread, id, skill), (time, event)) in std::mem::take(&mut self.historical) {
            self.admit(thread, id, skill, Some(time.to_owned()), || event.clone());
        }
        let events = self.preview.into_values().map(|mut event| {
            if let Some(name) = event["thread_id"].as_str().and_then(|id| names.get(id)) {
                event["thread_name"] = name.clone();
            }
            event
        }).collect();
        (events, self.total, self.counts)
    }
}

pub(super) fn skill_summary(
    threads: &BTreeMap<&str, (Projection, bool)>,
    history: &[Value],
    thread_names: &std::collections::HashMap<String, Value>,
    boundary: Option<Time>,
    enabled: bool,
) -> (Vec<Value>, usize, BTreeMap<String, usize>) {
    if !enabled { return (Vec::new(), 0, BTreeMap::new()); }
    let mut summary = SkillSummary::new(history, thread_names, boundary, enabled);
    for (lookup_thread, (projection, _)) in threads {
        for (id, call) in &projection.calls { summary.observe(lookup_thread, projection.id.as_deref(), id, call); }
    }
    summary.finish(thread_names)
}

/// Observe each resolved winner once; retain no call borrow after admission.
#[derive(Default)]
struct RowTools {
    count: usize,
    counts: BTreeMap<String, usize>,
    nested: BTreeMap<String, usize>,
    preview: BTreeMap<(std::cmp::Reverse<Option<Time>>, String), Value>,
}

impl RowTools {
    fn observe(&mut self, id: &str, call: &Call, enabled: bool,
        totals: &mut BTreeMap<String, usize>, nested_totals: &mut BTreeMap<String, usize>) {
        self.count += 1;
        *self.counts.entry(call.tool.clone()).or_default() += 1;
        *totals.entry(call.tool.clone()).or_default() += 1;
        if let Some(nested) = call.nested.as_ref().filter(|_| enabled) {
            for (tool, count) in &nested.tools {
                *self.nested.entry(tool.clone()).or_default() += count;
                *nested_totals.entry(tool.clone()).or_default() += count;
            }
        }
        if !enabled { return; }
        let rank = (std::cmp::Reverse(call.time), id.to_owned());
        if self.preview.len() == 100 && self.preview.last_key_value().is_some_and(|(last, _)| &rank >= last) { return; }
        if self.preview.len() == 100 { self.preview.pop_last(); }
        let event = {
            let duration = call.duration_ms();
            let nested_tools = call.nested.as_ref().map_or_else(|| json!({}), |nested| json!(nested.tools));
            let mut event = json!({"call_id":id,"tool":call.tool,"timestamp":call.time.map(stamp),
                   "completed_at":call.completed.map(stamp),"duration_ms":duration,"nested_tools":nested_tools});
            if call.nested.as_ref().is_some_and(|nested| nested.truncated) {
                event["nested_truncated"] = true.into();
            }
            event
        };
        self.preview.insert(rank, event);
    }
}

/// Feed resolved calls for one row; preserve operation indices in tie order.
#[derive(Default)]
struct RowJev {
    preview: BTreeMap<(std::cmp::Reverse<Option<Time>>, String, usize), Value>,
}

impl RowJev {
    fn observe(&mut self, thread: Option<&str>, id: &str, call: &Call, enabled: bool) {
        if !enabled { return; }
        for (index, operation) in call.jev.iter().enumerate() {
            let rank = (std::cmp::Reverse(call.time), id.to_owned(), index);
            if self.preview.len() == 100 && self.preview.last_key_value().is_some_and(|(last, _)| &rank >= last) { continue; }
            if self.preview.len() == 100 { self.preview.pop_last(); }
            self.preview.insert(rank, json!({
                "thread_id":thread, "call_id":id, "index":index, "operation":operation.operation,
                "nested":operation.nested, "timestamp":call.time.map(stamp), "completed_at":call.completed.map(stamp),
            }));
        }
    }
}

/// Metadata-only preview for one thread, after outgoing call winner resolution.
#[derive(Default)]
struct CommunicationSummary {
    preview: BTreeMap<(std::cmp::Reverse<Option<String>>, u8,
        std::cmp::Reverse<Option<Time>>, String, usize), Value>,
}

impl CommunicationSummary {
    fn incoming(&mut self, thread: &str, ordinal: usize, message: &super::agent::Incoming, boundary: Option<Time>) {
        if !boundary.is_none_or(|cutoff| message.time().is_some_and(|time| time >= cutoff)) { return; }
        let rank = (std::cmp::Reverse(message.time().map(stamp)), 0u8,
            std::cmp::Reverse(None), String::new(), ordinal);
        if self.preview.len() == 100 && self.preview.last_key_value().is_some_and(|(last, _)| &rank >= last) { return; }
        if self.preview.len() == 100 { self.preview.pop_last(); }
        self.preview.insert(rank, message.render(thread));
    }

    fn outgoing(&mut self, thread: &str, id: &str, call: &Call) {
        for (ordinal, message) in call.agent_messages.iter().enumerate() {
            let rank = (std::cmp::Reverse(call.time.map(stamp)), 1u8,
                std::cmp::Reverse(call.time), id.to_owned(), ordinal);
            if self.preview.len() == 100 && self.preview.last_key_value().is_some_and(|(last, _)| &rank >= last) { continue; }
            if self.preview.len() == 100 { self.preview.pop_last(); }
            self.preview.insert(rank, message.render(thread, id, call.time, call.completed, call.duration_ms()));
        }
    }

    fn finish(self) -> Vec<Value> { self.preview.into_values().collect() }
}

impl Projection {
    pub fn for_id(id: Option<String>) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub(super) fn token_updated_at(&self) -> Option<Time> {
        self.tokens_at
    }

    pub fn updated(&self) -> Option<Time> {
        self.updated
    }

    pub(super) fn set_workdir(&mut self, value: Option<&str>) {
        self.workdir = value
            .filter(|value| {
                !value.is_empty()
                    && value.chars().count() <= 4096
                    && !value.chars().any(char::is_control)
            })
            .map(str::to_owned);
    }

    pub fn prune_order(&mut self) {
        self.call_order.retain(|id| self.calls.contains_key(id));
    }

    pub fn restore_calls(&mut self, calls: BTreeMap<String, Call>) -> bool {
        if self.id.as_deref().is_some_and(|id| !uuid(id))
            || self
                .allowance
                .as_ref()
                .is_some_and(|value| !super::allowance::valid(value))
            || !super::metadata::valid_checkpoint(&self.execution)
            || self.agent_incoming.len() > 100
            || self.agent_incoming.iter().any(|message| !message.valid())
            || [&self.model, &self.effort, &self.trigger]
                .iter()
                .any(|value| {
                    value
                        .as_deref()
                        .is_some_and(|name| !valid_name(name, 160, true))
                })
            || self
                .tokens
                .iter()
                .flatten()
                .flatten()
                .any(|value| *value < 0)
            || self.errors.len() > 50
            || self.errors.iter().any(|error| {
                error_identifier(Some(&error.code)).is_none()
                    || (error.failure.is_none() && error_identifier(Some(&error.reason)).is_none())
                    || (error.failure.is_none() && error.call_id.is_some())
                    || error.failure.as_ref().is_some_and(|failure| {
                        !failure.valid()
                            || failure.code != error.code
                            || !error.reason.is_empty()
                            || error.severity != Severity::Error
                            || error.record_hash.is_none()
                    })
                    || error
                        .call_id
                        .as_deref()
                        .is_some_and(|id| error_identifier(Some(id)).is_none())
                    || error.record_hash.is_some() != error.record_offset.is_some()
                    || error.record_hash.as_ref().is_some_and(|hash| {
                        hash.len() != 64
                            || !hash
                                .bytes()
                                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    })
            })
            || self.intervals.len() > 1024
            || calls.values().any(|call| {
                !valid_name(&call.tool, 160, true)
                    || !call.valid_agent_messages()
                    || call.files.len() > 100
                    || call
                        .files
                        .iter()
                        .any(|event| !lam_session_core::valid_file_operation(event))
                    || call
                        .request_position
                        .as_ref()
                        .is_some_and(|position| !position.valid())
                    || call
                        .response_position
                        .as_ref()
                        .is_some_and(|position| !position.valid())
                    || call
                        .failure
                        .as_ref()
                        .is_some_and(|failure| !failure.valid())
                    || call.sequence >= self.next_sequence
                    || !call.commands.valid()
                    || call.jev.len() > INVOCATION_LIMIT
                    || call.skills.len() > 40
                    || call.skills.iter().any(|name| {
                        name.is_empty()
                            || name.chars().count() > 160
                            || name == "."
                            || name == ".."
                            || name
                                .chars()
                                .any(|ch| ch.is_control() || ch == '/' || ch == '\\')
                    })
                    || call.jev.iter().any(|event| {
                        event.nested != matches!(call.tool.as_str(), "exec" | "functions.exec")
                    })
                    || (!matches!(call.tool.as_str(), "exec" | "functions.exec")
                        && call.jev.len() > 1)
                    || !call.sqlite.valid()
                    || call.sql_failed && call.sqlite.is_empty()
                    || call.sqlite.0.iter().any(|event| {
                        if matches!(call.tool.as_str(), "exec" | "functions.exec") {
                            !event.nested
                                || call
                                    .nested
                                    .as_ref()
                                    .is_none_or(|nested| !nested.tools.contains_key(&event.tool))
                        } else {
                            event.nested || event.tool != call.tool
                        }
                    })
                    || !call.commands.is_empty()
                        && !matches!(
                            call.tool.as_str(),
                            "exec" | "functions.exec" | "exec_command" | "functions.exec_command"
                        )
                    || call
                        .observation
                        .as_ref()
                        .is_some_and(|observed| !observed.valid_for(&call.tool))
                    || call.nested.as_ref().is_some_and(|nested| {
                        !matches!(call.tool.as_str(), "exec" | "functions.exec")
                            || call.observation.is_some()
                            || !nested.valid()
                    })
            })
            || calls.keys().any(|id| !valid_name(id, 160, true))
        {
            return false;
        }
        let mut order: Vec<_> = calls.iter().map(|(id, call)| (call.sequence, id)).collect();
        order.sort_unstable();
        self.call_order = order.into_iter().map(|(_, id)| id.clone()).collect();
        self.calls = calls;
        true
    }

    pub fn retire_call(&mut self, id: &str) {
        if let Some(call) = self.calls.remove(id) {
            self.newly_trimmed_calls = self.newly_trimmed_calls.saturating_add(1);
            let count = call.observation_count();
            if self.dirty_calls.remove(id) && count > 0 {
                if self.retired_activity_count + count <= FILE_CALL_LIMIT {
                    self.retired_activity_count += count;
                    self.retired_observations.push((id.to_owned(), call));
                } else {
                    self.observation_overflow = true;
                }
            }
            self.retired_calls.insert(id.to_owned());
        }
    }

    pub fn consume(
        &mut self,
        raw: &[u8],
        offset: u64,
        tool_events_enabled: bool,
        files_enabled: bool,
        mcp_sources: &BTreeMap<String, bool>,
        mcp_switches: super::McpSwitches,
        errors_enabled: bool,
        sqlite_enabled: bool,
        command_switches: super::CommandSwitches,
    ) -> bool {
        let line = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw);
        let Ok(record) = JsonValue::parse(line, false) else {
            return false;
        };
        let Some(payload @ JsonValue::Object(_)) = field(&record, "payload") else {
            return true;
        };
        if !errors_enabled {
            self.errors_incomplete = true;
        }
        if !sqlite_enabled {
            self.sqlite_incomplete = true;
        }
        if !command_switches.worktrees
            || !command_switches.git
            || !command_switches.checks
            || !command_switches.jev_calls
            || !command_switches.skills
            || !command_switches.usage
        {
            self.commands_incomplete = true;
        }
        let when = timestamp(text(field(&record, "timestamp")));
        self.updated = self.updated.max(when);
        let event = text(field(payload, "type"));
        match text(field(&record, "type")) {
            Some("session_meta") => {
                if command_switches.worktrees {
                    if let Some(cwd) = text(field(payload, "cwd")).filter(|value| {
                        !value.is_empty()
                            && value.chars().count() <= 4096
                            && !value.chars().any(char::is_control)
                    }) {
                        self.set_workdir(Some(cwd));
                        self.workdir_position = Some(super::details::Position::new(offset, raw));
                    }
                }
                self.execution
                    .extend(super::metadata::session(payload, true));
                if let Some(id) = text(field(payload, "id")).filter(|id| uuid(id)) {
                    if self.id.as_deref() != Some(id) {
                        self.dirty_calls.extend(
                            self.calls
                                .iter()
                                .filter(|(_, call)| call.observation_count() != 0)
                                .map(|(id, _)| id.clone()),
                        );
                    }
                    self.id = Some(id.to_owned());
                    self.created = when;
                }
                self.kind = Some(match text(field(payload, "originator")) {
                    Some("codex_work_desktop") => ActivityKind::Work,
                    Some("Codex Desktop") => ActivityKind::Codex,
                    _ => ActivityKind::Unknown,
                });
                if let Some(
                    trigger @ ("user" | "subagent" | "guardian_review" | "dot" | "orbit"
                    | "automation" | "schedule" | "heartbeat"),
                ) = text(field(payload, "thread_source"))
                {
                    self.trigger = Some(trigger.to_owned());
                }
            }
            Some("turn_context") => self.context(payload, when),
            Some("event_msg") => match event {
                Some("thread_settings_applied") => {
                    let id = text(field(payload, "thread_id")).filter(|id| !id.is_empty());
                    if id.is_none() || id == self.id.as_deref() {
                        if let Some(context @ JsonValue::Object(_)) =
                            field(payload, "thread_settings")
                        {
                            self.context(context, when);
                        }
                    }
                }
                Some("task_started" | "task_complete") => {
                    self.lifecycle(payload, when, event == Some("task_started"))
                }
                Some("token_count") => {
                    if let (true, Some(when), Some(limits)) =
                        (command_switches.usage, when, field(payload, "rate_limits"))
                    {
                        if self
                            .allowance
                            .as_ref()
                            .and_then(|value| value["updated_at"].as_str())
                            .is_none_or(|previous| stamp(when).as_str() >= previous)
                        {
                            if let Some(value) =
                                super::allowance::project(&json_value(limits), when)
                            {
                                self.allowance = Some(value);
                            }
                        }
                    }
                    if let Some(info) = field(payload, "info") {
                        self.usage(field(info, "total_token_usage"), when, 1);
                        if let Some(JsonValue::Int(value)) = field(info, "model_context_window") {
                            if *value > 0 {
                                self.execution.insert("context_window".into(), json!(value));
                            }
                        }
                    }
                }
                Some(reason) if errors_enabled => {
                    self.session_error(payload, reason, when, raw, offset)
                }
                Some(_) => {}
                None => {}
            },
            Some("token_usage_record") => self.usage(field(payload, "thread_token_usage"), when, 2),
            Some("response_item") => {
                if event == Some("message")
                    && text(field(&record, "channel")) != Some("analysis")
                    && text(field(payload, "channel")) != Some("analysis")
                    && matches!(text(field(payload, "role")), Some("user" | "assistant"))
                {
                    if let Some(JsonValue::Array(content)) = field(payload, "content") {
                        for item in content.iter() {
                            if matches!(
                                text(field(item, "type")),
                                Some("input_text" | "output_text" | "text")
                            ) {
                                if let Some(message) = text(field(item, "text"))
                                    .and_then(|text| super::agent::Incoming::parse(text, when))
                                {
                                    self.agent_incoming.push(message);
                                    if self.agent_incoming.len() > 100 {
                                        self.agent_incoming.remove(0);
                                    }
                                }
                            }
                        }
                    }
                }
                self.tool_event(
                    payload,
                    event,
                    when,
                    raw,
                    offset,
                    tool_events_enabled,
                    files_enabled,
                    mcp_sources,
                    mcp_switches,
                    errors_enabled,
                    sqlite_enabled,
                    command_switches,
                )
            }
            _ => {}
        }
        true
    }

    fn session_error(
        &mut self,
        payload: &JsonValue<'_>,
        reason: &str,
        time: Option<Time>,
        raw: &[u8],
        offset: u64,
    ) {
        if error_identifier(Some(reason)).is_none()
            || !(matches!(reason, "error" | "turn_aborted" | "turn_failed")
                || reason.ends_with("_failed"))
        {
            return;
        }
        let code = error_identifier(text(
            field(payload, "error").and_then(|error| field(error, "code")),
        ))
        .or_else(|| error_identifier(text(field(payload, "error_code"))))
        .unwrap_or(reason);
        let error = SessionError {
            time,
            code: code.into(),
            reason: reason.into(),
            severity: if reason == "turn_aborted" {
                Severity::Warning
            } else {
                Severity::Error
            },
            record_offset: Some(offset),
            record_hash: Some(format!("{:x}", Sha256::digest(raw))),
            failure: None,
            call_id: None,
        };
        self.pending_errors.push(error.clone());
        self.errors.push(error);
        self.errors
            .sort_by_key(|error| std::cmp::Reverse(error.time));
        self.errors.truncate(50);
    }

    fn context(&mut self, value: &JsonValue<'_>, when: Option<Time>) {
        if when >= self.context_at {
            self.context_seen = true;
            self.context_at = when;
            self.model = name(text(field(value, "model")));
            self.effort = name(text(
                field(value, "reasoning_effort").or_else(|| field(value, "effort")),
            ));
            self.execution
                .extend(super::metadata::session(value, false));
        }
    }

    fn lifecycle(&mut self, payload: &JsonValue<'_>, when: Option<Time>, started: bool) {
        let Some(when) = when else { return };
        let latest = Some(when) >= self.status_at;
        if latest {
            self.status_cached = false;
            self.status_at = Some(when);
            self.status = Some(if started {
                Status::Running
            } else {
                Status::Completed
            });
        }
        let start = timestamp(text(field(payload, "started_at")));
        if started {
            if latest {
                self.task_start = start.or(Some(when));
            }
        } else {
            let end = timestamp(text(field(payload, "completed_at"))).unwrap_or(when);
            if let Some(start) = start.or(self.task_start).filter(|start| end >= *start) {
                self.intervals.insert(start, end);
                if self.intervals.len() > 1024 {
                    self.intervals.pop_first();
                }
            }
            if latest {
                self.task_start = None;
            }
        }
    }

    pub(super) fn restore_cached_lifecycle(&mut self, cached: &Value) {
        if self.id.as_deref() != cached["thread_id"].as_str() {
            return;
        }
        let Some(time) = cached["task_time"]
            .as_str()
            .and_then(lam_history_store::parse_utc_stamp)
        else {
            return;
        };
        if self.status_at.is_some_and(|observed| observed >= time) {
            return;
        }
        let status = match cached["status"].as_str() {
            Some("running") => Status::Running,
            Some("completed") => Status::Completed,
            _ => return,
        };
        self.status = Some(status);
        self.status_at = Some(time);
        self.task_start = cached["task_start"]
            .as_str()
            .and_then(lam_history_store::parse_utc_stamp);
        self.status_cached = true;
    }

    pub(super) fn lifecycle_entry(&self, filename: &str, offset: u64) -> Option<Value> {
        let status = match self.status? {
            Status::Running => "running",
            Status::Completed => "completed",
            _ => return None,
        };
        let offset = i64::try_from(offset).ok()?;
        Some(
            json!({"source_file":filename,"thread_id":self.id.as_ref()?,"status":status,"offset":offset,"task_time":stamp(self.status_at?),"task_start":self.task_start.map(stamp)}),
        )
    }

    fn usage(&mut self, usage: Option<&JsonValue<'_>>, when: Option<Time>, priority: u8) {
        let (Some(value), Some(when)) = (usage, when) else {
            return;
        };
        if (Some(when), priority) < (self.tokens_at, self.token_priority) {
            return;
        }
        let counters = COUNTERS.map(|key| match field(value, key) {
            Some(JsonValue::Int(value)) if *value >= 0 => Some(*value),
            _ => None,
        });
        if counters.iter().any(Option::is_some) {
            self.tokens = Some(counters);
            self.tokens_at = Some(when);
            self.token_priority = priority;
        }
    }

    fn tool_event(
        &mut self,
        payload: &JsonValue<'_>,
        event: Option<&str>,
        when: Option<Time>,
        raw: &[u8],
        offset: u64,
        tool_events_enabled: bool,
        files_enabled: bool,
        mcp_sources: &BTreeMap<String, bool>,
        mcp_switches: super::McpSwitches,
        errors_enabled: bool,
        sqlite_enabled: bool,
        command_switches: super::CommandSwitches,
    ) {
        match event {
            Some("function_call" | "custom_tool_call") => {
                let id = text(field(payload, "call_id"))
                    .filter(|id| !id.is_empty())
                    .or_else(|| text(field(payload, "id")));
                let (Some(id), Some(tool)) = (name(id), qualified_tool(payload)) else {
                    return;
                };
                let arguments = field(payload, "arguments").or_else(|| field(payload, "input"));
                let shell = matches!(tool.as_str(), "exec_command" | "functions.exec_command");
                let observed = lam_history_store::mcp::identity(&tool).is_some();
                let is_agent =
                    super::agent::Outgoing::new(&tool, None, None, false, 0, 0).is_some();
                let args = if shell
                    || observed
                    || is_agent
                    || tool.to_ascii_lowercase().contains("sqlite")
                {
                    match arguments {
                        Some(JsonValue::Str(value)) => serde_json::from_str::<Value>(value).ok(),
                        Some(value @ JsonValue::Object(_)) => Some(json_value(value)),
                        _ => None,
                    }
                } else {
                    None
                };
                let args = args.as_ref().and_then(Value::as_object);
                let mut observation = observed.then(|| Observed::new(&tool, args)).flatten();
                let mut sources_incomplete = false;
                if observation
                    .as_ref()
                    .is_some_and(|event| !mcp_switches.allows(&event.server))
                {
                    observation = None;
                    sources_incomplete = true;
                }
                if let Some(event) = observation
                    .as_mut()
                    .filter(|event| mcp_sources.get(&event.server) == Some(&false))
                {
                    event.metadata.clear();
                    sources_incomplete = true;
                }
                let mut commands = CommandMetadata::default();
                let mut skills = if command_switches.skills {
                    lam_session_core::skill_reads(&tool, args)
                } else {
                    Vec::new()
                };
                let mut jev = jev_operation(&tool, false)
                    .filter(|_| command_switches.jev_calls)
                    .into_iter()
                    .collect::<Vec<_>>();
                commands.observe_selected(
                    &tool,
                    args,
                    command_switches.git,
                    command_switches.checks,
                );
                let mut sqlite = SqlMetadata::default();
                if sqlite_enabled {
                    sqlite.observe(&tool, args, false);
                }
                let mut files = Vec::new();
                if files_enabled && !matches!(tool.as_str(), "exec" | "functions.exec") {
                    let raw_args = arguments.map(json_value).unwrap_or(Value::Null);
                    let decoded =
                        if matches!(tool.as_str(), "apply_patch" | "functions.apply_patch") {
                            raw_args
                        } else if let Some(value) = raw_args.as_str() {
                            serde_json::from_str(value).unwrap_or(raw_args)
                        } else {
                            raw_args
                        };
                    files = lam_session_core::file_operations(&tool, &decoded, false);
                }
                let (nested, agent_messages) = if matches!(tool.as_str(), "exec" | "functions.exec")
                {
                    text(arguments)
                        .map(|source| {
                            NestedActivity::from_code(
                                source,
                                &mut commands,
                                &mut jev,
                                &mut skills,
                                &mut sqlite,
                                &mut files,
                                self.id.as_deref(),
                                tool_events_enabled,
                                files_enabled,
                                sqlite_enabled,
                                command_switches,
                                mcp_sources,
                                mcp_switches,
                                &mut sources_incomplete,
                            )
                        })
                        .map_or_else(
                            || (None, Vec::new()),
                            |(nested, messages)| (Some(nested), messages),
                        )
                } else {
                    (
                        None,
                        super::agent::Outgoing::new(&tool, args, self.id.as_deref(), false, 0, 0)
                            .into_iter()
                            .collect(),
                    )
                };
                let (completed, sequence, sql_failed, failure, response_position) =
                    if let Some(call) = self.calls.get(&id) {
                        (
                            call.completed,
                            call.sequence,
                            !sqlite.is_empty() && call.sql_failed,
                            call.failure.clone(),
                            call.response_position.clone(),
                        )
                    } else {
                        self.call_order.push_back(id.clone());
                        let sequence = self.next_sequence;
                        self.next_sequence = self.next_sequence.saturating_add(1);
                        (None, sequence, false, None, None)
                    };
                self.dirty_calls.insert(id.clone());
                self.retired_calls.remove(&id);
                self.calls.insert(
                    id,
                    Call {
                        tool,
                        time: when,
                        completed,
                        sequence,
                        observation,
                        nested,
                        commands,
                        jev,
                        skills,
                        sqlite,
                        sql_failed,
                        failure,
                        agent_messages,
                        files,
                        files_incomplete: !files_enabled,
                        mcp_sources_incomplete: sources_incomplete,
                        request_position: Some(super::details::Position::new(offset, raw)),
                        response_position,
                    },
                );
                if self.calls.len() > FILE_CALL_LIMIT {
                    if let Some(old) = self.call_order.pop_front() {
                        self.retire_call(&old);
                    }
                }
            }
            Some("function_call_output" | "custom_tool_call_output") => {
                let id = text(field(payload, "call_id"));
                if let Some((id, call)) =
                    id.and_then(|id| self.calls.get_mut(id).map(|call| (id, call)))
                {
                    call.complete(
                        field(payload, "output"),
                        when,
                        files_enabled,
                        mcp_sources,
                        mcp_switches,
                        errors_enabled,
                    );
                    call.response_position = Some(super::details::Position::new(offset, raw));
                    self.dirty_calls.insert(id.to_owned());
                } else {
                    // Detail lookup accepts an empty or absent call_id followed by id,
                    // while the collector only marks a response complete by call_id.
                    if id.is_none_or(str::is_empty) {
                        if let Some((id, call)) = text(field(payload, "id"))
                            .and_then(|id| self.calls.get_mut(id).map(|call| (id, call)))
                        {
                            call.response_position =
                                Some(super::details::Position::new(offset, raw));
                            self.dirty_calls.insert(id.to_owned());
                        }
                    }
                    if let Some(failure) = errors_enabled
                        .then(|| {
                            tool_failure(
                                &field(payload, "output")
                                    .map(json_value)
                                    .unwrap_or(Value::Null),
                            )
                        })
                        .flatten()
                    {
                        let error = SessionError {
                            time: when,
                            code: failure.code.clone(),
                            reason: String::new(),
                            severity: Severity::Error,
                            record_offset: Some(offset),
                            record_hash: Some(format!("{:x}", Sha256::digest(raw))),
                            failure: Some(failure),
                            call_id: error_identifier(id).map(str::to_owned),
                        };
                        self.pending_errors.push(error.clone());
                        self.errors.push(error);
                        self.errors
                            .sort_by_key(|error| std::cmp::Reverse(error.time));
                        self.errors.truncate(50);
                    }
                }
            }
            _ => {}
        }
    }

    #[cfg(test)]
    pub fn merge(&mut self, other: &Self) {
        self.merge_selected(other, None);
    }

    /// Merge one source in traversal order. Later selected calls replace earlier
    /// calls with the same ID before any window totals are computed. An empty
    /// selection still contributes this source's metadata.
    #[cfg(test)]
    pub(super) fn merge_selected(&mut self, other: &Self, selected: Option<&BTreeSet<String>>) {
        self.merge_metadata(other);
        self.calls.extend(other.calls.iter()
            .filter(|(id, _)| selected.is_none_or(|ids| ids.contains(*id)))
            .map(|(id, call)| (id.clone(), call.clone())));
    }

    /// Snapshot-only merge for the fixed publication windows. Retain the union
    /// of each window's incoming preview before cloning metadata. Keeping only
    /// the all-time winners would lose valid events at submillisecond cutoffs.
    pub(super) fn merge_snapshot(
        &mut self,
        other: &Self,
        selected: Option<&BTreeSet<String>>,
        cutoffs: &[Option<Time>; 4],
    ) {
        let mut retained = BTreeSet::new();
        for boundary in cutoffs {
            let mut preview = BTreeMap::new();
            for (ordinal, message) in self.agent_incoming.iter().chain(&other.agent_incoming).enumerate() {
                if boundary.is_none_or(|cutoff| message.time().is_some_and(|time| time >= cutoff)) {
                    retain_preview(&mut preview,
                        (std::cmp::Reverse(message.time().map(stamp)), ordinal), ordinal, 100);
                }
            }
            retained.extend(preview.into_values());
        }
        let existing = self.agent_incoming.len();
        self.agent_incoming = retained.into_iter().map(|ordinal| {
            if ordinal < existing { self.agent_incoming[ordinal].clone() }
            else { other.agent_incoming[ordinal - existing].clone() }
        }).collect();
        self.merge_metadata_fields(other);
        self.calls.extend(other.calls.iter()
            .filter(|(id, _)| selected.is_none_or(|ids| ids.contains(*id)))
            .map(|(id, call)| (id.clone(), call.clone())));
    }

    /// Apply cursor metadata exactly once per source, never once per payload
    /// page. Timestamp tie rules intentionally differ for context and status.
    #[cfg(test)]
    pub(super) fn merge_metadata(&mut self, other: &Self) {
        self.agent_incoming
            .extend(other.agent_incoming.iter().cloned());
        self.merge_metadata_fields(other);
    }

    fn merge_metadata_fields(&mut self, other: &Self) {
        for (key, value) in &other.execution {
            self.execution
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        self.created = [self.created, other.created].into_iter().flatten().min();
        if other.updated >= self.updated {
            self.updated = other.updated;
            self.kind = other.kind;
            self.trigger = other.trigger.clone();
        }
        if other.context_seen && other.context_at >= self.context_at {
            self.context_at = other.context_at;
            self.model = other.model.clone();
            self.effort = other.effort.clone();
            if other.context_at.is_some() {
                self.execution.extend(other.execution.clone());
            }
        }
        if other.tokens_at.is_some() && other.tokens_at >= self.tokens_at {
            self.tokens_at = other.tokens_at;
            self.tokens = other.tokens;
        }
        if other.status_at > self.status_at {
            self.status_at = other.status_at;
            self.status_cached = other.status_cached;
            self.status = other.status;
            self.task_start = other.task_start;
        }
        self.intervals
            .extend(other.intervals.iter().map(|(&start, &end)| (start, end)));
    }

    #[cfg(test)]
    pub fn file_events(&self, boundary: Option<Time>, files_enabled: bool) -> Vec<Value> {
        if !files_enabled {
            return Vec::new();
        }
        let mut result = Vec::new();
        for (id, call) in &self.calls {
            if !boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff)) {
                continue;
            }
            for file in &call.files {
                let mut event = file.clone();
                event["thread_id"] = json!(self.id);
                event["thread_name"] = Value::Null;
                event["call_id"] = json!(id);
                event["timestamp"] = json!(call.time.map(stamp));
                event["completed_at"] = json!(call.completed.map(stamp));
                if event["nested"] == true {
                    event["duration_ms"] = Value::Null;
                    event["container_duration_ms"] = json!(call.duration_ms());
                } else {
                    event["duration_ms"] = json!(call.duration_ms());
                }
                result.push(event);
            }
        }
        result.sort_by(|a, b| b["timestamp"].as_str().cmp(&a["timestamp"].as_str()));
        result
    }
    #[cfg(test)]
    pub fn skill_events(&self, boundary: Option<Time>) -> Vec<Value> {
        let mut events = BTreeMap::new();
        for (id, call) in &self.calls {
            if boundary.is_some_and(|cutoff| call.time.is_none_or(|time| time < cutoff)) {
                continue;
            }
            for skill in &call.skills {
                events.insert((id, skill), json!({"thread_id":self.id,"thread_name":null,"call_id":id,"skill":skill,"tool":call.tool,"has_document":true,"timestamp":call.time.map(stamp),"completed_at":call.completed.map(stamp),"duration_ms":call.duration_ms()}));
            }
        }
        events.into_values().collect()
    }

    /// Match the legacy stable timestamp sort while owning at most 100
    /// events. Explicit tie keys preserve incoming order and sorted call order
    /// without requiring a complete sorted call-reference array.
    /// Render metadata only on preview admission, never message contents.
    fn communication<'a>(
        &'a self,
        calls: impl IntoIterator<Item = (&'a String, &'a Call)>,
        boundary: Option<Time>,
    ) -> Vec<Value> {
        let mut summary = CommunicationSummary::default();
        let thread = self.id.as_deref().unwrap_or("");
        for (ordinal, message) in self.agent_incoming.iter().enumerate() {
            summary.incoming(thread, ordinal, message, boundary);
        }
        for (id, call) in calls {
            summary.outgoing(thread, id, call);
        }
        summary.finish()
    }

    pub fn row(
        &self,
        partial: bool,
        boundary: Option<Time>,
        reference: Time,
        totals: &mut BTreeMap<String, usize>,
        nested_totals: &mut BTreeMap<String, usize>,
        tool_events_enabled: bool,
        files_enabled: bool,
        jev_calls_enabled: bool,
    ) -> Value {
        let eligible = |call: &Call| boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff));
        let mut tools = RowTools::default();
        let mut jev = RowJev::default();
        for (id, call) in self.calls.iter().filter(|(_, call)| eligible(call)) {
            tools.observe(id, call, tool_events_enabled, totals, nested_totals);
            jev.observe(self.id.as_deref(), id, call, jev_calls_enabled);
        }
        let RowTools { count: call_count, counts, nested: nested_counts, preview } = tools;
        let events: Vec<_> = preview.into_values().collect();
        let mut changes = FileSummary::new(200);
        let mut reads = FileSummary::new(200);
        changes.observe(self, boundary, files_enabled, Some(false));
        reads.observe(self, boundary, files_enabled, Some(true));
        let file_changes = changes.finish().0;
        let file_reads = reads.finish().0;
        let durations = self.intervals.iter()
            .map(|(start, end)| (*end - *start).num_milliseconds().max(0));
        let running_duration = self.task_start.filter(|start| {
            self.status == Some(Status::Running) && !self.intervals.contains_key(start)
        }).map(|start| (reference - start).num_milliseconds().max(0));
        let (task_runs, task_duration_ms) = durations.chain(running_duration)
            .fold((0usize, 0i64), |(count, total), duration| (count + 1, total + duration));
        let tokens = self
            .tokens
            .map(|values| COUNTERS.into_iter().zip(values).collect::<BTreeMap<_, _>>());
        let communication = self.communication(self.calls.iter().filter(|(_, call)| eligible(call)), boundary);
        let jev_calls: Vec<_> = jev.preview.into_values().collect();
        json!({
            "thread_id":self.id, "created_at":self.created.map(stamp), "updated_at":self.updated.map(stamp),
            "model":self.model, "reasoning_effort":self.effort, "context_updated_at":self.context_at.map(stamp), "execution":self.execution,
            "environment":"local", "activity_type":self.kind.unwrap_or(ActivityKind::Unknown), "trigger":self.trigger.as_deref().unwrap_or("unknown"),
            "status":self.status.unwrap_or(Status::Observed), "status_updated_at":self.status_at.map(stamp),
            "status_source":self.status_at.map(|_| if self.status_cached {"cached_lifecycle"} else {"session_lifecycle"}),
            "tokens":tokens.map_or_else(|| json!({}), |values| json!(values)), "token_updated_at":self.tokens_at.map(stamp),
            "tool_calls":call_count, "tools":counts, "nested_tools":nested_counts, "tool_events":events, "jev_calls":jev_calls, "partial_history":partial,
            "task_runs": (task_runs > 0).then_some(task_runs),
            "task_duration_ms": (task_runs > 0).then_some(task_duration_ms),
            "agent_messages":communication,"file_changes":file_changes,"file_reads":file_reads,
        })
    }
}

#[cfg(test)]
mod snapshot_merge_tests {
    use super::*;

    #[test]
    fn snapshot_task_duration_matches_full_vector() {
        let reference = lam_history_store::parse_utc_stamp("2026-11-14T00:00:00Z").unwrap();
        for count in [0, 1, 100, 2400] {
            for status in [None, Some(Status::Observed), Some(Status::Running), Some(Status::Completed)] {
                for start_kind in 0..4 {
                    let mut projection = Projection::for_id(None);
                    projection.status = status;
                    for index in 0..count {
                        let start = reference - chrono::Duration::milliseconds(index * 10);
                        let end = start + chrono::Duration::milliseconds(if index % 2 == 0 { 7 } else { -3 });
                        projection.intervals.insert(start, end);
                    }
                    projection.task_start = match start_kind {
                        0 => None,
                        1 => Some(reference),
                        2 => Some(reference - chrono::Duration::milliseconds(99999)),
                        _ => Some(reference + chrono::Duration::milliseconds(5)),
                    };
                    let mut durations: Vec<_> = projection.intervals.iter()
                        .map(|(start, end)| (*end - *start).num_milliseconds().max(0)).collect();
                    if let Some(start) = projection.task_start.filter(|start| {
                        projection.status == Some(Status::Running) && !projection.intervals.contains_key(start)
                    }) {
                        durations.push((reference - start).num_milliseconds().max(0));
                    }
                    let row = projection.row(false, None, reference, &mut BTreeMap::new(),
                        &mut BTreeMap::new(), false, false, false);
                    assert_eq!(row["task_runs"], json!((!durations.is_empty()).then_some(durations.len())));
                    assert_eq!(row["task_duration_ms"], json!((!durations.is_empty()).then(|| durations.iter().sum::<i64>())));
                }
            }
        }
    }

    fn call(tool: &str, time: Option<&str>) -> Call {
        serde_json::from_value(json!({"tool":tool,"time":time,"completed":null,"sequence":1})).unwrap()
    }

    fn source(tag: &str, time: Option<Time>) -> Projection {
        let mut source = Projection::for_id(Some("00000000-0000-4000-8000-000000000001".into()));
        source.created = time;
        source.updated = time;
        source.context_at = time;
        source.context_seen = true;
        source.model = Some(tag.into());
        source.effort = Some(tag.into());
        source.trigger = Some(tag.into());
        source.execution.insert("origin".into(), json!(tag));
        source.tokens_at = time;
        source.tokens = Some([Some(tag.len() as i64); 6]);
        source.status_at = time;
        source.status = Some(Status::Running);
        source.task_start = time;
        if let Some(time) = time {
            source.intervals.insert(time, time + chrono::Duration::seconds(tag.len() as i64));
        }
        source.calls.insert("duplicate".into(), call(tag, Some("2026-11-13T01:00:00Z")));
        source.calls.insert(format!("unique-{tag}"), call(tag, None));
        source
    }

    fn assert_equivalent(left: &Projection, right: &Projection) {
        assert_eq!(serde_json::to_value(left).unwrap(), serde_json::to_value(right).unwrap());
        assert_eq!(serde_json::to_value(&left.calls).unwrap(), serde_json::to_value(&right.calls).unwrap());
        let reference = timestamp(Some("2026-11-13T03:00:00Z")).unwrap();
        for boundary in [None, Some(reference - chrono::Duration::hours(3)), Some(reference)] {
            for enabled in [false, true] {
                let (mut lt, mut ln, mut rt, mut rn) = Default::default();
                assert_eq!(
                    left.row(false, boundary, reference, &mut lt, &mut ln, enabled, enabled, enabled),
                    right.row(false, boundary, reference, &mut rt, &mut rn, enabled, enabled, enabled));
                assert_eq!(lt, rt);
                assert_eq!(ln, rn);
            }
        }
    }

    #[test]
    fn snapshot_skill_summary_matches_history_precedence_and_full_event_sort() {
        let time = timestamp(Some("2026-11-13T01:00:00Z")).unwrap();
        let mut threads = BTreeMap::new();
        let mut names = std::collections::HashMap::new();
        let mut history = Vec::new();
        for thread in ["a","b","c"] {
            names.insert(thread.to_owned(),json!(format!("Thread {thread}")));
            let mut projection = Projection::for_id(Some(thread.into()));
            for n in 0..600 {
                let id = format!("call-{n:04}");
                let mut value = call("fixture",None);
                value.time = match n%4 { 0=>None,1=>Some(time),2=>Some(time-chrono::Duration::hours(2)),_=>Some(time+chrono::Duration::hours(1)) };
                value.completed = value.time.map(|time|time+chrono::Duration::seconds(2));
                value.skills = vec!["shared".into(),format!("skill-{}",n%7),"shared".into()];
                projection.calls.insert(id.clone(),value);
                for marker in ["earlier","last"] {
                    history.push(json!({"thread_id":thread,"call_id":id,"skill":"shared",
                        "timestamp":stamp(time),"marker":marker}));
                    history.push(json!({"thread_id":thread,"call_id":id,"skill":"history-only",
                        "timestamp":stamp(time),"marker":marker}));
                }
            }
            threads.insert(thread,(projection,false));
        }
        names.insert("retired".into(),json!("Retired"));
        history.extend([
            json!({"thread_id":"retired","call_id":"old","skill":"old","timestamp":stamp(time)}),
            json!({"thread_id":"absent","call_id":"old","skill":"absent","timestamp":stamp(time)}),
            json!({"thread_id":"retired","call_id":"invalid","skill":"bad-time","timestamp":"not-a-time"}),
            json!({"thread_id":"retired","call_id":"missing","skill":"missing-time","timestamp":null}),
        ]);
        for boundary in [None,Some(time-chrono::Duration::hours(1)),Some(time),Some(time+chrono::Duration::hours(3))] {
            for enabled in [false,true] {
                let actual = skill_summary(&threads,&history,&names,boundary,enabled);
                assert_eq!(actual,legacy_skill_summary(&threads,&history,&names,boundary,enabled));
                let mut streamed = SkillSummary::new(&history, &names, boundary, enabled);
                for (lookup_thread, (projection, _)) in threads.iter().rev() {
                    for (id, call) in projection.calls.iter().rev() {
                        let temporary = call.clone();
                        streamed.observe(lookup_thread, projection.id.as_deref(), id, &temporary);
                        assert!(streamed.preview.len() <= 500);
                    }
                }
                assert_eq!(streamed.finish(&names), actual);
                assert!(actual.0.len()<=500);
                if enabled && boundary.is_none() { assert!(actual.1>500); }
            }
        }
        // Verify precedence without truncation: a null-time current record wins
        // all-time, but eligible history survives when current is out of window.
        let mut current = Projection::for_id(Some("a".into()));
        let mut value = call("current",None);
        value.skills=vec!["same".into(),"same".into()];
        current.calls.insert("one".into(),value);
        let small = BTreeMap::from([("a",(current,false))]);
        let old = vec![
            json!({"thread_id":"a","call_id":"one","skill":"same","timestamp":stamp(time),"marker":"earlier"}),
            json!({"thread_id":"a","call_id":"one","skill":"same","timestamp":stamp(time),"marker":"last"}),
        ];
        let all = skill_summary(&small,&old,&names,None,true);
        assert_eq!(all.1,1);
        assert_eq!(all.0[0]["tool"],"current");
        assert!(all.0[0]["timestamp"].is_null());
        let window = skill_summary(&small,&old,&names,Some(time),true);
        assert_eq!(window.1,1);
        assert_eq!(window.0[0]["marker"],"last");
    }

    fn legacy_skill_summary(
        threads: &BTreeMap<&str,(Projection,bool)>, history: &[Value],
        thread_names: &std::collections::HashMap<String,Value>, boundary: Option<Time>, enabled: bool,
    ) -> (Vec<Value>,usize,BTreeMap<String,usize>) {
            let current_skills: Vec<_> = threads
                .values()
                .filter(|_| enabled)
                .flat_map(|(thread, _)| thread.skill_events(boundary))
                .collect();
            let mut merged_skills = BTreeMap::new();
            if enabled {
                for event in history {
                    let (Some(thread), Some(call), Some(skill), Some(timestamp)) = (
                        event["thread_id"].as_str(),
                        event["call_id"].as_str(),
                        event["skill"].as_str(),
                        event["timestamp"].as_str(),
                    ) else {
                        continue;
                    };
                    if !thread_names.contains_key(thread)
                        || boundary.is_some_and(|cutoff| {
                            lam_history_store::parse_utc_stamp(timestamp)
                                .is_none_or(|time| time < cutoff)
                        })
                    {
                        continue;
                    }
                    merged_skills.insert(
                        (thread.to_owned(), call.to_owned(), skill.to_owned()),
                        event.clone(),
                    );
                }
                for event in current_skills {
                    if let (Some(thread), Some(call), Some(skill)) = (
                        event["thread_id"].as_str(),
                        event["call_id"].as_str(),
                        event["skill"].as_str(),
                    ) {
                        merged_skills.insert(
                            (thread.to_owned(), call.to_owned(), skill.to_owned()),
                            event.clone(),
                        );
                    }
                }
            }
            let mut skills: Vec<_> = merged_skills.into_values().collect();
            for event in &mut skills {
                if let Some(name) = event["thread_id"]
                    .as_str()
                    .and_then(|id| thread_names.get(id))
                {
                    event["thread_name"] = name.clone();
                }
            }
            skills.sort_by(|a, b| b["timestamp"].as_str().cmp(&a["timestamp"].as_str()));
            let mut skill_counts = BTreeMap::<String, usize>::new();
            for event in &skills {
                if let Some(skill) = event["skill"].as_str() {
                    *skill_counts.entry(skill.to_owned()).or_default() += 1;
                }
            }
            let skill_total = skills.len();
            skills.truncate(500);
        (skills,skill_total,skill_counts)
    }

    #[test]
    fn snapshot_call_previews_match_sorted_winners_without_full_reference_array() {
        let time = timestamp(Some("2026-11-13T01:00:00Z")).unwrap();
        let mut projection = Projection::for_id(Some("fixture".into()));
        for n in 0..1200 {
            let mut value = call(&format!("tool-{}", n % 3), None);
            value.time = match n % 5 {
                0 => None,
                1 => Some(time),
                2 => Some(time - chrono::Duration::hours(2)),
                3 => Some(time + chrono::Duration::hours(1)),
                _ => Some(time + chrono::Duration::nanoseconds(999_999)),
            };
            value.completed = value.time.map(|time| time + chrono::Duration::seconds(3));
            value.nested = Some(NestedActivity {
                tools:[("nested-a".into(), n % 5),("nested-b".into(),2)].into(),
                events:Vec::new(),isolated:false,truncated:n % 2 == 0,
                tool_counts_incomplete:false,single_invocation:false,
            });
            value.jev = (0..n % 4).map(|index| JevOperation {
                operation:match index { 0 => JevKind::Rank, 1 => JevKind::Evaluate, _ => JevKind::Status },
                nested:index != 0,
            }).collect();
            projection.calls.insert(format!("call-{n:04}"), value);
        }
        // Later source replacement changes both counts and previews, not an
        // additional observation of the superseded call.
        let mut replacement = Projection::for_id(projection.id.clone());
        replacement.calls.insert("call-0001".into(), call("winner", None));
        projection.merge(&replacement);
        for boundary in [None, Some(time - chrono::Duration::hours(1)), Some(time), Some(time + chrono::Duration::hours(3))] {
            let mut calls: Vec<_> = projection.calls.iter().filter(|(_, call)|
                boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff))).collect();
            calls.sort_by_key(|(_, call)| std::cmp::Reverse(call.time));
            for tools_enabled in [false, true] {
                for jev_enabled in [false, true] {
                    let mut expected_counts = BTreeMap::<String,usize>::new();
                    let mut expected_nested = BTreeMap::<String,usize>::new();
                    for (_, call) in &calls {
                        *expected_counts.entry(call.tool.clone()).or_default() += 1;
                        if let Some(nested) = call.nested.as_ref().filter(|_| tools_enabled) {
                            for (tool, count) in &nested.tools { *expected_nested.entry(tool.clone()).or_default() += count; }
                        }
                    }
                    let expected_tools: Vec<_> = calls.iter().filter(|_| tools_enabled).take(100).map(|(id, call)| {
                        let mut event = json!({"call_id":id,"tool":call.tool,"timestamp":call.time.map(stamp),
                            "completed_at":call.completed.map(stamp),"duration_ms":call.duration_ms(),
                            "nested_tools":call.nested.as_ref().map_or_else(||json!({}),|nested|json!(nested.tools))});
                        if call.nested.as_ref().is_some_and(|nested|nested.truncated) { event["nested_truncated"]=true.into(); }
                        event
                    }).collect();
                    let mut streamed = RowTools::default();
                    let (mut streamed_totals, mut streamed_nested) = Default::default();
                    for (id, call) in calls.iter().rev() {
                        let temporary = (*call).clone();
                        streamed.observe(id, &temporary, tools_enabled, &mut streamed_totals, &mut streamed_nested);
                        assert!(streamed.preview.len() <= 100);
                    }
                    assert_eq!(streamed.count, calls.len());
                    assert_eq!(streamed.counts, expected_counts);
                    assert_eq!(streamed.nested, expected_nested);
                    assert_eq!(streamed_totals, expected_counts);
                    assert_eq!(streamed_nested, expected_nested);
                    assert_eq!(streamed.preview.into_values().collect::<Vec<_>>(), expected_tools);
                    let thread_id = projection.id.as_deref();
                    let expected_jev: Vec<_> = calls.iter().filter(|_|jev_enabled).flat_map(|(id,call)|
                        call.jev.iter().enumerate().map(move |(index,operation)|json!({
                            "thread_id":thread_id,"call_id":id,"index":index,"operation":operation.operation,
                            "nested":operation.nested,"timestamp":call.time.map(stamp),"completed_at":call.completed.map(stamp)
                        }))).take(100).collect();
                    let mut streamed_jev = RowJev::default();
                    for (id, call) in calls.iter().rev() {
                        let temporary = (*call).clone();
                        streamed_jev.observe(thread_id, id, &temporary, jev_enabled);
                        assert!(streamed_jev.preview.len() <= 100);
                    }
                    assert_eq!(streamed_jev.preview.into_values().collect::<Vec<_>>(), expected_jev);

                    let (mut totals, mut nested_totals) = (BTreeMap::from([("seed".into(),7)]), BTreeMap::from([("seed".into(),11)]));
                    let row = projection.row(false,boundary,time,&mut totals,&mut nested_totals,tools_enabled,false,jev_enabled);
                    assert_eq!(row["tool_calls"],calls.len());
                    assert_eq!(row["tools"],json!(expected_counts));
                    assert_eq!(row["nested_tools"],json!(expected_nested));
                    assert_eq!(row["tool_events"],json!(expected_tools));
                    assert_eq!(row["jev_calls"],json!(expected_jev));
                    expected_counts.insert("seed".into(),7);
                    expected_nested.insert("seed".into(),11);
                    assert_eq!(totals,expected_counts);
                    assert_eq!(nested_totals,expected_nested);
                }
            }
        }
        let mut preview = BTreeMap::new();
        for rank in (0..10_000).rev() {
            retain_preview(&mut preview,rank,rank,100);
            assert!(preview.len()<=100);
        }
        assert_eq!(preview.into_keys().collect::<Vec<_>>(),(0..100).collect::<Vec<_>>());
    }

    #[test]
    fn snapshot_communication_matches_full_sort_and_preserves_metadata_only_previews() {
        let time = timestamp(Some("2026-11-13T01:00:00Z")).unwrap();
        let mut projection = Projection::for_id(Some("fixture-thread".into()));
        for n in 0..150 {
            let when = match n % 3 { 0 => None, 1 => Some(time), _ => Some(time + chrono::Duration::hours(1)) };
            projection.agent_incoming.push(super::super::agent::Incoming::parse(
                &format!("Message Type: MESSAGE\nTask name: task-{n}\nSender: sender-{n}\nPayload:\nPRIVATE_BODY_MUST_NOT_RENDER"), when).unwrap());
        }
        for n in 0..600 {
            let when = match n % 5 {
                0 => None,
                1 => Some(time),
                2 => Some(time + chrono::Duration::hours(1)),
                3 => Some(time - chrono::Duration::hours(2)),
                _ => Some(time + chrono::Duration::nanoseconds(999_999)),
            };
            let mut value = call("exec", None);
            value.time = when;
            value.completed = when.map(|time| time + chrono::Duration::seconds(2));
            value.agent_messages = (0..3).map(|index| {
                super::super::agent::Outgoing::new("collaboration.send_message",
                    json!({"target":format!("target-{n}-{index}"),"message":"PRIVATE_BODY_MUST_NOT_RENDER"}).as_object(),
                    Some("sender"), true, index, index).unwrap()
            }).collect();
            projection.calls.insert(format!("call-{n:04}"), value);
        }
        for boundary in [None, Some(time - chrono::Duration::hours(1)), Some(time), Some(time + chrono::Duration::hours(3))] {
            let mut calls: Vec<_> = projection.calls.iter()
                .filter(|(_, call)| boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff))).collect();
            calls.sort_by_key(|(_, call)| std::cmp::Reverse(call.time));
            let thread = projection.id.as_deref().unwrap();
            let mut legacy: Vec<_> = projection.agent_incoming.iter()
                .filter(|message| boundary.is_none_or(|cutoff| message.time().is_some_and(|time| time >= cutoff)))
                .map(|message| message.render(thread)).collect();
            for (id, call) in &calls {
                legacy.extend(call.agent_messages.iter().map(|message| message.render(thread, id, call.time, call.completed, call.duration_ms())));
            }
            legacy.sort_by(|left, right| right["timestamp"].as_str().cmp(&left["timestamp"].as_str()));
            legacy.truncate(100);
            let actual = projection.communication(calls.iter().copied(), boundary);
            assert_eq!(projection.communication(calls.iter().rev().copied(), boundary), actual);
            assert!(actual.len() <= 100);
            assert_eq!(actual, legacy);
            let mut streamed = CommunicationSummary::default();
            for (ordinal, message) in projection.agent_incoming.iter().enumerate() {
                let temporary = message.clone();
                streamed.incoming(thread, ordinal, &temporary, boundary);
                assert!(streamed.preview.len() <= 100);
            }
            for (id, call) in calls.iter().rev() {
                let temporary = (*call).clone();
                streamed.outgoing(thread, id, &temporary);
                assert!(streamed.preview.len() <= 100);
            }
            assert_eq!(streamed.finish(), legacy);

            assert!(!serde_json::to_string(&actual).unwrap().contains("PRIVATE_BODY_MUST_NOT_RENDER"));
            for tools_enabled in [false, true] {
                let row = projection.row(false, boundary, time, &mut BTreeMap::new(), &mut BTreeMap::new(), tools_enabled, false, false);
                assert_eq!(row["agent_messages"], json!(legacy));
            }
        }
        // Unknown times remain all-time only, and an incoming tie precedes an
        // outgoing tie even when later calls were visited in a different order.
        projection.agent_incoming.truncate(1);
        projection.calls.retain(|id, _| id == "call-0000");
        let calls: Vec<_> = projection.calls.iter().collect();
        let unknown = projection.communication(calls.iter().copied(), None);
        assert_eq!(unknown.len(), 4);
        assert_eq!(unknown[0]["direction"], "incoming");
        assert!(projection.communication(std::iter::empty(), Some(time)).is_empty());
    }

    #[test]
    fn snapshot_file_summary_matches_full_sort_with_bounded_previews() {
        let time = timestamp(Some("2026-11-13T01:00:00Z")).unwrap();
        let mut threads = BTreeMap::new();
        for thread in ["a", "b", "c"] {
            let mut projection = Projection::for_id(Some(thread.into()));
            for n in 0..600 {
                let when = match n % 4 {
                    0 => None,
                    1 => Some(time),
                    2 => Some(time - chrono::Duration::hours(2)),
                    _ => Some(time + chrono::Duration::hours(1)),
                };
                let mut value = call("fixture", None);
                value.time = when;
                value.completed = when.map(|time| time + chrono::Duration::seconds(2));
                value.files = (0..3).map(|i| json!({
                    "operation":if i == 0 {"read"} else {"write"},
                    "path":format!("fixture/{thread}/{n}/{i}"),
                    "tool":"fixture","nested": i == 2
                })).collect();
                projection.calls.insert(format!("call-{n:04}"), value);
            }
            // This replacement must be applied before reducing counts. Keeping
            // both source versions would add extra writes and alter tie previews.
            let mut replacement = Projection::for_id(Some(thread.into()));
            let mut winner = call("winner", Some("2026-11-13T01:00:00Z"));
            winner.files = vec![json!({"operation":"read","path":"winner","tool":"winner","nested":false})];
            replacement.calls.insert("call-0001".into(), winner);
            projection.merge(&replacement);
            threads.insert(thread, projection);
        }
        for boundary in [None, Some(time - chrono::Duration::hours(1)), Some(time), Some(time + chrono::Duration::hours(3))] {
            for enabled in [false, true] {
                for limit in [0, 1, 200, 1000] {
                    let mut reducer = FileSummary::new(limit);
                    let mut legacy = Vec::new();
                    for projection in threads.values() {
                        {
                            let temporary = projection.clone();
                            reducer.observe(&temporary, boundary, enabled, None);
                        }
                        assert!(reducer.preview.len() <= limit);
                        legacy.extend(projection.file_events(boundary, enabled));
                    }
                    legacy.sort_by(|a, b| b["timestamp"].as_str().cmp(&a["timestamp"].as_str()));
                    let total = legacy.len();
                    let mut operations = BTreeMap::new();
                    for event in &legacy {
                        *operations.entry(event["operation"].as_str().unwrap().to_owned()).or_insert(0usize) += 1;
                    }
                    legacy.truncate(limit);
                    let actual = reducer.finish();
                    assert_eq!(actual, (legacy, total, operations));
                }
                for projection in threads.values() {
                    let row = projection.row(false, boundary, time, &mut BTreeMap::new(), &mut BTreeMap::new(), true, enabled, true);
                    let legacy = projection.file_events(boundary, enabled);
                    let reads: Vec<_> = legacy.iter().filter(|event| event["operation"] == "read").take(200).cloned().collect();
                    let changes: Vec<_> = legacy.iter().filter(|event| event["operation"] != "read").take(200).cloned().collect();
                    assert_eq!(row["file_reads"], json!(reads));
                    assert_eq!(row["file_changes"], json!(changes));
                }
            }
        }
    }

    #[test]
    fn snapshot_merge_selected_matches_legacy_clone_filter_for_source_order_and_windows() {
        let time = timestamp(Some("2026-11-13T01:00:00Z"));
        for reverse in [false, true] {
            let mut sources = [source("first", time), source("second", time), source("unknown", None)];
            if reverse { sources.reverse(); }
            let mut actual = Projection::for_id(sources[0].id.clone());
            let mut expected = Projection::for_id(sources[0].id.clone());
            for (n, source) in sources.iter().enumerate() {
                // Include an empty selection: metadata still participates.
                let selected: BTreeSet<_> = if n == 1 { BTreeSet::new() } else { source.calls.keys().cloned().collect() };
                let mut filtered = source.clone();
                filtered.calls.retain(|id, _| selected.contains(id));
                legacy_merge(&mut expected, &filtered);
                actual.merge_selected(source, Some(&selected));
                assert_equivalent(&actual, &expected);
            }
            assert_eq!(actual.calls["duplicate"].tool_name(), sources[2].calls["duplicate"].tool_name());
            assert_eq!(actual.calls.len(), 3, "winner replacement must precede counts");
            let mut merged_all = Projection::for_id(sources[0].id.clone());
            let mut legacy_all = Projection::for_id(sources[0].id.clone());
            for source in &sources { merged_all.merge(source); legacy_merge(&mut legacy_all, source); }
            assert_equivalent(&merged_all, &legacy_all);
        }
    }

    #[test]
    fn snapshot_incoming_merge_matches_full_metadata_for_fixed_windows() {
        let reference = timestamp(Some("2026-11-13T01:00:00Z")).unwrap()
            + chrono::Duration::nanoseconds(500_000);
        let cutoffs = super::super::HOURS.map(|hours| hours.map(|hours| reference - chrono::Duration::hours(hours)));
        for reverse in [false, true] {
            let mut sources = Vec::new();
            for n in 0..20 {
                let mut item = source(&format!("source-{n}"), Some(reference));
                item.agent_incoming.clear();
                for index in 0..100 {
                    let time = match n % 5 {
                        0 => None,
                        1 => Some(reference - chrono::Duration::hours(1) - chrono::Duration::nanoseconds(1)),
                        2 => Some(reference - chrono::Duration::hours(1)),
                        3 => Some(reference - chrono::Duration::hours(24)),
                        _ => Some(reference - chrono::Duration::hours(168)),
                    };
                    item.agent_incoming.push(super::super::agent::Incoming::parse(
                        &format!("Message Type: MESSAGE\nTask name: task-{n}-{index}\nSender: sender-{n}\nPayload:\nPRIVATE_BODY_MUST_NOT_RENDER"), time).unwrap());
                }
                sources.push(item);
            }
            if reverse { sources.reverse(); }
            let mut actual = Projection::for_id(Some("fixture-thread".into()));
            let mut expected = Projection::for_id(actual.id.clone());
            for (index, item) in sources.iter().enumerate() {
                let selected: BTreeSet<_> = if index % 3 == 0 { BTreeSet::new() }
                    else { item.calls.keys().cloned().collect() };
                actual.merge_snapshot(item, Some(&selected), &cutoffs);
                let mut filtered = item.clone();
                filtered.calls.retain(|id, _| selected.contains(id));
                legacy_merge(&mut expected, &filtered);
                assert!(actual.agent_incoming.len() <= 400);
                for boundary in cutoffs {
                    for tools in [false, true] {
                        let row = |projection: &Projection| projection.row(false, boundary, reference,
                            &mut BTreeMap::new(), &mut BTreeMap::new(), tools, false, false);
                        assert_eq!(row(&actual), row(&expected));
                    }
                }
            }
            assert_eq!(expected.agent_incoming.len(), 2000);
            assert_eq!(actual.communication(std::iter::empty(), cutoffs[0]).len(), 100);
            assert!(!serde_json::to_string(&actual.communication(std::iter::empty(), None)).unwrap()
                .contains("PRIVATE_BODY_MUST_NOT_RENDER"));
        }
    }

    #[test]
    fn snapshot_metadata_merge_retains_timestamp_ties_and_interval_start_dedup() {
        let time = timestamp(Some("2026-11-13T01:00:00Z"));
        let first = source("first", time);
        let mut second = source("second", time);
        second.status = Some(Status::Observed);
        second.status_cached = true;
        let mut merged = Projection::for_id(first.id.clone());
        merged.merge_metadata(&first);
        merged.merge_metadata(&second);
        assert!(merged.calls.is_empty());
        assert_eq!(merged.model.as_deref(), Some("second"), "context uses >= on equal timestamps");
        assert_eq!(merged.trigger.as_deref(), Some("second"), "updated uses >=");
        assert!(merged.status == first.status, "status uses >, not >=");
        assert!(!merged.status_cached);
        assert_eq!(merged.intervals.len(), 1);
        assert_eq!(merged.intervals[&time.unwrap()], time.unwrap() + chrono::Duration::seconds(6));
        assert_eq!(merged.tokens, second.tokens);
        let mut expected = Projection::for_id(first.id.clone());
        let mut first = first;
        let mut second = second;
        first.calls.clear(); second.calls.clear();
        legacy_merge(&mut expected, &first); legacy_merge(&mut expected, &second);
        assert_equivalent(&merged, &expected);
    }
    fn legacy_merge(target: &mut Projection, other: &Projection) {
        target.agent_incoming
            .extend(other.agent_incoming.iter().cloned());
        for (key, value) in &other.execution {
            target.execution
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        target.created = [target.created, other.created].into_iter().flatten().min();
        if other.updated >= target.updated {
            target.updated = other.updated;
            target.kind = other.kind;
            target.trigger = other.trigger.clone();
        }
        if other.context_seen && other.context_at >= target.context_at {
            target.context_at = other.context_at;
            target.model = other.model.clone();
            target.effort = other.effort.clone();
            if other.context_at.is_some() {
                target.execution.extend(other.execution.clone());
            }
        }
        if other.tokens_at.is_some() && other.tokens_at >= target.tokens_at {
            target.tokens_at = other.tokens_at;
            target.tokens = other.tokens;
        }
        if other.status_at > target.status_at {
            target.status_at = other.status_at;
            target.status_cached = other.status_cached;
            target.status = other.status;
            target.task_start = other.task_start;
        }
        target.intervals
            .extend(other.intervals.iter().map(|(&start, &end)| (start, end)));
        target.calls.extend(
            other
                .calls
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }

}
