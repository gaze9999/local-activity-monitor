//! Read-only Platform responses and environment credential availability.
use bigdecimal::{BigDecimal, RoundingMode};
use chrono::{DateTime, Utc};
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
    let seconds = value.as_i64()?;
    super::schedules::seconds_timestamp(seconds as f64)
        .as_str()
        .map(str::to_owned)
}

fn round_cost(mut value: BigDecimal) -> BigDecimal {
    while value.digits() > 28 {
        let scale = value.as_bigint_and_exponent().1 - (value.digits() - 28) as i64;
        value = value.with_scale_round(scale, RoundingMode::HalfEven);
    }
    value
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
                            .as_i64()
                            .filter(|n| *n >= 0)
                            .map(|n| sum + n as u128)
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
                if scale > 18 || (scale < -12 && coefficient != 0.into()) {
                    return None;
                }
                let bounded = round_cost(value.clone());
                if bounded > BigDecimal::from(1_000_000_000_000i64)
                    || bounded < BigDecimal::from(-1_000_000_000_000i64)
                {
                    return None;
                }
                let value = if coefficient == 0.into() && scale < -12 {
                    BigDecimal::from(0)
                } else {
                    value
                };
                let total = amounts.entry(currency.into()).or_default();
                *total = round_cost(&*total + value);
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
                    if let Some(projected) = crate::json_bytes::parse(&bytes)
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
    async fn apply_bytes(kind: Kind, body: &[u8]) -> Value {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
        response.extend_from_slice(body);
        let worker = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0; 2048];
            stream.read(&mut request).await.unwrap();
            let _ = stream.write_all(&response).await;
        });
        let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(std::time::Duration::from_secs(2)).build().unwrap();
        let mut result = json!({"health":"missing_credentials","items":[]});
        apply_response(kind, client.get(format!("http://{address}/")), &mut result).await;
        worker.await.unwrap();
        result
    }

    #[tokio::test]
    async fn encoded_responses_preserve_decimal_projection_and_raw_byte_limit() {
        for (kind, body) in [
            (Kind::Models, r#"{"data":[{"id":"model_1","secret":"PRIVATE"}]}"#),
            (Kind::Usage, r#"{"data":[{"start_time":0,"end_time":1,"results":[{"input_tokens":9223372036854775807}]}],"has_more":false}"#),
            (Kind::Decisions, r#"{"data":[{"start_time":0,"end_time":1,"results":[{"input_tokens":9223372036854775807}]}],"has_more":false}"#),
            (Kind::Costs, r#"{"data":[{"start_time":0,"end_time":1,"results":[{"amount":{"value":0.123456789012345678,"currency":"usd"}}]}],"has_more":false}"#),
        ] {
            for (width, big_endian, prefix) in [
                (1, false, b"\xef\xbb\xbf".as_slice()),
                (2, false, b"\xff\xfe"), (2, true, b"\xfe\xff"),
                (4, false, b"\xff\xfe\0\0"), (4, true, b"\0\0\xfe\xff"),
            ] {
                for bom in [false, true] {
                    let mut raw = if bom { prefix.to_vec() } else { Vec::new() };
                    for byte in body.bytes() {
                        match width {
                            1 => raw.push(byte),
                            2 => raw.extend(if big_endian { u16::from(byte).to_be_bytes() } else { u16::from(byte).to_le_bytes() }),
                            4 => raw.extend(if big_endian { u32::from(byte).to_be_bytes() } else { u32::from(byte).to_le_bytes() }),
                            _ => unreachable!(),
                        }
                    }
                    let result = apply_bytes(kind, &raw).await;
                    assert_eq!(result["health"], "ok", "{width} {big_endian} {bom}");
                    assert!(!result.to_string().contains("PRIVATE"));
                    match kind {
                        Kind::Models => assert_eq!(result["items"][0]["id"], "model_1"),
                        Kind::Costs => assert_eq!(result["items"][0]["amounts"]["usd"], "0.123456789012345678"),
                        _ => assert_eq!(result["items"][0]["input_tokens"], 9223372036854775807i64),
                    }
                }
            }
        }
        let mut raw = b"\xff\xfe".to_vec();
        for byte in b"{\"data\":[]}" { raw.extend(u16::from(*byte).to_le_bytes()); }
        while raw.len() < 1048576 { raw.extend(b" \0"); }
        assert_eq!(raw.len(), 1048576);
        assert_eq!(apply_bytes(Kind::Models, &raw).await["health"], "ok");
        raw.extend(b" \0");
        assert_eq!(apply_bytes(Kind::Models, &raw).await["health"], "error");
        assert_eq!(apply_bytes(Kind::Models, b"\xff\xfe{").await["health"], "error");
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

#[cfg(test)]
mod date_boundary_tests {
    use super::*;

    #[test]
    fn platform_dates_preserve_integer_wire_type_and_python_calendar_bounds() {
        for (raw, expected) in [
            ("0", Some("1970-01-01T00:00:00.000Z")),
            ("-62135596800", Some("0001-01-01T00:00:00.000Z")),
            ("253402300799", Some("9999-12-31T23:59:59.000Z")),
            ("-62135596801", None),
            ("253402300800", None),
            ("-0", Some("1970-01-01T00:00:00.000Z")),
            ("-0.0", None),
            ("0.0", None),
            ("1e3", None),
            ("true", None),
            ("null", None),
            ("18446744073709551615", None),
        ] {
            let value: Value = serde_json::from_str(raw).unwrap();
            assert_eq!(date(&value).as_deref(), expected, "{raw}");
            let projected = project(Kind::Models, &json!({"data":[{"id":"test", "created":value}]})).unwrap();
            assert_eq!(projected["items"][0]["created_at"], json!(expected), "{raw}");
        }
        assert!(project(Kind::Usage, &serde_json::from_str(r#"{"has_more":false,"data":[{"start_time":0.0,"end_time":1,"results":[]}]}"#).unwrap()).is_none());
        assert!(project(Kind::Usage, &json!({"has_more":false,"data":[{"start_time":0,"end_time":1,"results":[]}]})).is_some());
    }
}

#[cfg(test)]
mod cost_context_tests {
    use super::*;
    #[test]
    fn costs_round_each_ordered_addition_with_python_decimal_context() {
        let cases: &[(&[&str], Option<&str>)] = &[
            (&["999999999999","1.000000000000000001"], Some("1000000000000.000000000000000")),
            (&["999999999999","1.000000000000000001","-1000000000000"], Some("0.000000000000000")),
            (&["999999999999","-1000000000000","1.000000000000000001"], Some("0.000000000000000001")),
            (&["999999999999.999999999999999999"], Some("1000000000000.000000000000000")),
            (&["1.2345678901234565","999999999999"], Some("1000000000000.234567890123456")),
            (&["1.2345678901234575","999999999999"], Some("1000000000000.234567890123458")),
            (&["-1.2345678901234565","-999999999999"], Some("-1000000000000.234567890123456")),
            (&["-0.00"], Some("0.00")),
            (&["1.00","-1.00"], Some("0.00")),
            (&["0e10"], Some("0")),
            (&["0e-18"], Some("0.000000000000000000")),
            (&["1000000000000.000000000000000001"], Some("1000000000000.000000000000000")),
            (&["1000000000000.000000000000001"], None),
            (&["-1000000000000.000000000000000001"], Some("-1000000000000.000000000000000")),
            (&["-1000000000000.000000000000001"], None),
            (&["1e13"], None),
            (&["1e-19"], None),
            (&vec!["1e12"; 1000], Some("1000000000000000")),
        ];
        for (amounts, expected) in cases {
            let entries: Vec<Value> = amounts.iter().map(|number| {
                json!({"amount":{"value":serde_json::from_str::<Value>(number).unwrap(),"currency":"usd"}})
            }).collect();
            let result = project(Kind::Costs, &json!({"has_more":false,"data":[{"start_time":0,"end_time":1,"results":entries}]}));
            let actual = result.as_ref().and_then(|value| value["items"][0]["amounts"]["usd"].as_str());
            assert_eq!(actual, *expected, "{} amounts, first {:?}", amounts.len(), amounts.first());
        }
    }
}

#[cfg(test)]
mod mixed_cost_tests {
    use super::*;
    #[test]
    fn currencies_keep_independent_ordered_decimal_contexts() {
        let entries: Vec<Value> = [
            ("usd", "999999999999"), ("jpy", "0.25"),
            ("usd", "1.000000000000000001"), ("jpy", "0.75"),
            ("usd", "-1000000000000"),
        ].into_iter().map(|(currency, number)| json!({"amount":{"currency":currency,"value":serde_json::from_str::<Value>(number).unwrap()}})).collect();
        let result = project(Kind::Costs, &json!({"has_more":false,"data":[{"start_time":0,"end_time":1,"results":entries}]})).unwrap();
        assert_eq!(result["items"][0]["amounts"], json!({"usd":"0.000000000000000","jpy":"1.00"}));
    }
}

#[cfg(test)]
mod integer_zero_tests {
    use super::*;
    #[test]
    fn usage_counts_accept_integer_negative_zero_only() {
        for (raw, expected) in [("-0", Some(0)), ("0", Some(0)), ("9223372036854775807", Some(i64::MAX)), ("-1", None), ("9223372036854775808", None), ("-0.0", None), ("1e0", None), ("true", None)] {
            let number: Value = serde_json::from_str(raw).unwrap();
            for kind in [Kind::Usage, Kind::Decisions] {
                let result = project(kind, &json!({"has_more":false,"data":[{"start_time":0,"end_time":1,"results":[{"input_tokens":number,"input_cached_tokens":number,"output_tokens":number,"num_model_requests":number}]}]})).unwrap();
                for field in ["input_tokens", "input_cached_tokens", "output_tokens", "num_model_requests"] {
                    assert_eq!(result["items"][0][field].as_i64(), expected, "{raw}: {field}");
                }
            }
        }
    }
}

#[cfg(test)]
mod aggregate_count_tests {
    use super::*;
    #[test]
    fn thousand_maximum_counts_preserve_wide_exact_aggregate() {
        let row = json!({"input_tokens":i64::MAX,"input_cached_tokens":i64::MAX,"output_tokens":i64::MAX,"num_model_requests":i64::MAX});
        for kind in [Kind::Usage, Kind::Decisions] {
            let result = project(kind, &json!({"has_more":false,"data":[{"start_time":0,"end_time":1,"results":vec![row.clone();1000]}]})).unwrap();
            for field in ["input_tokens", "input_cached_tokens", "output_tokens", "num_model_requests"] {
                assert_eq!(result["items"][0][field].to_string(), "9223372036854775807000");
            }
        }
    }
}
