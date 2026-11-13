use chrono::{DateTime, Duration, Utc};
use lam_history_store::{
    parse_utc_stamp, Access, Cursor, ExpectedCursor, HistoryWrite, Namespace, ObservationBatch,
    ObservationStore, ObservationWrite, PageQuery, Section, Store, StoredCall,
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
fn checkpoint_batches_read_selected_sources_and_report_exhausted_calls() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursors: Vec<_> = ["one", "two"].into_iter().map(|key| Cursor { source_key:key.into(), offset:10, signature:"anchor".into(), payload:"{}".into() }).collect();
    let calls: Vec<_> = [("one","a"),("one","b"),("two","c")].into_iter().map(|(key,id)| StoredCall {source_key:key.into(),call_id:id.into(),event_time:Some("2026-11-12T10:00:00Z".into()),payload:"{}".into()}).collect();
    store.save_checkpoints(&cursors, &calls, &[]).unwrap();
    let page = store.checkpoints(&["one".into(),"missing".into(),"two".into()], 2).unwrap();
    assert_eq!(page.checkpoints.len(), 2);
    assert_eq!(page.checkpoints[0].calls.len(), 2);
    assert!(page.checkpoints[1].calls.is_empty());
    assert_eq!(page.incomplete_sources, ["two"]);
    let complete = store.checkpoints(&["two".into()], 5).unwrap();
    assert_eq!(complete.checkpoints[0].calls[0].call_id, "c");
    assert!(complete.incomplete_sources.is_empty());
    let expected = [ExpectedCursor {source_key:"one",signature:Some("anchor")},ExpectedCursor {source_key:"two",signature:Some("stale")},ExpectedCursor {source_key:"missing",signature:Some("anchor")}];
    let (accepted, excluded) = store.matching_checkpoints(&expected, 10).unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].cursor.source_key, "one");
    assert_eq!(excluded, ["two", "missing"]);
    let (accepted, excluded) = store.matching_checkpoints(&[ExpectedCursor {source_key:"one",signature:Some("anchor")}], 1).unwrap();
    assert!(accepted.is_empty());
    assert_eq!(excluded, ["one"]);
    let (accepted, excluded) = store.matching_checkpoints_with_partials(&expected, 1).unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].0.cursor.source_key, "one");
    assert!(accepted[0].0.calls.is_empty());
    assert!(accepted[0].1);
    assert_eq!(excluded, ["two", "missing"]);
    assert!(store.matching_checkpoints(&[ExpectedCursor {source_key:"one",signature:None}], 10).is_err());
    assert!(store.checkpoints(&["one".into(),"one".into()], 5).is_err());
    assert!(store.checkpoints(&vec!["x".into(); 65], 5).is_err());
}

#[test]
fn checkpoint_call_headers_page_by_time_and_validate_signature() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key:"one".into(), offset:10, signature:"anchor".into(), payload:"{}".into() };
    let calls: Vec<_> = [("a",Some("2026-11-12T10:00:00Z")),("b",Some("2026-11-12T10:00:00Z")),("c",Some("2026-11-11T10:00:00Z")),("d",None),("e",None)]
        .into_iter().map(|(id,time)| StoredCall { source_key:"one".into(), call_id:id.into(), event_time:time.map(str::to_owned), payload:"{}".into() }).collect();
    store.save_checkpoints(&[cursor], &calls, &[]).unwrap();
    let expected = ExpectedCursor { source_key:"one", signature:Some("anchor") };
    let mut after = None;
    let mut ids = Vec::new();
    loop {
        let page = store.checkpoint_call_headers(&expected, after.as_ref(), 2).unwrap().unwrap();
        if page.is_empty() { break; }
        ids.extend(page.iter().map(|row| row.call_id.clone()));
        after = page.last().cloned();
    }
    assert_eq!(ids, ["a", "b", "c", "d", "e"]);
    let selected = store.selected_checkpoint(&expected, &["b".into(), "d".into()]).unwrap().unwrap();
    assert_eq!(selected.calls.iter().map(|call| call.call_id.as_str()).collect::<Vec<_>>(), ["b", "d"]);
    assert!(store.selected_checkpoint(&expected, &["missing".into()]).unwrap().is_none());
    assert!(store.selected_checkpoint(&expected, &["a".into(), "a".into()]).is_err());
    assert!(store.selected_checkpoint(&ExpectedCursor { source_key:"one", signature:Some("stale") }, &[]).unwrap().is_none());
    assert!(store.checkpoint_call_headers(&ExpectedCursor { source_key:"one", signature:Some("stale") }, None, 2).unwrap().is_none());
    assert!(store.checkpoint_call_headers(&ExpectedCursor { source_key:"one", signature:None }, None, 2).is_err());
    assert!(store.checkpoint_call_headers(&expected, None, 513).is_err());
}

