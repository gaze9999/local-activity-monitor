use lam_history_store::{
    Access, Cursor, ExpectedCursor, HistoryWrite, Namespace, PageQuery, Section, Store, StoreError,
    StoredCall, APPLICATION_ID, FILE_NAME, SCHEMA_VERSION,
};
use rusqlite::{params, Connection};
use tempfile::tempdir;

const RECENT: &str = "2026-10-10T00:00:00.000Z";
const OLD: &str = "2026-10-01T00:00:00.000Z";

fn row<'a>(
    section: Section,
    key: &'a str,
    stamp: Option<&'a str>,
    payload: &'a str,
) -> HistoryWrite<'a> {
    HistoryWrite {
        section,
        key,
        ordinal: 0,
        event_time: stamp,
        payload,
    }
}

fn query(section: Section, limit: u32) -> PageQuery<'static> {
    PageQuery {
        section,
        limit,
        offset: 0,
        since: None,
        after: None,
        excluded_sources: &[],
    }
}

#[test]
fn fresh_store_matches_existing_schema_and_enforces_read_only() {
    let temp = tempdir().unwrap();
    let root = temp.path().join("project with spaces");
    assert!(Store::open(&root, Access::ReadOnly).unwrap().is_none());
    assert!(!root.exists());
    let mut writer = Store::open(&root, Access::ReadWrite).unwrap().unwrap();
    writer
        .save_history(Namespace::Activity, "{}", 1, None, &[])
        .unwrap();
    let db = Connection::open(root.join(FILE_NAME)).unwrap();
    let header = db.query_row(
        "SELECT application_id, user_version FROM pragma_application_id(), pragma_user_version()",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    ).unwrap();
    assert_eq!(header, (APPLICATION_ID, SCHEMA_VERSION));
    assert_eq!(
        db.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            root.join(FILE_NAME)
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(root.metadata().unwrap().permissions().mode() & 0o777, 0o700);
    }
    let mut reader = Store::open(&root, Access::ReadOnly).unwrap().unwrap();
    assert_eq!(reader.page(query(Section::Sql, 1)).unwrap().total, 0);
    assert!(matches!(reader.prune(RECENT, 1), Err(StoreError::Sql(_))));
}

