use base64::{engine::general_purpose::URL_SAFE, Engine};
use lam_history_store::{normalize_utc_stamp, Access, Page, PageQuery, Position, Section, Store};
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::HashMap;
use std::path::Path;

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
        let number = |key, default| {
            u32::try_from(get(key).unwrap_or(default).trim().parse::<i64>().ok()?).ok()
        };
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

pub fn read(directory: &Path, query: &str, codex: bool) -> Result<HistoryResponse, HistoryError> {
    let query = Query::parse(query, codex).ok_or(HistoryError::Invalid)?;
    let store = Store::open(directory, Access::ReadOnly).map_err(|_| HistoryError::Unavailable)?;
    let page = match store {
        Some(mut store) => store
            .page(PageQuery {
                section: query.section,
                limit: query.limit,
                offset: query.offset,
                since: query.since.as_deref(),
                after: query.after.as_ref(),
                excluded_sources: &[],
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
