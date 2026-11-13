use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::BTreeMap;

use super::project::{stamp, Projection};

pub const GIT_EVENT_LIMIT: usize = 500;
pub const CHECK_EVENT_LIMIT: usize = 500;

pub struct Calls<'a>(&'a BTreeMap<&'a str, (Projection, bool)>);

fn retain_preview<K: Ord, V>(preview: &mut BTreeMap<K, V>, rank: K, value: V, limit: usize) {
    if preview.len() == limit && preview.last_key_value().is_some_and(|(last, _)| &rank >= last) { return; }
    if preview.len() == limit { preview.pop_last(); }
    preview.insert(rank, value);
}

impl<'a> Calls<'a> {
    pub fn new(threads: &'a BTreeMap<&str, (Projection, bool)>) -> Self {
        Self(threads)
    }

    /// Read already resolved call winners without building a full sorted
    /// reference array. Stable thread/call/index ties reproduce the legacy
    /// traversal, and only selected borrowed previews become JSON.
    pub fn window(&self, boundary: Option<DateTime<Utc>>) -> (Value, Vec<Value>) {
        let mut git = BTreeMap::new();
        let mut checks = BTreeMap::new();
        let mut operations = BTreeMap::<&str, usize>::new();
        let mut total = 0;
        for (&thread, (projection, _)) in self.0 {
            for (id, call) in &projection.calls {
                if call.commands.is_empty() || boundary.is_some_and(|cutoff| call.time.is_none_or(|time| time < cutoff)) { continue; }
                total += call.commands.git.len();
                for (index, event) in call.commands.git.iter().enumerate() {
                    *operations.entry(&event.operation).or_default() += 1;
                    retain_preview(&mut git, (Reverse(call.time), thread, id.as_str(), index),
                        (thread, id, call, index, event), GIT_EVENT_LIMIT);
                }
                for (index, event) in call.commands.checks.iter().enumerate() {
                    retain_preview(&mut checks, (Reverse(call.time), thread, id.as_str(), index),
                        (thread, id, call, event), CHECK_EVENT_LIMIT);
                }
            }
        }
        let git: Vec<_> = git.into_values().map(|(thread, id, call, index, event)| json!({
            "thread_id":thread, "thread_name":null, "call_id":id, "index":index,
            "timestamp":call.time.map(stamp), "completed_at":call.completed_at().map(stamp), "duration_ms":call.duration_ms(),
            "operation":event.operation, "repository":event.repository,
        })).collect();
        let checks = checks.into_values().map(|(thread, id, call, event)| json!({
            "thread_id":thread, "thread_name":null, "call_id":id,
            "timestamp":call.time.map(stamp), "completed_at":call.completed_at().map(stamp), "duration_ms":call.duration_ms(),
            "operation":event.operation, "category":event.category,
        })).collect();
        (json!({"events":git,"operations":operations,"total":total}), checks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::project::Call;

    #[test]
    fn snapshot_command_previews_match_legacy_sort_and_full_counts() {
        let time = DateTime::parse_from_rfc3339("2026-11-13T01:00:00Z").unwrap().with_timezone(&Utc);
        let mut threads = BTreeMap::new();
        for thread in ["a", "b", "c"] {
            let mut projection = Projection::for_id(Some(thread.into()));
            for n in 0..700 {
                let when = match n % 5 {
                    0 => None, 1 => Some(time), 2 => Some(time - chrono::Duration::hours(2)),
                    3 => Some(time + chrono::Duration::hours(1)),
                    _ => Some(time + chrono::Duration::nanoseconds(999_999)),
                };
                let git: Vec<_> = (0..n % 4).map(|index|json!({
                    "operation":if index % 2 == 0 {"status"} else {"diff"},
                    "repository":if index == 0 {Value::Null} else {json!(format!("repo-{thread}-{n}"))}
                })).collect();
                let checks: Vec<_> = (0..n % 3).map(|index|json!({
                    "operation":if index == 0 {"cargo test"} else {"cargo check"},"category":"runtime"
                })).collect();
                let call: Call = serde_json::from_value(json!({
                    "tool":"exec","time":when,"completed":when.filter(|_|n%2==0).map(|time|time+chrono::Duration::seconds(2)),
                    "sequence":n,"commands":{"git":git,"checks":checks}
                })).unwrap();
                assert!(call.commands.valid());
                projection.calls.insert(format!("call-{n:04}"),call);
            }
            let mut replacement = Projection::for_id(Some(thread.into()));
            replacement.calls.insert("call-0001".into(),serde_json::from_value(json!({
                "tool":"winner","time":null,"completed":null,"sequence":1,
                "commands":{"git":[{"operation":"log","repository":"replacement"}],"checks":[]}
            })).unwrap());
            projection.merge(&replacement);
            threads.insert(thread,(projection,false));
        }
        for boundary in [None,Some(time-chrono::Duration::hours(1)),Some(time),Some(time+chrono::Duration::hours(3))] {
            let actual = Calls::new(&threads).window(boundary);
            assert_eq!(actual,legacy_window(&threads,boundary));
            assert!(actual.0["events"].as_array().unwrap().len()<=GIT_EVENT_LIMIT);
            assert!(actual.1.len()<=CHECK_EVENT_LIMIT);
            if boundary.is_none() { assert!(actual.0["total"].as_u64().unwrap()>GIT_EVENT_LIMIT as u64); }
        }
        let mut preview = BTreeMap::new();
        for rank in (0..10_000).rev() {
            retain_preview(&mut preview,rank,rank,GIT_EVENT_LIMIT);
            assert!(preview.len()<=GIT_EVENT_LIMIT);
        }
        assert_eq!(preview.into_keys().collect::<Vec<_>>(),(0..GIT_EVENT_LIMIT).collect::<Vec<_>>());
        assert_eq!(Calls::new(&BTreeMap::new()).window(None),(json!({"events":[],"operations":{},"total":0}),Vec::new()));
    }

    fn legacy_window(threads: &BTreeMap<&str, (Projection, bool)>, boundary: Option<DateTime<Utc>>) -> (Value, Vec<Value>) {
        let mut calls: Vec<_> = threads
            .iter()
            .flat_map(|(&thread, (projection, _))| {
                projection.calls.iter().filter_map(move |(id, call)| {
                    (!call.commands.is_empty()).then_some((thread, id.as_str(), call))
                })
            })
            .collect();
        calls.sort_by_key(|(_, _, call)| Reverse(call.time));
        let mut git = Vec::new();
        let mut checks = Vec::new();
        let mut operations = BTreeMap::<&str, usize>::new();
        let mut total = 0;
        for &(thread, id, call) in &calls {
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
