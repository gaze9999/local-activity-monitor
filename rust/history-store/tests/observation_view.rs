use chrono::{DateTime, Duration, Utc};
use lam_history_store::{
    parse_utc_stamp, Access, Cursor, HistoryWrite, Namespace, ObservationBatch, ObservationStore,
    ObservationWrite, PageQuery, Section, Store,
};
use serde_json::json;
use std::sync::Arc;
use tempfile::tempdir;

fn windows(hour: &str) -> [Option<DateTime<Utc>>; 4] {
    [
        parse_utc_stamp(hour),
        parse_utc_stamp("2026-11-11T10:00:00Z"),
        parse_utc_stamp("2026-11-05T10:00:00Z"),
        None,
    ]
}

#[test]
fn view_reuses_stable_windows_and_invalidates_on_real_changes() {
    let directory = tempdir().unwrap();
    let now = parse_utc_stamp("2026-11-12T10:00:00Z").unwrap();
    let mut writer = Store::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    writer.save_history(Namespace::Activity, r#"{"version":3,"retention_days":90}"#, 1, None, &[
        HistoryWrite {
            section: Section::Sql, key: "boundary", ordinal: 0, event_time: Some("2026-11-12T09:30:00.000Z"),
            payload: r#"{"timestamp":"2026-11-12T09:30:00.000Z","engine":"SQLite","operation":"read","statement":"SELECT","recognition":"recorded_code"}"#,
        },
        HistoryWrite {
            section: Section::Mcp, key: "stable", ordinal: 0, event_time: Some("2026-11-12T09:50:00.000Z"),
            payload: r#"{"timestamp":"2026-11-12T09:50:00.000Z","server":"fixture","tool":"read","nested":true}"#,
        },
    ]).unwrap();
    let mut store = ObservationStore::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let initial = store.read(now, &windows("2026-11-12T09:00:00Z")).unwrap();
    assert_eq!(initial.sql[0].total, 1);
    assert_eq!(initial.mcp[0].servers[0].1.nested, 1);
    assert_eq!(initial.events.len(), 2);

    let cursor = Cursor {
        source_key: "demo".into(),
        offset: 1,
        signature: "fixture".into(),
        payload: "{}".into(),
    };
    store
        .save_observation_batch(
            std::slice::from_ref(&cursor),
            &[],
            &[],
            &[],
            &[],
            ObservationBatch {
                rows: &[],
                errors: &[],
                now,
            },
        )
        .unwrap();
    let same = store
        .read(
            now + Duration::minutes(10),
            &windows("2026-11-12T09:10:00Z"),
        )
        .unwrap();
    assert!(Arc::ptr_eq(&initial.sql[0], &same.sql[0]));
    assert!(Arc::ptr_eq(&initial.mcp[0], &same.mcp[0]));
    assert!(Arc::ptr_eq(&initial.events, &same.events));

    let crossed = store
        .read(
            now + Duration::minutes(31),
            &windows("2026-11-12T09:31:00Z"),
        )
        .unwrap();
    assert_eq!(crossed.sql[0].total, 0);
    assert_eq!(crossed.sql[3].total, 1);
    assert!(Arc::ptr_eq(&same.mcp[0], &crossed.mcp[0]));

    let event = ObservationWrite {
        source_key: "demo".into(),
        event: json!({
            "timestamp":"2026-11-12T09:55:00.000Z","engine":"SQLite","operation":"write",
            "statement":"UPDATE","recognition":"recorded_code","call_id":"update"
        }),
    };
    store
        .save_observation_batch(
            &[cursor],
            &[],
            &[],
            &[],
            &[],
            ObservationBatch {
                errors: &[],
                rows: &[event],
                now,
            },
        )
        .unwrap();
    let own_write = store
        .read(
            now + Duration::minutes(31),
            &windows("2026-11-12T09:31:00Z"),
        )
        .unwrap();
    assert_eq!(own_write.sql[0].total, 1);
    assert_eq!(own_write.sql[3].total, 2);

    writer
        .save_history(
            Namespace::Activity,
            r#"{"version":3,"retention_days":90}"#,
            1,
            None,
            &[HistoryWrite {
                section: Section::Mcp,
                key: "external",
                ordinal: 0,
                event_time: None,
                payload: r#"{"server":"fixture","tool":"inspect","nested":false}"#,
            }],
        )
        .unwrap();
    let external = store
        .read(
            now + Duration::minutes(31),
            &windows("2026-11-12T09:31:00Z"),
        )
        .unwrap();
    assert_eq!(external.mcp[0].total, 1);
    assert_eq!(external.mcp[3].total, 2);
    assert_eq!(external.mcp[3].undated_total(), 1);
    assert_eq!(external.events.len(), 3);
}

