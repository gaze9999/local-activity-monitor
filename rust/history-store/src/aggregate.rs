use rusqlite::{params_from_iter, types::Value, Connection};

use crate::history::Filter;
use crate::{Result, Section, Store, StoreError};

const EPOCH: &str = "CAST(strftime('%s',event_time) AS INTEGER)";
const CLASS_LIMIT: usize = 200;
const SERIES_LIMIT: usize = 1440;
const GROUP_LIMIT: usize = 14_400;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallCounts {
    pub total: i64,
    pub nested: i64,
}

impl CallCounts {
    pub fn direct(&self) -> i64 {
        self.total - self.nested
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub epoch_seconds: i64,
    pub calls: CallCounts,
    pub operations: Vec<(String, i64)>,
    pub servers: Vec<(String, CallCounts)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aggregate {
    pub total: i64,
    pub dated_total: i64,
    pub operations: Vec<(String, i64)>,
    pub statements: Vec<(String, i64)>,
    pub servers: Vec<(String, CallCounts)>,
    pub series: Vec<Bucket>,
    pub bucket_seconds: i64,
    pub classification_truncated: bool,
    pub classification_series_truncated: Option<bool>,
}

impl Aggregate {
    pub fn undated_total(&self) -> i64 {
        self.total - self.dated_total
    }
}

impl Default for Aggregate {
    fn default() -> Self {
        Self {
            total: 0,
            dated_total: 0,
            operations: Vec::new(),
            statements: Vec::new(),
            servers: Vec::new(),
            series: Vec::new(),
            bucket_seconds: 60,
            classification_truncated: false,
            classification_series_truncated: None,
        }
    }
}

impl Store {
    /// Query one to sixteen normalized UTC boundaries within the same read snapshot.
    pub fn aggregate_many(
        &mut self,
        section: Section,
        boundaries: &[Option<&str>],
        excluded_sources: &[&str],
    ) -> Result<Vec<Aggregate>> {
        if !(1..=16).contains(&boundaries.len()) {
            return Err(StoreError::InvalidQuery);
        }
        let filters = boundaries
            .iter()
            .map(|boundary| Filter::new(section, *boundary, excluded_sources))
            .collect::<Result<Vec<_>>>()?;
        let tx = self.db.transaction()?;
        let values = filters
            .iter()
            .map(|filter| aggregate(&tx, filter))
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(values)
    }
}

pub(crate) fn aggregate(db: &Connection, filter: &Filter) -> rusqlite::Result<Aggregate> {
    let Filter { clause, args } = filter;
    let (total, dated, earliest, latest): (i64, i64, Option<i64>, Option<i64>) = db.query_row(
        &format!("SELECT count(*), count({EPOCH}), min({EPOCH}), max({EPOCH}){clause}"),
        params_from_iter(args),
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let mut result = Aggregate {
        total,
        dated_total: dated,
        ..Aggregate::default()
    };
    if total == 0 {
        return Ok(result);
    }

    let classify = |column| {
        let mut statement = db.prepare(&format!(
            "SELECT CAST({column} AS TEXT), count(*){clause} AND {column} IS NOT NULL
             GROUP BY {column} ORDER BY count(*) DESC, {column} LIMIT {}",
            CLASS_LIMIT + 1
        ))?;
        let rows =
            statement.query_map(params_from_iter(args), |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    };
    result.operations = classify("operation")?;
    result.statements = classify("statement")?;
    {
        let mut statement = db.prepare(&format!(
            "SELECT CAST(server AS TEXT),count(*),coalesce(sum(nested),0){clause} AND server IS NOT NULL
             GROUP BY server ORDER BY count(*) DESC,server LIMIT {}",
            CLASS_LIMIT + 1
        ))?;
        let rows = statement.query_map(params_from_iter(args), |row| {
            Ok((
                row.get(0)?,
                CallCounts {
                    total: row.get(1)?,
                    nested: row.get(2)?,
                },
            ))
        })?;
        result.servers = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    }
    result.classification_truncated = [
        result.operations.len(),
        result.statements.len(),
        result.servers.len(),
    ]
    .into_iter()
    .any(|size| size > CLASS_LIMIT);
    result.operations.truncate(CLASS_LIMIT);
    result.statements.truncate(CLASS_LIMIT);
    result.servers.truncate(CLASS_LIMIT);

    if let (Some(first), Some(last)) = (earliest, latest) {
        let minutes = last.div_euclid(60) - first.div_euclid(60) + 1;
        let bucket = ((minutes + 1438) / 1439).max(1) * 60;
        result.bucket_seconds = bucket;
        result.classification_series_truncated =
            Some(load_series(db, filter, bucket, &mut result.series)?);
    }
    Ok(result)
}

fn load_series(
    db: &Connection,
    filter: &Filter,
    bucket: i64,
    output: &mut Vec<Bucket>,
) -> rusqlite::Result<bool> {
    let Filter { clause, args } = filter;
    let alignment = [Value::Integer(bucket), Value::Integer(bucket)];
    let mut statement = db.prepare(&format!(
        "SELECT ({EPOCH}/?)*?,count(*),coalesce(sum(nested),0){clause} AND {EPOCH} IS NOT NULL
         GROUP BY 1 ORDER BY 1 LIMIT {SERIES_LIMIT}"
    ))?;
    let rows = statement.query_map(params_from_iter(alignment.iter().chain(args)), |row| {
        Ok(Bucket {
            epoch_seconds: row.get(0)?,
            calls: CallCounts {
                total: row.get(1)?,
                nested: row.get(2)?,
            },
            operations: Vec::new(),
            servers: Vec::new(),
        })
    })?;
    *output = rows.collect::<rusqlite::Result<Vec<_>>>()?;

    let mut truncated = false;
    for field in ["operation", "server"] {
        let mut statement = db.prepare(&format!(
            "SELECT ({EPOCH}/?)*?, CAST({field} AS TEXT), count(*),coalesce(sum(nested),0){clause}
             AND {EPOCH} IS NOT NULL AND {field} IS NOT NULL GROUP BY 1,{field}
             ORDER BY 1,{field} LIMIT {}",
            GROUP_LIMIT + 1
        ))?;
        let mut rows = statement.query(params_from_iter(alignment.iter().chain(args)))?;
        for _ in 0..GROUP_LIMIT {
            let Some(row) = rows.next()? else { break };
            let stamp: i64 = row.get(0)?;
            let index = output.binary_search_by_key(&stamp, |entry| entry.epoch_seconds);
            if let Ok(index) = index {
                let name = row.get(1)?;
                let total = row.get(2)?;
                if field == "server" {
                    output[index].servers.push((
                        name,
                        CallCounts {
                            total,
                            nested: row.get(3)?,
                        },
                    ));
                } else {
                    output[index].operations.push((name, total));
                }
            }
        }
        truncated |= rows.next()?.is_some();
    }
    Ok(truncated)
}