#[test]
fn selected_checkpoint_streams_more_than_byte_budget_in_stable_order() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key: "one".into(), offset: 10, signature: "anchor".into(), payload: "{}".into() };
    let calls: Vec<_> = (0..20).map(|i| StoredCall { source_key: "one".into(), call_id: format!("call-{i:02}"),
        event_time: None, payload: "x".repeat(1024 * 1024) }).collect();
    store.save_checkpoints(&[cursor], &calls, &[]).unwrap();
    let ids: Vec<_> = calls.iter().rev().map(|call| call.call_id.clone()).collect();
    let expected = ExpectedCursor { source_key: "one", signature: Some("anchor") };
    assert!(store.selected_checkpoint(&expected, &ids).is_err());
    let mut actual = Vec::new();
    let mut pages = 0;
    assert!(store.visit_selected_checkpoint(&expected, &ids, |cursor, batch| {
        pages += 1;
        let bytes = cursor.source_key.len() + cursor.signature.len() + cursor.payload.len()
            + batch.iter().map(|call| call.source_key.len() + call.call_id.len() + call.payload.len() + call.event_time.as_ref().map_or(0, String::len)).sum::<usize>();
        assert!(bytes <= 16 * 1024 * 1024);
        assert!(batch.len() <= 512);
        assert!(batch.iter().all(|call| call.payload.len() == 1024 * 1024));
        actual.extend(batch.iter().map(|call| call.call_id.clone()));
        Ok(())
    }).unwrap());
    assert_eq!(pages, 2);
    assert_eq!(actual, ids);
}

#[test]
fn selected_checkpoint_stream_preflights_and_stops_on_consumer_error() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key: "one".into(), offset: 10, signature: "anchor".into(), payload: "{}".into() };
    let calls: Vec<_> = (0..513).map(|i| StoredCall { source_key: "one".into(), call_id: format!("call-{i:03}"), event_time: None, payload: "{}".into() }).collect();
    store.save_checkpoints(&[cursor], &calls, &[]).unwrap();
    let ids: Vec<_> = calls.iter().map(|call| call.call_id.clone()).collect();
    let expected = ExpectedCursor { source_key: "one", signature: Some("anchor") };
    let mut lengths = Vec::new();
    assert!(store.visit_selected_checkpoint(&expected, &ids, |_, calls| { lengths.push(calls.len()); Ok(()) }).unwrap());
    assert_eq!(lengths, [512, 1]);
    let mut missing = ids.clone(); missing.push("missing".into());
    assert!(!store.visit_selected_checkpoint(&expected, &missing, |_, _| panic!("missing call must not publish a partial result")).unwrap());
    assert!(!store.visit_selected_checkpoint(&ExpectedCursor { source_key: "one", signature: Some("stale") }, &ids, |_, _| panic!("stale cursor")).unwrap());
    assert!(store.visit_selected_checkpoint(&expected, &[ids[0].clone(), ids[0].clone()], |_, _| panic!("duplicate id")).is_err());
    let mut visits = 0;
    assert!(store.visit_selected_checkpoint(&expected, &ids, |_, _| { visits += 1; Err(lam_history_store::StoreError::InvalidQuery) }).is_err());
    assert_eq!(visits, 1);
    assert!(store.visit_selected_checkpoint(&expected, &[], |_, batch| { assert!(batch.is_empty()); Ok(()) }).unwrap());
}

