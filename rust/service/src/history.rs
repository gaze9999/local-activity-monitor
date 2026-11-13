use base64::{engine::general_purpose::URL_SAFE, Engine};
use lam_history_store::{normalize_utc_stamp, Access, Page, PageQuery, Position, Section, Store};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

static DECIMAL_DIGIT: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"^\p{Nd}$").expect("decimal digit"));

pub(crate) fn query_number(value: &str) -> Option<u32> {
    u32::try_from(query_integer(value)?).ok()
}

pub(crate) fn decimal_digits(value: &str, max_digits: usize) -> Option<u64> {
    if value.is_empty() || value.chars().count() > max_digits
        || !value.chars().all(|ch| DECIMAL_DIGIT.is_match(ch.encode_utf8(&mut [0; 4]))) {
        return None;
    }
    query_integer(value)
}

fn query_integer(value: &str) -> Option<u64> {
    let value = value.trim();
    let (negative, digits) = match value.as_bytes().first() {
        Some(b'-') => (true, &value[1..]),
        Some(b'+') => (false, &value[1..]),
        _ => (false, value),
    };
    let mut result = 0_u64;
    let mut saw_digit = false;
    let mut separator = false;
    for ch in digits.chars() {
        if ch == '_' {
            if !saw_digit || separator { return None; }
            separator = true;
            continue;
        }
        if !DECIMAL_DIGIT.is_match(ch.encode_utf8(&mut [0; 4])) { return None; }
        let code = ch as u32;
        let mut first = code;
        while let Some(previous) = first.checked_sub(1).and_then(char::from_u32) {
            if !DECIMAL_DIGIT.is_match(previous.encode_utf8(&mut [0; 4])) { break; }
            first -= 1;
        }
        result = result.checked_mul(10)?.checked_add(u64::from((code - first) % 10))?;
        saw_digit = true;
        separator = false;
    }
    (saw_digit && !separator && (!negative || result == 0)).then_some(result)
}

pub enum HistoryError {
    Invalid,
    Unavailable,
}

#[derive(Serialize)]
pub struct HistoryResponse {
    items: Vec<Box<RawValue>>,
    total: i64,
    limit: u32,
    offset: u32,
    next_cursor: Option<String>,
}

struct Query {
    section: Section,
    limit: u32,
    offset: u32,
    since: Option<String>,
    after: Option<Position>,
}

impl Query {
    fn parse(query: &str, codex: bool) -> Option<Self> {
        if query.len() > 512 {
            return None;
        }
        let mut values = HashMap::new();
        for (name, value) in form_urlencoded::parse(query.as_bytes()) {
            if !matches!(
                name.as_ref(),
                "namespace" | "section" | "limit" | "offset" | "since" | "cursor"
            ) || values.insert(name, value).is_some()
            {
                return None;
            }
        }
        let get = |key| values.get(key).map(|value| value.as_ref());
        let section = match (get("namespace").unwrap_or("activity"), get("section")?) {
            ("activity", "sql") => Section::Sql,
            ("activity", "web") => Section::Web,
            ("activity", "mcp") => Section::Mcp,
            ("errors", "events") => Section::Errors,
            ("threads", "skills") if codex => Section::Skills,
            ("threads", "entries") if codex => Section::Threads,
            _ => return None,
        };
        let number = |key, default| query_number(get(key).unwrap_or(default));
        let limit = number("limit", "100")?;
        let offset = number("offset", "0")?;
        if !(1..=200).contains(&limit) || offset > 1_000_000 {
            return None;
        }
        let since = match get("since") {
            Some(value) => Some(normalize_utc_stamp(value)?),
            None => None,
        };
        let after = match get("cursor") {
            Some(value) if offset == 0 => Some(decode_cursor(value)?),
            Some(_) => return None,
            None => None,
        };
        Some(Self {
            section,
            limit,
            offset,
            since,
            after,
        })
    }
}

fn decode_cursor(value: &str) -> Option<Position> {
    if !(1..=400).contains(&value.len()) {
        return None;
    }
    let bytes = URL_SAFE
        .decode(value.replace('+', "-").replace('/', "_"))
        .ok()?;
    let [stamp, key]: [String; 2] = serde_json::from_slice(&bytes).ok()?;
    if !(1..=256).contains(&key.chars().count()) || stamp.chars().count() > 40 {
        return None;
    }
    // Keep the original sort key. The query cutoff is the only normalized timestamp.
    let event_time = if stamp.is_empty() {
        None
    } else {
        normalize_utc_stamp(&stamp)?;
        Some(stamp)
    };
    Some(Position { event_time, key })
}

pub fn read(
    directory: &Path,
    query: &str,
    codex: bool,
    excluded_sources: &[&str],
    switches: crate::sessions::McpSwitches,
    errors_enabled: bool,
    sqlite_enabled: bool,
    skills_enabled: bool,
) -> Result<HistoryResponse, HistoryError> {
    let query = Query::parse(query, codex).ok_or(HistoryError::Invalid)?;
    if (query.section == Section::Errors && !errors_enabled)
        || (query.section == Section::Sql && !sqlite_enabled)
        || (query.section == Section::Skills && !skills_enabled)
        || (query.section == Section::Mcp && !switches.mcp)
        || (query.section == Section::Web && !switches.web)
    {
        return Err(HistoryError::Invalid);
    }
    let store = Store::open(directory, Access::ReadOnly).map_err(|_| HistoryError::Unavailable)?;
    let page = match store {
        Some(mut store) => store
            .page(PageQuery {
                section: query.section,
                limit: query.limit,
                offset: query.offset,
                since: query.since.as_deref(),
                after: query.after.as_ref(),
                excluded_sources: if query.section == Section::Mcp {
                    excluded_sources
                } else {
                    &[]
                },
            })
            .map_err(|_| HistoryError::Unavailable)?,
        None => Page {
            items: Vec::new(),
            total: 0,
            next: None,
        },
    };
    let items = page
        .items
        .into_iter()
        .map(|row| {
            if !matches!(query.section, Section::Errors | Section::Mcp) {
                return RawValue::from_string(row.payload);
            }
            let mut value: serde_json::Value = serde_json::from_str(&row.payload)?;
            if query.section == Section::Errors {
                lam_history_store::sanitize_error_identifiers(&mut value);
            } else {
                lam_history_store::mcp::sanitize_mcp_resources(&mut value);
            }
            RawValue::from_string(serde_json::to_string(&value)?)
        })
        .collect::<serde_json::Result<Vec<_>>>()
        .map_err(|_| HistoryError::Invalid)?;
    let next_cursor = page
        .next
        .map(|position| {
            let cursor = [position.event_time.unwrap_or_default(), position.key];
            serde_json::to_vec(&cursor).map(|bytes| URL_SAFE.encode(bytes))
        })
        .transpose()
        .map_err(|_| HistoryError::Unavailable)?;
    Ok(HistoryResponse {
        items,
        total: page.total,
        limit: query.limit,
        offset: query.offset,
        next_cursor,
    })
}
