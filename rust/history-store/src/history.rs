use rusqlite::{params, params_from_iter, types::Value, Transaction, TransactionBehavior};

use crate::{Result, Store, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    Activity,
    Errors,
    Threads,
}

impl Namespace {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Activity => "activity",
            Self::Errors => "errors",
            Self::Threads => "threads",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Sql,
    Web,
    Mcp,
    Errors,
    Threads,
    Skills,
}

impl Section {
    pub(crate) fn names(self) -> (Namespace, &'static str) {
        match self {
            Self::Sql => (Namespace::Activity, "sql"),
            Self::Web => (Namespace::Activity, "web"),
            Self::Mcp => (Namespace::Activity, "mcp"),
            Self::Errors => (Namespace::Errors, "events"),
            Self::Threads => (Namespace::Threads, "entries"),
            Self::Skills => (Namespace::Threads, "skills"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub event_time: Option<String>,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryItem {
    pub position: Position,
    pub payload: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<HistoryItem>,
    pub total: i64,
    pub next: Option<Position>,
}

pub struct PageQuery<'a> {
    pub section: Section,
    pub limit: u32,
    pub offset: u32,
    pub since: Option<&'a str>,
    pub after: Option<&'a Position>,
    pub excluded_sources: &'a [&'a str],
}

pub struct HistoryWrite<'a> {
    pub section: Section,
    pub key: &'a str,
    pub ordinal: i64,
    pub event_time: Option<&'a str>,
    pub payload: &'a str,
}

pub(crate) struct Filter {
    pub clause: String,
    pub args: Vec<Value>,
}

impl Filter {
    pub fn new(section: Section, since: Option<&str>, excluded_sources: &[&str]) -> Result<Self> {
        if excluded_sources.len() > 64
            || excluded_sources
                .iter()
                .any(|source| !(1..=80).contains(&source.chars().count()))
        {
            return Err(StoreError::InvalidQuery);
        }
        let (namespace, section) = section.names();
        let mut filter = String::from(" FROM history_items WHERE namespace=? AND section=?");
        let mut args = vec![
            Value::Text(namespace.name().into()),
            Value::Text(section.into()),
        ];
        if !excluded_sources.is_empty() {
            let placeholders = vec!["?"; excluded_sources.len()].join(",");
            filter.push_str(&format!(
                " AND (server IS NULL OR server NOT IN ({placeholders}))"
            ));
            args.extend(
                excluded_sources
                    .iter()
                    .map(|source| Value::Text((*source).into())),
            );
        }
        if let Some(since) = since {
            filter.push_str(" AND event_time>=?");
            args.push(Value::Text(since.into()));
        }
        Ok(Self {
            clause: filter,
            args,
        })
    }
}

impl Store {
    /// Atomically persist projected and merged metadata using the caller's stable keys.
    pub fn save_history(
        &mut self,
        namespace: Namespace,
        metadata: &str,
        updated_at: i64,
        boundary: Option<&str>,
        rows: &[HistoryWrite<'_>],
    ) -> Result<bool> {
        if rows.iter().any(|row| row.section.names().0 != namespace) {
            return Err(StoreError::InvalidQuery);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = write_history(&tx, namespace, metadata, updated_at, boundary, rows)?;
        tx.commit()?;
        Ok(changed)
    }

    /// Read the count and page in one snapshot, with undated rows ordered last.
    pub fn page(&mut self, query: PageQuery<'_>) -> Result<Page> {
        if !(1..=200).contains(&query.limit)
            || query.offset > 1_000_000
            || query.after.is_some_and(|position| {
                query.offset != 0 || !(1..=256).contains(&position.key.chars().count())
            })
        {
            return Err(StoreError::InvalidQuery);
        }
        let Filter {
            clause: filter,
            args,
        } = Filter::new(query.section, query.since, query.excluded_sources)?;
        let tx = self.db.transaction()?;
        let total = tx.query_row(
            &format!("SELECT count(*){filter}"),
            params_from_iter(&args),
            |row| row.get(0),
        )?;
        let columns = format!("SELECT payload,event_time,item_key{filter}");
        let order = " ORDER BY event_time DESC, item_key DESC LIMIT ?";
        let limit = i64::from(query.limit) + 1;
        let read = |suffix: &str, extra: Vec<Value>| -> rusqlite::Result<Vec<HistoryItem>> {
            let mut statement = tx.prepare(&format!("{columns}{suffix}"))?;
            let rows =
                statement.query_map(params_from_iter(args.iter().chain(extra.iter())), |row| {
                    Ok(HistoryItem {
                        payload: row.get(0)?,
                        position: Position {
                            event_time: row.get(1)?,
                            key: row.get(2)?,
                        },
                    })
                })?;
            rows.collect()
        };
        let mut items = match query.after {
            None => read(
                &format!("{order} OFFSET ?"),
                vec![limit.into(), i64::from(query.offset).into()],
            )?,
            Some(after) if after.event_time.is_none() => read(
                &format!(" AND event_time IS NULL AND item_key<?{order}"),
                vec![after.key.clone().into(), limit.into()],
            )?,
            Some(after) => {
                let mut items = read(
                    &format!(" AND (event_time, item_key) < (?, ?){order}"),
                    vec![
                        after.event_time.clone().into(),
                        after.key.clone().into(),
                        limit.into(),
                    ],
                )?;
                let remaining = limit - items.len() as i64;
                if query.since.is_none() && remaining > 0 {
                    items.extend(read(
                        &format!(" AND event_time IS NULL{order}"),
                        vec![remaining.into()],
                    )?);
                }
                items
            }
        };
        tx.commit()?;
        let more = items.len() > query.limit as usize;
        items.truncate(query.limit as usize);
        let next = items
            .last()
            .filter(|_| more)
            .map(|item| item.position.clone());
        Ok(Page { items, total, next })
    }

    /// Delete at most `batch` expired dated rows across namespaces, excluding threads.
    pub fn prune(&mut self, boundary: &str, batch: u32) -> Result<usize> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = prune_batch(&tx, boundary, batch)?;
        tx.commit()?;
        Ok(removed)
    }
}

pub(crate) fn prune_batch(tx: &Transaction<'_>, boundary: &str, batch: u32) -> Result<usize> {
    Ok(tx.execute(
        "DELETE FROM history_items WHERE rowid IN (
                SELECT rowid FROM history_items WHERE event_time<? AND section!='entries'
                ORDER BY event_time LIMIT ?
             )",
        params![boundary, batch],
    )?)
}

pub(crate) fn write_history(
    tx: &Transaction<'_>,
    namespace: Namespace,
    metadata: &str,
    updated_at: i64,
    boundary: Option<&str>,
    rows: &[HistoryWrite<'_>],
) -> Result<bool> {
    let mut changed = tx.execute(
            "INSERT INTO history_state(namespace, metadata, updated_at) VALUES (?,?,?)
             ON CONFLICT(namespace) DO UPDATE SET metadata=excluded.metadata, updated_at=excluded.updated_at
             WHERE history_state.metadata!=excluded.metadata",
            params![namespace.name(), metadata, updated_at],
        )? > 0;
    {
        let mut insert = tx.prepare(
                "INSERT INTO history_items(namespace, section, item_key, ordinal, event_time, payload,
                 server, operation, statement, nested)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, json_extract(?6,'$.server'), json_extract(?6,'$.operation'),
                         json_extract(?6,'$.statement'), CASE WHEN json_extract(?6,'$.nested')=1 THEN 1 ELSE 0 END)
                 ON CONFLICT(namespace,section,item_key) DO UPDATE SET event_time=excluded.event_time,
                 payload=excluded.payload, server=excluded.server, operation=excluded.operation,
                 statement=excluded.statement, nested=excluded.nested WHERE history_items.payload!=excluded.payload",
            )?;
        for row in rows {
            if row.section != Section::Threads
                && boundary
                    .zip(row.event_time)
                    .is_some_and(|(since, time)| time < since)
            {
                continue;
            }
            changed |= insert.execute(params![
                namespace.name(),
                row.section.names().1,
                row.key,
                row.ordinal,
                row.event_time,
                row.payload
            ])? > 0;
        }
    }
    Ok(changed)
}
