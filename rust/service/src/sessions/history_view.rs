use chrono::{DateTime, SecondsFormat, Utc};
use lam_history_store::{Aggregate, CallCounts};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub fn render(aggregate: &Aggregate, window: &str) -> Value {
    let series: Vec<_> = aggregate.series.iter().map(|bucket| {
        let time = DateTime::<Utc>::from_timestamp(bucket.epoch_seconds, 0)
            .expect("SQLite timestamp range").to_rfc3339_opts(SecondsFormat::Secs, true);
        json!({
            "time":time,"calls":bucket.calls.total,"direct":bucket.calls.direct(),"nested":bucket.calls.nested,
            "operations":numbers(&bucket.operations),"servers":servers(&bucket.servers)
        })
    }).collect();
    let mut value = json!({
        "total":aggregate.total,"dated_total":aggregate.dated_total,"undated_total":aggregate.undated_total(),
        "operations":numbers(&aggregate.operations),"statements":numbers(&aggregate.statements),
        "servers":servers(&aggregate.servers),"series":series,"bucket_seconds":aggregate.bucket_seconds,
        "classification_truncated":aggregate.classification_truncated,"scope":"retained_database","window":window
    });
    if let Some(truncated) = aggregate.classification_series_truncated {
        value["classification_series_truncated"] = truncated.into();
    }
    value
}

fn numbers(values: &[(String, i64)]) -> BTreeMap<&str, i64> {
    values
        .iter()
        .map(|(key, value)| (key.as_str(), *value))
        .collect()
}

fn servers(values: &[(String, CallCounts)]) -> Map<String, Value> {
    values
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                json!({
                    "total":value.total,"direct":value.direct(),"nested":value.nested
                }),
            )
        })
        .collect()
}
