//! Safe local allowance projection from session token-count events.
use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;
static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9_.:-]{1,80}$").unwrap());
static BALANCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{1,16}(?:\.[0-9]{1,16})?$").unwrap());
fn number(value: &Value, max: f64) -> Option<f64> {
    let value = value.as_f64()?;
    (value.is_finite() && value >= 0.0 && value <= max).then_some(value)
}
fn numeric(value: &Value, max: f64) -> Option<Value> {
    number(value, max).map(|_| value.clone())
}
pub fn project(value: &Value, when: DateTime<Utc>) -> Option<Value> {
    if !value.is_object() {
        return None;
    }
    let mut result =
        json!({"updated_at":when.to_rfc3339_opts(SecondsFormat::Millis,true),"limits":[]});
    for key in ["limit_id", "plan_type"] {
        if let Some(v) = value[key].as_str().filter(|s| LABEL.is_match(s)) {
            result[key] = json!(v);
        }
    }
    let mut limits = Vec::new();
    for key in ["primary", "secondary"] {
        let item = &value[key];
        if !item.is_object() {
            continue;
        }
        let mut window = json!({"id":key});
        if let Some(used) = number(&item["used_percent"], 100.0) {
            window["used_percent"] = item["used_percent"].clone();
            window["remaining_percent"] = json!(100.0 - used);
        }
        if let Some(minutes) = numeric(&item["window_minutes"], 525600.0)
            .filter(|n| n.as_f64().is_some_and(|n| n > 0.0))
        {
            window["window_minutes"] = minutes;
        }
        if let Some(reset) = number(&item["resets_at"], 253402300799.0) {
            let seconds = reset.floor() as i64;
            if let Some(time) = DateTime::<Utc>::from_timestamp(
                seconds,
                ((reset - seconds as f64) * 1_000_000_000.0) as u32,
            ) {
                window["resets_at"] = json!(time.to_rfc3339_opts(SecondsFormat::Millis, true));
            }
        }
        if window.as_object()?.len() > 1 {
            limits.push(window);
        }
    }
    result["limits"] = json!(limits);
    if let Some(credits) = value["credits"].as_object() {
        let mut clean = Map::new();
        for key in ["has_credits", "unlimited"] {
            if let Some(flag) = credits.get(key).and_then(Value::as_bool) {
                clean.insert(key.into(), json!(flag));
            }
        }
        let balance = credits.get("balance");
        let balance = balance.and_then(|value| {
            if let Some(s) = value.as_str().filter(|s| BALANCE.is_match(s)) {
                s.parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 9007199254740992.0)
                    .map(|v| json!(v))
            } else {
                numeric(value, 9007199254740992.0)
            }
        });
        if let Some(value) = balance {
            clean.insert("balance".into(), value);
        }
        if !clean.is_empty() {
            result["credits"] = json!(clean);
        }
    }
    (!result["limits"].as_array()?.is_empty() || result.get("credits").is_some()).then_some(result)
}
pub fn valid(value: &Value) -> bool {
    let Some(map) = value.as_object() else {
        return false;
    };
    if !map.keys().all(|key| {
        ["updated_at", "limits", "limit_id", "plan_type", "credits"].contains(&key.as_str())
    }) || !value["updated_at"]
        .as_str()
        .is_some_and(|s| DateTime::parse_from_rfc3339(s).is_ok())
        || ["limit_id", "plan_type"].iter().any(|key| {
            value
                .get(*key)
                .is_some_and(|v| !v.as_str().is_some_and(|s| LABEL.is_match(s)))
        })
    {
        return false;
    }
    let Some(limits) = value["limits"].as_array().filter(|items| items.len() <= 2) else {
        return false;
    };
    if limits.iter().any(|window| {
        let Some(map) = window.as_object() else {
            return true;
        };
        !map.keys().all(|key| {
            [
                "id",
                "used_percent",
                "remaining_percent",
                "window_minutes",
                "resets_at",
            ]
            .contains(&key.as_str())
        }) || !window["id"]
            .as_str()
            .is_some_and(|id| ["primary", "secondary"].contains(&id))
            || ["used_percent", "remaining_percent"]
                .iter()
                .any(|key| window.get(*key).is_some_and(|v| number(v, 100.0).is_none()))
            || window
                .get("window_minutes")
                .is_some_and(|v| number(v, 525600.0).is_none_or(|n| n <= 0.0))
            || window.get("resets_at").is_some_and(|v| {
                !v.as_str()
                    .is_some_and(|s| DateTime::parse_from_rfc3339(s).is_ok())
            })
    }) {
        return false;
    }
    if let Some(credits) = value.get("credits") {
        let Some(map) = credits.as_object() else {
            return false;
        };
        if map.is_empty()
            || !map
                .keys()
                .all(|key| ["has_credits", "unlimited", "balance"].contains(&key.as_str()))
            || ["has_credits", "unlimited"]
                .iter()
                .any(|key| credits.get(*key).is_some_and(|v| !v.is_boolean()))
            || credits
                .get("balance")
                .is_some_and(|v| number(v, 9007199254740992.0).is_none())
        {
            return false;
        }
    }
    !limits.is_empty() || value.get("credits").is_some()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn safe_projection() {
        let now = DateTime::from_timestamp(0, 0).unwrap();
        let value=project(&json!({"limit_id":"codex","plan_type":"pro","primary":{"used_percent":25,"window_minutes":300,"resets_at":0},"credits":{"balance":"1.25","has_credits":true,"secret":"private"}}),now).unwrap();
        assert_eq!(value["limits"][0]["remaining_percent"], 75.0);
        assert_eq!(value["credits"]["balance"], 1.25);
        assert!(!value.to_string().contains("private"));
        assert!(project(&json!({"primary":{"used_percent":101}}), now).is_none());
    }
}
