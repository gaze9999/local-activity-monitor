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
    let id = stem.get(stem.len().checked_sub(36)?..)?;
    uuid(id).then(|| id.to_owned())
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
    #[serde(default, skip_serializing_if = "SqlMetadata::is_empty")]
    sqlite: SqlMetadata,
    #[serde(default)]
    sql_failed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    failure: Option<ToolFailure>,
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
}

impl NestedActivity {
    fn from_code(source: &str, commands: &mut CommandMetadata, sqlite: &mut SqlMetadata) -> Self {
        let nested = nested_invocations(source);
        let mut tools = BTreeMap::new();
        let mut events = Vec::new();
        for invocation in &nested.calls {
            let args = invocation.literal.as_ref().and_then(Value::as_object);
            commands.observe(invocation.tool, args);
            sqlite.observe(invocation.tool, args, true);
            if let Some(tool) = name(Some(invocation.tool)) {
                if events.len() < OBSERVATION_LIMIT {
                    if let Some(event) = Observed::new(
                        &tool,
                        invocation.literal.as_ref().and_then(Value::as_object),
                    ) {
                        events.push(NestedEvent {
                            tool: tool.clone(),
                            event,
                        });
                    }
                }
                *tools.entry(tool).or_default() += 1;
            }
        }
        let isolated = !nested.truncated && nested.calls.len() == 1 && events.len() == 1;
        Self {
            tools,
            events,
            isolated,
            truncated: nested.truncated,
        }
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
            + usize::from(self.failure.is_some())
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

    fn complete(&mut self, output: Option<&JsonValue<'_>>, when: Option<Time>) {
        self.completed = when;
        let sqlite = !self.sqlite.is_empty();
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
            self.failure = tool_failure(&output);
            if sqlite {
                self.sql_failed = lam_history_store::mcp::output_failed(&output);
            }
            if let Some(observed) = observed {
                observed.complete(&output);
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
    created: Option<Time>,
    updated: Option<Time>,
    model: Option<String>,
    effort: Option<String>,
    context_at: Option<Time>,
    context_seen: bool,
    #[serde(default)]
    execution: BTreeMap<String, Value>,
    kind: Option<ActivityKind>,
    trigger: Option<String>,
    tokens: Option<[Option<i64>; 6]>,
    tokens_at: Option<Time>,
    token_priority: u8,
    status: Option<Status>,
    status_at: Option<Time>,
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
}

impl Projection {
    pub fn for_id(id: Option<String>) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub fn updated(&self) -> Option<Time> {
        self.updated
    }

    pub fn prune_order(&mut self) {
        self.call_order.retain(|id| self.calls.contains_key(id));
    }

    pub fn restore_calls(&mut self, calls: BTreeMap<String, Call>) -> bool {
        if self.id.as_deref().is_some_and(|id| !uuid(id))
            || !super::metadata::valid_checkpoint(&self.execution)
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
                    || call
                        .failure
                        .as_ref()
                        .is_some_and(|failure| !failure.valid())
                    || call.sequence >= self.next_sequence
                    || !call.commands.valid()
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

    pub fn consume(&mut self, raw: &[u8], offset: u64) -> bool {
        let line = raw.strip_prefix(b"\xef\xbb\xbf").unwrap_or(raw);
        let Ok(record) = JsonValue::parse(line, false) else {
            return false;
        };
        let Some(payload @ JsonValue::Object(_)) = field(&record, "payload") else {
            return true;
        };
        let when = timestamp(text(field(&record, "timestamp")));
        self.updated = self.updated.max(when);
        let event = text(field(payload, "type"));
        match text(field(&record, "type")) {
            Some("session_meta") => {
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
                    if let Some(info) = field(payload, "info") {
                        self.usage(field(info, "total_token_usage"), when, 1);
                        if let Some(JsonValue::Int(value)) = field(info, "model_context_window") {
                            if *value > 0 {
                                self.execution.insert("context_window".into(), json!(value));
                            }
                        }
                    }
                }
                Some(reason) => self.session_error(payload, reason, when, raw, offset),
                None => {}
            },
            Some("token_usage_record") => self.usage(field(payload, "thread_token_usage"), when, 2),
            Some("response_item") => self.tool_event(payload, event, when, raw, offset),
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
                let args = if shell || observed || tool.to_ascii_lowercase().contains("sqlite") {
                    match arguments {
                        Some(JsonValue::Str(value)) => serde_json::from_str::<Value>(value).ok(),
                        Some(value @ JsonValue::Object(_)) => Some(json_value(value)),
                        _ => None,
                    }
                } else {
                    None
                };
                let args = args.as_ref().and_then(Value::as_object);
                let observation = observed.then(|| Observed::new(&tool, args)).flatten();
                let mut commands = CommandMetadata::default();
                commands.observe(&tool, args);
                let mut sqlite = SqlMetadata::default();
                sqlite.observe(&tool, args, false);
                let nested = if matches!(tool.as_str(), "exec" | "functions.exec") {
                    text(arguments)
                        .map(|source| NestedActivity::from_code(source, &mut commands, &mut sqlite))
                } else {
                    None
                };
                let (completed, sequence, sql_failed, failure) =
                    if let Some(call) = self.calls.get(&id) {
                        (
                            call.completed,
                            call.sequence,
                            !sqlite.is_empty() && call.sql_failed,
                            call.failure.clone(),
                        )
                    } else {
                        self.call_order.push_back(id.clone());
                        let sequence = self.next_sequence;
                        self.next_sequence = self.next_sequence.saturating_add(1);
                        (None, sequence, false, None)
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
                        sqlite,
                        sql_failed,
                        failure,
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
                    call.complete(field(payload, "output"), when);
                    self.dirty_calls.insert(id.to_owned());
                } else if let Some(failure) = tool_failure(
                    &field(payload, "output")
                        .map(json_value)
                        .unwrap_or(Value::Null),
                ) {
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
            _ => {}
        }
    }

    pub fn merge(&mut self, other: &Self) {
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
            self.status = other.status;
            self.task_start = other.task_start;
        }
        self.intervals
            .extend(other.intervals.iter().map(|(&start, &end)| (start, end)));
        self.calls.extend(
            other
                .calls
                .iter()
                .map(|(key, value)| (key.clone(), value.clone())),
        );
    }

    pub fn row(
        &self,
        partial: bool,
        boundary: Option<Time>,
        reference: Time,
        totals: &mut BTreeMap<String, usize>,
        nested_totals: &mut BTreeMap<String, usize>,
    ) -> Value {
        let mut calls = self
            .calls
            .iter()
            .filter(|(_, call)| {
                boundary.is_none_or(|cutoff| call.time.is_some_and(|time| time >= cutoff))
            })
            .collect::<Vec<_>>();
        calls.sort_by_key(|(_, call)| std::cmp::Reverse(call.time));
        let mut counts = BTreeMap::<String, usize>::new();
        let mut nested_counts = BTreeMap::<String, usize>::new();
        for (_, call) in &calls {
            *counts.entry(call.tool.clone()).or_default() += 1;
            *totals.entry(call.tool.clone()).or_default() += 1;
            if let Some(nested) = &call.nested {
                for (tool, count) in &nested.tools {
                    *nested_counts.entry(tool.clone()).or_default() += count;
                    *nested_totals.entry(tool.clone()).or_default() += count;
                }
            }
        }
        let events: Vec<_> = calls.iter().take(100).map(|(id, call)| {
            let duration = call.duration_ms();
            let nested_tools = call.nested.as_ref().map_or_else(|| json!({}), |nested| json!(nested.tools));
            let mut event = json!({"call_id":id,"tool":call.tool,"timestamp":call.time.map(stamp),
                   "completed_at":call.completed.map(stamp),"duration_ms":duration,"nested_tools":nested_tools});
            if call.nested.as_ref().is_some_and(|nested| nested.truncated) {
                event["nested_truncated"] = true.into();
            }
            event
        }).collect();
        let mut durations: Vec<_> = self
            .intervals
            .iter()
            .map(|(start, end)| (*end - *start).num_milliseconds().max(0))
            .collect();
        if let Some(start) = self.task_start.filter(|start| {
            self.status == Some(Status::Running) && !self.intervals.contains_key(start)
        }) {
            durations.push((reference - start).num_milliseconds().max(0));
        }
        let tokens = self
            .tokens
            .map(|values| COUNTERS.into_iter().zip(values).collect::<BTreeMap<_, _>>());
        json!({
            "thread_id":self.id, "created_at":self.created.map(stamp), "updated_at":self.updated.map(stamp),
            "model":self.model, "reasoning_effort":self.effort, "context_updated_at":self.context_at.map(stamp), "execution":self.execution,
            "environment":"local", "activity_type":self.kind.unwrap_or(ActivityKind::Unknown), "trigger":self.trigger.as_deref().unwrap_or("unknown"),
            "status":self.status.unwrap_or(Status::Observed), "status_updated_at":self.status_at.map(stamp),
            "status_source":self.status_at.map(|_| "session_lifecycle"),
            "tokens":tokens.map_or_else(|| json!({}), |values| json!(values)), "token_updated_at":self.tokens_at.map(stamp),
            "tool_calls":calls.len(), "tools":counts, "nested_tools":nested_counts, "tool_events":events, "partial_history":partial,
            "task_runs": (!durations.is_empty()).then_some(durations.len()),
            "task_duration_ms": (!durations.is_empty()).then(|| durations.iter().sum::<i64>()),
        })
    }
}
