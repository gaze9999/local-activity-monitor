use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

use super::{statements, Kind};

#[derive(Debug, Serialize)]
pub struct SqlDiagnostic {
    pub statement: String,
    pub operation: Kind,
    pub engine: &'static str,
    pub duration_ms: Option<f64>,
    pub rows_affected: Option<i64>,
    pub rows_returned: Option<i64>,
}

static MODULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:sqlx|sqlite|rusqlite|state_db|database)(?:[:._-]|$)")
        .expect("module pattern")
});
static SQLITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)sqlite|rusqlite|state_db").expect("SQLite pattern"));
static STATEMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\b(?:summary|statement|query|sql)="((?:\\.|[^"\\]){0,4096})""#)
        .expect("diagnostic statement")
});
static QUOTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""(?:\\.|[^"\\])*"|'(?:''|[^'])*'"#).expect("quoted values"));
static SECONDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\belapsed_secs=([0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?)\b")
        .expect("duration seconds")
});
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:elapsed|duration)=([0-9]+(?:\.[0-9]+)?)(ns|us|µs|ms|s)\b").expect("duration")
});
static ROWS_AFFECTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\brows_affected=([0-9]{1,19})\b").expect("rows affected"));
static ROWS_RETURNED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\brows_returned=([0-9]{1,19})\b").expect("rows returned"));

pub fn project_diagnostic(module: &str, body: &str) -> Option<SqlDiagnostic> {
    if !MODULE.is_match(module) {
        return None;
    }
    let body = &body[..body
        .char_indices()
        .nth(8192)
        .map_or(body.len(), |(index, _)| index)];
    let sql = STATEMENT
        .captures(body)
        .and_then(|captures| captures.get(1))
        .map_or_else(
            || body.split('\n').next().unwrap_or_default(),
            |text| text.as_str(),
        );
    let (statement, operation) = statements(sql, None).into_iter().next().map_or_else(
        || ("DATABASE_EVENT".into(), Kind::Diagnostic),
        |operation| (operation.statement, operation.kind),
    );
    let clean = QUOTED.replace_all(body, "");
    let rows = |pattern: &Regex| {
        pattern
            .captures(&clean)
            .and_then(|capture| capture[1].parse::<i64>().ok())
    };
    Some(SqlDiagnostic {
        statement,
        operation,
        engine: if SQLITE.is_match(module) {
            "SQLite"
        } else {
            "SQL"
        },
        duration_ms: duration(&clean),
        rows_affected: rows(&ROWS_AFFECTED),
        rows_returned: rows(&ROWS_RETURNED),
    })
}

fn duration(text: &str) -> Option<f64> {
    let value = if let Some(seconds) = SECONDS.captures(text) {
        seconds[1].parse::<f64>().ok()? * 1000.0
    } else {
        let elapsed = DURATION.captures(text)?;
        let unit = match &elapsed[2] {
            "ns" => 0.000001,
            "us" | "µs" => 0.001,
            "ms" => 1.0,
            "s" => 1000.0,
            _ => return None,
        };
        elapsed[1].parse::<f64>().ok()? * unit
    };
    (0.0..=86_400_000.0)
        .contains(&value)
        .then(|| (value * 1_000_000.0).round_ties_even() / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_selects_typed_numbers_outside_quoted_fields() {
        let value = project_diagnostic("sqlx::query",
            r#"summary="SELECT PRIVATE FROM secret" elapsed=241µs rows_affected=0 rows_returned=3 password="PRIVATE elapsed=900ms""#
        ).unwrap();
        assert_eq!(value.statement, "SELECT");
        assert_eq!(value.operation, Kind::Read);
        assert_eq!(value.duration_ms, Some(0.241));
        assert_eq!(value.rows_affected, Some(0));
        assert_eq!(value.rows_returned, Some(3));
        assert!(!serde_json::to_string(&value).unwrap().contains("PRIVATE"));
        assert!(project_diagnostic("codex_core", "elapsed=1ms").is_none());
    }

    #[test]
    fn diagnostic_rejects_overflow_and_uses_known_units() {
        let value = project_diagnostic(
            "codex_state::state_db",
            "unknown elapsed_secs=0.00125 rows_returned=9999999999999999999",
        )
        .unwrap();
        assert_eq!(value.statement, "DATABASE_EVENT");
        assert_eq!(value.engine, "SQLite");
        assert_eq!(value.duration_ms, Some(1.25));
        assert_eq!(value.rows_returned, None);
        assert_eq!(
            project_diagnostic("sqlite", "duration=999999999999999999s")
                .unwrap()
                .duration_ms,
            None
        );
    }
}
