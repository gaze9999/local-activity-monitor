//! Read-only Platform responses and environment credential availability.
use bigdecimal::BigDecimal;
use chrono::{DateTime, SecondsFormat, Utc};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::{collections::BTreeMap, str::FromStr, sync::LazyLock};

static ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.:/-]{1,200}$").unwrap());
static OWNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.:/-]{1,160}$").unwrap());
static CURRENCY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z]{3}$").unwrap());
#[derive(Clone, Copy)]
pub enum Kind {
    Models,
    Usage,
    Decisions,
    Costs,
}
impl Kind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "models" => Some(Self::Models),
            "usage" => Some(Self::Usage),
            "decisions" => Some(Self::Decisions),
            "costs" => Some(Self::Costs),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Models => "models",
            Self::Usage => "usage",
            Self::Decisions => "decisions",
            Self::Costs => "costs",
        }
    }
    pub fn variable(self) -> &'static str {
        match self {
            Self::Models => "OPENAI_API_KEY",
            _ => "OPENAI_ADMIN_KEY",
        }
    }
    pub fn path(self) -> &'static str {
        match self {
            Self::Models => "/models",
            Self::Usage => "/organization/usage/completions",
            Self::Decisions => "/organization/usage/decisions",
            Self::Costs => "/organization/costs",
        }
    }
}
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 4096 && key.bytes().all(|c| (33..=126).contains(&c))
}
pub fn credentials() -> Value {
    let mut available = Map::new();
    for kind in [Kind::Models, Kind::Usage, Kind::Decisions, Kind::Costs] {
        available.insert(
            kind.key().into(),
            json!(std::env::var(kind.variable())
                .ok()
                .is_some_and(|key| valid_key(&key))),
        );
    }
    json!({"mode":if available["decisions"]==true {"with_key"} else {"no_key"},"available":available})
}
fn date(value: &Value) -> Option<String> {
    let number = value.as_f64()?;
    if !number.is_finite() {
        return None;
    }
    let secs = number.floor();
    if secs < i64::MIN as f64 || secs >= i64::MAX as f64 {
        return None;
    }
    Some(
        DateTime::<Utc>::from_timestamp(secs as i64, ((number - secs) * 1_000_000_000.0) as u32)?
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}
pub fn project(kind: Kind, value: &Value) -> Option<Value> {
    let rows = value.get("data")?.as_array()?;
    if rows.len() > if matches!(kind, Kind::Models) { 500 } else { 7 } {
        return None;
    }
    let mut items = Vec::new();
    if matches!(kind, Kind::Models) {
        for row in rows {
            let id = row.get("id")?.as_str().filter(|id| ID.is_match(id))?;
            let owner = row
                .get("owned_by")
                .and_then(Value::as_str)
                .filter(|v| OWNER.is_match(v));
            items.push(json!({"id":id,"created_at":date(&row["created"]),"owned_by":owner}));
        }
        return Some(json!({"items":items}));
    }
    let more = value.get("has_more")?.as_bool()?;
    for row in rows {
        let entries = row.get("results")?.as_array()?;
        if entries.len() > 1000 {
            return None;
        }
        let start = date(&row["start_time"])?;
        let end = date(&row["end_time"])?;
        if start >= end {
            return None;
        }
        let mut item = json!({"start_time":start,"end_time":end});
        if matches!(kind, Kind::Usage | Kind::Decisions) {
            for field in [
                "input_tokens",
                "input_cached_tokens",
                "output_tokens",
                "num_model_requests",
            ] {
                let sum = if entries.is_empty() {
                    None
                } else {
                    entries.iter().try_fold(0u128, |sum, entry| {
                        entry
                            .get(field)?
                            .as_u64()
                            .filter(|n| *n <= i64::MAX as u64)
                            .map(|n| sum + u128::from(n))
                    })
                };
                item[field] = sum
                    .and_then(|n| serde_json::from_str(&n.to_string()).ok())
                    .unwrap_or(Value::Null);
            }
        } else {
            let mut amounts = BTreeMap::<String, BigDecimal>::new();
            for entry in entries {
                let amount = entry.get("amount")?;
                let number = amount.get("value")?.as_number()?;
                let currency = amount
                    .get("currency")?
                    .as_str()
                    .filter(|v| CURRENCY.is_match(v))?;
                let value = BigDecimal::from_str(&number.to_string()).ok()?;
                let (coefficient, scale) = value.as_bigint_and_exponent();
                if scale > 18
                    || (scale < -12 && coefficient != 0.into())
                    || value > BigDecimal::from(1_000_000_000_000i64)
                    || value < BigDecimal::from(-1_000_000_000_000i64)
                {
                    return None;
                }
                let value = if coefficient == 0.into() && scale < -12 {
                    BigDecimal::from(0)
                } else {
                    value
                };
                *amounts.entry(currency.into()).or_default() += value;
            }
            item["amounts"] = json!(amounts
                .into_iter()
                .map(|(key, value)| (key, value.to_plain_string()))
                .collect::<BTreeMap<_, _>>());
        }
        items.push(item);
    }
    Some(json!({"items":items,"has_more":more}))
}
async fn apply_response(kind: Kind, request: reqwest::RequestBuilder, result: &mut Value) {
    result["health"] = json!("error");
    if let Ok(mut response) = request.send().await {
        match response.status().as_u16() {
            401 | 403 => result["health"] = json!("unauthorized"),
            429 => result["health"] = json!("rate_limited"),
            200..=299 => {
                let mut bytes = Vec::new();
                let mut valid = true;
                loop {
                    match response.chunk().await {
                        Ok(Some(chunk))
                            if chunk.len() <= 1048576usize.saturating_sub(bytes.len()) =>
                        {
                            bytes.extend_from_slice(&chunk)
                        }
                        Ok(None) => break,
                        _ => {
                            valid = false;
                            break;
                        }
                    }
                }
                if valid {
                    if let Some(projected) = serde_json::from_slice::<Value>(&bytes)
                        .ok()
                        .and_then(|value| project(kind, &value))
                    {
                        if let (Some(target), Some(values)) =
                            (result.as_object_mut(), projected.as_object())
                        {
                            target.extend(values.clone());
                        }
                        result["health"] = json!("ok");
                    }
                }
            }
            _ => {}
        }
    }
}

pub struct Source {
    kind: Kind,
    client: reqwest::Client,
    cache: tokio::sync::Mutex<Option<(tokio::time::Instant, Value)>>,
    shared: std::sync::Arc<super::account::Shared>,
}
impl Source {
    pub fn new(
        kind: Kind,
        shared: std::sync::Arc<super::account::Shared>,
    ) -> Result<Self, reqwest::Error> {
        Ok(Self {
            kind,
            shared,
            cache: tokio::sync::Mutex::new(None),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(5))
                .build()?,
        })
    }
    pub fn settings(&self) -> super::account::Settings {
        self.shared.settings()
    }
    pub fn configure(&self, value: super::account::Settings) {
        self.shared.configure(value)
    }
    pub async fn query(&self) -> Value {
        let mut cache = self.cache.lock().await;
        if let Some((time, value)) = &*cache {
            if time.elapsed() < std::time::Duration::from_secs(60) {
                self.shared.publish(value.clone());
                return value.clone();
            }
        }
        let stamp = DateTime::<Utc>::from(std::time::SystemTime::now());
        let mut result = json!({"source":self.kind.key(),"health":"missing_credentials","items":[],"checked_at":stamp.to_rfc3339()});
        if let Some(key) = std::env::var(self.kind.variable())
            .ok()
            .filter(|key| valid_key(key))
        {
            let mut request = self
                .client
                .get(format!("https://api.openai.com/v1{}", self.kind.path()))
                .bearer_auth(key)
                .header("Accept", "application/json");
            if !matches!(self.kind, Kind::Models) {
                let end = stamp.timestamp();
                let start = end.div_euclid(86400) * 86400 - 6 * 86400;
                request = request.query(&[
                    ("start_time", start.to_string()),
                    ("end_time", end.to_string()),
                    ("bucket_width", "1d".to_owned()),
                    ("limit", "7".to_owned()),
                ]);
                result["start_time"] = json!(date(&json!(start)));
                result["end_time"] = json!(date(&json!(end)));
            }
            apply_response(self.kind, request, &mut result).await;
        }
        *cache = Some((tokio::time::Instant::now(), result.clone()));
        self.shared.publish(result.clone());
        result
    }
    pub async fn run(self: std::sync::Arc<Self>) {
        loop {
            let setting = self.shared.settings();
            if !setting.enabled {
                self.shared.notify.notified().await;
                continue;
            }
            let last = *self
                .shared
                .completed
                .read()
                .unwrap_or_else(|error| error.into_inner());
            if self
                .shared
                .force_due
                .swap(false, std::sync::atomic::Ordering::AcqRel)
            {
                if self.shared.settings().enabled {
                    self.query().await;
                }
                continue;
            }
            let due = last
                .map(|last| last + std::time::Duration::from_secs(setting.interval))
                .unwrap_or_else(std::time::Instant::now);
            tokio::select! {_=self.shared.notify.notified()=>{},_=tokio::time::sleep_until(tokio::time::Instant::from_std(due))=>{self.query().await;}}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn local_transport_masks_credentials_and_classifies_status() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        for (status, expected) in [
            ("200 OK", "ok"),
            ("401 Unauthorized", "unauthorized"),
            ("429 Too Many Requests", "rate_limited"),
            ("302 Found", "error"),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let response = format!("HTTP/1.1 {status}\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{{\"data\":[]}}");
            let worker = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                let size = stream.read(&mut request).await.unwrap();
                assert!(String::from_utf8_lossy(&request[..size])
                    .to_ascii_lowercase()
                    .contains("authorization: bearer synthetic-key"));
                stream.write_all(response.as_bytes()).await.unwrap();
            });
            let mut result = json!({"health":"missing_credentials","items":[]});
            apply_response(
                Kind::Models,
                client
                    .get(format!("http://{address}/models"))
                    .bearer_auth("synthetic-key"),
                &mut result,
            )
            .await;
            worker.await.unwrap();
            assert_eq!(result["health"], expected);
            assert!(!result.to_string().contains("synthetic-key"));
        }
    }
    #[test]
    fn projection_masks_and_validates() {
        let value =
            json!({"data":[{"id":"gpt-test","owned_by":"openai","created":0,"secret":"private"}]});
        let projected = project(Kind::Models, &value).unwrap();
        assert_eq!(
            projected["items"][0]["created_at"],
            "1970-01-01T00:00:00.000Z"
        );
        assert!(!projected.to_string().contains("private"));
        assert!(project(Kind::Models, &json!({"data":[{"id":"bad id"}]})).is_none());
        let rows = json!({"has_more":false,"data":[{"start_time":0,"end_time":60,"results":[{"input_tokens":4},{"input_tokens":7}]}]});
        let usage = project(Kind::Usage, &rows).unwrap();
        assert_eq!(usage["items"][0]["input_tokens"], 11);
        assert!(usage["items"][0]["output_tokens"].is_null());
        let costs:Value=serde_json::from_str(r#"{"has_more":false,"data":[{"start_time":0,"end_time":60,"results":[{"amount":{"currency":"usd","value":0.1}},{"amount":{"currency":"usd","value":0.2}}]}]}"#).unwrap();
        assert_eq!(
            project(Kind::Costs, &costs).unwrap()["items"][0]["amounts"]["usd"],
            "0.3"
        );
        assert!(valid_key("test-key"));
        assert!(!valid_key("test key"));
        assert!(!valid_key(""));
    }

    #[test]
    fn rejects_extreme_cost_exponents_and_accepts_bounded_zero() {
        let make = |number: &str| -> Value {
            serde_json::from_str(&format!(r#"{{"has_more":false,"data":[{{"start_time":1,"end_time":2,"results":[{{"amount":{{"currency":"usd","value":{number}}}}}]}}]}}"#)).unwrap()
        };
        assert!(project(Kind::Costs, &make("1e1000000")).is_none());
        assert!(project(Kind::Costs, &make("1e-19")).is_none());
        assert_eq!(
            project(Kind::Costs, &make("0e1000000")).unwrap()["items"][0]["amounts"]["usd"],
            "0"
        );
    }
}