#[test]
fn error_preview_is_recent_while_history_follows_retention() {
    let now = parse_utc_stamp("2026-11-12T10:00:00Z").unwrap();
    for (days, expected) in [(90, 2), (0, 3)] {
        let directory = tempdir().unwrap();
        let mut store = Store::open(directory.path(), Access::ReadWrite)
            .unwrap()
            .unwrap();
        store
            .save_history(
                Namespace::Activity,
                &format!(r#"{{"version":3,"retention_days":{days}}}"#),
                0,
                None,
                &[],
            )
            .unwrap();
        store.save_history(Namespace::Errors, r#"{"version":1}"#, 0, None, &[HistoryWrite {
            section: Section::Errors, key: "expired", ordinal: 0,
            event_time: Some("2026-01-01T00:00:00.000Z"),
            payload: r#"{"timestamp":"2026-01-01T00:00:00.000Z","source":"codex_core","category":"runtime","severity":"error"}"#,
        }]).unwrap();
        let mut observations = ObservationStore::open(directory.path(), Access::ReadWrite)
            .unwrap()
            .unwrap();
        let errors = [
            json!({"timestamp":"2026-11-10T09:00:00Z","source":"codex_core","category":"runtime","severity":"warning","record_id":1}),
            json!({"timestamp":"2026-11-12T09:00:00Z","source":"codex_core","category":"runtime","severity":"error","record_id":2}),
        ];
        observations.save_diagnostics(&[], &errors, now).unwrap();
        let preview = observations.error_preview(now).unwrap();
        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0]["record_id"], 2);
        assert_eq!(
            store
                .page(PageQuery {
                    section: Section::Errors,
                    limit: 10,
                    offset: 0,
                    since: None,
                    after: None,
                    excluded_sources: &[],
                })
                .unwrap()
                .total,
            expected
        );
    }
}

#[test]
fn tool_failure_projects_fields_without_retaining_response() {
    use lam_history_store::mcp::{output_error, output_failed};
    use serde_json::Map;

    let input = json!({"isError":true,"error":{"code":"fixture_failure",
        "message":"PRIVATE_BODY","request_id":"req-safe","trace_id":"sk-PRIVATE","status_code":429}});
    let projected = output_error(&input, |_| Map::new()).unwrap();
    assert_eq!(projected["code"], "fixture_failure");
    assert_eq!(projected["request_id"], "req-safe");
    assert_eq!(projected["http_status"], 429);
    assert!(!json!(projected).to_string().contains("PRIVATE"));
    assert!(output_failed(&input));
    let process = json!("Process exited with code 7\n");
    assert_eq!(
        output_error(&process, |_| Map::new()).unwrap()["code"],
        "process_exit"
    );
    assert!(!output_failed(&json!({"exit_code":0,"status":"ok"})));
    let protected = json!({"isError":true,"error":{"code":"sk-PRIVATE"}});
    assert_eq!(
        output_error(&protected, |_| Map::new()).unwrap()["code"],
        "tool_error"
    );
    let oversized_json = json!({"status":"failed","padding":"x".repeat(1024 * 1024)}).to_string();
    assert!(!output_failed(&json!(oversized_json)));
    let late_exit = format!("{}\nProcess exited with code 7\n", "x".repeat(1024 * 1024));
    assert!(!output_failed(&json!(late_exit)));
    let unicode_json = json!({"status":"failed","padding":"漢".repeat(512 * 1024)}).to_string();
    assert!(output_failed(&json!(unicode_json)));
}
