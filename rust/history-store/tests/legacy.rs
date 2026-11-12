use lam_history_store::{
    parse_utc_stamp, Access, Namespace, PageQuery, Section, Store, StoreError, FILE_NAME,
};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const STAMP: &str = "2026-11-12T09:00:00.000Z";
const NOW: &str = "2026-11-12T09:30:00.000Z";
const THREAD: &str = "00000000-0000-4000-8000-000000000001";

fn write(directory: &Path, name: &str, value: &Value) {
    fs::write(directory.join(name), value.to_string()).unwrap();
}

fn items(store: &mut Store, section: Section) -> Vec<Value> {
    store
        .page(PageQuery {
            section,
            limit: 20,
            offset: 0,
            since: None,
            after: None,
            excluded_sources: &[],
        })
        .unwrap()
        .items
        .into_iter()
        .map(|item| serde_json::from_str(&item.payload).unwrap())
        .collect()
}

#[test]
fn imports_all_namespaces_and_projects_only_safe_metadata() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let now = parse_utc_stamp(NOW).unwrap();
    write(
        root,
        "activity-history.json",
        &json!({
            "version":2,"retention_days":7,
            "sql":[{"timestamp":STAMP,"statement":"SELECT","operation":"read","engine":"sqlite","recognition":"observed",
                    "database":"file:fixture.db?token=PRIVATE_TOKEN","rows_returned":0,"duration_ms":1.5,"query":"PRIVATE_QUERY"}],
            "web":[
                {"timestamp":STAMP,"server":"web","thread_id":THREAD,"call_id":"web","tool":"open",
                 "metadata":{"operations":["open","unsupported"],"references":["https://example.com/ref?secret=PRIVATE_QUERY#part","http://127.0.0.1/PRIVATE_HOST","https://user:pass@example.com/PRIVATE_CREDENTIAL"]},
                 "result":{"references":["https://example.org/path"]}},
                {"timestamp":STAMP,"server":"web","thread_id":THREAD,"call_id":"web","tool":"open",
                 "metadata":{"operations":["click"],"references":["https://example.com/ref","https://example.com/next"]}},
                {"timestamp":"2026-10-01T09:00:00Z","server":"web","call_id":"expired"},
                {"timestamp":"2026-11-12T10:00:00Z","server":"web","call_id":"future"}
            ],
            "mcp":[{"timestamp":STAMP,"server":"local_documents","tool":"read","call_id":"mcp","nested":true,
                     "metadata":{"resources":{"page_id":"page-1","path":"PRIVATE_PATH"}},
                     "result":{"status":"ok","input_tokens":0,"cache_hit":false,"document_count":2,
                               "secret_count":999,"api_key_enabled":true,"usage":{"rows":{"unit":"count","value":3}},
                               "credits":{"balance":"42.5","has_credits":true},
                               "ocr":{"status":"completed","processed_items":0,"errors":["PRIVATE_ERROR"]},
                               "findings":[{"severity":"HIGH","body":"PRIVATE_FINDING"}],"content":"PRIVATE_BODY"}}],
            "token":"PRIVATE_METADATA"
        }),
    );
    write(
        root,
        "error-history.json",
        &json!({
            "version":1,"events":[
                {"timestamp":STAMP,"source":"session","category":"conversation","severity":"error","code":"fixture",
                 "reason":"first","attempt":0,"message":"PRIVATE_OLD"},
                {"timestamp":STAMP,"source":"session","category":"conversation","severity":"error","code":"fixture",
                 "reason":"latest","attempt":0,"message":"PRIVATE_LATEST"},
                {"timestamp":"2026-11-10T09:00:00Z","source":"session","category":"conversation","severity":"error","code":"old"}
            ]
        }),
    );
    write(
        root,
        "thread-state.json",
        &json!({
            "version":1,"entries":{
                "rollout-example.jsonl":{"thread_id":THREAD,"status":"running","offset":0,"task_time":STAMP,"task_start":null,"cwd":"PRIVATE_CWD"},
                "../another.jsonl":{"thread_id":THREAD,"status":"running","offset":0,"task_time":STAMP}
            },"skills":[{"thread_id":THREAD,"call_id":"skill","skill":"資料 檢查","timestamp":STAMP,"path":"PRIVATE_SKILL","has_document":true}]
        }),
    );
    let originals = [
        "activity-history.json",
        "error-history.json",
        "thread-state.json",
    ]
    .map(|name| fs::read(root.join(name)).unwrap());
    for namespace in [Namespace::Activity, Namespace::Errors, Namespace::Threads] {
        assert!(Store::import_legacy(root, namespace, now).unwrap());
        assert!(!Store::import_legacy(root, namespace, now).unwrap());
    }
    let mut store = Store::open(root, Access::ReadOnly).unwrap().unwrap();
    let sql = items(&mut store, Section::Sql);
    assert_eq!(sql.len(), 1);
    assert_eq!(sql[0]["database"], "file:fixture.db");
    assert_eq!(sql[0]["rows_returned"], 0);
    let web = items(&mut store, Section::Web);
    assert_eq!(web.len(), 1);
    assert_eq!(web[0]["metadata"]["operations"], json!(["open", "click"]));
    assert_eq!(
        web[0]["metadata"]["references"],
        json!(["https://example.com/ref", "https://example.com/next"])
    );
    let mcp = items(&mut store, Section::Mcp);
    assert_eq!(mcp[0]["metadata"]["resources"], json!({"page_id":"page-1"}));
    assert_eq!(mcp[0]["result"]["input_tokens"], 0);
    assert_eq!(mcp[0]["result"]["cache_hit"], false);
    assert_eq!(mcp[0]["result"]["usage_rows_count"], 3);
    assert_eq!(mcp[0]["result"]["ocr_errors"], 1);
    assert_eq!(mcp[0]["result"]["credits_balance"], 42.5);
    assert_eq!(mcp[0]["result"]["findings_high"], 1);
    assert!(mcp[0]["result"].get("secret_count").is_none());
    assert!(mcp[0]["result"].get("api_key_enabled").is_none());
    let errors = items(&mut store, Section::Errors);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["reason"], "latest");
    assert_eq!(errors[0]["attempt"], 0);
    let threads = items(&mut store, Section::Threads);
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["offset"], 0);
    assert!(threads[0]["task_start"].is_null());
    let skills = items(&mut store, Section::Skills);
    assert_eq!(skills[0]["skill"], "資料 檢查");
    assert_eq!(skills[0]["has_document"], false);
    for values in [sql, web, mcp, errors, threads, skills] {
        assert!(!json!(values).to_string().contains("PRIVATE_"));
    }
    assert_eq!(
        originals,
        [
            "activity-history.json",
            "error-history.json",
            "thread-state.json"
        ]
        .map(|name| fs::read(root.join(name)).unwrap())
    );
}

