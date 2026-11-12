use chrono::{DateTime, Utc};
use lam_history_store::{parse_utc_stamp, sql_event_id};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const EVENT_LIMIT: usize = 500;

pub fn identified(events: Vec<Value>) -> Vec<Value> {
    events
        .into_iter()
        .filter_map(|mut event| {
            event["id"] = sql_event_id(&event)?.into();
            Some(event)
        })
        .collect()
}

pub fn window(events: &[Value], cutoff: Option<DateTime<Utc>>, health: &str) -> Value {
    let mut operations = BTreeMap::<&str, usize>::new();
    let mut selected = Vec::new();
    let mut total = 0;
    for event in events {
        if cutoff.is_some_and(|cutoff| {
            event["timestamp"]
                .as_str()
                .and_then(parse_utc_stamp)
                .is_none_or(|time| time < cutoff)
        }) {
            continue;
        }
        if let Some(operation) = event["operation"].as_str() {
            *operations.entry(operation).or_default() += 1;
        }
        total += 1;
        if selected.len() < EVENT_LIMIT {
            selected.push(event);
        }
    }
    json!({"events":selected,"total":total,"operations":operations,"history_health":health})
}
