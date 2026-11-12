use lam_history_store::{Access, PageQuery, Section, Store, StoreError, APPLICATION_ID, FILE_NAME};
use rusqlite::{params, Connection};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const STAMP: &str = "2026-10-10 10:00:00.123456+08:00";

fn legacy(directory: &Path, version: i64) -> Connection {
    let db = Connection::open(directory.join(FILE_NAME)).unwrap();
    db.execute_batch(
        "CREATE TABLE history_state (namespace TEXT PRIMARY KEY, metadata TEXT NOT NULL, updated_at INTEGER NOT NULL);
         CREATE TABLE history_items (namespace TEXT NOT NULL, section TEXT NOT NULL, item_key TEXT NOT NULL, ordinal INTEGER NOT NULL, event_time TEXT, payload TEXT NOT NULL, PRIMARY KEY(namespace, section, item_key));
         CREATE INDEX history_time ON history_items(namespace, section, event_time);",
    ).unwrap();
    if version >= 2 {
        db.execute_batch(
            "CREATE INDEX history_recent ON history_items(namespace, section, event_time DESC, item_key DESC);
             CREATE INDEX history_cleanup ON history_items(event_time);",
        ).unwrap();
    }
    if version >= 3 {
        db.execute_batch(
            "CREATE TABLE session_cursors (source_key TEXT PRIMARY KEY, offset INTEGER NOT NULL, signature TEXT NOT NULL, payload TEXT NOT NULL);
             CREATE TABLE session_calls (source_key TEXT NOT NULL, call_id TEXT NOT NULL, event_time TEXT, payload TEXT NOT NULL, PRIMARY KEY(source_key,call_id));
             CREATE INDEX session_calls_recent ON session_calls(source_key,event_time DESC,call_id);",
        ).unwrap();
    }
    db.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
    db
}