#[test]
fn malformed_and_oversized_legacy_files_do_not_create_storage() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let now = parse_utc_stamp(NOW).unwrap();
    for data in [
        br#"{"version":1,"events":null}"#.to_vec(),
        vec![b' '; 512 * 1024 + 1],
    ] {
        fs::write(root.join("error-history.json"), &data).unwrap();
        assert!(matches!(
            Store::import_legacy(root, Namespace::Errors, now),
            Err(StoreError::InvalidLegacy)
        ));
        assert!(!root.join(FILE_NAME).exists());
        assert_eq!(fs::read(root.join("error-history.json")).unwrap(), data);
    }
}

#[test]
fn existing_sqlite_rows_take_precedence_even_without_namespace_metadata() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let now = parse_utc_stamp(NOW).unwrap();
    drop(Store::open(root, Access::ReadWrite).unwrap());
    let db = rusqlite::Connection::open(root.join(FILE_NAME)).unwrap();
    db.execute("INSERT INTO history_items(namespace,section,item_key,ordinal,event_time,payload) VALUES('activity','web','prior',0,?,?)",
        [STAMP, r#"{"tool":"prior"}"#]).unwrap();
    write(
        root,
        "activity-history.json",
        &json!({"version":1,"sql":[],"web":[{"timestamp":STAMP,"server":"web","tool":"legacy"}]}),
    );
    assert!(!Store::import_legacy(root, Namespace::Activity, now).unwrap());
    let mut store = Store::open(root, Access::ReadOnly).unwrap().unwrap();
    assert_eq!(items(&mut store, Section::Web), [json!({"tool":"prior"})]);
}

#[test]
fn simultaneous_imports_claim_the_namespace_once() {
    use std::sync::{Arc, Barrier};
    let dir = tempdir().unwrap();
    let root = dir.path();
    let now = parse_utc_stamp(NOW).unwrap();
    write(
        root,
        "activity-history.json",
        &json!({"version":1,"sql":[],"web":[{"timestamp":STAMP,"server":"web","tool":"legacy"}]}),
    );
    let barrier = Arc::new(Barrier::new(3));
    let workers = (0..2)
        .map(|_| {
            let root = root.to_path_buf();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Store::import_legacy(&root, Namespace::Activity, now).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    assert_eq!(
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(|done| *done)
            .count(),
        1
    );
    let mut store = Store::open(root, Access::ReadOnly).unwrap().unwrap();
    assert_eq!(items(&mut store, Section::Web).len(), 1);
}

#[cfg(unix)]
#[test]
fn redirected_legacy_files_are_rejected_before_opening_a_database() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let now = parse_utc_stamp(NOW).unwrap();
    let owned = root.join("example.json");
    fs::write(&owned, br#"{"version":1,"entries":{}}"#).unwrap();
    std::os::unix::fs::symlink(&owned, root.join("thread-state.json")).unwrap();
    assert!(matches!(
        Store::import_legacy(root, Namespace::Threads, now),
        Err(StoreError::UnsafePath)
    ));
    assert!(!root.join(FILE_NAME).exists());
}

#[test]
fn imports_legacy_unicode_encodings_with_or_without_bom() {
    let now = parse_utc_stamp(NOW).unwrap();
    let text = json!({"version":1,"entries":{},"skills":[
        {"thread_id":THREAD,"call_id":"demo","skill":"資料 檢查","timestamp":STAMP}
    ]})
    .to_string();
    for (prefix, data) in [
        (
            b"\xff\xfe".as_slice(),
            text.encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        (
            b"".as_slice(),
            text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        ),
        (
            b"\xff\xfe\0\0".as_slice(),
            text.chars()
                .map(u32::from)
                .flat_map(u32::to_le_bytes)
                .collect(),
        ),
        (
            b"".as_slice(),
            text.chars()
                .map(u32::from)
                .flat_map(u32::to_be_bytes)
                .collect(),
        ),
    ] {
        let dir = tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("thread-state.json"), [prefix, &data].concat()).unwrap();
        assert!(Store::import_legacy(root, Namespace::Threads, now).unwrap());
        let mut store = Store::open(root, Access::ReadOnly).unwrap().unwrap();
        assert_eq!(items(&mut store, Section::Skills)[0]["skill"], "資料 檢查");
    }
}