#[test]
fn selected_checkpoint_stream_keeps_one_snapshot_during_writer_change() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let mut writer = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key: "one".into(), offset: 10, signature: "anchor".into(), payload: "{}".into() };
    let calls: Vec<_> = (0..514).map(|i| StoredCall { source_key: "one".into(), call_id: format!("call-{i:03}"), event_time: None, payload: "old".into() }).collect();
    store.save_checkpoints(&[cursor.clone()], &calls, &[]).unwrap();
    let ids: Vec<_> = calls.iter().map(|call| call.call_id.clone()).collect();
    let expected = ExpectedCursor { source_key: "one", signature: Some("anchor") };
    let mut visits = 0;
    assert!(store.visit_selected_checkpoint(&expected, &ids, |_, batch| {
        visits += 1;
        assert!(batch.iter().all(|call| call.payload == "old"));
        if visits == 1 {
            let mut changed = calls[513].clone(); changed.payload = "new".into();
            // The 514th row has not been selected when the first batch callback runs.
            // A same-signature change must not split the subsequent read view.
            writer.save_checkpoints(&[cursor.clone()], &[changed], &[])?;
        }
        Ok(())
    }).unwrap());
    assert_eq!(visits, 2);
    assert_eq!(store.selected_checkpoint(&expected, &[ids[513].clone()]).unwrap().unwrap().calls[0].payload, "new");
}

#[test]
fn selected_checkpoint_stream_rejects_oversized_record_before_callback() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key: "one".into(), offset: 10, signature: "anchor".into(), payload: "{}".into() };
    let calls = [StoredCall { source_key: "one".into(), call_id: "a".into(), event_time: None, payload: "{}".into() },
        StoredCall { source_key: "one".into(), call_id: "b".into(), event_time: None, payload: "x".repeat(16 * 1024 * 1024) }];
    store.save_checkpoints(&[cursor], &calls, &[]).unwrap();
    let expected = ExpectedCursor { source_key: "one", signature: Some("anchor") };
    assert!(store.visit_selected_checkpoint(&expected, &["a".into(), "b".into()], |_, _| panic!("oversized selection")).is_err());
}

#[test]
fn selected_checkpoint_reader_resumes_one_snapshot_beyond_byte_budget() {
    let directory = tempdir().unwrap();
    let mut writer = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key: "one".into(), offset: 10, signature: "anchor".into(), payload: "{}".into() };
    let calls: Vec<_> = (0..20).map(|i| StoredCall { source_key: "one".into(), call_id: format!("call-{i:02}"), event_time: None, payload: "x".repeat(1024 * 1024) }).collect();
    writer.save_checkpoints(std::slice::from_ref(&cursor), &calls, &[]).unwrap();
    let ids: Vec<_> = calls.iter().map(|call| call.call_id.clone()).collect();
    let expected = ExpectedCursor { source_key: "one", signature: Some("anchor") };
    let mut reader = Store::open(directory.path(), Access::ReadOnly).unwrap().unwrap()
        .into_selected_checkpoint_reader(&expected, ids.clone()).unwrap().unwrap();
    let first = reader.next_page().unwrap().unwrap();
    assert_eq!(first.calls.len(), 15);
    assert!(!reader.is_complete());
    let mut changed = calls[19].clone(); changed.payload = "new".into();
    writer.save_checkpoints(&[cursor], &[changed], &[]).unwrap();
    let second = reader.next_page().unwrap().unwrap();
    assert_eq!(second.calls.len(), 5);
    assert!(second.calls.iter().all(|call| call.payload.len() == 1024 * 1024));
    assert!(reader.is_complete());
    assert!(reader.next_page().unwrap().is_none());
    let actual: Vec<_> = first.calls.iter().chain(&second.calls).map(|call| call.call_id.clone()).collect();
    assert_eq!(actual, ids);
    for page in [&first, &second] {
        let bytes = page.cursor.source_key.len() + page.cursor.signature.len() + page.cursor.payload.len()
            + page.calls.iter().map(|call| call.source_key.len() + call.call_id.len() + call.payload.len() + call.event_time.as_ref().map_or(0, String::len)).sum::<usize>();
        assert!(bytes <= 16 * 1024 * 1024);
    }
    assert_eq!(writer.selected_checkpoint(&expected, &["call-19".into()]).unwrap().unwrap().calls[0].payload, "new");
}