fn version(db: &Connection) -> i64 {
    db.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

fn backup(directory: &Path, version: i64) -> std::path::PathBuf {
    directory.join(format!("{FILE_NAME}.schema-v{version}.bak"))
}

#[test]
fn upgrade_v1_rekeys_every_section_and_matches_python_key_encoding() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    let old = legacy(root, 1);
    let cases = [
        (
            "activity",
            "web",
            "0",
            r#"{"thread_id":"thread","call_id":"舊😀","index":1,"timestamp":"2026-10-10 10:00:00.123456+08:00","tool":"web.run","revision":"previous"}"#,
            "4243c4f7c49a67c22d07506b2c9d9a9944145d9c57347951f74ee40e1020f86b",
        ),
        (
            "activity",
            "web",
            "1",
            r#"{"thread_id":"thread","call_id":"舊😀","index":1,"timestamp":"2026-10-10 10:00:00.123456+08:00","tool":"web.run","revision":"latest\ud800"}"#,
            "4243c4f7c49a67c22d07506b2c9d9a9944145d9c57347951f74ee40e1020f86b",
        ),
        (
            "activity",
            "mcp",
            "0",
            r#"{"server":"docs","thread_id":"thread","call_id":"舊😀","index":1,"timestamp":"2026-10-10 10:00:00.123456+08:00","tool":"web.run","nested":true}"#,
            "7cb11b31ae2c6cb11966819851daeec8cbd685125209e9829480ee56afcbb201",
        ),
        (
            "activity",
            "sql",
            "0",
            r#"{"source":"session","thread_id":"thread","call_id":"first","index":1,"file":"活動","record_id":3,"record_offset":4,"record_hash":"digest","timestamp":"2026-10-10 10:00:00.123456+08:00","statement":"SELECT"}"#,
            "3f365538eeb2201a2f884b8f356d0d38bba652937d51ef39516deeba16f91af9",
        ),
        (
            "errors",
            "events",
            "0",
            r#"{"timestamp":"2026-10-10 10:00:00.123456+08:00","source":"session","category":"conversation","severity":"error","code":"舊😀","thread_id":"thread","attempt":1}"#,
            "f0c32c7fa98f1d4f801f8ccd0604b716a5ce8d69deab6200c8d7e3d9a1b20145",
        ),
        (
            "threads",
            "skills",
            "0",
            r#"{"thread_id":"thread","call_id":"first","skill":"資料"}"#,
            "df2b24faca21346af22739e58e832c3ec46c8df0950307aeabbcaf74d114c263",
        ),
        (
            "threads",
            "entries",
            "thread-0",
            r#"{"task_time":"2026-10-10 10:00:00.123456+08:00"}"#,
            "thread-0",
        ),
    ];
    for (ordinal, (namespace, section, key, payload, _)) in cases.iter().enumerate() {
        old.execute(
            "INSERT INTO history_items VALUES(?,?,?,?,?,?)",
            params![namespace, section, key, ordinal as i64, STAMP, payload],
        )
        .unwrap();
    }
    old.execute(
        "INSERT INTO history_state VALUES('activity',?,1)",
        [r#"{"retention_days":0}"#],
    )
    .unwrap();
    assert!(matches!(
        Store::open(root, Access::ReadOnly),
        Err(StoreError::UnsupportedSchema { version: 1, .. })
    ));
    assert!(!backup(root, 1).exists());
    assert!(Store::upgrade(root).unwrap());
    assert!(!Store::upgrade(root).unwrap());

    assert_eq!(version(&old), 4);
    let saved = Connection::open(backup(root, 1)).unwrap();
    assert_eq!(version(&saved), 1);
    assert_eq!(
        saved
            .query_row("SELECT count(*) FROM history_items", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        old.query_row("SELECT count(*) FROM history_items", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        6
    );
    assert_eq!(
        old.query_row(
            "SELECT updated_at FROM history_state WHERE namespace='activity'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    for (namespace, section, _, payload, key) in &cases[1..] {
        let row: (String, String, i64) = old.query_row(
            "SELECT event_time, payload, nested FROM history_items WHERE namespace=? AND section=? AND item_key=?",
            [namespace, section, key],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).unwrap();
        assert_eq!(
            row,
            (
                "2026-10-10T02:00:00.123Z".into(),
                (*payload).into(),
                i64::from(*section == "mcp")
            )
        );
    }
    assert_eq!(
        old.query_row(
            "SELECT ordinal FROM history_items WHERE section='web'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[test]
fn upgrade_v2_and_v3_preserves_keys_and_copies_committed_wal_pages() {
    for old_version in [2, 3] {
        let temp = tempdir().unwrap();
        let root = temp.path();
        let old = legacy(root, old_version);
        old.pragma_update(None, "journal_mode", "WAL").unwrap();
        old.execute(
            "INSERT INTO history_items VALUES('activity','mcp','stable-v2',0,?,?)",
            params![
                "2026-10-10T02:00:00.123Z",
                r#"{"server":"docs","operation":"read","nested":true}"#
            ],
        )
        .unwrap();
        if old_version == 3 {
            old.execute(
                "INSERT INTO session_cursors VALUES('legacy',33,'signature','{}')",
                [],
            )
            .unwrap();
        }
        assert!(root.join(format!("{FILE_NAME}-wal")).exists());
        let mut store = Store::open(root, Access::ReadWrite).unwrap().unwrap();
        let page = store
            .page(PageQuery {
                section: Section::Mcp,
                limit: 10,
                offset: 0,
                since: None,
                after: None,
                excluded_sources: &[],
            })
            .unwrap();
        assert_eq!(page.items[0].position.key, "stable-v2");
        let totals = store.aggregate_many(Section::Mcp, &[None], &[]).unwrap();
        assert_eq!(totals[0].servers[0].1.nested, 1);
        if old_version == 3 {
            assert_eq!(
                store
                    .checkpoint("legacy", 10)
                    .unwrap()
                    .unwrap()
                    .cursor
                    .offset,
                33
            );
        }
        let saved = Connection::open(backup(root, old_version)).unwrap();
        assert_eq!(version(&saved), old_version);
        assert_eq!(
            saved
                .query_row("SELECT item_key FROM history_items", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "stable-v2"
        );
        if old_version == 3 {
            assert_eq!(
                saved
                    .query_row("SELECT offset FROM session_cursors", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                33
            );
        }
    }
}

#[test]
fn invalid_v1_row_rolls_back_and_retries_with_the_original_backup() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    let old = legacy(root, 1);
    old.execute(
        "INSERT INTO history_items VALUES('activity','web','first',0,'invalid','{}')",
        [],
    )
    .unwrap();
    assert!(matches!(
        Store::upgrade(root),
        Err(StoreError::InvalidMigration)
    ));
    assert_eq!(version(&old), 1);
    assert_eq!(
        old.query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='history_recent'",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    let original_backup = fs::read(backup(root, 1)).unwrap();
    old.execute(
        "UPDATE history_items SET event_time=? WHERE item_key='first'",
        [STAMP],
    )
    .unwrap();
    assert!(Store::upgrade(root).unwrap());
    assert_eq!(version(&old), 4);
    assert_eq!(original_backup, fs::read(backup(root, 1)).unwrap());
}

#[test]
fn invalid_existing_backup_stops_migration_without_overwriting_it() {
    let temp = tempdir().unwrap();
    let root = temp.path();
    let old = legacy(root, 3);
    let path = backup(root, 3);
    fs::write(&path, b"invalid backup").unwrap();
    assert!(Store::upgrade(root).is_err());
    assert_eq!(version(&old), 3);
    assert_eq!(fs::read(path).unwrap(), b"invalid backup");
}

#[cfg(unix)]
#[test]
fn linked_backup_is_rejected() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().unwrap();
    let root = temp.path();
    let old = legacy(root, 2);
    let outside = root.join("existing.txt");
    fs::write(&outside, b"preserve").unwrap();
    symlink(&outside, backup(root, 2)).unwrap();
    assert!(matches!(Store::upgrade(root), Err(StoreError::UnsafePath)));
    assert_eq!(version(&old), 2);
    assert_eq!(fs::read(outside).unwrap(), b"preserve");
}

#[test]
fn legacy_timestamps_match_python_with_basic_week_and_fractional_offsets() {
    use lam_history_store::normalize_utc_stamp;

    for (value, expected) in [
        ("20261010T100000+0800", "2026-10-10T02:00:00.000Z"),
        ("2026-W41-6T10:00Z", "2026-10-10T10:00:00.000Z"),
        ("2026W416x10Z", "2026-10-10T10:00:00.000Z"),
        ("2026-W41T10Z", "2026-10-05T10:00:00.000Z"),
        ("2026W41T10Z", "2026-10-05T10:00:00.000Z"),
        (
            "2026-10-10X10:00:00,123456+08:00",
            "2026-10-10T02:00:00.123Z",
        ),
        ("2026-10-10T10:00:00+08:30:10.5", "2026-10-10T01:29:49.500Z"),
        ("2026-10-10T10:00:00+01:99", "2026-10-10T07:21:00.000Z"),
    ] {
        assert_eq!(
            normalize_utc_stamp(value).as_deref(),
            Some(expected),
            "{value}"
        );
    }
    for value in [
        "2026-10-10",
        "2026-10-10T10:00",
        "2026-10-10T10.5+08",
        "2026-10-10T10:00:60Z",
        "2026-10-10T10:00:00+24:00",
    ] {
        assert!(normalize_utc_stamp(value).is_none(), "{value}");
    }
}

#[test]
fn concurrent_openers_upgrade_once_and_share_the_verified_backup() {
    use std::sync::{Arc, Barrier};

    let temp = tempdir().unwrap();
    let root = temp.path();
    let old = legacy(root, 3);
    old.execute(
        "INSERT INTO history_items VALUES('activity','web','legacy',0,NULL,'{}')",
        [],
    )
    .unwrap();
    let start = Arc::new(Barrier::new(3));
    let workers = (0..2)
        .map(|_| {
            let start = Arc::clone(&start);
            let directory = root.to_path_buf();
            std::thread::spawn(move || {
                start.wait();
                Store::upgrade(&directory).unwrap()
            })
        })
        .collect::<Vec<_>>();
    start.wait();
    let upgraded = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .filter(|did_upgrade| *did_upgrade)
        .count();
    assert_eq!(upgraded, 1);
    assert_eq!(version(&old), 4);
    assert_eq!(version(&Connection::open(backup(root, 3)).unwrap()), 3);
    assert_eq!(
        old.query_row("SELECT count(*) FROM history_items", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