#[test]
fn page_preserves_ties_nulls_and_classification_filters() {
    let temp = tempdir().unwrap();
    let mut store = Store::open(temp.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let rows = [
        row(
            Section::Mcp,
            "a",
            Some(RECENT),
            r#"{"server":"visible","nested":true}"#,
        ),
        row(Section::Mcp, "b", Some(RECENT), r#"{"server":"visible"}"#),
        row(Section::Mcp, "c", None, "{}"),
        row(Section::Mcp, "d", Some(RECENT), r#"{"server":"hidden"}"#),
        row(Section::Mcp, "e", None, r#"{"server":"visible"}"#),
    ];
    assert!(store
        .save_history(Namespace::Activity, "{}", 1, None, &rows)
        .unwrap());
    assert!(!store
        .save_history(Namespace::Activity, "{}", 2, None, &rows)
        .unwrap());
    let mut cursor = None;
    let mut seen = Vec::new();
    loop {
        let page = store
            .page(PageQuery {
                section: Section::Mcp,
                limit: 2,
                offset: 0,
                since: None,
                after: cursor.as_ref(),
                excluded_sources: &["hidden"],
            })
            .unwrap();
        assert_eq!(page.total, 4);
        seen.extend(page.items.into_iter().map(|item| item.position.key));
        cursor = page.next;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(seen, ["b", "a", "e", "c"]);
    let dated = store
        .page(PageQuery {
            since: Some(RECENT),
            ..query(Section::Mcp, 200)
        })
        .unwrap();
    assert_eq!(dated.total, 3);
    assert!(matches!(
        store.page(query(Section::Mcp, 201)),
        Err(StoreError::InvalidQuery)
    ));
    let db = Connection::open(temp.path().join(FILE_NAME)).unwrap();
    let classified = db
        .query_row(
            "SELECT server,nested FROM history_items WHERE item_key='a'",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .unwrap();
    assert_eq!(classified, ("visible".into(), 1));
}

#[test]
fn aggregate_counts_windows_and_widens_buckets_without_losing_rows() {
    let temp = tempdir().unwrap();
    let mut store = Store::open(temp.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let names = (0..201)
        .map(|index| format!("server-{index:03}"))
        .collect::<Vec<_>>();
    let payloads = names
        .iter()
        .map(|name| {
            format!(
                r#"{{"server":"{name}","operation":"inspect","statement":"SELECT","nested":true}}"#
            )
        })
        .collect::<Vec<_>>();
    let mut rows = names
        .iter()
        .zip(&payloads)
        .map(|(name, payload)| row(Section::Mcp, name, Some(RECENT), payload))
        .collect::<Vec<_>>();
    rows.push(row(
        Section::Mcp,
        "old",
        Some(OLD),
        r#"{"server":"server-000","operation":"inspect"}"#,
    ));
    rows.push(row(Section::Mcp, "undated", None, "{}"));
    store
        .save_history(Namespace::Activity, "{}", 1, None, &rows)
        .unwrap();

    let totals = store
        .aggregate_many(Section::Mcp, &[None, Some(RECENT)], &[])
        .unwrap();
    let [all, recent] = totals.as_slice() else {
        panic!("expected two windows");
    };
    assert_eq!(
        (all.total, all.dated_total, all.undated_total()),
        (203, 202, 1)
    );
    assert_eq!(all.bucket_seconds, 600);
    assert_eq!(
        all.series
            .iter()
            .map(|bucket| bucket.calls.total)
            .sum::<i64>(),
        202
    );
    assert_eq!(all.operations, [("inspect".into(), 202)]);
    assert_eq!(all.statements, [("SELECT".into(), 201)]);
    assert_eq!(all.servers[0].0, "server-000");
    assert_eq!((all.servers[0].1.total, all.servers[0].1.direct()), (2, 1));
    assert_eq!(all.servers.len(), 200);
    assert!(all.classification_truncated);
    assert_eq!(all.classification_series_truncated, Some(false));
    assert_eq!(recent.total, 201);
    assert_eq!(recent.bucket_seconds, 60);
    assert_eq!(recent.series[0].operations, [("inspect".into(), 201)]);
    let filtered = store
        .aggregate_many(Section::Mcp, &[None], &["server-000"])
        .unwrap();
    assert_eq!(filtered[0].total, 201);
    assert!(!filtered[0].classification_truncated);
    assert!(matches!(
        store.aggregate_many(Section::Mcp, &[None; 17], &[]),
        Err(StoreError::InvalidQuery)
    ));
}

#[test]
fn bad_batch_rolls_back_metadata_and_rows_and_prune_keeps_threads() {
    let temp = tempdir().unwrap();
    let mut store = Store::open(temp.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let old = row(Section::Web, "old", Some(OLD), "{}");
    store
        .save_history(Namespace::Activity, r#"{"version":3}"#, 1, None, &[old])
        .unwrap();
    let invalid = [
        row(Section::Web, "new", Some(RECENT), "{}"),
        row(Section::Web, "bad", Some(RECENT), "not-json"),
    ];
    assert!(store
        .save_history(Namespace::Activity, r#"{"version":9}"#, 2, None, &invalid)
        .is_err());
    assert_eq!(store.page(query(Section::Web, 20)).unwrap().total, 1);
    let db = Connection::open(temp.path().join(FILE_NAME)).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT updated_at FROM history_state WHERE namespace='activity'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    store
        .save_history(
            Namespace::Threads,
            "{}",
            1,
            Some(RECENT),
            &[row(Section::Threads, "thread", Some(OLD), "{}")],
        )
        .unwrap();
    assert_eq!(store.prune(RECENT, 0).unwrap(), 0);
    assert_eq!(store.prune(RECENT, 1).unwrap(), 1);
    assert_eq!(store.page(query(Section::Web, 10)).unwrap().total, 0);
    assert_eq!(store.page(query(Section::Threads, 10)).unwrap().total, 1);
}

#[test]
fn checkpoint_upserts_retire_and_roll_back_together() {
    let temp = tempdir().unwrap();
    let mut store = Store::open(temp.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let mut cursor = Cursor {
        source_key: "source".into(),
        offset: 11,
        signature: "[1,2,\"hash\"]".into(),
        payload: r#"{"version":1,"state":{}}"#.into(),
    };
    let first = StoredCall {
        source_key: cursor.source_key.clone(),
        call_id: "first".into(),
        event_time: Some(OLD.into()),
        payload: r#"{"tool":"example"}"#.into(),
    };
    let second = StoredCall {
        call_id: "second".into(),
        event_time: Some(RECENT.into()),
        ..first.clone()
    };
    store
        .save_checkpoints(&[cursor.clone()], &[first.clone(), second.clone()], &[])
        .unwrap();
    assert_eq!(
        store
            .checkpoint("source", 1)
            .unwrap()
            .unwrap()
            .calls
            .as_slice(),
        std::slice::from_ref(&second)
    );
    assert!(store.checkpoint("missing", 10).unwrap().is_none());

    let db = Connection::open(temp.path().join(FILE_NAME)).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_call BEFORE INSERT ON session_calls
         WHEN NEW.call_id='bad' BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END",
    )
    .unwrap();
    cursor.offset = 22;
    let bad = StoredCall {
        call_id: "bad".into(),
        ..first.clone()
    };
    let retired = [("source".into(), "first".into())];
    assert!(store
        .save_checkpoints(&[cursor.clone()], &[bad], &retired)
        .is_err());
    let before = store.checkpoint("source", 10).unwrap().unwrap();
    assert_eq!(before.cursor.offset, 11);
    assert_eq!(before.calls, [second.clone(), first]);
    store.save_checkpoints(&[cursor], &[], &retired).unwrap();
    let after = store.checkpoint("source", 10).unwrap().unwrap();
    assert_eq!(after.cursor.offset, 22);
    assert_eq!(after.calls, [second]);
}

#[test]
fn existing_writer_lock_does_not_block_read_or_allow_a_second_write() {
    let temp = tempdir().unwrap();
    let mut store = Store::open(temp.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    store
        .save_history(
            Namespace::Activity,
            "{}",
            1,
            None,
            &[row(Section::Web, "committed", Some(RECENT), "{}")],
        )
        .unwrap();
    let db = Connection::open(temp.path().join(FILE_NAME)).unwrap();
    db.execute_batch("BEGIN IMMEDIATE; DELETE FROM history_items")
        .unwrap();
    let mut reader = Store::open(temp.path(), Access::ReadOnly).unwrap().unwrap();
    assert_eq!(reader.page(query(Section::Web, 10)).unwrap().total, 1);
    assert!(store.prune(RECENT, 1).is_err());
    db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(reader.page(query(Section::Web, 10)).unwrap().total, 1);
}

#[test]
fn foreign_and_future_schemas_are_preserved() {
    for (app, version) in [(0, 0), (APPLICATION_ID, 99)] {
        let temp = tempdir().unwrap();
        let db = Connection::open(temp.path().join(FILE_NAME)).unwrap();
        db.execute_batch("CREATE TABLE sentinel(value TEXT NOT NULL)")
            .unwrap();
        db.execute("INSERT INTO sentinel VALUES (?)", params!["keep"])
            .unwrap();
        db.pragma_update(None, "application_id", app).unwrap();
        db.pragma_update(None, "user_version", version).unwrap();
        for mode in [Access::ReadOnly, Access::ReadWrite] {
            assert!(matches!(
                Store::open(temp.path(), mode),
                Err(StoreError::UnsupportedSchema { .. })
            ));
        }
        assert_eq!(
            db.query_row("SELECT value FROM sentinel", [], |row| row
                .get::<_, String>(0))
                .unwrap(),
            "keep"
        );
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            version
        );
    }
}

#[cfg(unix)]
#[test]
fn redirected_database_directory_and_sidecars_are_rejected() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let owned = temp.path().join("owned");
    let redirected = temp.path().join("redirected");
    std::fs::create_dir(&owned).unwrap();
    symlink(&owned, &redirected).unwrap();
    assert!(matches!(
        Store::open(&redirected, Access::ReadWrite),
        Err(StoreError::UnsafePath)
    ));
    let target = temp.path().join("unowned");
    std::fs::write(&target, "sentinel").unwrap();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let entry = owned.join(format!("{FILE_NAME}{suffix}"));
        symlink(&target, &entry).unwrap();
        assert!(matches!(
            Store::open(&owned, Access::ReadWrite),
            Err(StoreError::UnsafePath)
        ));
        std::fs::remove_file(entry).unwrap();
    }
    assert_eq!(std::fs::read_to_string(target).unwrap(), "sentinel");
}

#[test]
fn guarded_checkpoint_batch_commits_other_keys_without_touching_conflicts() {
    let temp = tempdir().unwrap();
    let mut store = Store::open(temp.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let cursor = |key: &str, revision: &str, offset: i64| Cursor {
        source_key: key.into(),
        signature: revision.into(),
        offset,
        payload: "{}".into(),
    };
    let stored = |key: &str, call_id: &str| StoredCall {
        source_key: key.into(),
        call_id: call_id.into(),
        event_time: None,
        payload: "{}".into(),
    };
    store
        .save_checkpoints(
            &[cursor("changed", "committed", 9)],
            &[stored("changed", "keep")],
            &[],
        )
        .unwrap();
    let conflicts = store
        .save_checkpoint_batch(
            &[
                cursor("changed", "stale", 2),
                cursor("independent", "new", 1),
            ],
            &[stored("changed", "discard"), stored("independent", "save")],
            &[("changed".into(), "keep".into())],
            &["changed".into()],
            &[
                ExpectedCursor {
                    source_key: "changed",
                    signature: Some("previous"),
                },
                ExpectedCursor {
                    source_key: "independent",
                    signature: None,
                },
            ],
        )
        .unwrap();
    assert_eq!(conflicts.into_iter().collect::<Vec<_>>(), ["changed"]);
    let retained = store.checkpoint("changed", 10).unwrap().unwrap();
    assert_eq!(retained.cursor.offset, 9);
    assert_eq!(retained.calls, [stored("changed", "keep")]);
    let independent = store.checkpoint("independent", 10).unwrap().unwrap();
    assert_eq!(independent.cursor.offset, 1);
    assert_eq!(independent.calls, [stored("independent", "save")]);
}

#[test]
fn retention_updates_metadata_and_prunes_only_one_batch() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let db = Connection::open(directory.path().join(FILE_NAME)).unwrap();
    db.execute_batch(
        "INSERT INTO history_state VALUES ('activity','{\"version\":3,\"other\":true}',0);
         WITH RECURSIVE ids(n) AS (
             SELECT 1 UNION ALL SELECT n+1 FROM ids WHERE n<501
         )
         INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload)
         SELECT 'activity','web',n,0,'2026-10-01T00:00:00.000Z','{}' FROM ids;
         INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload)
         VALUES ('threads','entries','thread',0,'2026-10-01T00:00:00.000Z','{}');",
    )
    .unwrap();
    let now = RECENT.parse().unwrap();
    assert_eq!(store.set_activity_retention(0, now).unwrap(), 0);
    assert_eq!(store.set_activity_retention(1, now).unwrap(), 500);
    let remaining: i64 = db
        .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 2);
    assert_eq!(store.continue_retention_cleanup(2, now).unwrap(), 0);
    assert_eq!(store.continue_retention_cleanup(1, now).unwrap(), 1);
    let metadata: String = db
        .query_row("SELECT metadata FROM history_state", [], |row| row.get(0))
        .unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap();
    assert_eq!(
        metadata,
        serde_json::json!({"version":3,"other":true,"retention_days":1})
    );
}

#[test]
fn retention_change_rolls_back_when_cleanup_fails() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let db = Connection::open(directory.path().join(FILE_NAME)).unwrap();
    db.execute_batch(
        "INSERT INTO history_state VALUES ('activity','{\"version\":3,\"retention_days\":90}',0);
         INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload)
         VALUES ('activity','web','old',0,'2026-10-01T00:00:00.000Z','{}');
         CREATE TRIGGER reject_cleanup BEFORE DELETE ON history_items
         BEGIN SELECT RAISE(ABORT,'fixture cleanup failure'); END;",
    )
    .unwrap();
    let now = RECENT.parse().unwrap();
    assert!(store.set_activity_retention(1, now).is_err());
    assert_eq!(store.activity_retention().unwrap(), 90);
    let remaining: i64 = db
        .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
    db.execute_batch("DROP TRIGGER reject_cleanup").unwrap();
    assert_eq!(store.set_activity_retention(1, now).unwrap(), 1);
    assert_eq!(store.activity_retention().unwrap(), 1);
}

#[test]
fn aggregate_accepts_null_nested_projection() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let db = Connection::open(directory.path().join(FILE_NAME)).unwrap();
    db.execute("INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload,server) VALUES ('activity','mcp','legacy',0,?1,'{}','fixture')", [RECENT]).unwrap();
    let totals = store.aggregate_many(Section::Mcp, &[None], &[]).unwrap();
    assert_eq!(totals[0].total, 1);
    assert_eq!(totals[0].servers[0].1.nested, 0);
    db.execute(
        "INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload,server,nested) VALUES ('activity','mcp','nested',0,?1,'{}','fixture',1)",
        [RECENT],
    ).unwrap();
    let totals = store.aggregate_many(Section::Mcp, &[None], &[]).unwrap();
    assert_eq!(totals[0].total, 2);
    assert_eq!(totals[0].servers[0].1.nested, 1);
    assert_eq!(totals[0].series[0].calls.nested, 1);
    assert_eq!(totals[0].series[0].servers[0].1.nested, 1);
}

#[test]
fn retention_change_stops_an_old_cleanup_before_its_next_batch() {
    let directory = tempdir().unwrap();
    let mut cleanup = Store::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let db = Connection::open(directory.path().join(FILE_NAME)).unwrap();
    db.execute_batch(
        "WITH RECURSIVE ids(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM ids WHERE n<501)
         INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload)
         SELECT 'activity','web',n,0,'2026-10-01T00:00:00.000Z','{}' FROM ids;",
    )
    .unwrap();
    let now = RECENT.parse().unwrap();
    assert_eq!(cleanup.set_activity_retention(1, now).unwrap(), 500);
    let mut settings = Store::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    assert_eq!(settings.set_activity_retention(0, now).unwrap(), 0);
    assert_eq!(cleanup.continue_retention_cleanup(1, now).unwrap(), 0);
    let remaining: i64 = db
        .query_row("SELECT count(*) FROM history_items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(remaining, 1);
    assert_eq!(settings.activity_retention().unwrap(), 0);
}