#[test]
fn selected_checkpoint_reader_bounds_rows_and_releases_cancelled_snapshot() {
    let directory = tempdir().unwrap();
    let mut writer = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key: "one".into(), offset: 10, signature: "anchor".into(), payload: "{}".into() };
    let calls: Vec<_> = (0..514).map(|i| StoredCall { source_key: "one".into(), call_id: format!("call-{i:03}"), event_time: None, payload: "old".into() }).collect();
    writer.save_checkpoints(std::slice::from_ref(&cursor), &calls, &[]).unwrap();
    let ids: Vec<_> = calls.iter().map(|call| call.call_id.clone()).collect();
    let expected = ExpectedCursor { source_key: "one", signature: Some("anchor") };
    let mut reader = Store::open(directory.path(), Access::ReadOnly).unwrap().unwrap()
        .into_selected_checkpoint_reader(&expected, ids).unwrap().unwrap();
    assert_eq!(reader.next_page().unwrap().unwrap().calls.len(), 512);
    let mut changed = calls[513].clone(); changed.payload = "new".into();
    writer.save_checkpoints(&[cursor], &[changed], &[]).unwrap();
    let db = rusqlite::Connection::open(directory.path().join(lam_history_store::FILE_NAME)).unwrap();
    let busy: i64 = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0)).unwrap();
    assert_eq!(busy, 1);
    drop(reader);
    let busy: i64 = db.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0)).unwrap();
    assert_eq!(busy, 0);
}

#[test]
fn selected_checkpoint_reader_errors_are_not_successful_eof() {
    let directory = tempdir().unwrap();
    let mut writer = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursor = Cursor { source_key: "one".into(), offset: 10, signature: "anchor".into(), payload: "{}".into() };
    let call = StoredCall { source_key: "one".into(), call_id: "large".into(), event_time: None, payload: "x".repeat(16 * 1024 * 1024) };
    writer.save_checkpoints(&[cursor], &[call], &[]).unwrap();
    let expected = ExpectedCursor { source_key: "one", signature: Some("anchor") };
    for id in ["large", "missing"] {
        let mut reader = Store::open(directory.path(), Access::ReadOnly).unwrap().unwrap()
            .into_selected_checkpoint_reader(&expected, vec![id.into()]).unwrap().unwrap();
        assert!(reader.next_page().is_err());
        assert!(!reader.is_complete());
        assert!(reader.next_page().is_err());
    }
    assert!(Store::open(directory.path(), Access::ReadOnly).unwrap().unwrap()
        .into_selected_checkpoint_reader(&ExpectedCursor { source_key: "one", signature: Some("stale") }, vec![]).unwrap().is_none());
    assert!(Store::open(directory.path(), Access::ReadOnly).unwrap().unwrap()
        .into_selected_checkpoint_reader(&expected, vec!["large".into(), "large".into()]).is_err());
    let mut empty = Store::open(directory.path(), Access::ReadOnly).unwrap().unwrap()
        .into_selected_checkpoint_reader(&expected, vec![]).unwrap().unwrap();
    assert!(empty.next_page().unwrap().unwrap().calls.is_empty());
    assert!(empty.is_complete());
    assert!(empty.next_page().unwrap().is_none());
    assert!(writer.into_selected_checkpoint_reader(&expected, vec![]).is_err());
}

#[test]
fn oversized_call_does_not_hide_following_checkpoint() {
    let directory = tempdir().unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let cursors: Vec<_> = ["large", "next"].into_iter().map(|key| Cursor { source_key:key.into(), offset:10, signature:"anchor".into(), payload:"{}".into() }).collect();
    let calls = vec![
        StoredCall { source_key:"large".into(), call_id:"a".into(), event_time:None, payload:"x".repeat(16 * 1024 * 1024) },
        StoredCall { source_key:"next".into(), call_id:"b".into(), event_time:None, payload:"{}".into() },
    ];
    store.save_checkpoints(&cursors, &calls, &[]).unwrap();
    let expected = [ExpectedCursor {source_key:"large",signature:Some("anchor")}, ExpectedCursor {source_key:"next",signature:Some("anchor")}];
    let (accepted, excluded) = store.matching_checkpoints_with_partials(&expected, 10).unwrap();
    assert!(excluded.is_empty());
    assert_eq!(accepted.len(), 2);
    assert!(accepted[0].1);
    assert!(accepted[0].0.calls.is_empty());
    assert!(!accepted[1].1);
    assert_eq!(accepted[1].0.calls[0].call_id, "b");
}

