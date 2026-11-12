use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::project::{stamp, Call, Projection};

pub const GIT_EVENT_LIMIT: usize = 500;
pub const CHECK_EVENT_LIMIT: usize = 500;

pub struct Calls<'a>(Vec<(&'a str, &'a str, &'a Call)>);

impl<'a> Calls<'a> {
    pub fn new(threads: &'a BTreeMap<&str, (Projection, bool)>) -> Self {
        let mut calls: Vec<_> = threads
            .iter()
            .flat_map(|(&thread, (projection, _))| {
                projection.calls.iter().filter_map(move |(id, call)| {
                    (!call.commands.is_empty()).then_some((thread, id.as_str(), call))
                })
            })
            .collect();
        calls.sort_by_key(|(_, _, call)| Reverse(call.time));
        Self(calls)
    }

    pub fn window(&self, boundary: Option<DateTime<Utc>>) -> (Value, Vec<Value>) {
        let mut git = Vec::new();
        let mut checks = Vec::new();
        let mut operations = BTreeMap::<&str, usize>::new();
        let mut total = 0;
        for &(thread, id, call) in &self.0 {
            if boundary.is_some_and(|cutoff| call.time.is_none_or(|time| time < cutoff)) {
                continue;
            }
            total += call.commands.git.len();
            for event in &call.commands.git {
                *operations.entry(&event.operation).or_default() += 1;
            }
            let available_git = GIT_EVENT_LIMIT - git.len();
            let available_checks = CHECK_EVENT_LIMIT - checks.len();
            if available_git == 0 && available_checks == 0 {
                continue;
            }
            let timestamp = call.time.map(stamp);
            let completed = call.completed_at().map(stamp);
            let duration = call.duration_ms();
            for (index, event) in call.commands.git.iter().take(available_git).enumerate() {
                git.push(json!({
                    "thread_id":thread, "thread_name":null, "call_id":id, "index":index,
                    "timestamp":timestamp, "completed_at":completed, "duration_ms":duration,
                    "operation":event.operation, "repository":event.repository,
                }));
            }
            for event in call.commands.checks.iter().take(available_checks) {
                checks.push(json!({
                    "thread_id":thread, "thread_name":null, "call_id":id,
                    "timestamp":timestamp, "completed_at":completed, "duration_ms":duration,
                    "operation":event.operation, "category":event.category,
                }));
            }
        }
        (
            json!({"events":git,"operations":operations,"total":total}),
            checks,
        )
    }
}