#[test]
fn future_activity_survives_restart_until_due() {
    let directory = tempdir().unwrap();
    let now = parse_utc_stamp("2026-11-12T10:00:00Z").unwrap();
    let events = [
        json!({"timestamp":"2026-11-12T10:02:00Z","engine":"SQLite","operation":"read","statement":"SELECT","recognition":"recorded_code"}),
        json!({"timestamp":"2026-11-12T10:02:00Z","server":"web","tool":"search","call_id":"web-1","metadata":{"references":["https://example.com/path?secret=private"],"operations":["search_query"]}}),
        json!({"timestamp":"2026-11-12T10:02:00Z","server":"fixture","tool":"read","call_id":"mcp-1","metadata":{"resources":{"projectId":"project-a","password":"private"}},"result":{"status":"ok","body":"private"}}),
    ];
    let mut store = ObservationStore::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    store.save_diagnostics(&events, &[], now).unwrap();
    assert!(store.read(now, &[None; 4]).unwrap().events.is_empty());
    assert_eq!(store.next_pending_activity().unwrap(), Some(now + Duration::minutes(1)));
    drop(store);
    let mut store = ObservationStore::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    assert!(!store.process_pending_activity(now).unwrap());
    assert!(store.process_pending_activity(now + Duration::minutes(1)).unwrap());
    let view = store.read(now + Duration::minutes(1), &[None; 4]).unwrap();
    assert_eq!(view.events.len(), 3);
    let web = view.events.iter().find(|event| event["server"] == "web").unwrap();
    assert_eq!(web["metadata"]["references"], json!(["https://example.com/path"]));
    let mcp = view.events.iter().find(|event| event["server"] == "fixture").unwrap();
    assert_eq!(mcp["metadata"]["resources"]["projectId"], "project-a");
    assert!(!json!(view.events.as_ref()).to_string().contains("private"));
    assert!(store.next_pending_activity().unwrap().is_none());
}

#[test]
fn pending_activity_keeps_submillisecond_time_until_due() {
    let directory = tempdir().unwrap();
    let now = parse_utc_stamp("2026-11-12T10:00:00Z").unwrap();
    let event = json!({"timestamp":"2026-11-12T10:02:00.000500Z","engine":"SQLite","operation":"read","statement":"SELECT","recognition":"recorded_code"});
    let mut store = ObservationStore::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    store.save_diagnostics(&[event], &[], now).unwrap();
    let early = now + Duration::minutes(1);
    assert!(!store.process_pending_activity(early).unwrap());
    assert!(store.next_pending_activity().unwrap().is_some());
    assert!(store.process_pending_activity(early + Duration::microseconds(500)).unwrap());
    assert_eq!(store.read(early + Duration::microseconds(500), &[None; 4]).unwrap().events.len(), 1);
    assert!(store.next_pending_activity().unwrap().is_none());
}

#[test]
fn skill_history_skips_conflicting_sources_and_preserves_other_sources() {
    let directory = tempdir().unwrap();
    let now = parse_utc_stamp("2026-11-12T10:00:00Z").unwrap();
    let mut store = Store::open(directory.path(), Access::ReadWrite)
        .unwrap()
        .unwrap();
    let original = Cursor {
        source_key: "conflict".into(),
        offset: 1,
        signature: "live".into(),
        payload: "{}".into(),
    };
    store.save_checkpoints(&[original], &[], &[]).unwrap();
    let cursor = |key: &str| Cursor {
        source_key: key.into(),
        offset: 2,
        signature: "next".into(),
        payload: "{}".into(),
    };
    let skill = |key: &str, name: &str| ObservationWrite {
        source_key: key.into(),
        event: json!({"thread_id":"00000000-0000-4000-8000-000000000001","call_id":key,"skill":name,"timestamp":"2026-11-12T09:00:00Z"}),
    };
    let skills = [skill("conflict", "hidden"), skill("fresh", "visible")];
    let lifecycle = [
        ObservationWrite { source_key:"conflict".into(), event:json!({"source_file":"rollout-conflict.jsonl","thread_id":"00000000-0000-4000-8000-000000000001","offset":5,"status":"running","task_time":"2026-11-12T09:00:00Z","task_start":null}) },
        ObservationWrite { source_key:"fresh".into(), event:json!({"source_file":"rollout-fresh.jsonl","thread_id":"00000000-0000-4000-8000-000000000002","offset":7,"status":"completed","task_time":"2026-11-12T09:00:00Z","task_start":null}) },
    ];
    let (conflicts, _) = store
        .save_observation_batch(
            &[cursor("conflict"), cursor("fresh")],
            &[],
            &[],
            &[],
            &[
                ExpectedCursor {
                    source_key: "conflict",
                    signature: Some("stale"),
                },
                ExpectedCursor {
                    source_key: "fresh",
                    signature: None,
                },
            ],
            ObservationBatch {
                rows: &[],
                errors: &[],
                skills: &skills,
                lifecycle: &lifecycle,
                now,
            },
        )
        .unwrap();
    assert!(conflicts.contains("conflict"));
    assert!(store.thread_lifecycle("rollout-conflict.jsonl").unwrap().is_none());
    assert_eq!(store.thread_lifecycle("rollout-fresh.jsonl").unwrap().unwrap()["status"], "completed");
    let page = store
        .page(PageQuery {
            section: Section::Skills,
            limit: 20,
            offset: 0,
            since: None,
            after: None,
            excluded_sources: &[],
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&page.items[0].payload).unwrap()["skill"],
        "visible"
    );
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
                skills: &[],
                lifecycle: &[],
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
                skills: &[],
                lifecycle: &[],
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
    let filtered = store
        .read_filtered(
            now + Duration::minutes(31),
            &windows("2026-11-12T09:31:00Z"),
            &["fixture"],
        )
        .unwrap();
    assert_eq!(filtered.mcp[3].total, 0);
    assert_eq!(filtered.sql[3].total, 2);
    assert!(Arc::ptr_eq(&external.events, &filtered.events));
    assert!(Arc::ptr_eq(&external.sql[3], &filtered.sql[3]));
    let enabled = store
        .read_filtered(
            now + Duration::minutes(31),
            &windows("2026-11-12T09:31:00Z"),
            &[],
        )
        .unwrap();
    assert_eq!(enabled.mcp[3].total, 2);
    assert!(store
        .read_filtered(now, &windows("2026-11-12T09:00:00Z"), &[""])
        .is_err());
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
fn error_preview_counts_the_compact_json_envelope() {
    let directory = tempdir().unwrap();
    let now = parse_utc_stamp("2026-11-12T10:00:00Z").unwrap();
    let mut store = ObservationStore::open(directory.path(), Access::ReadWrite).unwrap().unwrap();
    let long = "a".repeat(150);
    let events: Vec<_> = (0..1000).map(|record_id| json!({
        "timestamp":"2026-11-12T09:00:00Z", "source":"codex_core", "category":"runtime",
        "severity":"error", "record_id":record_id, "code":long, "module":long,
        "request_id":long, "trace_id":long, "tool":long
    })).collect();
    store.save_diagnostics(&[], &events, now).unwrap();
    let preview = store.error_preview(now).unwrap();
    assert!(!preview.is_empty() && preview.len() < events.len());
    let bytes = serde_json::to_vec(&json!({"version":1,"events":preview})).unwrap().len();
    assert!(bytes <= 512 * 1024);
    let mut one_more = preview.clone();
    one_more.push(preview[0].clone());
    assert!(serde_json::to_vec(&json!({"version":1,"events":one_more})).unwrap().len() > 512 * 1024);
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
